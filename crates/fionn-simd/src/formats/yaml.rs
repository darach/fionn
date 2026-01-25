// SPDX-License-Identifier: MIT OR Apache-2.0
//! YAML SIMD parser
//!
//! Provides SIMD-accelerated parsing for YAML documents with:
//! - Indentation-based structure detection
//! - Anchor (`&`) and alias (`*`) detection
//! - Merge key (`<<:`) detection
//! - Document separator (`---`) detection
//! - Comment (`#`) detection
//!
//! # SIMD Strategies
//!
//! - **Indentation**: Count leading spaces using vector comparison
//! - **Anchors/Aliases**: Detect `&` and `*` outside strings
//! - **Comments**: Detect `#` outside strings
//! - **Colons**: Detect `:` for key-value separation

use super::{ChunkMask, FormatParser, StructuralPositions};
use fionn_core::format::FormatKind;

/// YAML SIMD parser
#[derive(Debug, Clone, Default)]
pub struct YamlParser {
    /// Current indentation stack
    indent_stack: Vec<usize>,
    /// Whether we're in a multi-line string
    in_multiline: bool,
    /// Current quote character (if in string)
    quote_char: Option<u8>,
}

/// YAML-specific structural elements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlStructural {
    /// Document start `---`
    DocumentStart,
    /// Document end `...`
    DocumentEnd,
    /// Anchor definition `&name`
    Anchor,
    /// Alias reference `*name`
    Alias,
    /// Merge key `<<:`
    MergeKey,
    /// Mapping key `:`
    MappingKey,
    /// Sequence item `-`
    SequenceItem,
    /// Comment `#`
    Comment,
    /// Literal block `|`
    LiteralBlock,
    /// Folded block `>`
    FoldedBlock,
}

/// YAML parse error
#[derive(Debug, Clone)]
pub enum YamlError {
    /// Invalid indentation
    InvalidIndentation {
        /// Line number
        line: usize,
        /// Expected indentation
        expected: usize,
        /// Actual indentation
        actual: usize,
    },
    /// Unterminated string
    UnterminatedString {
        /// Line number
        line: usize,
    },
    /// Invalid anchor name
    InvalidAnchor {
        /// Line number
        line: usize,
        /// Anchor name
        name: String,
    },
    /// Undefined alias
    UndefinedAlias {
        /// Line number
        line: usize,
        /// Alias name
        name: String,
    },
}

impl YamlParser {
    /// Create a new YAML parser
    #[must_use]
    pub const fn new() -> Self {
        Self {
            indent_stack: Vec::new(),
            in_multiline: false,
            quote_char: None,
        }
    }

    /// Get the format kind
    #[must_use]
    pub const fn format_kind() -> FormatKind {
        FormatKind::Yaml
    }

    /// Reset parser state for new document
    pub fn reset(&mut self) {
        self.indent_stack.clear();
        self.in_multiline = false;
        self.quote_char = None;
    }

    /// Count leading spaces (indentation) using SIMD
    #[must_use]
    pub fn count_indent(line: &[u8]) -> usize {
        // Use memchr to find first non-space character
        line.iter().take_while(|&&b| b == b' ').count()
    }

    /// Detect structural characters in a 64-byte chunk
    #[must_use]
    pub fn scan_chunk(&self, chunk: &[u8; 64]) -> ChunkMask {
        let mut mask = ChunkMask::new();

        // Build string mask (track quote state)
        let mut in_string = false;
        let mut prev_escape = false;

        for (i, &byte) in chunk.iter().enumerate() {
            // Track escapes
            if prev_escape {
                mask.escape_mask |= 1 << i;
                prev_escape = false;
                continue;
            }

            if byte == b'\\' && in_string {
                prev_escape = true;
                continue;
            }

            // Track strings (single and double quotes)
            if (byte == b'"' || byte == b'\'') && !prev_escape {
                mask.string_boundary_at(i);
                in_string = !in_string;
            }

            if in_string {
                mask.string_mask |= 1 << i;
                continue;
            }

            // Structural characters outside strings
            match byte {
                b'#' => {
                    mask.comment_mask |= !0u64 << i; // Rest of line is comment
                    break;
                }
                b':' | b'-' | b'&' | b'*' | b'|' | b'>' | b'[' | b']' | b'{' | b'}' => {
                    mask.structural_mask |= 1 << i;
                }
                _ => {}
            }
        }

        mask
    }

    /// Detect anchor at position
    #[must_use]
    pub fn detect_anchor(input: &[u8], pos: usize) -> Option<&[u8]> {
        if pos >= input.len() || input[pos] != b'&' {
            return None;
        }

        // Find end of anchor name
        let start = pos + 1;
        let end = input[start..]
            .iter()
            .position(|&b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'-')
            .map_or(input.len(), |p| start + p);

        if end > start {
            Some(&input[start..end])
        } else {
            None
        }
    }

    /// Detect alias at position
    #[must_use]
    pub fn detect_alias(input: &[u8], pos: usize) -> Option<&[u8]> {
        if pos >= input.len() || input[pos] != b'*' {
            return None;
        }

        // Find end of alias name
        let start = pos + 1;
        let end = input[start..]
            .iter()
            .position(|&b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'-')
            .map_or(input.len(), |p| start + p);

        if end > start {
            Some(&input[start..end])
        } else {
            None
        }
    }

    /// Detect document separator (`---` or `...`)
    #[must_use]
    pub fn detect_document_marker(line: &[u8]) -> Option<YamlStructural> {
        let trimmed = line.iter().take_while(|&&b| b != b'\n').collect::<Vec<_>>();

        if trimmed.len() >= 3 {
            if trimmed[0..3] == [&b'-', &b'-', &b'-'] {
                return Some(YamlStructural::DocumentStart);
            }
            if trimmed[0..3] == [&b'.', &b'.', &b'.'] {
                return Some(YamlStructural::DocumentEnd);
            }
        }
        None
    }

    /// Detect merge key (`<<:`)
    #[must_use]
    pub fn detect_merge_key(line: &[u8]) -> bool {
        memchr::memmem::find(line, b"<<:").is_some()
    }
}

impl ChunkMask {
    #[allow(clippy::unused_self, clippy::missing_const_for_fn)] // Placeholder for future string tracking
    fn string_boundary_at(&self, _pos: usize) {
        // Track string boundaries for later processing
    }
}

impl FormatParser for YamlParser {
    type Error = YamlError;

    fn parse_structural(&self, input: &[u8]) -> Result<StructuralPositions, Self::Error> {
        let mut positions = StructuralPositions::new();

        // Find newlines using SIMD
        positions.newlines = memchr::memchr_iter(b'\n', input).collect();

        // Find potential structural characters
        for (i, &byte) in input.iter().enumerate() {
            match byte {
                b'#' => positions.comment_starts.push(i),
                b'"' | b'\'' => positions.string_boundaries.push(i),
                b':' | b'-' | b'&' | b'*' | b'|' | b'>' => positions.delimiters.push(i),
                b'\\' => positions.escapes.push(i),
                _ => {}
            }
        }

        Ok(positions)
    }

    fn detect_indent(&self, input: &[u8], pos: usize) -> usize {
        // Find start of line
        let line_start = input[..pos]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);

        Self::count_indent(&input[line_start..])
    }

    fn is_in_string(&self, _input: &[u8], _pos: usize) -> bool {
        // Would need full parse state to determine accurately
        self.quote_char.is_some()
    }

    fn is_in_comment(&self, input: &[u8], pos: usize) -> bool {
        // Find start of line and check for # before pos
        let line_start = input[..pos]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);

        input[line_start..pos].contains(&b'#')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_indent() {
        assert_eq!(YamlParser::count_indent(b"no indent"), 0);
        assert_eq!(YamlParser::count_indent(b"  two spaces"), 2);
        assert_eq!(YamlParser::count_indent(b"    four spaces"), 4);
    }

    #[test]
    fn test_detect_anchor() {
        assert_eq!(
            YamlParser::detect_anchor(b"&default", 0),
            Some(b"default".as_slice())
        );
        assert_eq!(
            YamlParser::detect_anchor(b"&my_anchor value", 0),
            Some(b"my_anchor".as_slice())
        );
        assert_eq!(YamlParser::detect_anchor(b"no anchor", 0), None);
    }

    #[test]
    fn test_detect_alias() {
        assert_eq!(
            YamlParser::detect_alias(b"*default", 0),
            Some(b"default".as_slice())
        );
        assert_eq!(
            YamlParser::detect_alias(b"*ref ", 0),
            Some(b"ref".as_slice())
        );
        assert_eq!(YamlParser::detect_alias(b"no alias", 0), None);
    }

    #[test]
    fn test_detect_document_marker() {
        assert_eq!(
            YamlParser::detect_document_marker(b"---"),
            Some(YamlStructural::DocumentStart)
        );
        assert_eq!(
            YamlParser::detect_document_marker(b"..."),
            Some(YamlStructural::DocumentEnd)
        );
        assert_eq!(YamlParser::detect_document_marker(b"key: value"), None);
    }

    #[test]
    fn test_detect_merge_key() {
        assert!(YamlParser::detect_merge_key(b"<<: *default"));
        assert!(!YamlParser::detect_merge_key(b"key: value"));
    }
}
