// SPDX-License-Identifier: MIT OR Apache-2.0
//! `TapeSource` implementation for `UnifiedTape`
//!
//! This module provides the [`TapeSource`] trait implementation for [`UnifiedTape`],
//! enabling format-agnostic tape traversal for gron, diff, and other operations.

use super::tape::{TapeNode, TapeValue as UnifiedTapeValue, UnifiedTape};
use fionn_core::Result;
use fionn_core::format::FormatKind;
use fionn_core::tape_source::{
    TapeIterator, TapeNodeKind, TapeNodeRef, TapeSource, TapeValue as CoreTapeValue,
};
use std::borrow::Cow;

/// Convert `UnifiedTape`'s `TapeValue` to `fionn_core`'s `TapeValue`
fn convert_value<'a>(value: &'a UnifiedTapeValue<'a>) -> CoreTapeValue<'a> {
    match value {
        UnifiedTapeValue::Null => CoreTapeValue::Null,
        UnifiedTapeValue::Bool(b) => CoreTapeValue::Bool(*b),
        UnifiedTapeValue::Int(n) => CoreTapeValue::Int(*n),
        UnifiedTapeValue::Float(n) => CoreTapeValue::Float(*n),
        UnifiedTapeValue::String(s) => CoreTapeValue::String(Cow::Borrowed(s.as_ref())),
        UnifiedTapeValue::RawNumber(s) => CoreTapeValue::RawNumber(Cow::Borrowed(s.as_ref())),
    }
}

impl TapeSource for UnifiedTape<'_> {
    fn format(&self) -> FormatKind {
        self.source_format
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn node_at(&self, index: usize) -> Option<TapeNodeRef<'_>> {
        if index >= self.nodes.len() {
            return None;
        }

        let node = &self.nodes[index];
        let (kind, value) = match node {
            TapeNode::ObjectStart { count } => (TapeNodeKind::ObjectStart { count: *count }, None),
            TapeNode::ObjectEnd => (TapeNodeKind::ObjectEnd, None),
            TapeNode::ArrayStart { count } => (TapeNodeKind::ArrayStart { count: *count }, None),
            TapeNode::ArrayEnd => (TapeNodeKind::ArrayEnd, None),
            TapeNode::Key(s) => (
                TapeNodeKind::Key,
                Some(CoreTapeValue::String(Cow::Borrowed(s.as_ref()))),
            ),
            TapeNode::Value(v) => (TapeNodeKind::Value, Some(convert_value(v))),
            // Format-specific nodes treated as values with string representation
            TapeNode::Comment(s) => (
                TapeNodeKind::Value,
                Some(CoreTapeValue::String(Cow::Borrowed(s.as_ref()))),
            ),
            TapeNode::Reference { target, .. } => (
                TapeNodeKind::Value,
                Some(CoreTapeValue::String(Cow::Borrowed(target.as_ref()))),
            ),
            TapeNode::Definition { name, .. } => (
                TapeNodeKind::Key,
                Some(CoreTapeValue::String(Cow::Borrowed(name.as_ref()))),
            ),
            TapeNode::Section { path } => {
                // Sections become keys with the full path
                let full_path = path.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(".");
                (
                    TapeNodeKind::Key,
                    Some(CoreTapeValue::String(Cow::Owned(full_path))),
                )
            }
            TapeNode::TabularHeader { fields, .. } => {
                // Headers become objects
                (
                    TapeNodeKind::ObjectStart {
                        count: fields.len(),
                    },
                    None,
                )
            }
            TapeNode::TabularRow { values } => {
                // Rows become arrays
                (
                    TapeNodeKind::ArrayStart {
                        count: values.len(),
                    },
                    None,
                )
            }
        };

        Some(TapeNodeRef {
            kind,
            value,
            format: self.source_format,
        })
    }

    fn value_at(&self, index: usize) -> Option<CoreTapeValue<'_>> {
        if index >= self.nodes.len() {
            return None;
        }

        match &self.nodes[index] {
            TapeNode::Value(v) => Some(convert_value(v)),
            TapeNode::Key(s) | TapeNode::Comment(s) => {
                Some(CoreTapeValue::String(Cow::Borrowed(s.as_ref())))
            }
            TapeNode::Reference { target, .. } => {
                Some(CoreTapeValue::String(Cow::Borrowed(target.as_ref())))
            }
            TapeNode::Definition { name, .. } => {
                Some(CoreTapeValue::String(Cow::Borrowed(name.as_ref())))
            }
            _ => None,
        }
    }

    fn key_at(&self, index: usize) -> Option<Cow<'_, str>> {
        if index >= self.nodes.len() {
            return None;
        }

        match &self.nodes[index] {
            TapeNode::Key(s) => Some(Cow::Borrowed(s.as_ref())),
            TapeNode::Definition { name, .. } => Some(Cow::Borrowed(name.as_ref())),
            TapeNode::Section { path } => {
                if path.is_empty() {
                    None
                } else {
                    Some(Cow::Owned(
                        path.iter().map(AsRef::as_ref).collect::<Vec<_>>().join("."),
                    ))
                }
            }
            _ => None,
        }
    }

    fn skip_value(&self, start_index: usize) -> Result<usize> {
        if start_index >= self.nodes.len() {
            return Ok(start_index);
        }

        match &self.nodes[start_index] {
            TapeNode::ObjectStart { .. } => {
                // Find matching ObjectEnd
                let mut depth = 1;
                let mut index = start_index + 1;
                while index < self.nodes.len() && depth > 0 {
                    match &self.nodes[index] {
                        TapeNode::ObjectStart { .. } => depth += 1,
                        TapeNode::ObjectEnd => depth -= 1,
                        _ => {}
                    }
                    index += 1;
                }
                Ok(index)
            }
            TapeNode::ArrayStart { .. } => {
                // Find matching ArrayEnd
                let mut depth = 1;
                let mut index = start_index + 1;
                while index < self.nodes.len() && depth > 0 {
                    match &self.nodes[index] {
                        TapeNode::ArrayStart { .. } => depth += 1,
                        TapeNode::ArrayEnd => depth -= 1,
                        _ => {}
                    }
                    index += 1;
                }
                Ok(index)
            }
            TapeNode::TabularHeader { fields, .. } => {
                // Skip header and all rows until next header or end
                let mut index = start_index + 1;
                while index < self.nodes.len() {
                    match &self.nodes[index] {
                        TapeNode::TabularHeader { .. }
                        | TapeNode::ObjectStart { .. }
                        | TapeNode::ArrayStart { .. }
                        | TapeNode::ObjectEnd
                        | TapeNode::ArrayEnd => break,
                        TapeNode::TabularRow { values } => {
                            // Skip row based on field count
                            if values.len() == fields.len() {
                                index += 1;
                                continue;
                            }
                            break;
                        }
                        _ => index += 1,
                    }
                }
                Ok(index)
            }
            TapeNode::TabularRow { .. } => {
                // Just skip the single row node
                Ok(start_index + 1)
            }
            _ => {
                // Scalar nodes, keys, comments, references, definitions, sections
                Ok(start_index + 1)
            }
        }
    }

    fn resolve_path(&self, path: &str) -> Result<Option<usize>> {
        if path.is_empty() {
            return Ok(Some(0));
        }

        let components = fionn_core::parse_simd(path);
        let mut current_index = 0;

        for component in components {
            if current_index >= self.nodes.len() {
                return Ok(None);
            }

            match component {
                fionn_core::PathComponent::Field(field_name) => {
                    // Look for the field within the current object
                    match &self.nodes[current_index] {
                        TapeNode::ObjectStart { count } => {
                            let mut found = false;
                            let mut idx = current_index + 1;
                            let mut fields_checked = 0;

                            while idx < self.nodes.len() && fields_checked < *count {
                                if let TapeNode::Key(key) = &self.nodes[idx] {
                                    if key.as_ref() == field_name {
                                        // Found the key, next node is the value
                                        current_index = idx + 1;
                                        found = true;
                                        break;
                                    }
                                    // Skip key and value
                                    idx = self.skip_value(idx + 1)?;
                                    fields_checked += 1;
                                } else {
                                    idx += 1;
                                }
                            }

                            if !found {
                                return Ok(None);
                            }
                        }
                        _ => return Ok(None),
                    }
                }
                fionn_core::PathComponent::ArrayIndex(target_idx) => {
                    // Navigate to array element
                    match &self.nodes[current_index] {
                        TapeNode::ArrayStart { count } => {
                            if target_idx >= *count {
                                return Ok(None);
                            }

                            let mut idx = current_index + 1;
                            let mut elem_idx = 0;

                            while idx < self.nodes.len() && elem_idx < target_idx {
                                if matches!(self.nodes[idx], TapeNode::ArrayEnd) {
                                    return Ok(None);
                                }
                                idx = self.skip_value(idx)?;
                                elem_idx += 1;
                            }

                            current_index = idx;
                        }
                        _ => return Ok(None),
                    }
                }
            }
        }

        Ok(Some(current_index))
    }

    fn iter(&self) -> TapeIterator<'_, Self> {
        TapeIterator::new(self)
    }
}

impl UnifiedTape<'_> {
    /// Get the number of fields/elements in a container at the given index
    #[must_use]
    pub fn container_count(&self, index: usize) -> Option<usize> {
        if index >= self.nodes.len() {
            return None;
        }

        match &self.nodes[index] {
            TapeNode::ObjectStart { count } | TapeNode::ArrayStart { count } => Some(*count),
            TapeNode::TabularHeader { fields, .. } => Some(fields.len()),
            TapeNode::TabularRow { values } => Some(values.len()),
            _ => None,
        }
    }

    /// Iterate over object fields starting at a given object index
    ///
    /// Returns an iterator of (`key_index`, `value_index`) pairs
    #[must_use]
    pub fn object_fields(&self, object_index: usize) -> Option<UnifiedObjectFieldIterator<'_>> {
        if object_index >= self.nodes.len() {
            return None;
        }

        if let TapeNode::ObjectStart { count } = &self.nodes[object_index] {
            Some(UnifiedObjectFieldIterator {
                tape: self,
                current_index: object_index + 1,
                remaining: *count,
            })
        } else {
            None
        }
    }

    /// Iterate over array elements starting at a given array index
    ///
    /// Returns an iterator of element indices
    #[must_use]
    pub fn array_elements(&self, array_index: usize) -> Option<UnifiedArrayElementIterator<'_>> {
        if array_index >= self.nodes.len() {
            return None;
        }

        if let TapeNode::ArrayStart { count } = &self.nodes[array_index] {
            Some(UnifiedArrayElementIterator {
                tape: self,
                current_index: array_index + 1,
                remaining: *count,
            })
        } else {
            None
        }
    }
}

/// Iterator over object fields in `UnifiedTape`
pub struct UnifiedObjectFieldIterator<'a> {
    tape: &'a UnifiedTape<'a>,
    current_index: usize,
    remaining: usize,
}

impl Iterator for UnifiedObjectFieldIterator<'_> {
    type Item = (usize, usize); // (key_index, value_index)

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        // Find the next key
        while self.current_index < self.tape.nodes.len() {
            if matches!(self.tape.nodes[self.current_index], TapeNode::ObjectEnd) {
                return None;
            }

            if matches!(self.tape.nodes[self.current_index], TapeNode::Key(_)) {
                let key_index = self.current_index;
                let value_index = self.current_index + 1;

                // Skip past the value to get to the next key
                match TapeSource::skip_value(self.tape, value_index) {
                    Ok(next_index) => {
                        self.current_index = next_index;
                        self.remaining -= 1;
                        return Some((key_index, value_index));
                    }
                    Err(_) => return None,
                }
            }

            self.current_index += 1;
        }

        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for UnifiedObjectFieldIterator<'_> {}

/// Iterator over array elements in `UnifiedTape`
pub struct UnifiedArrayElementIterator<'a> {
    tape: &'a UnifiedTape<'a>,
    current_index: usize,
    remaining: usize,
}

impl Iterator for UnifiedArrayElementIterator<'_> {
    type Item = usize; // element_index

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        if self.current_index >= self.tape.nodes.len() {
            return None;
        }

        if matches!(self.tape.nodes[self.current_index], TapeNode::ArrayEnd) {
            return None;
        }

        let element_index = self.current_index;

        // Skip past this element to get to the next
        match TapeSource::skip_value(self.tape, element_index) {
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

impl ExactSizeIterator for UnifiedArrayElementIterator<'_> {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_simple_object<'a>() -> UnifiedTape<'a> {
        let mut tape = UnifiedTape::new(FormatKind::Json);
        tape.nodes.push(TapeNode::ObjectStart { count: 2 });
        tape.nodes.push(TapeNode::Key(Cow::Borrowed("name")));
        tape.nodes
            .push(TapeNode::Value(UnifiedTapeValue::String(Cow::Borrowed(
                "Alice",
            ))));
        tape.nodes.push(TapeNode::Key(Cow::Borrowed("age")));
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Int(30)));
        tape.nodes.push(TapeNode::ObjectEnd);
        tape
    }

    fn make_simple_array<'a>() -> UnifiedTape<'a> {
        let mut tape = UnifiedTape::new(FormatKind::Json);
        tape.nodes.push(TapeNode::ArrayStart { count: 3 });
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Int(1)));
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Int(2)));
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Int(3)));
        tape.nodes.push(TapeNode::ArrayEnd);
        tape
    }

    fn make_nested_object<'a>() -> UnifiedTape<'a> {
        let mut tape = UnifiedTape::new(FormatKind::Json);
        tape.nodes.push(TapeNode::ObjectStart { count: 1 });
        tape.nodes.push(TapeNode::Key(Cow::Borrowed("user")));
        tape.nodes.push(TapeNode::ObjectStart { count: 1 });
        tape.nodes.push(TapeNode::Key(Cow::Borrowed("name")));
        tape.nodes
            .push(TapeNode::Value(UnifiedTapeValue::String(Cow::Borrowed(
                "Bob",
            ))));
        tape.nodes.push(TapeNode::ObjectEnd);
        tape.nodes.push(TapeNode::ObjectEnd);
        tape
    }

    #[test]
    fn test_tape_source_format() {
        let tape = make_simple_object();
        assert_eq!(tape.format(), FormatKind::Json);
    }

    #[test]
    fn test_tape_source_len() {
        let tape = make_simple_object();
        assert_eq!(tape.len(), 6); // ObjectStart, Key, Value, Key, Value, ObjectEnd
    }

    #[test]
    fn test_tape_source_node_at_object_start() {
        let tape = make_simple_object();
        let node = tape.node_at(0).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ObjectStart { count: 2 }));
    }

    #[test]
    fn test_tape_source_node_at_object_end() {
        let tape = make_simple_object();
        let node = tape.node_at(5).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ObjectEnd));
    }

    #[test]
    fn test_tape_source_node_at_key() {
        let tape = make_simple_object();
        let node = tape.node_at(1).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Key));
        assert!(matches!(node.value, Some(CoreTapeValue::String(s)) if s == "name"));
    }

    #[test]
    fn test_tape_source_node_at_value() {
        let tape = make_simple_object();
        let node = tape.node_at(2).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Value));
        assert!(matches!(node.value, Some(CoreTapeValue::String(s)) if s == "Alice"));
    }

    #[test]
    fn test_tape_source_node_at_int_value() {
        let tape = make_simple_object();
        let node = tape.node_at(4).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::Value));
        assert!(matches!(node.value, Some(CoreTapeValue::Int(30))));
    }

    #[test]
    fn test_tape_source_node_at_array_start() {
        let tape = make_simple_array();
        let node = tape.node_at(0).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ArrayStart { count: 3 }));
    }

    #[test]
    fn test_tape_source_node_at_array_end() {
        let tape = make_simple_array();
        let node = tape.node_at(4).unwrap();
        assert!(matches!(node.kind, TapeNodeKind::ArrayEnd));
    }

    #[test]
    fn test_tape_source_node_at_out_of_bounds() {
        let tape = make_simple_object();
        assert!(tape.node_at(100).is_none());
    }

    #[test]
    fn test_tape_source_value_at_string() {
        let tape = make_simple_object();
        let value = tape.value_at(2).unwrap();
        assert!(matches!(value, CoreTapeValue::String(s) if s == "Alice"));
    }

    #[test]
    fn test_tape_source_value_at_int() {
        let tape = make_simple_object();
        let value = tape.value_at(4).unwrap();
        assert!(matches!(value, CoreTapeValue::Int(30)));
    }

    #[test]
    fn test_tape_source_value_at_key() {
        let tape = make_simple_object();
        // Keys also return values (the key string)
        let value = tape.value_at(1).unwrap();
        assert!(matches!(value, CoreTapeValue::String(s) if s == "name"));
    }

    #[test]
    fn test_tape_source_value_at_container() {
        let tape = make_simple_object();
        // Container nodes don't have values
        assert!(tape.value_at(0).is_none());
    }

    #[test]
    fn test_tape_source_key_at() {
        let tape = make_simple_object();
        let key = tape.key_at(1).unwrap();
        assert_eq!(key, "name");
        let key = tape.key_at(3).unwrap();
        assert_eq!(key, "age");
    }

    #[test]
    fn test_tape_source_key_at_not_key() {
        let tape = make_simple_object();
        // Non-key nodes don't return keys
        assert!(tape.key_at(0).is_none()); // ObjectStart
        assert!(tape.key_at(2).is_none()); // Value
    }

    #[test]
    fn test_tape_source_skip_value_object() {
        let tape = make_simple_object();
        let next = TapeSource::skip_value(&tape, 0).unwrap();
        assert_eq!(next, 6); // Past ObjectEnd
    }

    #[test]
    fn test_tape_source_skip_value_array() {
        let tape = make_simple_array();
        let next = TapeSource::skip_value(&tape, 0).unwrap();
        assert_eq!(next, 5); // Past ArrayEnd
    }

    #[test]
    fn test_tape_source_skip_value_scalar() {
        let tape = make_simple_array();
        let next = TapeSource::skip_value(&tape, 1).unwrap();
        assert_eq!(next, 2); // Just one node
    }

    #[test]
    fn test_tape_source_skip_value_nested() {
        let tape = make_nested_object();
        let next = TapeSource::skip_value(&tape, 0).unwrap();
        assert_eq!(next, tape.len()); // Skip entire structure
    }

    #[test]
    fn test_tape_source_resolve_path_simple() {
        let tape = make_simple_object();
        let result = TapeSource::resolve_path(&tape, "name").unwrap();
        assert_eq!(result, Some(2)); // Index of "Alice" value
    }

    #[test]
    fn test_tape_source_resolve_path_nested() {
        let tape = make_nested_object();
        let result = TapeSource::resolve_path(&tape, "user.name").unwrap();
        assert_eq!(result, Some(4)); // Index of "Bob" value
    }

    #[test]
    fn test_tape_source_resolve_path_not_found() {
        let tape = make_simple_object();
        let result = TapeSource::resolve_path(&tape, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_tape_source_resolve_path_empty() {
        let tape = make_simple_object();
        let result = TapeSource::resolve_path(&tape, "").unwrap();
        assert_eq!(result, Some(0)); // Root
    }

    #[test]
    fn test_tape_source_iter() {
        let tape = make_simple_object();
        let nodes: Vec<_> = tape.iter().collect();
        assert_eq!(nodes.len(), 6);
        assert!(matches!(nodes[0].kind, TapeNodeKind::ObjectStart { .. }));
        assert!(matches!(nodes[5].kind, TapeNodeKind::ObjectEnd));
    }

    #[test]
    fn test_container_count_object() {
        let tape = make_simple_object();
        assert_eq!(tape.container_count(0), Some(2));
    }

    #[test]
    fn test_container_count_array() {
        let tape = make_simple_array();
        assert_eq!(tape.container_count(0), Some(3));
    }

    #[test]
    fn test_container_count_not_container() {
        let tape = make_simple_object();
        assert_eq!(tape.container_count(2), None); // Value node
    }

    #[test]
    fn test_object_fields_iterator() {
        let tape = make_simple_object();
        let fields: Vec<_> = tape.object_fields(0).unwrap().collect();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], (1, 2)); // name -> Alice
        assert_eq!(fields[1], (3, 4)); // age -> 30
    }

    #[test]
    fn test_object_fields_not_object() {
        let tape = make_simple_array();
        assert!(tape.object_fields(0).is_none());
    }

    #[test]
    fn test_array_elements_iterator() {
        let tape = make_simple_array();
        let elements: Vec<_> = tape.array_elements(0).unwrap().collect();
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0], 1);
        assert_eq!(elements[1], 2);
        assert_eq!(elements[2], 3);
    }

    #[test]
    fn test_array_elements_not_array() {
        let tape = make_simple_object();
        assert!(tape.array_elements(0).is_none());
    }

    #[test]
    fn test_iterator_exact_size() {
        let tape = make_simple_object();
        let fields_iter = tape.object_fields(0).unwrap();
        assert_eq!(fields_iter.len(), 2);

        let tape2 = make_simple_array();
        let elements_iter = tape2.array_elements(0).unwrap();
        assert_eq!(elements_iter.len(), 3);
    }

    #[test]
    fn test_empty_object() {
        let mut tape = UnifiedTape::new(FormatKind::Json);
        tape.nodes.push(TapeNode::ObjectStart { count: 0 });
        tape.nodes.push(TapeNode::ObjectEnd);

        assert_eq!(tape.len(), 2);
        assert!(tape.object_fields(0).unwrap().next().is_none());
    }

    #[test]
    fn test_empty_array() {
        let mut tape = UnifiedTape::new(FormatKind::Json);
        tape.nodes.push(TapeNode::ArrayStart { count: 0 });
        tape.nodes.push(TapeNode::ArrayEnd);

        assert_eq!(tape.len(), 2);
        assert!(tape.array_elements(0).unwrap().next().is_none());
    }

    #[test]
    fn test_all_value_types() {
        let mut tape = UnifiedTape::new(FormatKind::Json);
        tape.nodes.push(TapeNode::ArrayStart { count: 5 });
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Null));
        tape.nodes
            .push(TapeNode::Value(UnifiedTapeValue::Bool(true)));
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Int(42)));
        tape.nodes
            .push(TapeNode::Value(UnifiedTapeValue::Float(1.5)));
        tape.nodes
            .push(TapeNode::Value(UnifiedTapeValue::RawNumber(Cow::Borrowed(
                "1e10",
            ))));
        tape.nodes.push(TapeNode::ArrayEnd);

        assert!(matches!(tape.value_at(1), Some(CoreTapeValue::Null)));
        assert!(matches!(tape.value_at(2), Some(CoreTapeValue::Bool(true))));
        assert!(matches!(tape.value_at(3), Some(CoreTapeValue::Int(42))));
        if let Some(CoreTapeValue::Float(f)) = tape.value_at(4) {
            assert!((f - 1.5).abs() < 0.001);
        } else {
            panic!("Expected Float");
        }
        assert!(matches!(tape.value_at(5), Some(CoreTapeValue::RawNumber(s)) if s == "1e10"));
    }

    #[test]
    fn test_yaml_format() {
        let tape = UnifiedTape::new(FormatKind::Yaml);
        assert_eq!(tape.format(), FormatKind::Yaml);
    }

    #[test]
    fn test_mixed_structure() {
        // {items: [1, 2], name: "test"}
        let mut tape = UnifiedTape::new(FormatKind::Json);
        tape.nodes.push(TapeNode::ObjectStart { count: 2 });
        tape.nodes.push(TapeNode::Key(Cow::Borrowed("items")));
        tape.nodes.push(TapeNode::ArrayStart { count: 2 });
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Int(1)));
        tape.nodes.push(TapeNode::Value(UnifiedTapeValue::Int(2)));
        tape.nodes.push(TapeNode::ArrayEnd);
        tape.nodes.push(TapeNode::Key(Cow::Borrowed("name")));
        tape.nodes
            .push(TapeNode::Value(UnifiedTapeValue::String(Cow::Borrowed(
                "test",
            ))));
        tape.nodes.push(TapeNode::ObjectEnd);

        let fields: Vec<_> = tape.object_fields(0).unwrap().collect();
        assert_eq!(fields.len(), 2);

        // First field is "items" -> array
        assert_eq!(fields[0], (1, 2));

        assert_eq!(tape.array_elements(2).unwrap().count(), 2);

        // Second field is "name" -> "test"
        assert_eq!(fields[1], (6, 7));
    }
}
