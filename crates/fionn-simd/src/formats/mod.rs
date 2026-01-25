// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format-specific SIMD parsers
//!
//! This module provides SIMD-accelerated parsers for various data formats:
//!
//! - `yaml` - YAML parser with indentation and anchor detection
//! - `toml` - TOML parser with section and inline table detection
//! - `csv` - CSV parser with delimiter and quote detection
//! - `ison` - ISON parser for LLM-optimized data interchange
//! - `toon` - TOON parser for token-oriented notation
//!
//! Each parser is feature-gated and provides SIMD-accelerated structural
//! character detection for skip tape generation.

#[cfg(feature = "yaml")]
pub mod yaml;

#[cfg(feature = "toml")]
pub mod toml;

#[cfg(feature = "csv")]
pub mod csv;

#[cfg(feature = "ison")]
pub mod ison;

#[cfg(feature = "toon")]
pub mod toon;

// Re-exports
#[cfg(feature = "yaml")]
pub use yaml::YamlParser;

#[cfg(feature = "toml")]
pub use toml::TomlParser;

#[cfg(feature = "csv")]
pub use csv::CsvParser;

#[cfg(feature = "ison")]
pub use ison::{IsonParser, IsonlParsedLine};

#[cfg(feature = "toon")]
pub use toon::ToonParser;

/// Common trait for format parsers
pub trait FormatParser {
    /// The error type for this parser
    type Error;

    /// Parse a chunk of input and return structural positions
    ///
    /// Returns positions of structural characters relevant to the format.
    ///
    /// # Errors
    ///
    /// Returns an error if the input contains invalid format-specific syntax.
    fn parse_structural(&self, input: &[u8]) -> Result<StructuralPositions, Self::Error>;

    /// Detect the indentation level at a given position
    fn detect_indent(&self, input: &[u8], pos: usize) -> usize;

    /// Check if position is inside a string literal
    fn is_in_string(&self, input: &[u8], pos: usize) -> bool;

    /// Check if position is inside a comment
    fn is_in_comment(&self, input: &[u8], pos: usize) -> bool;
}

/// Structural positions detected by SIMD scanning
#[derive(Debug, Clone, Default)]
pub struct StructuralPositions {
    /// Positions of structural delimiters (format-specific)
    pub delimiters: Vec<usize>,
    /// Positions of string boundaries (quotes)
    pub string_boundaries: Vec<usize>,
    /// Positions of comment starts
    pub comment_starts: Vec<usize>,
    /// Positions of newlines
    pub newlines: Vec<usize>,
    /// Positions of escape characters
    pub escapes: Vec<usize>,
}

impl StructuralPositions {
    /// Create new empty structural positions
    #[must_use]
    pub const fn new() -> Self {
        Self {
            delimiters: Vec::new(),
            string_boundaries: Vec::new(),
            comment_starts: Vec::new(),
            newlines: Vec::new(),
            escapes: Vec::new(),
        }
    }

    /// Check if any positions were found
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::is_empty() is not const
    pub fn is_empty(&self) -> bool {
        self.delimiters.is_empty()
            && self.string_boundaries.is_empty()
            && self.comment_starts.is_empty()
            && self.newlines.is_empty()
            && self.escapes.is_empty()
    }
}

/// SIMD mask operations for 64-byte chunks
#[derive(Debug, Clone, Copy, Default)]
pub struct ChunkMask {
    /// Bitmask for string regions (1 = in string)
    pub string_mask: u64,
    /// Bitmask for comment regions (1 = in comment)
    pub comment_mask: u64,
    /// Bitmask for escape sequences (1 = escaped)
    pub escape_mask: u64,
    /// Bitmask for structural characters
    pub structural_mask: u64,
}

impl ChunkMask {
    /// Create a new empty chunk mask
    #[must_use]
    pub const fn new() -> Self {
        Self {
            string_mask: 0,
            comment_mask: 0,
            escape_mask: 0,
            structural_mask: 0,
        }
    }

    /// Check if position is in a string
    #[must_use]
    pub const fn is_in_string(self, pos: usize) -> bool {
        (self.string_mask & (1 << pos)) != 0
    }

    /// Check if position is in a comment
    #[must_use]
    pub const fn is_in_comment(self, pos: usize) -> bool {
        (self.comment_mask & (1 << pos)) != 0
    }

    /// Check if position is escaped
    #[must_use]
    pub const fn is_escaped(self, pos: usize) -> bool {
        (self.escape_mask & (1 << pos)) != 0
    }

    /// Check if position is structural
    #[must_use]
    pub const fn is_structural(self, pos: usize) -> bool {
        (self.structural_mask & (1 << pos)) != 0
    }

    /// Get structural positions not in strings or comments
    #[must_use]
    pub const fn active_structural(self) -> u64 {
        self.structural_mask & !self.string_mask & !self.comment_mask
    }
}
