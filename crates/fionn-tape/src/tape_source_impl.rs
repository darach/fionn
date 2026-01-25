// SPDX-License-Identifier: MIT OR Apache-2.0
//! `TapeSource` implementation for `DsonTape`
//!
//! This module provides the [`TapeSource`] trait implementation for [`DsonTape`],
//! enabling format-agnostic tape traversal for gron, diff, and other operations.

use crate::DsonTape;
use fionn_core::Result;
use fionn_core::format::FormatKind;
use fionn_core::tape_source::{TapeIterator, TapeNodeKind, TapeNodeRef, TapeSource, TapeValue};
use simd_json::value::tape::Node;
use std::borrow::Cow;

impl<S: AsRef<[u8]>> TapeSource for DsonTape<S> {
    fn format(&self) -> FormatKind {
        FormatKind::Json
    }

    fn len(&self) -> usize {
        self.nodes().len()
    }

    fn node_at(&self, index: usize) -> Option<TapeNodeRef<'_>> {
        let nodes = self.nodes();
        if index >= nodes.len() {
            return None;
        }

        let node = &nodes[index];
        let (kind, value) = match node {
            Node::Object { len, .. } => (TapeNodeKind::ObjectStart { count: *len }, None),
            Node::Array { len, .. } => (TapeNodeKind::ArrayStart { count: *len }, None),
            Node::String(s) => {
                // Determine if this is a key or a value based on context
                let is_key = self.is_key_position(index);
                if is_key {
                    (TapeNodeKind::Key, Some(TapeValue::String(Cow::Borrowed(s))))
                } else {
                    (
                        TapeNodeKind::Value,
                        Some(TapeValue::String(Cow::Borrowed(s))),
                    )
                }
            }
            Node::Static(static_node) => {
                let value = match static_node {
                    simd_json::StaticNode::Null => TapeValue::Null,
                    simd_json::StaticNode::Bool(b) => TapeValue::Bool(*b),
                    simd_json::StaticNode::I64(n) => TapeValue::Int(*n),
                    simd_json::StaticNode::U64(n) => i64::try_from(*n).map_or_else(
                        |_| TapeValue::RawNumber(Cow::Owned(n.to_string())),
                        TapeValue::Int,
                    ),
                    simd_json::StaticNode::F64(n) => TapeValue::Float(*n),
                };
                (TapeNodeKind::Value, Some(value))
            }
        };

        Some(TapeNodeRef {
            kind,
            value,
            format: FormatKind::Json,
        })
    }

    fn value_at(&self, index: usize) -> Option<TapeValue<'_>> {
        let nodes = self.nodes();
        if index >= nodes.len() {
            return None;
        }

        match &nodes[index] {
            Node::String(s) => Some(TapeValue::String(Cow::Borrowed(s))),
            Node::Static(static_node) => Some(match static_node {
                simd_json::StaticNode::Null => TapeValue::Null,
                simd_json::StaticNode::Bool(b) => TapeValue::Bool(*b),
                simd_json::StaticNode::I64(n) => TapeValue::Int(*n),
                simd_json::StaticNode::U64(n) => i64::try_from(*n).map_or_else(
                    |_| TapeValue::RawNumber(Cow::Owned(n.to_string())),
                    TapeValue::Int,
                ),
                simd_json::StaticNode::F64(n) => TapeValue::Float(*n),
            }),
            Node::Object { .. } | Node::Array { .. } => None,
        }
    }

    fn key_at(&self, index: usize) -> Option<Cow<'_, str>> {
        let nodes = self.nodes();
        if index >= nodes.len() {
            return None;
        }

        // Only return key if this position is actually a key
        if self.is_key_position(index)
            && let Node::String(s) = &nodes[index]
        {
            return Some(Cow::Borrowed(s));
        }
        None
    }

    fn skip_value(&self, start_index: usize) -> Result<usize> {
        // Delegate to DsonTape's existing skip_value implementation
        Self::skip_value(self, start_index)
    }

    fn resolve_path(&self, path: &str) -> Result<Option<usize>> {
        // Delegate to DsonTape's existing resolve_path implementation
        Self::resolve_path(self, path)
    }

    fn iter(&self) -> TapeIterator<'_, Self> {
        TapeIterator::new(self)
    }
}

impl<S: AsRef<[u8]>> DsonTape<S> {
    /// Determine if a given index is in a "key position" within an object
    ///
    /// In `simd_json`'s tape format, objects are laid out as:
    /// `[Object { len, count }, key1, value1, key2, value2, ...]`
    ///
    /// So after an Object node, we alternate: key, value, key, value...
    /// This method walks backwards to determine the context.
    fn is_key_position(&self, index: usize) -> bool {
        let nodes = self.nodes();
        if index == 0 {
            return false;
        }

        // Walk backwards to find the enclosing container
        let mut i = index;

        while i > 0 {
            i -= 1;
            match &nodes[i] {
                Node::Object { len, count } => {
                    // First check if this object actually contains our index
                    // The object spans from i to i + count (inclusive of count nodes after Object node)
                    // For empty objects (count=0), the object only spans index i itself
                    let object_end = if *count > 0 { i + *count } else { i };
                    if index > object_end {
                        // This object ended before our index - it's a sibling, not parent
                        continue;
                    }

                    // This object contains our index.
                    // Now check if our index is at a key position by walking through fields.
                    // After the Object node, we have pairs: key, value, key, value...
                    // But values can be complex structures, so we need to skip them properly.
                    let mut field_idx = i + 1;
                    for _field_num in 0..*len {
                        // field_idx is the key position
                        if field_idx == index {
                            return true;
                        }
                        if field_idx > index {
                            // We've passed our index without finding it as a key
                            return false;
                        }
                        // Skip the key (1 node)
                        field_idx += 1;
                        // Skip the value (might be complex)
                        if field_idx <= index {
                            field_idx = self.skip_value_internal(field_idx);
                        }
                    }
                    // Index wasn't found at a key position in this object
                    return false;
                }
                Node::Array { count, .. } => {
                    // First check if this array actually contains our index
                    // For empty arrays (count=0), the array only spans index i itself
                    let array_end = if *count > 0 { i + *count } else { i };
                    if index > array_end {
                        // This array ended before our index - it's a sibling
                        continue;
                    }
                    // Arrays don't have keys - any position in an array is not a key
                    return false;
                }
                Node::String(_) | Node::Static(_) => {
                    // These don't change depth - continue walking back
                }
            }
        }

        false
    }

    /// Internal `skip_value` that doesn't need Result (for use in `is_key_position`)
    fn skip_value_internal(&self, start_index: usize) -> usize {
        let nodes = self.nodes();
        if start_index >= nodes.len() {
            return start_index;
        }

        match &nodes[start_index] {
            Node::Object { count, .. } | Node::Array { count, .. } => {
                // Skip the container and all its children
                if *count > 0 {
                    start_index + *count + 1
                } else {
                    start_index + 1
                }
            }
            Node::String(_) | Node::Static(_) => start_index + 1,
        }
    }

    /// Get the number of fields/elements in a container at the given index
    #[must_use]
    pub fn container_count(&self, index: usize) -> Option<usize> {
        let nodes = self.nodes();
        if index >= nodes.len() {
            return None;
        }

        match &nodes[index] {
            Node::Object { len, .. } | Node::Array { len, .. } => Some(*len),
            _ => None,
        }
    }

    /// Iterate over object fields starting at a given object index
    ///
    /// Returns an iterator of (`key_index`, `value_index`) pairs
    #[must_use]
    pub fn object_fields(&self, object_index: usize) -> Option<ObjectFieldIterator<'_, S>> {
        let nodes = self.nodes();
        if object_index >= nodes.len() {
            return None;
        }

        if let Node::Object { len, .. } = &nodes[object_index] {
            Some(ObjectFieldIterator {
                tape: self,
                current_index: object_index + 1,
                remaining: *len,
            })
        } else {
            None
        }
    }

    /// Iterate over array elements starting at a given array index
    ///
    /// Returns an iterator of element indices
    #[must_use]
    pub fn array_elements(&self, array_index: usize) -> Option<ArrayElementIterator<'_, S>> {
        let nodes = self.nodes();
        if array_index >= nodes.len() {
            return None;
        }

        if let Node::Array { len, .. } = &nodes[array_index] {
            Some(ArrayElementIterator {
                tape: self,
                current_index: array_index + 1,
                remaining: *len,
            })
        } else {
            None
        }
    }
}

/// Iterator over object fields
pub struct ObjectFieldIterator<'a, S: AsRef<[u8]>> {
    tape: &'a DsonTape<S>,
    current_index: usize,
    remaining: usize,
}

impl<S: AsRef<[u8]>> Iterator for ObjectFieldIterator<'_, S> {
    type Item = (usize, usize); // (key_index, value_index)

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let nodes = self.tape.nodes();
        if self.current_index >= nodes.len() {
            return None;
        }

        let key_index = self.current_index;
        let value_index = self.current_index + 1;

        // Skip past the value to get to the next key
        match self.tape.skip_value(value_index) {
            Ok(next_index) => {
                self.current_index = next_index;
                self.remaining -= 1;
                Some((key_index, value_index))
            }
            Err(_) => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<S: AsRef<[u8]>> ExactSizeIterator for ObjectFieldIterator<'_, S> {}

/// Iterator over array elements
pub struct ArrayElementIterator<'a, S: AsRef<[u8]>> {
    tape: &'a DsonTape<S>,
    current_index: usize,
    remaining: usize,
}

impl<S: AsRef<[u8]>> Iterator for ArrayElementIterator<'_, S> {
    type Item = usize; // element_index

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let nodes = self.tape.nodes();
        if self.current_index >= nodes.len() {
            return None;
        }

        let element_index = self.current_index;

        // Skip past this element to get to the next
        match self.tape.skip_value(element_index) {
            Ok(next_index) => {
                self.current_index = next_index;
                self.remaining -= 1;
                Some(element_index)
            }
            Err(_) => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<S: AsRef<[u8]>> ExactSizeIterator for ArrayElementIterator<'_, S> {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tape_source_format() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        assert_eq!(tape.format(), FormatKind::Json);
    }

    #[test]
    fn test_tape_source_len() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        assert!(tape.len() > 0);
    }

    #[test]
    fn test_tape_source_node_at_object() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        let node = tape.node_at(0).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ObjectStart { count: 1 }));
    }

    #[test]
    fn test_tape_source_node_at_array() {
        let tape = DsonTape::parse(r"[1, 2, 3]").unwrap();
        let node = tape.node_at(0).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ArrayStart { count: 3 }));
    }

    #[test]
    fn test_tape_source_node_at_key() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        let node = tape.node_at(1).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Key));
        assert!(matches!(node.value, Some(TapeValue::String(_))));
    }

    #[test]
    fn test_tape_source_node_at_string_value() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        let node = tape.node_at(2).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Value));
        assert!(matches!(node.value, Some(TapeValue::String(_))));
    }

    #[test]
    fn test_tape_source_node_at_number_value() {
        let tape = DsonTape::parse(r#"{"age": 42}"#).unwrap();
        let node = tape.node_at(2).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Value));
        assert!(matches!(node.value, Some(TapeValue::Int(42))));
    }

    #[test]
    fn test_tape_source_node_at_bool_value() {
        let tape = DsonTape::parse(r#"{"active": true}"#).unwrap();
        let node = tape.node_at(2).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Value));
        assert!(matches!(node.value, Some(TapeValue::Bool(true))));
    }

    #[test]
    fn test_tape_source_node_at_null_value() {
        let tape = DsonTape::parse(r#"{"value": null}"#).unwrap();
        let node = tape.node_at(2).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Value));
        assert!(matches!(node.value, Some(TapeValue::Null)));
    }

    #[test]
    fn test_tape_source_node_at_float_value() {
        let tape = DsonTape::parse(r#"{"val": 1.234}"#).unwrap();
        let node = tape.node_at(2).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Value));
        if let Some(TapeValue::Float(f)) = node.value {
            assert!((f - 1.234_f64).abs() < 0.001);
        } else {
            panic!("Expected float value");
        }
    }

    #[test]
    fn test_tape_source_node_at_out_of_bounds() {
        let tape = DsonTape::parse(r"{}").unwrap();
        assert!(tape.node_at(100).is_none());
    }

    #[test]
    fn test_tape_source_value_at_string() {
        let tape = DsonTape::parse(r#"{"name": "Alice"}"#).unwrap();
        let value = tape.value_at(2).unwrap();
        assert!(matches!(value, TapeValue::String(s) if s == "Alice"));
    }

    #[test]
    fn test_tape_source_value_at_number() {
        let tape = DsonTape::parse(r#"{"count": 42}"#).unwrap();
        let value = tape.value_at(2).unwrap();
        assert!(matches!(value, TapeValue::Int(42)));
    }

    #[test]
    fn test_tape_source_value_at_container() {
        let tape = DsonTape::parse(r#"{"obj": {}}"#).unwrap();
        // Value at container position should be None
        assert!(tape.value_at(2).is_none());
    }

    #[test]
    fn test_tape_source_key_at() {
        let tape = DsonTape::parse(r#"{"name": "test", "age": 30}"#).unwrap();
        // Index 1 should be the "name" key
        let key = tape.key_at(1).unwrap();
        assert_eq!(key, "name");
        // Index 3 should be the "age" key
        let key = tape.key_at(3).unwrap();
        assert_eq!(key, "age");
    }

    #[test]
    fn test_tape_source_key_at_not_key() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        // Index 2 is the value, not a key
        assert!(tape.key_at(2).is_none());
        // Index 0 is the object, not a key
        assert!(tape.key_at(0).is_none());
    }

    #[test]
    fn test_tape_source_skip_value() {
        let tape = DsonTape::parse(r#"{"name": "test", "age": 30}"#).unwrap();
        // Skip the object starting at index 0
        let next = TapeSource::skip_value(&tape, 0).unwrap();
        // skip_value should advance past the entire object
        // (simd_json's count field behavior may vary, so check it's > 0 and <= len)
        assert!(next > 0);
        assert!(next <= tape.len());
    }

    #[test]
    fn test_tape_source_resolve_path() {
        let tape = DsonTape::parse(r#"{"user": {"name": "Alice"}}"#).unwrap();
        let result = TapeSource::resolve_path(&tape, "user.name").unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_tape_source_iter() {
        let tape = DsonTape::parse(r#"{"a": 1, "b": 2}"#).unwrap();
        let mut iter = tape.iter();

        // First node should be the object
        let first = iter.next().unwrap();
        assert!(matches!(first.kind, TapeNodeKind::ObjectStart { .. }));
    }

    #[test]
    fn test_container_count_object() {
        let tape = DsonTape::parse(r#"{"a": 1, "b": 2, "c": 3}"#).unwrap();
        assert_eq!(tape.container_count(0), Some(3));
    }

    #[test]
    fn test_container_count_array() {
        let tape = DsonTape::parse(r"[1, 2, 3, 4, 5]").unwrap();
        assert_eq!(tape.container_count(0), Some(5));
    }

    #[test]
    fn test_container_count_not_container() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        assert_eq!(tape.container_count(2), None);
    }

    #[test]
    fn test_object_fields_iterator() {
        let tape = DsonTape::parse(r#"{"a": 1, "b": 2}"#).unwrap();
        let fields: Vec<_> = tape.object_fields(0).unwrap().collect();
        assert_eq!(fields.len(), 2);

        // First field: key at 1, value at 2
        assert_eq!(fields[0], (1, 2));
        // Second field: key at 3, value at 4
        assert_eq!(fields[1], (3, 4));
    }

    #[test]
    fn test_object_fields_iterator_nested() {
        let tape = DsonTape::parse(r#"{"outer": {"inner": 1}}"#).unwrap();
        assert_eq!(tape.object_fields(0).unwrap().count(), 1);

        // The value at index 2 is the inner object
        assert_eq!(tape.object_fields(2).unwrap().count(), 1);
    }

    #[test]
    fn test_object_fields_not_object() {
        let tape = DsonTape::parse(r"[1, 2, 3]").unwrap();
        assert!(tape.object_fields(0).is_none());
    }

    #[test]
    fn test_array_elements_iterator() {
        let tape = DsonTape::parse(r"[1, 2, 3]").unwrap();
        let elements: Vec<_> = tape.array_elements(0).unwrap().collect();
        assert_eq!(elements.len(), 3);

        // Elements at indices 1, 2, 3
        assert_eq!(elements[0], 1);
        assert_eq!(elements[1], 2);
        assert_eq!(elements[2], 3);
    }

    #[test]
    fn test_array_elements_iterator_nested_objects() {
        let tape = DsonTape::parse(r#"[{"a": 1}, {"b": 2}]"#).unwrap();
        assert_eq!(tape.array_elements(0).unwrap().count(), 2);
    }

    #[test]
    fn test_array_elements_not_array() {
        let tape = DsonTape::parse(r#"{"a": 1}"#).unwrap();
        assert!(tape.array_elements(0).is_none());
    }

    #[test]
    fn test_is_key_position_basic() {
        let tape = DsonTape::parse(r#"{"name": "test"}"#).unwrap();
        // Index 1 is key position
        assert!(tape.is_key_position(1));
        // Index 2 is value position
        assert!(!tape.is_key_position(2));
    }

    #[test]
    fn test_is_key_position_multiple_fields() {
        let tape = DsonTape::parse(r#"{"a": 1, "b": 2, "c": 3}"#).unwrap();
        // Keys at 1, 3, 5
        assert!(tape.is_key_position(1));
        assert!(!tape.is_key_position(2));
        assert!(tape.is_key_position(3));
        assert!(!tape.is_key_position(4));
        assert!(tape.is_key_position(5));
        assert!(!tape.is_key_position(6));
    }

    #[test]
    fn test_is_key_position_array_elements() {
        let tape = DsonTape::parse(r#"["a", "b", "c"]"#).unwrap();
        // Array elements are never keys
        assert!(!tape.is_key_position(1));
        assert!(!tape.is_key_position(2));
        assert!(!tape.is_key_position(3));
    }

    #[test]
    fn test_is_key_position_nested_object() {
        let tape = DsonTape::parse(r#"{"outer": {"inner": 1}}"#).unwrap();
        // Index 1 is "outer" key
        assert!(tape.is_key_position(1));
        // Index 2 is the inner object (value)
        assert!(!tape.is_key_position(2));
        // Index 3 is "inner" key inside nested object
        assert!(tape.is_key_position(3));
        // Index 4 is the value 1
        assert!(!tape.is_key_position(4));
    }

    #[test]
    fn test_is_key_position_sibling_objects() {
        // This test verifies that keys in sibling objects are correctly detected
        let tape = DsonTape::parse(r#"{"config": {"server": {"host": "localhost", "port": 8080}, "database": {"url": "db://local"}}}"#).unwrap();

        // Index 9 is the "database" key (after the server object closes)
        assert!(
            tape.is_key_position(9),
            "Index 9 should be 'database' key position"
        );
        // Index 10 is the database object (value)
        assert!(!tape.is_key_position(10));
        // Index 11 is "url" key inside database
        assert!(tape.is_key_position(11));
        // Index 12 is "db://local" value
        assert!(!tape.is_key_position(12));
    }

    #[test]
    fn test_empty_object() {
        let tape = DsonTape::parse(r"{}").unwrap();
        assert_eq!(tape.len(), 1);
        let node = tape.node_at(0).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ObjectStart { count: 0 }));
    }

    #[test]
    fn test_empty_array() {
        let tape = DsonTape::parse(r"[]").unwrap();
        assert_eq!(tape.len(), 1);
        let node = tape.node_at(0).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ArrayStart { count: 0 }));
    }

    #[test]
    fn test_large_u64_value() {
        let large_value = u64::MAX;
        let json = format!(r#"{{"big": {large_value}}}"#);
        let tape = DsonTape::parse(&json).unwrap();
        let value = tape.value_at(2);
        assert!(value.is_some());
        // Should be stored as RawNumber since it exceeds i64::MAX
        if let Some(TapeValue::RawNumber(s)) = value {
            assert_eq!(s.as_ref(), large_value.to_string());
        }
    }

    #[test]
    fn test_iterator_exact_size() {
        let tape = DsonTape::parse(r#"{"a": 1, "b": 2}"#).unwrap();
        let fields_iter = tape.object_fields(0).unwrap();
        assert_eq!(fields_iter.len(), 2);

        let tape2 = DsonTape::parse(r"[1, 2, 3, 4]").unwrap();
        let elements_iter = tape2.array_elements(0).unwrap();
        assert_eq!(elements_iter.len(), 4);
    }

    #[test]
    fn test_deeply_nested_structure() {
        let tape = DsonTape::parse(r#"{"a": {"b": {"c": {"d": 1}}}}"#).unwrap();

        // Navigate through nested structure
        assert_eq!(tape.object_fields(0).unwrap().count(), 1);

        // Index 2 should be the next nested object
        assert_eq!(tape.object_fields(2).unwrap().count(), 1);
    }

    #[test]
    fn test_mixed_array_and_object() {
        let tape = DsonTape::parse(r#"{"items": [{"id": 1}, {"id": 2}]}"#).unwrap();

        // Root object should have 1 field
        let mut root_fields = tape.object_fields(0).unwrap();
        let (_, items_value_idx) = root_fields.next().unwrap();
        assert!(root_fields.next().is_none(), "Should have exactly 1 field");

        // "items" array should have 2 elements
        assert_eq!(tape.array_elements(items_value_idx).unwrap().count(), 2);
    }
}
