// SPDX-License-Identifier: MIT OR Apache-2.0
//! Unified Tape Format for Multi-Format DSON/CRDT Operations
//!
//! This module provides extended tape types that support all fionn formats
//! (JSON, YAML, TOML, CSV, ISON, TOON) with format-specific markers and
//! original syntax preservation for lossless round-trips.
//!
//! # Design Goals
//!
//! 1. **Format-Agnostic Operations**: All DSON operations work on unified tape
//! 2. **Lossless Round-Trips**: OriginalSyntax preserves format-specific features
//! 3. **Cross-Format CRDT**: Delta-CRDT operations across format boundaries
//! 4. **Schema-Guided Skip Parsing**: Efficient filtering during parsing

use bumpalo::Bump;
use fionn_core::format::FormatKind;
use std::collections::HashMap;

// =============================================================================
// Extended Node Types
// =============================================================================

/// Extended node types for unified multi-format tape
///
/// This enum extends the basic node types to include format-specific markers
/// that enable lossless round-trip transformations.
#[repr(u8)]
#[derive(Debug, Clone, PartialEq)]
pub enum ExtendedNodeType {
    // =========================================================================
    // Universal types (shared across all formats)
    // =========================================================================
    /// Start of an object/map
    ObjectStart = 0,
    /// End of an object/map
    ObjectEnd = 1,
    /// Start of an array/sequence
    ArrayStart = 2,
    /// End of an array/sequence
    ArrayEnd = 3,
    /// String value (index into string arena)
    String(u32) = 4,
    /// Numeric value (f64 bits)
    Number(f64) = 5,
    /// Boolean value
    Bool(bool) = 6,
    /// Null/nil/none value
    Null = 7,
    /// Marker for skipped content (schema filtering)
    SkipMarker = 8,
    /// Key in a key-value pair (index into string arena)
    Key(u32) = 9,

    // =========================================================================
    // YAML-specific markers
    // =========================================================================
    /// YAML document start (`---`)
    YamlDocumentStart = 20,
    /// YAML document end (`...`)
    YamlDocumentEnd = 21,
    /// YAML anchor definition (`&anchor`)
    YamlAnchor {
        /// Anchor name (index into string arena)
        name_idx: u32,
        /// Target node index (resolved after parsing)
        target_idx: u32,
    } = 22,
    /// YAML alias reference (`*alias`)
    YamlAlias {
        /// Alias target name (index into string arena)
        target_name_idx: u32,
    } = 23,
    /// YAML tag (`!tag`)
    YamlTag {
        /// Tag string (index into string arena)
        tag_idx: u32,
    } = 24,

    // =========================================================================
    // TOML-specific markers
    // =========================================================================
    /// TOML table header (`[table]`)
    TomlTableStart {
        /// Table path (index into string arena)
        path_idx: u32,
    } = 30,
    /// TOML inline table (`{ key = value }`)
    TomlInlineTableStart = 31,
    /// TOML array of tables (`[[array.table]]`)
    TomlArrayTableStart {
        /// Table path (index into string arena)
        path_idx: u32,
    } = 32,
    /// TOML datetime value
    TomlDatetime {
        /// ISO 8601 string (index into string arena)
        datetime_idx: u32,
    } = 33,

    // =========================================================================
    // CSV-specific markers
    // =========================================================================
    /// CSV header row marker
    CsvHeaderRow = 40,
    /// CSV data row start
    CsvRowStart {
        /// Row index (0-based, excluding header)
        row_idx: u32,
    } = 41,
    /// CSV data row end
    CsvRowEnd = 42,
    /// CSV field (column value)
    CsvField {
        /// Column index (0-based)
        col_idx: u32,
        /// Value (index into string arena)
        value_idx: u32,
    } = 43,

    // =========================================================================
    // ISON-specific markers
    // =========================================================================
    /// ISON table block (`table.name|field1|field2|...`)
    IsonTableBlock {
        /// Table name (index into string arena)
        name_idx: u32,
    } = 50,
    /// ISON object block
    IsonObjectBlock = 51,
    /// ISON reference (`:type:id`)
    IsonReference {
        /// Reference kind
        kind: IsonRefKind,
        /// Type (index into string arena)
        type_idx: u32,
        /// ID (index into string arena)
        id_idx: u32,
    } = 52,
    /// ISON typed field
    IsonTypedField {
        /// Field type (int, string, bool, etc.)
        field_type: IsonFieldType,
    } = 53,

    // =========================================================================
    // TOON-specific markers
    // =========================================================================
    /// TOON tabular array header
    ToonTabularArrayHeader {
        /// Header text (index into string arena)
        header_idx: u32,
    } = 60,
    /// TOON folded key (`a.b.c:`)
    ToonFoldedKey {
        /// Full path (index into string arena)
        path_idx: u32,
    } = 61,
    /// TOON comment
    ToonComment {
        /// Comment text (index into string arena)
        text_idx: u32,
    } = 62,
}

/// ISON reference kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsonRefKind {
    /// Forward reference
    Forward,
    /// Back reference
    Back,
    /// Named reference
    Named,
}

/// ISON field type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsonFieldType {
    /// Integer type
    Int,
    /// String type
    String,
    /// Boolean type
    Bool,
    /// Floating point type
    Float,
    /// Null type
    Null,
    /// Date type
    Date,
    /// Datetime type
    Datetime,
    /// Binary type
    Binary,
}

// =============================================================================
// Original Syntax Preservation
// =============================================================================

/// Preserves original syntax for lossless round-trips
///
/// When converting between formats, this enum stores format-specific
/// information that would otherwise be lost, enabling exact reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginalSyntax {
    // =========================================================================
    // YAML original syntax
    // =========================================================================
    /// YAML anchor with name
    YamlAnchor {
        /// Anchor name (without `&`)
        name: String,
    },
    /// YAML alias with target
    YamlAlias {
        /// Target anchor name (without `*`)
        target: String,
    },
    /// YAML flow style (inline `[a, b]` or `{a: b}`)
    YamlFlowStyle,
    /// YAML block style (multi-line)
    YamlBlockStyle,
    /// YAML literal block scalar (`|`)
    YamlLiteralBlock,
    /// YAML folded block scalar (`>`)
    YamlFoldedBlock,
    /// YAML tag
    YamlTag {
        /// Tag string
        tag: String,
    },

    // =========================================================================
    // TOML original syntax
    // =========================================================================
    /// TOML dotted key (`a.b.c = value`)
    TomlDottedKey {
        /// Full dotted path
        full_key: String,
    },
    /// TOML triple-quoted string (`"""..."""`)
    TomlTripleQuotedString,
    /// TOML literal string (`'...'`)
    TomlLiteralString,
    /// TOML inline table `{ a = 1, b = 2 }`
    TomlInlineTable,
    /// TOML array of tables `[[table]]`
    TomlArrayTable,

    // =========================================================================
    // CSV original syntax
    // =========================================================================
    /// CSV quoted value
    CsvQuotedValue {
        /// Whether original had quotes
        has_quotes: bool,
    },
    /// CSV delimiter used
    CsvDelimiter {
        /// Delimiter character
        delimiter: char,
    },
    /// CSV newline style
    CsvNewlineStyle {
        /// Newline style (LF, CRLF, CR)
        style: NewlineStyle,
    },
    /// CSV escape character
    CsvEscapeChar {
        /// Escape character
        escape: char,
    },

    // =========================================================================
    // ISON original syntax
    // =========================================================================
    /// ISON reference
    IsonReference {
        /// Reference kind (forward, back, named)
        kind: IsonRefKind,
        /// Type name
        type_name: String,
        /// Reference ID
        id: String,
    },
    /// ISON schema header
    IsonSchemaHeader {
        /// Fields with their types
        fields: Vec<(String, IsonFieldType)>,
    },

    // =========================================================================
    // TOON original syntax
    // =========================================================================
    /// TOON folded key
    ToonFoldedKey {
        /// Full dotted path
        path: String,
    },
    /// TOON array header
    ToonArrayHeader {
        /// Header text
        header_text: String,
    },
    /// TOON comment
    ToonComment {
        /// Comment text (without `#`)
        text: String,
    },
}

/// Newline style for CSV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewlineStyle {
    /// Unix LF (`\n`)
    Lf,
    /// Windows CRLF (`\r\n`)
    CrLf,
    /// Old Mac CR (`\r`)
    Cr,
}

// =============================================================================
// Unified Tape Node
// =============================================================================

/// SIMD-aligned unified tape node (64 bytes for cache efficiency)
#[repr(C, align(64))]
#[derive(Debug, Clone)]
pub struct UnifiedNode {
    /// Extended node type
    pub node_type: ExtendedNodeType,
    /// Nesting depth
    pub depth: u8,
    /// Schema match flags
    pub flags: NodeFlags,
    /// Source format kind
    pub format_kind: FormatKind,
    /// Original syntax index (into `OriginalSyntax` arena, or `u32::MAX` for none)
    pub original_syntax_idx: u32,
    /// Reserved for future use / padding
    _padding: [u8; 3],
}

/// Node flags for metadata
#[derive(Debug, Clone, Copy, Default)]
pub struct NodeFlags(u8);

impl NodeFlags {
    /// Node matched schema filter
    pub const SCHEMA_MATCHED: u8 = 0b0000_0001;
    /// Node was modified by operation
    pub const MODIFIED: u8 = 0b0000_0010;
    /// Node has original syntax preserved
    pub const HAS_ORIGINAL_SYNTAX: u8 = 0b0000_0100;
    /// Node is tombstoned (CRDT delete)
    pub const TOMBSTONED: u8 = 0b0000_1000;

    /// Create default flags
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    /// Check if flag is set
    #[must_use]
    pub const fn has(&self, flag: u8) -> bool {
        (self.0 & flag) != 0
    }

    /// Set a flag
    pub const fn set(&mut self, flag: u8) {
        self.0 |= flag;
    }

    /// Clear a flag
    pub const fn clear(&mut self, flag: u8) {
        self.0 &= !flag;
    }
}

impl UnifiedNode {
    /// Create a new unified node
    #[must_use]
    pub const fn new(node_type: ExtendedNodeType, format_kind: FormatKind) -> Self {
        Self {
            node_type,
            depth: 0,
            flags: NodeFlags::new(),
            format_kind,
            original_syntax_idx: u32::MAX,
            _padding: [0; 3],
        }
    }

    /// Set the depth
    #[must_use]
    pub const fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    /// Set original syntax index
    #[must_use]
    pub const fn with_original_syntax(mut self, idx: u32) -> Self {
        self.original_syntax_idx = idx;
        self.flags.0 |= NodeFlags::HAS_ORIGINAL_SYNTAX;
        self
    }

    /// Check if this node has original syntax
    #[must_use]
    pub const fn has_original_syntax(&self) -> bool {
        self.original_syntax_idx != u32::MAX
    }
}

// =============================================================================
// Unified Tape
// =============================================================================

/// Zero-allocation unified tape using arena memory management
#[derive(Debug)]
pub struct UnifiedTape<'arena> {
    /// Arena-allocated nodes
    nodes: Vec<UnifiedNode>,
    /// String arena for deduplicated strings
    pub strings: UnifiedStringArena<'arena>,
    /// Original syntax entries
    pub original_syntax: Vec<OriginalSyntax>,
    /// Tape metadata
    pub metadata: UnifiedTapeMetadata,
}

/// String arena for unified tape
#[derive(Debug)]
pub struct UnifiedStringArena<'arena> {
    /// Arena for string allocation
    arena: &'arena Bump,
    /// Stored strings
    pub strings: Vec<&'arena str>,
    /// Deduplication map
    dedup_map: HashMap<&'arena str, u32>,
    /// Total bytes used
    pub total_bytes: usize,
}

/// Metadata for unified tape
#[derive(Debug, Clone, Default)]
pub struct UnifiedTapeMetadata {
    /// Source format
    pub source_format: FormatKind,
    /// Total node count
    pub node_count: usize,
    /// Schema match ratio
    pub schema_match_ratio: f64,
    /// Original input size
    pub original_size: usize,
    /// Skipped node count
    pub skipped_count: usize,
    /// Count of nodes with original syntax preserved
    pub preserved_syntax_count: usize,
}

impl<'arena> UnifiedTape<'arena> {
    /// Create a new unified tape
    #[must_use]
    pub fn new(arena: &'arena Bump, format: FormatKind) -> Self {
        Self::with_capacity(arena, format, 64)
    }

    /// Create a new unified tape with capacity
    #[must_use]
    pub fn with_capacity(arena: &'arena Bump, format: FormatKind, capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(capacity),
            strings: UnifiedStringArena::new(arena),
            original_syntax: Vec::new(),
            metadata: UnifiedTapeMetadata {
                source_format: format,
                ..Default::default()
            },
        }
    }

    /// Add a node to the tape
    pub fn add_node(&mut self, node: UnifiedNode) {
        self.nodes.push(node);
        self.metadata.node_count += 1;
    }

    /// Add a string to the arena and return its index
    pub fn add_string(&mut self, s: &str) -> u32 {
        self.strings.add_string(s)
    }

    /// Add original syntax and return its index
    pub fn add_original_syntax(&mut self, syntax: OriginalSyntax) -> u32 {
        let idx = u32::try_from(self.original_syntax.len()).unwrap_or(u32::MAX);
        self.original_syntax.push(syntax);
        self.metadata.preserved_syntax_count += 1;
        idx
    }

    /// Get the nodes slice
    #[must_use]
    pub fn nodes(&self) -> &[UnifiedNode] {
        &self.nodes
    }

    /// Get string by index
    #[must_use]
    pub fn get_string(&self, idx: u32) -> Option<&str> {
        self.strings.strings.get(idx as usize).copied()
    }

    /// Get original syntax by index
    #[must_use]
    pub fn get_original_syntax(&self, idx: u32) -> Option<&OriginalSyntax> {
        self.original_syntax.get(idx as usize)
    }

    /// Get metadata
    #[must_use]
    pub const fn metadata(&self) -> &UnifiedTapeMetadata {
        &self.metadata
    }
}

impl<'arena> UnifiedStringArena<'arena> {
    /// Create a new string arena
    #[must_use]
    pub fn new(arena: &'arena Bump) -> Self {
        Self {
            arena,
            strings: Vec::new(),
            dedup_map: HashMap::new(),
            total_bytes: 0,
        }
    }

    /// Add a string to the arena (with deduplication)
    pub fn add_string(&mut self, s: &str) -> u32 {
        // Check for existing
        if let Some(&idx) = self.dedup_map.get(s) {
            return idx;
        }

        // Allocate new
        let allocated = self.arena.alloc_str(s);
        let idx = u32::try_from(self.strings.len()).unwrap_or(u32::MAX);
        self.strings.push(allocated);
        self.dedup_map.insert(allocated, idx);
        self.total_bytes += s.len();
        idx
    }

    /// Get total bytes
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

// =============================================================================
// Tape Segment (for batch results)
// =============================================================================

/// A segment of unified tape representing a single document/line
///
/// This replaces `Vec<String>` in batch results, preserving format information.
#[derive(Debug)]
pub struct TapeSegment<'arena> {
    /// Start index in parent tape
    pub start_idx: usize,
    /// End index in parent tape (exclusive)
    pub end_idx: usize,
    /// Source format
    pub format: FormatKind,
    /// Reference to parent tape
    tape: &'arena UnifiedTape<'arena>,
}

impl<'arena> TapeSegment<'arena> {
    /// Create a new tape segment
    #[must_use]
    pub const fn new(tape: &'arena UnifiedTape<'arena>, start_idx: usize, end_idx: usize) -> Self {
        Self {
            start_idx,
            end_idx,
            format: tape.metadata.source_format,
            tape,
        }
    }

    /// Get the nodes in this segment
    #[must_use]
    pub fn nodes(&self) -> &[UnifiedNode] {
        &self.tape.nodes()[self.start_idx..self.end_idx]
    }

    /// Get string by index from parent tape
    #[must_use]
    pub fn get_string(&self, idx: u32) -> Option<&str> {
        self.tape.get_string(idx)
    }

    /// Get original syntax by index from parent tape
    #[must_use]
    pub fn get_original_syntax(&self, idx: u32) -> Option<&OriginalSyntax> {
        self.tape.get_original_syntax(idx)
    }

    /// Get node count in segment
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end_idx - self.start_idx
    }

    /// Check if segment is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start_idx >= self.end_idx
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_flags() {
        let mut flags = NodeFlags::new();
        assert!(!flags.has(NodeFlags::SCHEMA_MATCHED));

        flags.set(NodeFlags::SCHEMA_MATCHED);
        assert!(flags.has(NodeFlags::SCHEMA_MATCHED));

        flags.clear(NodeFlags::SCHEMA_MATCHED);
        assert!(!flags.has(NodeFlags::SCHEMA_MATCHED));
    }

    #[test]
    fn test_unified_node() {
        let node = UnifiedNode::new(ExtendedNodeType::ObjectStart, FormatKind::Json)
            .with_depth(2)
            .with_original_syntax(5);

        assert_eq!(node.depth, 2);
        assert!(node.has_original_syntax());
        assert_eq!(node.original_syntax_idx, 5);
    }

    #[test]
    fn test_unified_tape_creation() {
        let arena = Bump::new();
        let tape = UnifiedTape::new(&arena, FormatKind::Json);

        assert_eq!(tape.metadata().source_format, FormatKind::Json);
        assert_eq!(tape.metadata().node_count, 0);
    }

    #[test]
    fn test_unified_tape_add_node() {
        let arena = Bump::new();
        let mut tape = UnifiedTape::new(&arena, FormatKind::Json);

        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::ObjectStart,
            FormatKind::Json,
        ));
        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::ObjectEnd,
            FormatKind::Json,
        ));

        assert_eq!(tape.nodes().len(), 2);
        assert_eq!(tape.metadata().node_count, 2);
    }

    #[test]
    fn test_string_arena_deduplication() {
        let arena = Bump::new();
        let mut tape = UnifiedTape::new(&arena, FormatKind::Json);

        let idx1 = tape.add_string("hello");
        let idx2 = tape.add_string("world");
        let idx3 = tape.add_string("hello"); // duplicate

        assert_eq!(idx1, idx3); // same string = same index
        assert_ne!(idx1, idx2);
    }

    #[test]
    fn test_original_syntax() {
        let arena = Bump::new();
        let mut tape = UnifiedTape::new(&arena, FormatKind::Json);

        // Use CSV delimiter syntax which is always available
        let syntax_idx = tape.add_original_syntax(OriginalSyntax::CsvDelimiter { delimiter: ',' });

        let node = UnifiedNode::new(
            ExtendedNodeType::CsvField {
                col_idx: 0,
                value_idx: tape.add_string("test"),
            },
            FormatKind::Json,
        )
        .with_original_syntax(syntax_idx);

        tape.add_node(node);

        assert!(tape.nodes()[0].has_original_syntax());
        let syntax = tape.get_original_syntax(syntax_idx);
        assert!(matches!(syntax, Some(OriginalSyntax::CsvDelimiter { .. })));
    }

    #[test]
    fn test_extended_node_type_yaml() {
        let node_type = ExtendedNodeType::YamlAnchor {
            name_idx: 0,
            target_idx: 1,
        };
        assert!(matches!(node_type, ExtendedNodeType::YamlAnchor { .. }));
    }

    #[test]
    fn test_extended_node_type_toml() {
        let node_type = ExtendedNodeType::TomlTableStart { path_idx: 0 };
        assert!(matches!(node_type, ExtendedNodeType::TomlTableStart { .. }));
    }

    #[test]
    fn test_extended_node_type_csv() {
        let node_type = ExtendedNodeType::CsvField {
            col_idx: 0,
            value_idx: 1,
        };
        assert!(matches!(node_type, ExtendedNodeType::CsvField { .. }));
    }

    #[test]
    fn test_extended_node_type_ison() {
        let node_type = ExtendedNodeType::IsonReference {
            kind: IsonRefKind::Forward,
            type_idx: 0,
            id_idx: 1,
        };
        assert!(matches!(node_type, ExtendedNodeType::IsonReference { .. }));
    }

    #[test]
    fn test_extended_node_type_toon() {
        let node_type = ExtendedNodeType::ToonFoldedKey { path_idx: 0 };
        assert!(matches!(node_type, ExtendedNodeType::ToonFoldedKey { .. }));
    }
}
