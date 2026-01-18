// SPDX-License-Identifier: MIT OR Apache-2.0
//! ISON SIMD parser
//!
//! Provides SIMD-accelerated parsing for ISON (Interchange Simple Object Notation),
//! a token-efficient format designed for LLM and agentic AI workflows.
//!
//! # Format Features
//!
//! - Block-based structure (`table.name`, `object.name`)
//! - Space-delimited fields
//! - Type annotations (`field:type`)
//! - Reference system (`:id`, `:type:id`, `:RELATIONSHIP:id`)
//! - Comments (`#`)
//! - ISONL streaming format (pipe-delimited)
//!
//! # SIMD Strategies
//!
//! - **Block headers**: Detect `table.` and `object.` at line starts
//! - **References**: Detect `:` patterns outside strings
//! - **Field boundaries**: Detect space delimiters

use super::{ChunkMask, FormatParser, StructuralPositions};
use fionn_core::format::FormatKind;

/// ISON SIMD parser
#[derive(Debug, Clone, Default)]
pub struct IsonParser {
    /// Current block name
    current_block: Option<String>,
    /// Field names for current block
    field_names: Vec<String>,
    /// Whether parsing ISONL (streaming) format
    #[allow(dead_code)]
    streaming: bool,
}

/// ISON block kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsonBlockKind {
    /// Table block (multiple rows)
    Table,
    /// Object block (single row)
    Object,
}

/// ISON-specific structural elements
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsonStructural {
    /// Block header (`table.name` or `object.name`)
    BlockHeader {
        /// Block kind (Table or Object)
        kind: IsonBlockKind,
        /// Block name
        name: String,
    },
    /// Field declaration row
    FieldDeclaration {
        /// List of field declarations
        fields: Vec<IsonField>,
    },
    /// Data row
    DataRow,
    /// Reference (`:id`, `:type:id`)
    Reference(IsonReference),
    /// Comment
    Comment,
    /// Summary section (`---`)
    Summary,
    /// ISONL delimiter (`|`)
    IsonlDelimiter,
}

/// ISON field with optional type
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsonField {
    /// Field name
    pub name: String,
    /// Optional type annotation
    pub field_type: Option<IsonType>,
}

/// ISON type annotations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsonType {
    /// Integer type
    Int,
    /// Float type
    Float,
    /// String type
    String,
    /// Boolean type
    Bool,
    /// Computed/derived field
    Computed,
    /// Reference to another table
    Reference,
}

/// ISON reference types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsonReference {
    /// Simple ID reference `:id`
    Simple(String),
    /// Typed reference `:type:id`
    Typed {
        /// Reference type
        ref_type: String,
        /// Reference ID
        id: String,
    },
    /// Relationship reference `:RELATIONSHIP:id`
    Relationship {
        /// Relationship name
        relationship: String,
        /// Reference ID
        id: String,
    },
}

/// ISON parse error
#[derive(Debug, Clone)]
pub enum IsonError {
    /// Invalid block header
    InvalidBlockHeader {
        /// Line number
        line: usize,
        /// Header content
        header: String,
    },
    /// Field count mismatch
    FieldCountMismatch {
        /// Line number
        line: usize,
        /// Expected field count
        expected: usize,
        /// Actual field count
        actual: usize,
    },
    /// Invalid type annotation
    InvalidType {
        /// Line number
        line: usize,
        /// Field name
        field: String,
        /// Type string
        type_str: String,
    },
    /// Invalid reference format
    InvalidReference {
        /// Line number
        line: usize,
        /// Reference content
        reference: String,
    },
    /// Unterminated string
    UnterminatedString {
        /// Line number
        line: usize,
    },
}

impl IsonParser {
    /// Create a new ISON parser
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current_block: None,
            field_names: Vec::new(),
            streaming: false,
        }
    }

    /// Create an ISONL (streaming) parser
    #[must_use]
    pub const fn streaming() -> Self {
        Self {
            current_block: None,
            field_names: Vec::new(),
            streaming: true,
        }
    }

    /// Get the format kind
    #[must_use]
    pub const fn format_kind() -> FormatKind {
        FormatKind::Ison
    }

    /// Reset parser state
    pub fn reset(&mut self) {
        self.current_block = None;
        self.field_names.clear();
    }

    /// Detect structural characters in a 64-byte chunk
    #[must_use]
    pub fn scan_chunk(&self, chunk: &[u8; 64]) -> ChunkMask {
        let mut mask = ChunkMask::new();
        let mut in_string = false;
        let mut prev_escape = false;

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

            match byte {
                b'#' => {
                    mask.comment_mask |= !0u64 << i;
                    break;
                }
                b':' | b' ' | b'|' | b'\t' => {
                    mask.structural_mask |= 1 << i;
                }
                _ => {}
            }
        }

        mask
    }

    /// Parse a block header (`table.name` or `object.name`)
    #[must_use]
    pub fn parse_block_header(line: &[u8]) -> Option<(IsonBlockKind, String)> {
        let line_str = std::str::from_utf8(line).ok()?;
        let trimmed = line_str.trim();

        if let Some(name) = trimmed.strip_prefix("table.") {
            let name = name.split_whitespace().next()?;
            return Some((IsonBlockKind::Table, name.to_string()));
        }

        if let Some(name) = trimmed.strip_prefix("object.") {
            let name = name.split_whitespace().next()?;
            return Some((IsonBlockKind::Object, name.to_string()));
        }

        None
    }

    /// Parse a field declaration row
    #[must_use]
    pub fn parse_field_declaration(line: &[u8]) -> Vec<IsonField> {
        let Ok(line_str) = std::str::from_utf8(line) else {
            return Vec::new();
        };

        line_str
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|field_spec| {
                if let Some((name, type_str)) = field_spec.split_once(':') {
                    IsonField {
                        name: name.to_string(),
                        field_type: IsonType::parse(type_str),
                    }
                } else {
                    IsonField {
                        name: field_spec.to_string(),
                        field_type: None,
                    }
                }
            })
            .collect()
    }

    /// Parse a reference
    #[must_use]
    pub fn parse_reference(s: &str) -> Option<IsonReference> {
        if !s.starts_with(':') {
            return None;
        }

        let parts: Vec<&str> = s[1..].split(':').collect();

        match parts.len() {
            1 => Some(IsonReference::Simple(parts[0].to_string())),
            2 => {
                let first = parts[0];
                let second = parts[1];

                // Check if it's a relationship (UPPERCASE) or type (lowercase)
                if first.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    Some(IsonReference::Relationship {
                        relationship: first.to_string(),
                        id: second.to_string(),
                    })
                } else {
                    Some(IsonReference::Typed {
                        ref_type: first.to_string(),
                        id: second.to_string(),
                    })
                }
            }
            _ => None,
        }
    }

    /// Parse a data row into values
    #[must_use]
    pub fn parse_data_row(line: &[u8]) -> Vec<String> {
        let line_str = match std::str::from_utf8(line) {
            Ok(s) => s.trim(),
            Err(_) => return Vec::new(),
        };

        let mut values = Vec::new();
        let mut current = String::new();
        let mut in_quote = false;

        for ch in line_str.chars() {
            if ch == '"' {
                in_quote = !in_quote;
                current.push(ch);
            } else if ch == ' ' && !in_quote {
                if !current.is_empty() {
                    values.push(std::mem::take(&mut current));
                }
            } else {
                current.push(ch);
            }
        }

        if !current.is_empty() {
            values.push(current);
        }

        values
    }

    /// Check if line is a summary marker
    #[must_use]
    pub fn is_summary_marker(line: &[u8]) -> bool {
        let trimmed: Vec<u8> = line
            .iter()
            .copied()
            .filter(|&b| b != b' ' && b != b'\t' && b != b'\n' && b != b'\r')
            .collect();
        trimmed == b"---"
    }

    /// Check if line is a comment
    #[must_use]
    pub fn is_comment(line: &[u8]) -> bool {
        line.iter()
            .find(|&&b| b != b' ' && b != b'\t')
            .is_some_and(|&b| b == b'#')
    }
}

impl IsonType {
    /// Parse type from string
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "string" => Some(Self::String),
            "bool" => Some(Self::Bool),
            "computed" => Some(Self::Computed),
            _ if !s.is_empty() => Some(Self::Reference), // Assume reference type
            _ => None,
        }
    }
}

impl FormatParser for IsonParser {
    type Error = IsonError;

    fn parse_structural(&self, input: &[u8]) -> Result<StructuralPositions, Self::Error> {
        let mut positions = StructuralPositions::new();

        positions.newlines = memchr::memchr_iter(b'\n', input).collect();

        for (i, &byte) in input.iter().enumerate() {
            match byte {
                b'#' => positions.comment_starts.push(i),
                b'"' => positions.string_boundaries.push(i),
                b':' | b' ' | b'|' => positions.delimiters.push(i),
                b'\\' => positions.escapes.push(i),
                _ => {}
            }
        }

        Ok(positions)
    }

    fn detect_indent(&self, _input: &[u8], _pos: usize) -> usize {
        // ISON doesn't use indentation
        0
    }

    #[allow(clippy::naive_bytecount)]
    fn is_in_string(&self, input: &[u8], pos: usize) -> bool {
        let quotes_before = input[..pos].iter().filter(|&&b| b == b'"').count();
        quotes_before % 2 == 1
    }

    fn is_in_comment(&self, input: &[u8], pos: usize) -> bool {
        Self::is_comment(&input[..=pos])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_block_header() {
        assert_eq!(
            IsonParser::parse_block_header(b"table.users"),
            Some((IsonBlockKind::Table, "users".to_string()))
        );
        assert_eq!(
            IsonParser::parse_block_header(b"object.config"),
            Some((IsonBlockKind::Object, "config".to_string()))
        );
        assert_eq!(IsonParser::parse_block_header(b"invalid"), None);
    }

    #[test]
    fn test_parse_field_declaration() {
        let fields = IsonParser::parse_field_declaration(b"id:int name:string active:bool");
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].name, "id");
        assert_eq!(fields[0].field_type, Some(IsonType::Int));
        assert_eq!(fields[1].name, "name");
        assert_eq!(fields[1].field_type, Some(IsonType::String));
    }

    #[test]
    fn test_parse_reference() {
        assert_eq!(
            IsonParser::parse_reference(":1"),
            Some(IsonReference::Simple("1".to_string()))
        );
        assert_eq!(
            IsonParser::parse_reference(":user:1"),
            Some(IsonReference::Typed {
                ref_type: "user".to_string(),
                id: "1".to_string()
            })
        );
        assert_eq!(
            IsonParser::parse_reference(":BELONGS_TO:1"),
            Some(IsonReference::Relationship {
                relationship: "BELONGS_TO".to_string(),
                id: "1".to_string()
            })
        );
    }

    #[test]
    fn test_parse_data_row() {
        let values = IsonParser::parse_data_row(b"1 Alice alice@example.com true");
        assert_eq!(values, vec!["1", "Alice", "alice@example.com", "true"]);
    }

    #[test]
    fn test_parse_data_row_quoted() {
        let values = IsonParser::parse_data_row(b"1 \"Alice Smith\" alice@example.com");
        assert_eq!(values, vec!["1", "\"Alice Smith\"", "alice@example.com"]);
    }

    #[test]
    fn test_is_comment() {
        assert!(IsonParser::is_comment(b"# This is a comment"));
        assert!(IsonParser::is_comment(b"  # Indented comment"));
        assert!(!IsonParser::is_comment(b"not a comment"));
    }

    #[test]
    fn test_is_summary_marker() {
        assert!(IsonParser::is_summary_marker(b"---"));
        assert!(IsonParser::is_summary_marker(b"  ---  "));
        assert!(!IsonParser::is_summary_marker(b"----"));
    }
}
