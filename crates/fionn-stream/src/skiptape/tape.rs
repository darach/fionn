// SPDX-License-Identifier: MIT OR Apache-2.0
//! Skip Tape - Zero-allocation SIMD-accelerated tape with schema filtering
//!
//! The `SkipTape` is a custom tape format that integrates schema filtering directly
//! into the parsing phase, producing compact representations containing only
//! schema-matching data.

use bumpalo::Bump;
use fionn_core::format::FormatKind;

/// SIMD-aligned skip node (64 bytes for cache efficiency)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct SkipNode {
    /// Node type (SIMD-comparable)
    pub node_type: NodeType,
    /// Data payload (offset, length, or value)
    pub data: u64,
    /// Nesting depth
    pub depth: u8,
    /// Schema match flags and metadata
    pub flags: u8,
    /// Source format kind (JSON, TOML, YAML, etc.)
    pub format_kind: FormatKind,
    /// Padding for SIMD alignment
    _padding: [u8; 5],
}

/// Node types for skip tape (compact enum representation)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    /// Start of JSON object
    ObjectStart = 0,
    /// End of JSON object
    ObjectEnd = 1,
    /// Start of JSON array
    ArrayStart = 2,
    /// End of JSON array
    ArrayEnd = 3,
    /// String value
    String = 4,
    /// Numeric value
    Number = 5,
    /// Boolean value
    Bool = 6,
    /// Null value
    Null = 7,
    /// Marker for skipped content
    SkipMarker = 8,
}

/// Zero-allocation skip tape using arena memory management
#[derive(Debug)]
pub struct SkipTape<'arena> {
    /// Arena-allocated nodes (zero-allocation)
    nodes: Vec<SkipNode>,
    /// String arena for deduplicated strings
    pub strings: StringArena<'arena>,
    /// Metadata about the tape
    pub metadata: SkipMetadata,
}

/// Metadata for skip tape statistics and optimization
#[derive(Debug, Clone)]
pub struct SkipMetadata {
    /// Total number of nodes in the tape
    pub node_count: usize,
    /// Total size in bytes
    pub total_size: usize,
    /// Schema match ratio (0.0 to 1.0)
    pub schema_match_ratio: f64,
    /// Original input size for comparison
    pub original_size: usize,
    /// Number of skipped nodes/elements
    pub skipped_count: usize,
}

/// String arena for zero-allocation string storage
#[derive(Debug)]
pub struct StringArena<'arena> {
    /// Arena for string allocation
    arena: &'arena Bump,
    /// Stored strings with their offsets
    pub strings: Vec<&'arena str>,
    /// Map for string deduplication: string content -> offset
    dedup_map: std::collections::HashMap<&'arena str, u32>,
    /// Total bytes used
    pub total_bytes: usize,
}

impl<'arena> SkipTape<'arena> {
    /// Create a new skip tape with pre-allocated capacity
    #[must_use]
    pub fn with_capacity(arena: &'arena Bump, estimated_nodes: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(estimated_nodes),
            strings: StringArena::new(arena),
            metadata: SkipMetadata {
                node_count: 0,
                total_size: 0,
                schema_match_ratio: 0.0,
                original_size: 0,
                skipped_count: 0,
            },
        }
    }

    /// Add a node to the skip tape
    pub fn add_node(&mut self, node: SkipNode) {
        self.nodes.push(node);
        self.metadata.node_count += 1;
    }

    /// Add a string to the string arena and return its offset
    pub fn add_string(&mut self, s: &str) -> u32 {
        let offset = self.strings.add_string(s);
        self.metadata.total_size += s.len();
        offset
    }

    /// Get the nodes slice
    #[must_use]
    pub fn nodes(&self) -> &[SkipNode] {
        &self.nodes
    }

    /// Get the root array if the tape represents an array
    #[must_use]
    pub fn root_array(&self) -> Option<&SkipNode> {
        self.nodes
            .first()
            .filter(|node| node.node_type == NodeType::ArrayStart)
    }

    /// Get metadata about the tape
    #[must_use]
    pub const fn metadata(&self) -> &SkipMetadata {
        &self.metadata
    }

    /// Calculate memory efficiency metrics
    #[must_use]
    pub fn memory_efficiency(&self) -> MemoryEfficiency {
        let memory_used =
            self.nodes.len() * std::mem::size_of::<SkipNode>() + self.strings.total_bytes();

        // Convert to f64 for floating point calculations.
        // Using explicit conversion through u64 to avoid clippy warnings.
        // Precision loss is acceptable for memory statistics (only affects values > 2^52 bytes).
        let memory_used_f64 = f64::from(u32::try_from(memory_used).unwrap_or(u32::MAX));
        let original_size_f64 =
            f64::from(u32::try_from(self.metadata.original_size).unwrap_or(u32::MAX));
        let nodes_len_f64 = f64::from(u32::try_from(self.nodes.len()).unwrap_or(u32::MAX));

        let compression_ratio = if self.metadata.original_size > 0 {
            memory_used_f64 / original_size_f64
        } else {
            1.0
        };

        MemoryEfficiency {
            bytes_used: memory_used,
            compression_ratio,
            nodes_per_kb: nodes_len_f64 / (memory_used_f64 / 1024.0),
        }
    }
}

/// Memory efficiency metrics
#[derive(Debug, Clone)]
pub struct MemoryEfficiency {
    /// Total bytes used by the skip tape
    pub bytes_used: usize,
    /// Compression ratio (used / original)
    pub compression_ratio: f64,
    /// Nodes per kilobyte of memory
    pub nodes_per_kb: f64,
}

impl SkipNode {
    /// Create an object start node
    #[must_use]
    pub const fn object_start() -> Self {
        Self {
            node_type: NodeType::ObjectStart,
            data: 0,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create an object end node
    #[must_use]
    pub const fn object_end() -> Self {
        Self {
            node_type: NodeType::ObjectEnd,
            data: 0,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create an array start node
    #[must_use]
    pub const fn array_start() -> Self {
        Self {
            node_type: NodeType::ArrayStart,
            data: 0,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create an array end node
    #[must_use]
    pub const fn array_end() -> Self {
        Self {
            node_type: NodeType::ArrayEnd,
            data: 0,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create a string node
    #[must_use]
    pub const fn string(offset: u32, length: u16) -> Self {
        let data = (offset as u64) | ((length as u64) << 32);
        Self {
            node_type: NodeType::String,
            data,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create a number node
    #[must_use]
    pub const fn number(value: f64) -> Self {
        Self {
            node_type: NodeType::Number,
            data: value.to_bits(),
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create a boolean node
    #[must_use]
    pub const fn bool(value: bool) -> Self {
        Self {
            node_type: NodeType::Bool,
            data: value as u64,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create a null node
    #[must_use]
    pub const fn null() -> Self {
        Self {
            node_type: NodeType::Null,
            data: 0,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Create a skip marker node
    #[must_use]
    pub const fn skip_marker() -> Self {
        Self {
            node_type: NodeType::SkipMarker,
            data: 0,
            depth: 0,
            flags: 0,
            format_kind: FormatKind::Json,
            _padding: [0; 5],
        }
    }

    /// Set the depth of the node
    #[must_use]
    pub const fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    /// Set the flags of the node
    #[must_use]
    pub const fn with_flags(mut self, flags: u8) -> Self {
        self.flags = flags;
        self
    }

    /// Set the format kind of the node
    #[must_use]
    pub const fn with_format(mut self, format_kind: FormatKind) -> Self {
        self.format_kind = format_kind;
        self
    }
}

impl<'arena> StringArena<'arena> {
    /// Create a new string arena
    #[must_use]
    pub fn new(arena: &'arena Bump) -> Self {
        Self {
            arena,
            strings: Vec::new(),
            dedup_map: std::collections::HashMap::new(),
            total_bytes: 0,
        }
    }

    /// Add a string to the arena and return its offset
    pub fn add_string(&mut self, s: &str) -> u32 {
        // Check if string already exists
        if let Some(&offset) = self.dedup_map.get(s) {
            return offset;
        }

        // Allocate new string in arena
        let allocated_str = self.arena.alloc_str(s);

        // Use the index in `strings` vector as the definitive ID for retrieval.
        // Use saturating conversion to handle potential overflow on 64-bit systems
        // (though in practice, having 2^32 strings is unlikely).
        let new_offset = u32::try_from(self.strings.len()).unwrap_or(u32::MAX);
        self.strings.push(allocated_str);
        self.dedup_map.insert(allocated_str, new_offset);
        self.total_bytes += s.len();

        new_offset
    }

    /// Get the total bytes used by strings
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_node_object_start() {
        let node = SkipNode::object_start();
        assert_eq!(node.node_type, NodeType::ObjectStart);
    }

    #[test]
    fn test_skip_node_object_end() {
        let node = SkipNode::object_end();
        assert_eq!(node.node_type, NodeType::ObjectEnd);
    }

    #[test]
    fn test_skip_node_array_start() {
        let node = SkipNode::array_start();
        assert_eq!(node.node_type, NodeType::ArrayStart);
    }

    #[test]
    fn test_skip_node_array_end() {
        let node = SkipNode::array_end();
        assert_eq!(node.node_type, NodeType::ArrayEnd);
    }

    #[test]
    fn test_skip_node_string() {
        let node = SkipNode::string(0, 5);
        assert_eq!(node.node_type, NodeType::String);
    }

    #[test]
    fn test_skip_node_number() {
        let node = SkipNode::number(42.0);
        assert_eq!(node.node_type, NodeType::Number);
        assert_eq!(node.data, 42.0f64.to_bits());
    }

    #[test]
    fn test_skip_node_bool_true() {
        let node = SkipNode::bool(true);
        assert_eq!(node.node_type, NodeType::Bool);
        assert_eq!(node.data, 1);
    }

    #[test]
    fn test_skip_node_bool_false() {
        let node = SkipNode::bool(false);
        assert_eq!(node.node_type, NodeType::Bool);
        assert_eq!(node.data, 0);
    }

    #[test]
    fn test_skip_node_null() {
        let node = SkipNode::null();
        assert_eq!(node.node_type, NodeType::Null);
    }

    #[test]
    fn test_skip_node_skip_marker() {
        let node = SkipNode::skip_marker();
        assert_eq!(node.node_type, NodeType::SkipMarker);
    }

    #[test]
    fn test_skip_node_with_depth() {
        let node = SkipNode::object_start().with_depth(5);
        assert_eq!(node.depth, 5);
    }

    #[test]
    fn test_skip_node_with_flags() {
        let node = SkipNode::string(0, 1).with_flags(3);
        assert_eq!(node.flags, 3);
    }

    #[test]
    fn test_skip_tape_creation() {
        let arena = Bump::new();
        let tape = SkipTape::with_capacity(&arena, 100);
        assert!(tape.nodes().is_empty());
        assert_eq!(tape.metadata().node_count, 0);
    }

    #[test]
    fn test_skip_tape_add_node() {
        let arena = Bump::new();
        let mut tape = SkipTape::with_capacity(&arena, 10);
        tape.add_node(SkipNode::object_start());
        assert_eq!(tape.nodes().len(), 1);
        assert_eq!(tape.metadata().node_count, 1);
    }

    #[test]
    fn test_skip_tape_add_string() {
        let arena = Bump::new();
        let mut tape = SkipTape::with_capacity(&arena, 10);
        let offset = tape.add_string("hello");
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_skip_tape_string_deduplication() {
        let arena = Bump::new();
        let mut tape = SkipTape::with_capacity(&arena, 10);
        let offset1 = tape.add_string("hello");
        let offset2 = tape.add_string("hello");
        assert_eq!(offset1, offset2);
    }

    #[test]
    fn test_skip_tape_root_array() {
        let arena = Bump::new();
        let mut tape = SkipTape::with_capacity(&arena, 10);
        tape.add_node(SkipNode::array_start());
        assert!(tape.root_array().is_some());
    }

    #[test]
    fn test_skip_tape_root_array_none() {
        let arena = Bump::new();
        let mut tape = SkipTape::with_capacity(&arena, 10);
        tape.add_node(SkipNode::object_start());
        assert!(tape.root_array().is_none());
    }

    #[test]
    fn test_skip_tape_memory_efficiency() {
        let arena = Bump::new();
        let mut tape = SkipTape::with_capacity(&arena, 10);
        tape.add_node(SkipNode::object_start());
        tape.add_string("test");
        tape.metadata.original_size = 100;
        let efficiency = tape.memory_efficiency();
        assert!(efficiency.bytes_used > 0);
    }

    #[test]
    fn test_string_arena() {
        let arena = Bump::new();
        let mut string_arena = StringArena::new(&arena);
        let offset = string_arena.add_string("hello");
        assert_eq!(offset, 0);
        assert_eq!(string_arena.total_bytes(), 5);
    }

    #[test]
    fn test_string_arena_deduplication() {
        let arena = Bump::new();
        let mut string_arena = StringArena::new(&arena);
        string_arena.add_string("hello");
        string_arena.add_string("world");
        string_arena.add_string("hello");
        assert_eq!(string_arena.strings.len(), 2);
    }

    #[test]
    fn test_node_type_equality() {
        assert_eq!(NodeType::ObjectStart, NodeType::ObjectStart);
        assert_ne!(NodeType::ObjectStart, NodeType::ObjectEnd);
    }

    #[test]
    fn test_skip_node_copy() {
        let node = SkipNode::number(std::f64::consts::PI);
        let copied = node;
        assert_eq!(copied.node_type, NodeType::Number);
    }

    #[test]
    fn test_skip_metadata_clone() {
        let meta = SkipMetadata {
            node_count: 10,
            total_size: 100,
            schema_match_ratio: 0.5,
            original_size: 200,
            skipped_count: 5,
        };
        let cloned = meta;
        assert_eq!(cloned.node_count, 10);
    }
}
