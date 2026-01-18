// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format-agnostic tape traversal abstraction
//!
//! This module provides the [`TapeSource`] trait for format-agnostic tape traversal,
//! enabling gron, diff, and other operations to work with any parsed format
//! (JSON, YAML, TOML, CSV, ISON, TOON).
//!
//! # Key Types
//!
//! - [`TapeSource`] - Trait for format-agnostic tape traversal
//! - [`TapeValue`] - Common value representation across all formats
//! - [`TapeNodeKind`] - Node type classification
//! - [`TapeNodeRef`] - Reference to a tape node with metadata
//!
//! # Example
//!
//! ```ignore
//! use fionn_core::tape_source::{TapeSource, TapeNodeKind};
//!
//! fn process_tape<T: TapeSource>(tape: &T) {
//!     for i in 0..tape.len() {
//!         if let Some(node) = tape.node_at(i) {
//!             match node.kind {
//!                 TapeNodeKind::ObjectStart { count } => println!("Object with {} fields", count),
//!                 TapeNodeKind::Value => println!("Value: {:?}", node.value),
//!                 _ => {}
//!             }
//!         }
//!     }
//! }
//! ```

use crate::Result;
use crate::format::FormatKind;
use std::borrow::Cow;
use std::fmt;

// ============================================================================
// TapeValue - Common value representation
// ============================================================================

/// Common value representation for all tape formats
///
/// This enum provides a unified way to represent scalar and raw values
/// across different data formats. It uses `Cow<'a, str>` for strings
/// to support both borrowed and owned data.
#[derive(Debug, Clone, PartialEq)]
pub enum TapeValue<'a> {
    /// Null/nil/none value
    Null,
    /// Boolean value
    Bool(bool),
    /// 64-bit signed integer
    Int(i64),
    /// 64-bit floating point
    Float(f64),
    /// String value (borrowed or owned)
    String(Cow<'a, str>),
    /// Raw number string (preserves original representation)
    RawNumber(Cow<'a, str>),
}

impl TapeValue<'_> {
    /// Check if this value is null
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Check if this value is a boolean
    #[must_use]
    pub const fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Check if this value is numeric (int, float, or raw number)
    #[must_use]
    pub const fn is_number(&self) -> bool {
        matches!(self, Self::Int(_) | Self::Float(_) | Self::RawNumber(_))
    }

    /// Check if this value is a string
    #[must_use]
    pub const fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Get as boolean if this is a Bool
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get as i64 if this is an Int
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as f64 if this is a Float
    #[must_use]
    pub const fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as string reference if this is a String
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Convert to owned version (static lifetime)
    #[must_use]
    pub fn into_owned(self) -> TapeValue<'static> {
        match self {
            Self::Null => TapeValue::Null,
            Self::Bool(b) => TapeValue::Bool(b),
            Self::Int(n) => TapeValue::Int(n),
            Self::Float(n) => TapeValue::Float(n),
            Self::String(s) => TapeValue::String(Cow::Owned(s.into_owned())),
            Self::RawNumber(s) => TapeValue::RawNumber(Cow::Owned(s.into_owned())),
        }
    }

    /// Format value for display (gron-style output)
    #[must_use]
    pub fn format_for_output(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(true) => "true".to_string(),
            Self::Bool(false) => "false".to_string(),
            Self::Int(n) => n.to_string(),
            Self::Float(n) => {
                // JSON doesn't support NaN or Infinity, map to null
                if n.is_nan() || n.is_infinite() {
                    "null".to_string()
                } else {
                    n.to_string()
                }
            }
            Self::String(s) => format!("\"{}\"", escape_json_string(s)),
            Self::RawNumber(s) => s.to_string(),
        }
    }
}

impl fmt::Display for TapeValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_for_output())
    }
}

// ============================================================================
// TapeNodeKind - Node type classification
// ============================================================================

/// Node type classification for tape traversal
///
/// Provides a uniform way to identify node types across all tape formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TapeNodeKind {
    /// Start of an object/map with field count
    ObjectStart {
        /// Number of key-value pairs in the object
        count: usize,
    },
    /// End of an object/map
    ObjectEnd,
    /// Start of an array/sequence with element count
    ArrayStart {
        /// Number of elements in the array
        count: usize,
    },
    /// End of an array/sequence
    ArrayEnd,
    /// Key in a key-value pair
    Key,
    /// Scalar value
    Value,
}

impl TapeNodeKind {
    /// Check if this is a container start node
    #[must_use]
    pub const fn is_container_start(&self) -> bool {
        matches!(self, Self::ObjectStart { .. } | Self::ArrayStart { .. })
    }

    /// Check if this is a container end node
    #[must_use]
    pub const fn is_container_end(&self) -> bool {
        matches!(self, Self::ObjectEnd | Self::ArrayEnd)
    }

    /// Check if this is an object-related node
    #[must_use]
    pub const fn is_object_related(&self) -> bool {
        matches!(self, Self::ObjectStart { .. } | Self::ObjectEnd | Self::Key)
    }

    /// Check if this is an array-related node
    #[must_use]
    pub const fn is_array_related(&self) -> bool {
        matches!(self, Self::ArrayStart { .. } | Self::ArrayEnd)
    }

    /// Get the element count for container nodes
    #[must_use]
    pub const fn element_count(&self) -> Option<usize> {
        match self {
            Self::ObjectStart { count } | Self::ArrayStart { count } => Some(*count),
            _ => None,
        }
    }
}

// ============================================================================
// TapeNodeRef - Reference to a tape node
// ============================================================================

/// Reference to a tape node with metadata
///
/// Returned by [`TapeSource::node_at`] to provide information about a node
/// without copying the underlying data.
#[derive(Debug, Clone)]
pub struct TapeNodeRef<'a> {
    /// The kind of this node
    pub kind: TapeNodeKind,
    /// The value if this is a Value or Key node
    pub value: Option<TapeValue<'a>>,
    /// The source format of the tape
    pub format: FormatKind,
}

impl<'a> TapeNodeRef<'a> {
    /// Create a new node reference
    #[must_use]
    pub const fn new(kind: TapeNodeKind, value: Option<TapeValue<'a>>, format: FormatKind) -> Self {
        Self {
            kind,
            value,
            format,
        }
    }

    /// Create an object start node
    #[must_use]
    pub const fn object_start(count: usize, format: FormatKind) -> Self {
        Self {
            kind: TapeNodeKind::ObjectStart { count },
            value: None,
            format,
        }
    }

    /// Create an object end node
    #[must_use]
    pub const fn object_end(format: FormatKind) -> Self {
        Self {
            kind: TapeNodeKind::ObjectEnd,
            value: None,
            format,
        }
    }

    /// Create an array start node
    #[must_use]
    pub const fn array_start(count: usize, format: FormatKind) -> Self {
        Self {
            kind: TapeNodeKind::ArrayStart { count },
            value: None,
            format,
        }
    }

    /// Create an array end node
    #[must_use]
    pub const fn array_end(format: FormatKind) -> Self {
        Self {
            kind: TapeNodeKind::ArrayEnd,
            value: None,
            format,
        }
    }

    /// Create a key node
    #[must_use]
    pub const fn key(name: Cow<'a, str>, format: FormatKind) -> Self {
        Self {
            kind: TapeNodeKind::Key,
            value: Some(TapeValue::String(name)),
            format,
        }
    }

    /// Create a value node
    #[must_use]
    pub const fn value(val: TapeValue<'a>, format: FormatKind) -> Self {
        Self {
            kind: TapeNodeKind::Value,
            value: Some(val),
            format,
        }
    }
}

// ============================================================================
// TapeSource Trait
// ============================================================================

/// Trait for format-agnostic tape traversal
///
/// This trait provides a common interface for traversing parsed data tapes
/// from any format (JSON, YAML, TOML, CSV, ISON, TOON). It enables operations
/// like gron, diff, and query to work uniformly across all formats.
///
/// # Implementation Notes
///
/// Implementors should provide efficient implementations for:
/// - `node_at` - O(1) random access is ideal
/// - `skip_value` - Skip nested structures efficiently
/// - `resolve_path` - Path-based navigation
///
/// # Example Implementation
///
/// ```ignore
/// impl TapeSource for MyTape {
///     fn format(&self) -> FormatKind { FormatKind::Json }
///     fn len(&self) -> usize { self.nodes.len() }
///     fn node_at(&self, index: usize) -> Option<TapeNodeRef<'_>> {
///         self.nodes.get(index).map(|n| convert_to_ref(n))
///     }
///     // ... other methods
/// }
/// ```
pub trait TapeSource {
    /// Get the source format of this tape
    fn format(&self) -> FormatKind;

    /// Get the total number of nodes in the tape
    fn len(&self) -> usize;

    /// Check if the tape is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read a node at the given index
    ///
    /// Returns `None` if the index is out of bounds.
    fn node_at(&self, index: usize) -> Option<TapeNodeRef<'_>>;

    /// Get the value at index if it's a value node
    ///
    /// Convenience method that extracts just the value from a Value node.
    fn value_at(&self, index: usize) -> Option<TapeValue<'_>> {
        self.node_at(index).and_then(|n| {
            if matches!(n.kind, TapeNodeKind::Value) {
                n.value
            } else {
                None
            }
        })
    }

    /// Get the key string at index if it's a key node
    ///
    /// Convenience method that extracts the key name from a Key node.
    fn key_at(&self, index: usize) -> Option<Cow<'_, str>> {
        self.node_at(index).and_then(|n| {
            if matches!(n.kind, TapeNodeKind::Key) {
                n.value.and_then(|v| {
                    if let TapeValue::String(s) = v {
                        Some(s)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
    }

    /// Skip to the end of a value starting at the given index
    ///
    /// For scalar values, returns `start_index + 1`.
    /// For containers, returns the index after the closing node.
    ///
    /// # Errors
    ///
    /// Returns an error if the index is invalid or the tape structure is malformed.
    fn skip_value(&self, start_index: usize) -> Result<usize>;

    /// Resolve a JSON-pointer-style path to a tape index
    ///
    /// Returns `Ok(Some(index))` if the path exists, `Ok(None)` if not found,
    /// or an error if the path is invalid.
    ///
    /// # Errors
    ///
    /// Returns an error if the path syntax is invalid.
    fn resolve_path(&self, path: &str) -> Result<Option<usize>>;

    /// Create an iterator over all nodes
    fn iter(&self) -> TapeIterator<'_, Self>
    where
        Self: Sized,
    {
        TapeIterator::new(self)
    }

    /// Get an iterator that yields (path, node) pairs
    ///
    /// Useful for gron-like operations that need path context.
    fn path_iter(&self) -> PathIterator<'_, Self>
    where
        Self: Sized,
    {
        PathIterator::new(self)
    }
}

// ============================================================================
// Iterators
// ============================================================================

/// Iterator over tape nodes
pub struct TapeIterator<'a, T: TapeSource> {
    tape: &'a T,
    index: usize,
}

impl<'a, T: TapeSource> TapeIterator<'a, T> {
    /// Create a new iterator starting at index 0
    #[must_use]
    pub const fn new(tape: &'a T) -> Self {
        Self { tape, index: 0 }
    }
}

impl<'a, T: TapeSource> Iterator for TapeIterator<'a, T> {
    type Item = TapeNodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.tape.len() {
            return None;
        }
        let node = self.tape.node_at(self.index);
        self.index += 1;
        node
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.tape.len().saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl<T: TapeSource> ExactSizeIterator for TapeIterator<'_, T> {}

/// Path component for path iteration
#[derive(Debug, Clone)]
pub enum PathComponent {
    /// Object field name
    Field(String),
    /// Array index
    Index(usize),
}

impl PathComponent {
    /// Format as path segment
    #[must_use]
    pub fn as_path_segment(&self) -> String {
        match self {
            Self::Field(name) => {
                // Simple fields don't need brackets
                if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    format!(".{name}")
                } else {
                    format!("[\"{}\"]", escape_json_string(name))
                }
            }
            Self::Index(i) => format!("[{i}]"),
        }
    }
}

/// State for path iteration
struct PathState {
    components: Vec<PathComponent>,
    array_indices: Vec<usize>,
}

impl PathState {
    fn new() -> Self {
        Self {
            components: Vec::with_capacity(16),
            array_indices: Vec::with_capacity(8),
        }
    }

    fn current_path(&self, prefix: &str) -> String {
        let mut path = prefix.to_string();
        for component in &self.components {
            path.push_str(&component.as_path_segment());
        }
        path
    }

    fn push_field(&mut self, name: &str) {
        self.components.push(PathComponent::Field(name.to_string()));
    }

    fn push_index(&mut self, index: usize) {
        self.components.push(PathComponent::Index(index));
    }

    fn pop(&mut self) {
        self.components.pop();
    }

    fn enter_array(&mut self) {
        self.array_indices.push(0);
    }

    fn exit_array(&mut self) {
        self.array_indices.pop();
    }

    fn next_array_index(&mut self) -> usize {
        let idx = self.array_indices.last().copied().unwrap_or(0);
        if let Some(last) = self.array_indices.last_mut() {
            *last += 1;
        }
        idx
    }
}

/// Iterator yielding (path, node) pairs for gron-style output
pub struct PathIterator<'a, T: TapeSource> {
    tape: &'a T,
    index: usize,
    state: PathState,
    prefix: String,
}

impl<'a, T: TapeSource> PathIterator<'a, T> {
    /// Create a new path iterator with default prefix "json"
    #[must_use]
    pub fn new(tape: &'a T) -> Self {
        Self::with_prefix(tape, "json")
    }

    /// Create a new path iterator with custom prefix
    #[must_use]
    pub fn with_prefix(tape: &'a T, prefix: &str) -> Self {
        Self {
            tape,
            index: 0,
            state: PathState::new(),
            prefix: prefix.to_string(),
        }
    }

    /// Get the current path string
    #[must_use]
    pub fn current_path(&self) -> String {
        self.state.current_path(&self.prefix)
    }
}

impl<'a, T: TapeSource> Iterator for PathIterator<'a, T> {
    type Item = (String, TapeNodeRef<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.tape.len() {
            return None;
        }

        let node = self.tape.node_at(self.index)?;
        let path = self.state.current_path(&self.prefix);
        self.index += 1;

        // Update state for next iteration based on node type
        match node.kind {
            TapeNodeKind::ObjectStart { .. } => {
                // Path already set, will be updated when we see keys
            }
            TapeNodeKind::ObjectEnd => {
                self.state.pop();
            }
            TapeNodeKind::ArrayStart { .. } => {
                self.state.enter_array();
            }
            TapeNodeKind::ArrayEnd => {
                self.state.exit_array();
                self.state.pop();
            }
            TapeNodeKind::Key => {
                if let Some(TapeValue::String(ref name)) = node.value {
                    self.state.push_field(name);
                }
            }
            TapeNodeKind::Value => {
                // After a value in an array, we need to track position
                if self.state.array_indices.is_empty() {
                    self.state.pop(); // Remove field after value
                } else {
                    self.state.pop(); // Remove current index
                    let next_idx = self.state.next_array_index();
                    self.state.push_index(next_idx);
                }
            }
        }

        Some((path, node))
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Escape a string for JSON output
#[must_use]
pub fn escape_json_string(s: &str) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(result, "\\u{:04x}", c as u32);
            }
            c => result.push(c),
        }
    }
    result
}

/// Unescape a JSON string
#[must_use]
pub fn unescape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') | None => result.push('\\'),
                Some('/') => result.push('/'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('b') => result.push('\u{0008}'),
                Some('f') => result.push('\u{000C}'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16)
                        && let Some(c) = char::from_u32(code)
                    {
                        result.push(c);
                    }
                }
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DsonError;

    // Mock tape for testing
    struct MockTape {
        nodes: Vec<(TapeNodeKind, Option<TapeValue<'static>>)>,
        format: FormatKind,
    }

    impl MockTape {
        fn new(format: FormatKind) -> Self {
            Self {
                nodes: Vec::new(),
                format,
            }
        }

        fn push(&mut self, kind: TapeNodeKind, value: Option<TapeValue<'static>>) {
            self.nodes.push((kind, value));
        }
    }

    impl TapeSource for MockTape {
        fn format(&self) -> FormatKind {
            self.format
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_at(&self, index: usize) -> Option<TapeNodeRef<'_>> {
            self.nodes.get(index).map(|(kind, value)| TapeNodeRef {
                kind: *kind,
                value: value.clone(),
                format: self.format,
            })
        }

        fn skip_value(&self, start_index: usize) -> Result<usize> {
            let node = self
                .node_at(start_index)
                .ok_or_else(|| DsonError::InvalidField("Index out of bounds".to_string()))?;

            match node.kind {
                TapeNodeKind::ObjectStart { count } => {
                    // Skip object: key + value for each field, plus ObjectEnd
                    let mut idx = start_index + 1;
                    for _ in 0..count {
                        idx += 1; // key
                        idx = self.skip_value(idx)?; // value
                    }
                    Ok(idx + 1) // ObjectEnd
                }
                TapeNodeKind::ArrayStart { count } => {
                    // Skip array: each element, plus ArrayEnd
                    let mut idx = start_index + 1;
                    for _ in 0..count {
                        idx = self.skip_value(idx)?;
                    }
                    Ok(idx + 1) // ArrayEnd
                }
                _ => Ok(start_index + 1),
            }
        }

        fn resolve_path(&self, _path: &str) -> Result<Option<usize>> {
            // Simple mock: always returns None
            Ok(None)
        }
    }

    #[test]
    fn test_tape_value_null() {
        let val = TapeValue::Null;
        assert!(val.is_null());
        assert!(!val.is_bool());
        assert_eq!(val.format_for_output(), "null");
    }

    #[test]
    fn test_tape_value_bool() {
        let val = TapeValue::Bool(true);
        assert!(val.is_bool());
        assert_eq!(val.as_bool(), Some(true));
        assert_eq!(val.format_for_output(), "true");

        let val = TapeValue::Bool(false);
        assert_eq!(val.format_for_output(), "false");
    }

    #[test]
    fn test_tape_value_int() {
        let val = TapeValue::Int(42);
        assert!(val.is_number());
        assert_eq!(val.as_int(), Some(42));
        assert_eq!(val.format_for_output(), "42");
    }

    #[test]
    fn test_tape_value_float() {
        let val = TapeValue::Float(3.15);
        assert!(val.is_number());
        assert_eq!(val.as_float(), Some(3.15));
    }

    #[test]
    fn test_tape_value_string() {
        let val = TapeValue::String(Cow::Borrowed("hello"));
        assert!(val.is_string());
        assert_eq!(val.as_str(), Some("hello"));
        assert_eq!(val.format_for_output(), "\"hello\"");
    }

    #[test]
    fn test_tape_value_string_escape() {
        let val = TapeValue::String(Cow::Borrowed("hello\nworld"));
        assert_eq!(val.format_for_output(), "\"hello\\nworld\"");
    }

    #[test]
    fn test_tape_value_into_owned() {
        let val = TapeValue::String(Cow::Borrowed("test"));
        let owned = val.into_owned();
        assert!(matches!(owned, TapeValue::String(Cow::Owned(_))));
    }

    #[test]
    fn test_tape_node_kind_container_start() {
        assert!(TapeNodeKind::ObjectStart { count: 2 }.is_container_start());
        assert!(TapeNodeKind::ArrayStart { count: 3 }.is_container_start());
        assert!(!TapeNodeKind::Value.is_container_start());
    }

    #[test]
    fn test_tape_node_kind_element_count() {
        assert_eq!(
            TapeNodeKind::ObjectStart { count: 5 }.element_count(),
            Some(5)
        );
        assert_eq!(TapeNodeKind::Value.element_count(), None);
    }

    #[test]
    fn test_mock_tape_basic() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("name"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("test"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        assert_eq!(tape.len(), 4);
        assert!(!tape.is_empty());
        assert_eq!(tape.format(), FormatKind::Json);
    }

    #[test]
    fn test_tape_iterator() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(1)));
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(2)));
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(3)));

        assert_eq!(tape.iter().count(), 3);
    }

    #[test]
    fn test_tape_iterator_exact_size() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(1)));
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(2)));

        let iter = tape.iter();
        assert_eq!(iter.len(), 2);
    }

    #[test]
    fn test_skip_value_scalar() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(42)));

        assert_eq!(tape.skip_value(0).unwrap(), 1);
    }

    #[test]
    fn test_skip_value_object() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("a"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(1)));
        tape.push(TapeNodeKind::ObjectEnd, None);

        assert_eq!(tape.skip_value(0).unwrap(), 4);
    }

    #[test]
    fn test_skip_value_array() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(TapeNodeKind::ArrayStart { count: 2 }, None);
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(1)));
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(2)));
        tape.push(TapeNodeKind::ArrayEnd, None);

        assert_eq!(tape.skip_value(0).unwrap(), 4);
    }

    #[test]
    fn test_value_at() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(42)));

        assert_eq!(tape.value_at(0), Some(TapeValue::Int(42)));
    }

    #[test]
    fn test_key_at() {
        let mut tape = MockTape::new(FormatKind::Json);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("test"))),
        );

        assert_eq!(tape.key_at(0), Some(Cow::Borrowed("test")));
    }

    #[test]
    fn test_escape_json_string() {
        assert_eq!(escape_json_string("hello"), "hello");
        assert_eq!(escape_json_string("hello\nworld"), "hello\\nworld");
        assert_eq!(escape_json_string("quote\"here"), "quote\\\"here");
        assert_eq!(escape_json_string("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn test_unescape_json_string() {
        assert_eq!(unescape_json_string("hello"), "hello");
        assert_eq!(unescape_json_string("hello\\nworld"), "hello\nworld");
        assert_eq!(unescape_json_string("quote\\\"here"), "quote\"here");
        assert_eq!(unescape_json_string("back\\\\slash"), "back\\slash");
    }

    #[test]
    fn test_escape_unescape_roundtrip() {
        let original = "Hello\n\"World\"\t\\Test";
        let escaped = escape_json_string(original);
        let unescaped = unescape_json_string(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn test_path_component_field() {
        let comp = PathComponent::Field("name".to_string());
        assert_eq!(comp.as_path_segment(), ".name");

        let comp = PathComponent::Field("with space".to_string());
        assert_eq!(comp.as_path_segment(), "[\"with space\"]");
    }

    #[test]
    fn test_path_component_index() {
        let comp = PathComponent::Index(42);
        assert_eq!(comp.as_path_segment(), "[42]");
    }

    #[test]
    fn test_tape_node_ref_constructors() {
        let node = TapeNodeRef::object_start(5, FormatKind::Json);
        assert!(matches!(node.kind, TapeNodeKind::ObjectStart { count: 5 }));

        let node = TapeNodeRef::array_start(3, FormatKind::Json);
        assert!(matches!(node.kind, TapeNodeKind::ArrayStart { count: 3 }));

        let node = TapeNodeRef::value(TapeValue::Null, FormatKind::Json);
        assert!(matches!(node.kind, TapeNodeKind::Value));
        assert_eq!(node.value, Some(TapeValue::Null));
    }
}
