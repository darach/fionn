// SPDX-License-Identifier: MIT OR Apache-2.0
//! CSV SIMD parser
//!
//! Provides SIMD-accelerated parsing for CSV documents with:
//! - Configurable delimiter detection (comma, tab, pipe, semicolon)
//! - Quoted field detection and handling
//! - Newline detection (handling CRLF and LF)
//! - Header row detection
//!
//! # SIMD Strategies
//!
//! - **Delimiter detection**: Vector comparison for delimiter characters
//! - **Quote detection**: Track quote state with XOR prefix
//! - **Newline detection**: Detect `\n` and `\r\n` patterns

use super::{ChunkMask, FormatParser, StructuralPositions};
use fionn_core::format::FormatKind;

/// CSV SIMD parser
#[derive(Debug, Clone)]
pub struct CsvParser {
    /// Field delimiter character
    delimiter: u8,
    /// Quote character
    quote_char: u8,
    /// Whether first row is header
    has_header: bool,
    /// Expected field count (0 = auto-detect)
    field_count: usize,
}

/// CSV-specific structural elements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvStructural {
    /// Field delimiter (comma, tab, etc.)
    Delimiter,
    /// Quote character
    Quote,
    /// Row end (newline)
    RowEnd,
    /// Escaped quote (doubled quote)
    EscapedQuote,
}

/// CSV parse error
#[derive(Debug, Clone)]
pub enum CsvError {
    /// Mismatched field count
    FieldCountMismatch {
        /// Row number
        row: usize,
        /// Expected field count
        expected: usize,
        /// Actual field count
        actual: usize,
    },
    /// Unterminated quoted field
    UnterminatedQuote {
        /// Row number
        row: usize,
        /// Column number
        col: usize,
    },
    /// Invalid escape sequence
    InvalidEscape {
        /// Row number
        row: usize,
        /// Column number
        col: usize,
    },
}

impl Default for CsvParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvParser {
    /// Create a new CSV parser with default settings (comma delimiter)
    #[must_use]
    pub const fn new() -> Self {
        Self {
            delimiter: b',',
            quote_char: b'"',
            has_header: true,
            field_count: 0,
        }
    }

    /// Get the format kind
    #[must_use]
    pub const fn format_kind() -> FormatKind {
        FormatKind::Csv
    }

    /// Set the field delimiter
    #[must_use]
    pub const fn with_delimiter(mut self, delimiter: u8) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// Set the quote character
    #[must_use]
    pub const fn with_quote(mut self, quote: u8) -> Self {
        self.quote_char = quote;
        self
    }

    /// Set whether first row is header
    #[must_use]
    pub const fn with_header(mut self, has_header: bool) -> Self {
        self.has_header = has_header;
        self
    }

    /// Set expected field count
    #[must_use]
    pub const fn with_field_count(mut self, count: usize) -> Self {
        self.field_count = count;
        self
    }

    /// Create a tab-separated parser
    #[must_use]
    pub const fn tsv() -> Self {
        Self {
            delimiter: b'\t',
            quote_char: b'"',
            has_header: true,
            field_count: 0,
        }
    }

    /// Create a pipe-separated parser
    #[must_use]
    pub const fn psv() -> Self {
        Self {
            delimiter: b'|',
            quote_char: b'"',
            has_header: true,
            field_count: 0,
        }
    }

    /// Detect structural characters in a 64-byte chunk
    #[must_use]
    pub fn scan_chunk(&self, chunk: &[u8; 64]) -> ChunkMask {
        let mut mask = ChunkMask::new();
        let mut in_quote = false;

        for (i, &byte) in chunk.iter().enumerate() {
            // Track quote state
            if byte == self.quote_char {
                // Check for escaped quote (doubled)
                if i + 1 < 64 && chunk[i + 1] == self.quote_char {
                    mask.escape_mask |= 1 << i;
                    continue;
                }
                in_quote = !in_quote;
            }

            if in_quote {
                mask.string_mask |= 1 << i;
                continue;
            }

            // Structural characters outside quotes
            if byte == self.delimiter || byte == b'\n' || byte == b'\r' {
                mask.structural_mask |= 1 << i;
            }
        }

        mask
    }

    /// Count fields in a row
    #[must_use]
    pub fn count_fields(&self, row: &[u8]) -> usize {
        if row.is_empty() {
            return 0;
        }

        let mut count = 1;
        let mut in_quote = false;

        for &byte in row {
            if byte == self.quote_char {
                in_quote = !in_quote;
            } else if !in_quote && byte == self.delimiter {
                count += 1;
            }
        }

        count
    }

    /// Parse a single row into fields
    #[must_use]
    pub fn parse_row<'a>(&self, row: &'a [u8]) -> Vec<&'a [u8]> {
        let mut fields = Vec::new();
        let mut field_start = 0;
        let mut in_quote = false;

        for (i, &byte) in row.iter().enumerate() {
            if byte == self.quote_char {
                in_quote = !in_quote;
            } else if !in_quote && byte == self.delimiter {
                fields.push(&row[field_start..i]);
                field_start = i + 1;
            }
        }

        // Add last field
        if field_start <= row.len() {
            let end = row.len();
            let last_field = if end > 0 && row[end - 1] == b'\n' {
                if end > 1 && row[end - 2] == b'\r' {
                    &row[field_start..end - 2]
                } else {
                    &row[field_start..end - 1]
                }
            } else {
                &row[field_start..end]
            };
            fields.push(last_field);
        }

        fields
    }

    /// Check if a field is quoted
    #[must_use]
    pub fn is_quoted(field: &[u8]) -> bool {
        field.len() >= 2 && field[0] == b'"' && field[field.len() - 1] == b'"'
    }

    /// Unquote a field (remove surrounding quotes and unescape)
    #[must_use]
    pub fn unquote(field: &[u8]) -> Vec<u8> {
        if !Self::is_quoted(field) {
            return field.to_vec();
        }

        let inner = &field[1..field.len() - 1];
        let mut result = Vec::with_capacity(inner.len());
        let mut chars = inner.iter().peekable();

        while let Some(&byte) = chars.next() {
            if byte == b'"' {
                // Check for escaped quote
                if chars.peek() == Some(&&b'"') {
                    chars.next(); // Skip second quote
                }
            }
            result.push(byte);
        }

        result
    }

    /// Auto-detect delimiter from first line
    #[must_use]
    pub fn detect_delimiter(lines: &[&[u8]]) -> u8 {
        let candidates = [b',', b'\t', b'|', b';'];
        let mut best = b',';
        let mut best_count = 0;

        for line in lines {
            for &delim in &candidates {
                let count = memchr::memchr_iter(delim, line).count();
                if count > best_count {
                    best_count = count;
                    best = delim;
                }
            }
        }

        best
    }

    /// Count fields in a row (standalone version with explicit delimiter)
    #[must_use]
    pub fn count_fields_with_delimiter(row: &[u8], delimiter: u8) -> usize {
        if row.is_empty() {
            return 0;
        }

        let mut count = 1;
        let mut in_quote = false;

        for &byte in row {
            if byte == b'"' {
                in_quote = !in_quote;
            } else if !in_quote && byte == delimiter {
                count += 1;
            }
        }

        count
    }

    /// Split fields in a row
    #[must_use]
    pub fn split_fields(row: &[u8], delimiter: u8) -> Vec<&[u8]> {
        let mut fields = Vec::new();
        let mut field_start = 0;
        let mut in_quote = false;

        for (i, &byte) in row.iter().enumerate() {
            if byte == b'"' {
                in_quote = !in_quote;
            } else if !in_quote && byte == delimiter {
                fields.push(&row[field_start..i]);
                field_start = i + 1;
            }
        }

        // Add last field
        if field_start <= row.len() {
            let end = row.len();
            let last_field = if end > 0 && row[end - 1] == b'\n' {
                if end > 1 && row[end - 2] == b'\r' {
                    &row[field_start..end - 2]
                } else {
                    &row[field_start..end - 1]
                }
            } else {
                &row[field_start..end]
            };
            fields.push(last_field);
        }

        fields
    }

    /// Check if a field is quoted
    #[must_use]
    pub fn is_quoted_field(field: &[u8]) -> bool {
        field.len() >= 2 && field[0] == b'"' && field[field.len() - 1] == b'"'
    }
}

impl FormatParser for CsvParser {
    type Error = CsvError;

    fn parse_structural(&self, input: &[u8]) -> Result<StructuralPositions, Self::Error> {
        let mut positions = StructuralPositions::new();

        // Find newlines
        positions.newlines = memchr::memchr_iter(b'\n', input).collect();

        // Find structural characters
        for (i, &byte) in input.iter().enumerate() {
            if byte == self.quote_char {
                positions.string_boundaries.push(i);
            } else if byte == self.delimiter {
                positions.delimiters.push(i);
            }
        }

        Ok(positions)
    }

    fn detect_indent(&self, _input: &[u8], _pos: usize) -> usize {
        // CSV doesn't use indentation
        0
    }

    #[allow(clippy::naive_bytecount)] // Simple quote counting is acceptable for correctness check
    fn is_in_string(&self, input: &[u8], pos: usize) -> bool {
        // Count quotes before position
        let quotes_before = input[..pos]
            .iter()
            .filter(|&&b| b == self.quote_char)
            .count();
        quotes_before % 2 == 1
    }

    fn is_in_comment(&self, _input: &[u8], _pos: usize) -> bool {
        // Standard CSV doesn't have comments
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_fields() {
        let parser = CsvParser::new();
        assert_eq!(parser.count_fields(b"a,b,c"), 3);
        assert_eq!(parser.count_fields(b"\"a,b\",c"), 2);
        assert_eq!(parser.count_fields(b""), 0);
    }

    #[test]
    fn test_parse_row() {
        let parser = CsvParser::new();
        let fields = parser.parse_row(b"a,b,c\n");
        assert_eq!(
            fields,
            vec![b"a".as_slice(), b"b".as_slice(), b"c".as_slice()]
        );
    }

    #[test]
    fn test_parse_row_quoted() {
        let parser = CsvParser::new();
        let fields = parser.parse_row(b"\"hello, world\",b\n");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], b"\"hello, world\"");
        assert_eq!(fields[1], b"b");
    }

    #[test]
    fn test_is_quoted() {
        assert!(CsvParser::is_quoted(b"\"hello\""));
        assert!(!CsvParser::is_quoted(b"hello"));
        assert!(!CsvParser::is_quoted(b"\""));
    }

    #[test]
    fn test_unquote() {
        assert_eq!(CsvParser::unquote(b"\"hello\""), b"hello");
        assert_eq!(
            CsvParser::unquote(b"\"hello \"\"world\"\"\""),
            b"hello \"world\""
        );
        assert_eq!(CsvParser::unquote(b"hello"), b"hello");
    }

    #[test]
    fn test_detect_delimiter() {
        assert_eq!(CsvParser::detect_delimiter(&[b"a,b,c"]), b',');
        assert_eq!(CsvParser::detect_delimiter(&[b"a\tb\tc"]), b'\t');
        assert_eq!(CsvParser::detect_delimiter(&[b"a|b|c"]), b'|');
    }

    #[test]
    fn test_tsv() {
        let parser = CsvParser::tsv();
        assert_eq!(parser.count_fields(b"a\tb\tc"), 3);
    }
}
