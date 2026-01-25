// SPDX-License-Identifier: MIT OR Apache-2.0
//! TOON SIMD parser
//!
//! Provides SIMD-accelerated parsing for TOON (Token-Oriented Object Notation),
//! a line-oriented, indentation-based format optimized for LLM token efficiency.
//!
//! # Format Features
//!
//! - Indentation-based structure (2 spaces default)
//! - Array headers with length (`[N]`) and optional fields (`{field,field}`)
//! - Configurable delimiters (comma, tab, pipe)
//! - Key folding (`a.b.c: value`)
//! - Minimal quoting requirements
//!
//! # SIMD Strategies
//!
//! - **Indentation**: Count leading spaces using vector comparison
//! - **Array headers**: Detect `[` followed by digits
//! - **Delimiters**: Track active delimiter per scope
//! - **Key-value**: Detect `:` for assignment

use super::{ChunkMask, FormatParser, StructuralPositions};
use fionn_core::format::FormatKind;

/// TOON SIMD parser
#[derive(Debug, Clone)]
pub struct ToonParser {
    /// Indentation size (default: 2)
    indent_size: usize,
    /// Stack of active delimiters per scope
    delimiter_stack: Vec<ToonDelimiter>,
    /// Whether in strict mode
    strict: bool,
    /// Key folding mode
    expand_paths: ExpandPathsMode,
}

/// TOON delimiter types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToonDelimiter {
    /// Comma delimiter (default)
    #[default]
    Comma,
    /// Tab delimiter
    Tab,
    /// Pipe delimiter
    Pipe,
}

/// Key folding/expansion mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpandPathsMode {
    /// No path expansion
    #[default]
    Off,
    /// Safe expansion with conflict handling
    Safe {
        /// Whether to error on conflicts
        strict: bool,
    },
}

/// TOON-specific structural elements
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToonStructural {
    /// Object key-value pair
    KeyValue {
        /// Key name
        key: String,
        /// Nesting depth
        depth: usize,
    },
    /// Array header with length and optional fields
    ArrayHeader {
        /// Declared array length
        length: usize,
        /// Active delimiter
        delimiter: ToonDelimiter,
        /// Optional field names
        fields: Option<Vec<String>>,
    },
    /// Tabular row (data in array)
    TabularRow {
        /// Row values
        values: Vec<String>,
    },
    /// List item (`-` prefix)
    ListItem,
    /// Folded/dotted key path
    FoldedKey {
        /// Path components
        path: Vec<String>,
    },
}

/// TOON array header details
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToonArrayHeader {
    /// Declared array length
    pub length: usize,
    /// Active delimiter for this array
    pub delimiter: ToonDelimiter,
    /// Field names for tabular arrays
    pub fields: Option<Vec<String>>,
}

/// TOON parse error
#[derive(Debug, Clone)]
pub enum ToonError {
    /// Invalid indentation
    InvalidIndentation {
        /// Line number
        line: usize,
        /// Expected indentation
        expected: usize,
        /// Actual indentation
        actual: usize,
    },
    /// Array length mismatch
    ArrayLengthMismatch {
        /// Line number
        line: usize,
        /// Declared length
        declared: usize,
        /// Actual length
        actual: usize,
    },
    /// Field count mismatch
    FieldCountMismatch {
        /// Line number
        line: usize,
        /// Expected count
        expected: usize,
        /// Actual count
        actual: usize,
    },
    /// Invalid delimiter
    InvalidDelimiter {
        /// Line number
        line: usize,
        /// Expected delimiter
        expected: ToonDelimiter,
        /// Found character
        found: char,
    },
    /// Invalid escape sequence
    InvalidEscape {
        /// Line number
        line: usize,
        /// Escape sequence
        sequence: String,
    },
    /// Key folding conflict
    FoldingConflict {
        /// Line number
        line: usize,
        /// Path that conflicts
        path: String,
    },
    /// Unterminated string
    UnterminatedString {
        /// Line number
        line: usize,
    },
}

impl Default for ToonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ToonParser {
    /// Create a new TOON parser with default settings
    #[must_use]
    pub const fn new() -> Self {
        Self {
            indent_size: 2,
            delimiter_stack: Vec::new(),
            strict: true,
            expand_paths: ExpandPathsMode::Off,
        }
    }

    /// Get the format kind
    #[must_use]
    pub const fn format_kind() -> FormatKind {
        FormatKind::Toon
    }

    /// Set indentation size
    #[must_use]
    pub const fn with_indent_size(mut self, size: usize) -> Self {
        self.indent_size = size;
        self
    }

    /// Set strict mode
    #[must_use]
    pub const fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Set path expansion mode
    #[must_use]
    pub const fn with_expand_paths(mut self, mode: ExpandPathsMode) -> Self {
        self.expand_paths = mode;
        self
    }

    /// Reset parser state
    pub fn reset(&mut self) {
        self.delimiter_stack.clear();
    }

    /// Get current active delimiter
    #[must_use]
    pub fn active_delimiter(&self) -> ToonDelimiter {
        self.delimiter_stack.last().copied().unwrap_or_default()
    }

    /// Push a new delimiter scope
    pub fn push_delimiter(&mut self, delimiter: ToonDelimiter) {
        self.delimiter_stack.push(delimiter);
    }

    /// Pop delimiter scope
    pub fn pop_delimiter(&mut self) {
        self.delimiter_stack.pop();
    }

    /// Count leading spaces (indentation) using SIMD
    #[must_use]
    pub fn count_indent(line: &[u8]) -> usize {
        line.iter().take_while(|&&b| b == b' ').count()
    }

    /// Compute depth from indentation
    ///
    /// # Errors
    ///
    /// Returns an error if the indentation is invalid in strict mode.
    pub const fn compute_depth(&self, indent: usize) -> Result<usize, ToonError> {
        if self.strict && !indent.is_multiple_of(self.indent_size) {
            return Err(ToonError::InvalidIndentation {
                line: 0,
                expected: (indent / self.indent_size) * self.indent_size,
                actual: indent,
            });
        }
        Ok(indent / self.indent_size)
    }

    /// Detect structural characters in a 64-byte chunk
    #[must_use]
    pub fn scan_chunk(&self, chunk: &[u8; 64]) -> ChunkMask {
        let mut mask = ChunkMask::new();
        let mut in_string = false;
        let mut prev_escape = false;

        let active_delim = self.active_delimiter().as_byte();

        for (i, &byte) in chunk.iter().enumerate() {
            if prev_escape {
                mask.escape_mask |= 1 << i;
                prev_escape = false;
                continue;
            }

            if byte == b'\\' && in_string {
                prev_escape = true;
                continue;
            }

            if byte == b'"' {
                in_string = !in_string;
            }

            if in_string {
                mask.string_mask |= 1 << i;
                continue;
            }

            // Structural characters
            if byte == b':'
                || byte == b'['
                || byte == b']'
                || byte == b'{'
                || byte == b'}'
                || byte == b'-'
                || byte == active_delim
            {
                mask.structural_mask |= 1 << i;
            }
        }

        mask
    }

    /// Parse an array header `[N]` or `[N]{field,field}`
    #[must_use]
    pub fn parse_array_header(line: &[u8]) -> Option<ToonArrayHeader> {
        let line_str = std::str::from_utf8(line).ok()?;

        // Find the key and header part
        let colon_pos = line_str.find(':')?;
        let header_start = line_str[..colon_pos].rfind('[')?;
        let header_part = &line_str[header_start..colon_pos];

        // Parse [N] or [N|] or [N\t]
        let bracket_end = header_part.find(']')?;
        let bracket_content = &header_part[1..bracket_end];

        // Parse length and delimiter
        let (length, delimiter) = if let Some(stripped) = bracket_content.strip_suffix('|') {
            (stripped.parse().ok()?, ToonDelimiter::Pipe)
        } else if let Some(stripped) = bracket_content.strip_suffix('\t') {
            (stripped.parse().ok()?, ToonDelimiter::Tab)
        } else {
            (bracket_content.parse().ok()?, ToonDelimiter::Comma)
        };

        // Parse optional fields {field,field}
        let fields = if let Some(fields_start) = header_part.find('{') {
            let fields_end = header_part.find('}')?;
            // Ensure valid slice bounds (} must come after {)
            if fields_end <= fields_start {
                return None;
            }
            let fields_str = &header_part[fields_start + 1..fields_end];
            Some(
                fields_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
            )
        } else {
            None
        };

        Some(ToonArrayHeader {
            length,
            delimiter,
            fields,
        })
    }

    /// Check if a key is a folded path (contains dots)
    #[must_use]
    pub fn is_folded_key(key: &str) -> bool {
        // Must contain dots and not be quoted
        !key.starts_with('"')
            && key.contains('.')
            && key
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    }

    /// Parse a folded key into path components (from string)
    #[must_use]
    pub fn parse_folded_key_str(key: &str) -> Vec<String> {
        key.split('.').map(String::from).collect()
    }

    /// Parse a folded key from a line (bytes) - returns path if line contains folded key
    #[must_use]
    pub fn parse_folded_key(line: &[u8]) -> Option<Vec<String>> {
        let line_str = std::str::from_utf8(line).ok()?;
        let trimmed = line_str.trim();

        // Find the colon
        let colon_pos = trimmed.find(':')?;
        let key_part = trimmed[..colon_pos].trim();

        // Check if it's a folded key
        if Self::is_folded_key(key_part) {
            Some(Self::parse_folded_key_str(key_part))
        } else {
            None
        }
    }

    /// Check if a line is a list item
    #[must_use]
    pub fn is_list_item(line: &[u8]) -> bool {
        let trimmed: Vec<u8> = line.iter().copied().skip_while(|&b| b == b' ').collect();
        trimmed.first() == Some(&b'-') && trimmed.get(1).is_none_or(|&b| b == b' ' || b == b'\n')
    }

    /// Parse a tabular row with the active delimiter
    #[must_use]
    pub fn parse_tabular_row(&self, line: &[u8]) -> Vec<String> {
        let line_str = match std::str::from_utf8(line) {
            Ok(s) => s.trim(),
            Err(_) => return Vec::new(),
        };

        let delim = self.active_delimiter().as_char();
        line_str
            .split(delim)
            .map(|s| s.trim().to_string())
            .collect()
    }

    /// Check if a value needs quoting
    #[must_use]
    pub fn needs_quoting(value: &str, delimiter: ToonDelimiter) -> bool {
        if value.is_empty() {
            return true;
        }

        // Reserved words
        if matches!(value, "true" | "false" | "null") {
            return true;
        }

        // Numeric patterns
        if value.parse::<f64>().is_ok() {
            return true;
        }

        // Special characters
        let special = [b':', b'"', b'\\', b'[', b']', b'{', b'}'];
        if value.bytes().any(|b| special.contains(&b)) {
            return true;
        }

        // Active delimiter
        if value.contains(delimiter.as_char()) {
            return true;
        }

        // Leading hyphen or whitespace
        if value.starts_with('-') || value.starts_with(' ') || value.ends_with(' ') {
            return true;
        }

        false
    }
}

impl ToonDelimiter {
    /// Get the byte value
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        match self {
            Self::Comma => b',',
            Self::Tab => b'\t',
            Self::Pipe => b'|',
        }
    }

    /// Get the char value
    #[must_use]
    pub const fn as_char(self) -> char {
        match self {
            Self::Comma => ',',
            Self::Tab => '\t',
            Self::Pipe => '|',
        }
    }

    /// Parse from header symbol
    #[must_use]
    pub fn from_symbol(s: &str) -> Option<Self> {
        match s {
            "|" => Some(Self::Pipe),
            "\t" => Some(Self::Tab),
            "" => Some(Self::Comma),
            _ => None,
        }
    }
}

impl FormatParser for ToonParser {
    type Error = ToonError;

    fn parse_structural(&self, input: &[u8]) -> Result<StructuralPositions, Self::Error> {
        let mut positions = StructuralPositions::new();

        positions.newlines = memchr::memchr_iter(b'\n', input).collect();

        for (i, &byte) in input.iter().enumerate() {
            match byte {
                b'"' => positions.string_boundaries.push(i),
                b':' | b'[' | b']' | b'{' | b'}' | b'-' | b',' | b'|' => {
                    positions.delimiters.push(i);
                }
                b'\\' => positions.escapes.push(i),
                _ => {}
            }
        }

        Ok(positions)
    }

    fn detect_indent(&self, input: &[u8], pos: usize) -> usize {
        let line_start = input[..pos]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);

        Self::count_indent(&input[line_start..])
    }

    #[allow(clippy::naive_bytecount)] // Simple quote counting is acceptable for correctness check
    fn is_in_string(&self, input: &[u8], pos: usize) -> bool {
        let quotes_before = input[..pos].iter().filter(|&&b| b == b'"').count();
        quotes_before % 2 == 1
    }

    fn is_in_comment(&self, _input: &[u8], _pos: usize) -> bool {
        // TOON doesn't have comments
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_indent() {
        assert_eq!(ToonParser::count_indent(b"no indent"), 0);
        assert_eq!(ToonParser::count_indent(b"  two spaces"), 2);
        assert_eq!(ToonParser::count_indent(b"    four spaces"), 4);
    }

    #[test]
    fn test_compute_depth() {
        let parser = ToonParser::new();
        assert_eq!(parser.compute_depth(0).unwrap(), 0);
        assert_eq!(parser.compute_depth(2).unwrap(), 1);
        assert_eq!(parser.compute_depth(4).unwrap(), 2);
    }

    #[test]
    fn test_parse_array_header() {
        let header = ToonParser::parse_array_header(b"items[3]:").unwrap();
        assert_eq!(header.length, 3);
        assert_eq!(header.delimiter, ToonDelimiter::Comma);
        assert!(header.fields.is_none());
    }

    #[test]
    fn test_parse_array_header_with_fields() {
        let header = ToonParser::parse_array_header(b"users[2]{id,name,role}:").unwrap();
        assert_eq!(header.length, 2);
        assert_eq!(
            header.fields,
            Some(vec![
                "id".to_string(),
                "name".to_string(),
                "role".to_string()
            ])
        );
    }

    #[test]
    fn test_parse_array_header_pipe() {
        let header = ToonParser::parse_array_header(b"data[5|]:").unwrap();
        assert_eq!(header.length, 5);
        assert_eq!(header.delimiter, ToonDelimiter::Pipe);
    }

    #[test]
    fn test_is_folded_key() {
        assert!(ToonParser::is_folded_key("a.b.c"));
        assert!(ToonParser::is_folded_key("user.profile.name"));
        assert!(!ToonParser::is_folded_key("simple"));
        assert!(!ToonParser::is_folded_key("\"quoted.key\""));
    }

    #[test]
    fn test_is_list_item() {
        assert!(ToonParser::is_list_item(b"- item"));
        assert!(ToonParser::is_list_item(b"  - indented"));
        assert!(!ToonParser::is_list_item(b"not-a-list"));
    }

    #[test]
    fn test_needs_quoting() {
        assert!(ToonParser::needs_quoting("", ToonDelimiter::Comma));
        assert!(ToonParser::needs_quoting("true", ToonDelimiter::Comma));
        assert!(ToonParser::needs_quoting("42", ToonDelimiter::Comma));
        assert!(ToonParser::needs_quoting("has,comma", ToonDelimiter::Comma));
        assert!(!ToonParser::needs_quoting("simple", ToonDelimiter::Comma));
    }

    #[test]
    fn test_parse_tabular_row() {
        let parser = ToonParser::new();
        let values = parser.parse_tabular_row(b"1,Alice,admin");
        assert_eq!(values, vec!["1", "Alice", "admin"]);
    }
}
