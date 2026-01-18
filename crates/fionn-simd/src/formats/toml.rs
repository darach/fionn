// SPDX-License-Identifier: MIT OR Apache-2.0
//! TOML SIMD parser
//!
//! Provides SIMD-accelerated parsing for TOML documents with:
//! - Section header (`[table]`) detection
//! - Array table (`[[array]]`) detection
//! - Inline table (`{key=value}`) detection
//! - Dotted key (`a.b.c`) detection
//! - Comment (`#`) detection
//!
//! # SIMD Strategies
//!
//! - **Section headers**: Detect `[` at line start
//! - **Brackets**: Match `[`, `]`, `{`, `}` for structure
//! - **Assignment**: Detect `=` for key-value pairs
//! - **Comments**: Detect `#` outside strings

use super::{ChunkMask, FormatParser, StructuralPositions};
use fionn_core::format::FormatKind;

/// TOML SIMD parser
#[derive(Debug, Clone, Default)]
pub struct TomlParser {
    /// Current table path
    current_table: Vec<String>,
    /// Whether we're in a multi-line string
    in_multiline: bool,
}

/// TOML-specific structural elements
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlStructural {
    /// Standard table `[table]`
    Table(Vec<String>),
    /// Array table `[[array]]`
    ArrayTable(Vec<String>),
    /// Inline table start `{`
    InlineTableStart,
    /// Inline table end `}`
    InlineTableEnd,
    /// Key-value assignment `=`
    Assignment,
    /// Dotted key `a.b.c`
    DottedKey(Vec<String>),
    /// Comment `#`
    Comment,
    /// Array start `[`
    ArrayStart,
    /// Array end `]`
    ArrayEnd,
}

/// TOML parse error
#[derive(Debug, Clone)]
pub enum TomlError {
    /// Invalid table header
    InvalidTableHeader {
        /// Line number
        line: usize,
        /// Header content
        header: String,
    },
    /// Duplicate table definition
    DuplicateTable {
        /// Line number
        line: usize,
        /// Table name
        table: String,
    },
    /// Invalid key
    InvalidKey {
        /// Line number
        line: usize,
        /// Key content
        key: String,
    },
    /// Unterminated string
    UnterminatedString {
        /// Line number
        line: usize,
    },
    /// Invalid inline table
    InvalidInlineTable {
        /// Line number
        line: usize,
    },
}

impl TomlParser {
    /// Create a new TOML parser
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current_table: Vec::new(),
            in_multiline: false,
        }
    }

    /// Get the format kind
    #[must_use]
    pub const fn format_kind() -> FormatKind {
        FormatKind::Toml
    }

    /// Reset parser state
    pub fn reset(&mut self) {
        self.current_table.clear();
        self.in_multiline = false;
    }

    /// Detect structural characters in a 64-byte chunk
    #[must_use]
    pub fn scan_chunk(&self, chunk: &[u8; 64]) -> ChunkMask {
        let mut mask = ChunkMask::new();

        let mut in_string = false;
        let mut in_literal = false; // Single-quoted literal string
        let mut prev_escape = false;

        for (i, &byte) in chunk.iter().enumerate() {
            // Track escapes (only in basic strings)
            if prev_escape && !in_literal {
                mask.escape_mask |= 1 << i;
                prev_escape = false;
                continue;
            }

            if byte == b'\\' && in_string && !in_literal {
                prev_escape = true;
                continue;
            }

            // Track strings
            if byte == b'"' && !in_literal && !prev_escape {
                in_string = !in_string;
            } else if byte == b'\'' && !in_string {
                in_literal = !in_literal;
            }

            if in_string || in_literal {
                mask.string_mask |= 1 << i;
                continue;
            }

            // Structural characters outside strings
            match byte {
                b'#' => {
                    mask.comment_mask |= !0u64 << i;
                    break;
                }
                b'[' | b']' | b'{' | b'}' | b'=' | b'.' | b',' => {
                    mask.structural_mask |= 1 << i;
                }
                _ => {}
            }
        }

        mask
    }

    /// Parse a table header `[table]` or `[[array]]`
    #[must_use]
    pub fn parse_table_header(line: &[u8]) -> Option<TomlStructural> {
        let trimmed: Vec<u8> = line
            .iter()
            .skip_while(|&&b| b == b' ' || b == b'\t')
            .take_while(|&&b| b != b'\n' && b != b'#')
            .copied()
            .collect();

        if trimmed.is_empty() {
            return None;
        }

        // Check for array table [[name]]
        if trimmed.starts_with(b"[[") && trimmed.ends_with(b"]]") {
            let inner = &trimmed[2..trimmed.len() - 2];
            let path = Self::parse_table_path(inner);
            return Some(TomlStructural::ArrayTable(path));
        }

        // Check for standard table [name]
        if trimmed.starts_with(b"[") && trimmed.ends_with(b"]") {
            let inner = &trimmed[1..trimmed.len() - 1];
            let path = Self::parse_table_path(inner);
            return Some(TomlStructural::Table(path));
        }

        None
    }

    /// Parse a dotted table path
    fn parse_table_path(path: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(path)
            .split('.')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .collect()
    }

    /// Detect dotted key in a line
    #[must_use]
    pub fn detect_dotted_key(line: &[u8]) -> Option<Vec<String>> {
        // Find the = sign
        let eq_pos = memchr::memchr(b'=', line)?;
        let key_part = &line[..eq_pos];

        // Check if key contains dots (outside quotes)
        let key_str = String::from_utf8_lossy(key_part);
        let trimmed = key_str.trim();

        if trimmed.contains('.') && !trimmed.starts_with('"') {
            let parts: Vec<String> = trimmed.split('.').map(|s| s.trim().to_string()).collect();
            if parts.len() > 1 {
                return Some(parts);
            }
        }

        None
    }

    /// Detect inline table
    #[must_use]
    pub fn detect_inline_table(line: &[u8]) -> bool {
        // Simple check: contains { and } on same line after =
        if let Some(eq_pos) = memchr::memchr(b'=', line) {
            let after_eq = &line[eq_pos + 1..];
            let has_open = memchr::memchr(b'{', after_eq).is_some();
            let has_close = memchr::memchr(b'}', after_eq).is_some();
            return has_open && has_close;
        }
        false
    }

    /// Parse section header - alias for `parse_table_header`
    #[must_use]
    pub fn parse_section_header(line: &[u8]) -> Option<TomlStructural> {
        Self::parse_table_header(line)
    }

    /// Parse a dotted key into its path components
    #[must_use]
    pub fn parse_dotted_key(key: &[u8]) -> Vec<String> {
        let key_str = String::from_utf8_lossy(key);
        key_str
            .trim()
            .split('.')
            .map(|s| s.trim().to_string())
            .collect()
    }
}

impl FormatParser for TomlParser {
    type Error = TomlError;

    fn parse_structural(&self, input: &[u8]) -> Result<StructuralPositions, Self::Error> {
        let mut positions = StructuralPositions::new();

        // Find newlines using SIMD
        positions.newlines = memchr::memchr_iter(b'\n', input).collect();

        // Find structural characters
        for (i, &byte) in input.iter().enumerate() {
            match byte {
                b'#' => positions.comment_starts.push(i),
                b'"' | b'\'' => positions.string_boundaries.push(i),
                b'[' | b']' | b'{' | b'}' | b'=' | b'.' => positions.delimiters.push(i),
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

        input[line_start..]
            .iter()
            .take_while(|&&b| b == b' ' || b == b'\t')
            .count()
    }

    fn is_in_string(&self, _input: &[u8], _pos: usize) -> bool {
        self.in_multiline
    }

    fn is_in_comment(&self, input: &[u8], pos: usize) -> bool {
        let line_start = input[..pos]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);

        // Check for # before pos on same line (simplified)
        input[line_start..pos].contains(&b'#')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_table_header() {
        assert_eq!(
            TomlParser::parse_table_header(b"[database]"),
            Some(TomlStructural::Table(vec!["database".to_string()]))
        );
        assert_eq!(
            TomlParser::parse_table_header(b"[database.connection]"),
            Some(TomlStructural::Table(vec![
                "database".to_string(),
                "connection".to_string()
            ]))
        );
        assert_eq!(
            TomlParser::parse_table_header(b"[[servers]]"),
            Some(TomlStructural::ArrayTable(vec!["servers".to_string()]))
        );
    }

    #[test]
    fn test_detect_dotted_key() {
        assert_eq!(
            TomlParser::detect_dotted_key(b"database.host = \"localhost\""),
            Some(vec!["database".to_string(), "host".to_string()])
        );
        assert_eq!(TomlParser::detect_dotted_key(b"name = \"value\""), None);
    }

    #[test]
    fn test_detect_inline_table() {
        assert!(TomlParser::detect_inline_table(b"point = {x = 1, y = 2}"));
        assert!(!TomlParser::detect_inline_table(b"name = \"value\""));
    }
}
