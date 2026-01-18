// SPDX-License-Identifier: MIT OR Apache-2.0
//! Zero-copy gron output using Cow.
//!
//! This module provides gron output that minimizes allocations by borrowing
//! from the source JSON when possible and only allocating when necessary
//! (e.g., for escaped strings or computed paths).

use super::path_builder::PathBuilder;
use super::simd_escape::escape_json_to_string;
use fionn_core::{DsonError, Result};
use simd_json::value::tape::Node;
use std::borrow::Cow;

/// A single gron output line with zero-copy semantics where possible.
#[derive(Debug, Clone)]
pub struct GronLine<'a> {
    /// The JSON path (e.g., "json.users\[0\].name")
    pub path: Cow<'a, str>,
    /// The value (e.g., "\"Alice\"" or "42" or "{}" or "[]")
    pub value: Cow<'a, str>,
}

impl<'a> GronLine<'a> {
    /// Create a new gron line with borrowed path and value.
    #[must_use]
    pub const fn borrowed(path: &'a str, value: &'a str) -> Self {
        Self {
            path: Cow::Borrowed(path),
            value: Cow::Borrowed(value),
        }
    }

    /// Create a new gron line with owned path and value.
    #[must_use]
    pub const fn owned(path: String, value: String) -> Self {
        Self {
            path: Cow::Owned(path),
            value: Cow::Owned(value),
        }
    }

    /// Create with owned path and borrowed value.
    #[must_use]
    pub const fn path_owned(path: String, value: &'a str) -> Self {
        Self {
            path: Cow::Owned(path),
            value: Cow::Borrowed(value),
        }
    }

    /// Create with borrowed path and owned value.
    #[must_use]
    pub const fn value_owned(path: &'a str, value: String) -> Self {
        Self {
            path: Cow::Borrowed(path),
            value: Cow::Owned(value),
        }
    }

    /// Format as standard gron line: `path = value;`
    #[must_use]
    pub fn format_standard(&self) -> String {
        format!("{} = {};", self.path, self.value)
    }

    /// Format as compact gron line: `path=value;`
    #[must_use]
    pub fn format_compact(&self) -> String {
        format!("{}={};", self.path, self.value)
    }

    /// Convert to owned version (for storing beyond borrow lifetime).
    #[must_use]
    pub fn into_owned(self) -> GronLine<'static> {
        GronLine {
            path: Cow::Owned(self.path.into_owned()),
            value: Cow::Owned(self.value.into_owned()),
        }
    }
}

/// Zero-copy gron output as a vector of lines.
#[derive(Debug, Default)]
pub struct GronOutput<'a> {
    /// The gron lines.
    pub lines: Vec<GronLine<'a>>,
}

impl<'a> GronOutput<'a> {
    /// Create a new empty output.
    #[must_use]
    pub const fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Create with pre-allocated capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            lines: Vec::with_capacity(capacity),
        }
    }

    /// Add a line to the output.
    pub fn push(&mut self, line: GronLine<'a>) {
        self.lines.push(line);
    }

    /// Get the number of lines.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.lines.len()
    }

    /// Check if output is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Iterate over lines.
    pub fn iter(&self) -> impl Iterator<Item = &GronLine<'a>> {
        self.lines.iter()
    }

    /// Convert to standard formatted string.
    #[must_use]
    pub fn to_standard_string(&self) -> String {
        let estimated_len: usize = self
            .lines
            .iter()
            .map(|l| l.path.len() + l.value.len() + 5)
            .sum();
        let mut result = String::with_capacity(estimated_len);
        for line in &self.lines {
            result.push_str(&line.path);
            result.push_str(" = ");
            result.push_str(&line.value);
            result.push_str(";\n");
        }
        result
    }

    /// Convert to compact formatted string.
    #[must_use]
    pub fn to_compact_string(&self) -> String {
        let estimated_len: usize = self
            .lines
            .iter()
            .map(|l| l.path.len() + l.value.len() + 3)
            .sum();
        let mut result = String::with_capacity(estimated_len);
        for line in &self.lines {
            result.push_str(&line.path);
            result.push('=');
            result.push_str(&line.value);
            result.push_str(";\n");
        }
        result
    }

    /// Convert to owned version.
    #[must_use]
    pub fn into_owned(self) -> GronOutput<'static> {
        GronOutput {
            lines: self.lines.into_iter().map(GronLine::into_owned).collect(),
        }
    }
}

impl<'a> IntoIterator for GronOutput<'a> {
    type Item = GronLine<'a>;
    type IntoIter = std::vec::IntoIter<GronLine<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.into_iter()
    }
}

impl<'a, 'b> IntoIterator for &'b GronOutput<'a> {
    type Item = &'b GronLine<'a>;
    type IntoIter = std::slice::Iter<'b, GronLine<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.iter()
    }
}

/// Convert JSON to zero-copy gron output.
///
/// # Errors
/// Returns an error if JSON parsing fails.
///
/// # Example
/// ```rust,ignore
/// let json = r#"{"name": "Alice", "age": 30}"#;
/// let output = gron_zerocopy(json, "json")?;
/// for line in &output {
///     println!("{}", line.format_standard());
/// }
/// ```
pub fn gron_zerocopy<'a>(json: &'a str, prefix: &str) -> Result<GronOutput<'a>> {
    let mut bytes = json.as_bytes().to_vec();
    let tape = simd_json::to_tape(&mut bytes)
        .map_err(|e| DsonError::ParseError(format!("JSON parse error: {e}")))?;

    let nodes = &tape.0;
    if nodes.is_empty() {
        return Ok(GronOutput::new());
    }

    // Estimate output size
    let estimated_lines = nodes.len();
    let mut output = GronOutput::with_capacity(estimated_lines);
    let mut path_builder = PathBuilder::new(prefix);

    traverse_zerocopy(nodes, 0, &mut path_builder, &mut output)?;

    Ok(output)
}

/// Traverse tape and generate zero-copy gron output.
fn traverse_zerocopy(
    nodes: &[Node<'_>],
    index: usize,
    path: &mut PathBuilder,
    output: &mut GronOutput<'_>,
) -> Result<usize> {
    if index >= nodes.len() {
        return Ok(index);
    }

    let node = &nodes[index];

    match node {
        Node::Object { len, count: _ } => {
            // Object initialization - path is computed, value is static
            output.push(GronLine::path_owned(path.current_path().to_string(), "{}"));

            let mut idx = index + 1;
            for _ in 0..*len {
                if idx >= nodes.len() {
                    break;
                }

                // Get field name
                let key = match &nodes[idx] {
                    Node::String(s) => *s,
                    _ => continue,
                };
                idx += 1;

                path.push_field(key);
                idx = traverse_zerocopy(nodes, idx, path, output)?;
                path.pop();
            }

            Ok(idx)
        }

        Node::Array { len, count: _ } => {
            // Array initialization - path is computed, value is static
            output.push(GronLine::path_owned(path.current_path().to_string(), "[]"));

            let mut idx = index + 1;
            for i in 0..*len {
                if idx >= nodes.len() {
                    break;
                }

                path.push_index(i);
                idx = traverse_zerocopy(nodes, idx, path, output)?;
                path.pop();
            }

            Ok(idx)
        }

        Node::String(s) => {
            // String value - escape and quote (escape_json_to_string includes quotes)
            let value = escape_json_to_string(s);
            output.push(GronLine::owned(path.current_path().to_string(), value));
            Ok(index + 1)
        }

        Node::Static(static_node) => {
            // Static values (null, true, false) - use static strings
            let value = match static_node {
                simd_json::StaticNode::Null => "null",
                simd_json::StaticNode::Bool(true) => "true",
                simd_json::StaticNode::Bool(false) => "false",
                simd_json::StaticNode::I64(n) => {
                    output.push(GronLine::owned(
                        path.current_path().to_string(),
                        n.to_string(),
                    ));
                    return Ok(index + 1);
                }
                simd_json::StaticNode::U64(n) => {
                    output.push(GronLine::owned(
                        path.current_path().to_string(),
                        n.to_string(),
                    ));
                    return Ok(index + 1);
                }
                simd_json::StaticNode::F64(n) => {
                    output.push(GronLine::owned(
                        path.current_path().to_string(),
                        n.to_string(),
                    ));
                    return Ok(index + 1);
                }
            };
            output.push(GronLine::path_owned(path.current_path().to_string(), value));
            Ok(index + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_object() {
        let json = r#"{"name": "Alice", "age": 30}"#;
        let output = gron_zerocopy(json, "json").unwrap();

        assert_eq!(output.len(), 3);
        assert_eq!(output.lines[0].path, "json");
        assert_eq!(output.lines[0].value, "{}");
    }

    #[test]
    fn test_nested_object() {
        let json = r#"{"user": {"name": "Bob"}}"#;
        let output = gron_zerocopy(json, "json").unwrap();

        assert_eq!(output.len(), 3);
        assert!(output.to_standard_string().contains("json.user.name"));
    }

    #[test]
    fn test_array() {
        let json = "[1, 2, 3]";
        let output = gron_zerocopy(json, "json").unwrap();

        assert_eq!(output.len(), 4); // [] + 3 elements
        assert_eq!(output.lines[0].value, "[]");
    }

    #[test]
    fn test_format_standard() {
        let line = GronLine::borrowed("json.name", "\"Alice\"");
        assert_eq!(line.format_standard(), r#"json.name = "Alice";"#);
    }

    #[test]
    fn test_format_compact() {
        let line = GronLine::borrowed("json.name", "\"Alice\"");
        assert_eq!(line.format_compact(), r#"json.name="Alice";"#);
    }

    #[test]
    fn test_into_owned() {
        let json = r#"{"x": 1}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let owned: GronOutput<'static> = output.into_owned();

        // Should work beyond original lifetime
        assert!(!owned.is_empty());
    }

    #[test]
    fn test_static_values() {
        let json = r#"{"a": true, "b": false, "c": null}"#;
        let output = gron_zerocopy(json, "json").unwrap();

        let standard = output.to_standard_string();
        assert!(standard.contains("true"));
        assert!(standard.contains("false"));
        assert!(standard.contains("null"));
    }

    #[test]
    fn test_iteration() {
        let json = r#"{"a": 1, "b": 2}"#;
        let output = gron_zerocopy(json, "json").unwrap();

        let mut count = 0;
        for _line in &output {
            count += 1;
        }
        assert_eq!(count, 3);
    }

    // =========================================================================
    // GronLine Constructor Tests
    // =========================================================================

    #[test]
    fn test_gron_line_borrowed() {
        let line = GronLine::borrowed("json.name", "\"Alice\"");
        assert!(matches!(&line.path, Cow::Borrowed(_)));
        assert!(matches!(&line.value, Cow::Borrowed(_)));
        assert_eq!(&*line.path, "json.name");
        assert_eq!(&*line.value, "\"Alice\"");
    }

    #[test]
    fn test_gron_line_owned() {
        let line = GronLine::owned("json.name".to_string(), "\"Alice\"".to_string());
        assert!(matches!(&line.path, Cow::Owned(_)));
        assert!(matches!(&line.value, Cow::Owned(_)));
        assert_eq!(&*line.path, "json.name");
        assert_eq!(&*line.value, "\"Alice\"");
    }

    #[test]
    fn test_gron_line_path_owned() {
        let line = GronLine::path_owned("json.name".to_string(), "\"Alice\"");
        assert!(matches!(&line.path, Cow::Owned(_)));
        assert!(matches!(&line.value, Cow::Borrowed(_)));
    }

    #[test]
    fn test_gron_line_value_owned() {
        let path = "json.name";
        let line = GronLine::value_owned(path, "\"Alice\"".to_string());
        assert!(matches!(&line.path, Cow::Borrowed(_)));
        assert!(matches!(&line.value, Cow::Owned(_)));
    }

    #[test]
    fn test_gron_line_into_owned() {
        let line = GronLine::borrowed("json.x", "42");
        let owned = line.into_owned();
        assert!(matches!(&owned.path, Cow::Owned(_)));
        assert!(matches!(&owned.value, Cow::Owned(_)));
        assert_eq!(&*owned.path, "json.x");
        assert_eq!(&*owned.value, "42");
    }

    #[test]
    fn test_gron_line_debug() {
        let line = GronLine::borrowed("json", "{}");
        let debug_str = format!("{line:?}");
        assert!(debug_str.contains("GronLine"));
        assert!(debug_str.contains("path"));
        assert!(debug_str.contains("value"));
    }

    #[test]
    fn test_gron_line_clone() {
        let line = GronLine::borrowed("json.x", "1");
        let cloned = line.clone();
        assert_eq!(&*cloned.path, &*line.path);
        assert_eq!(&*cloned.value, &*line.value);
    }

    // =========================================================================
    // GronOutput Tests
    // =========================================================================

    #[test]
    fn test_gron_output_new() {
        let output: GronOutput<'_> = GronOutput::new();
        assert!(output.is_empty());
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn test_gron_output_with_capacity() {
        let output: GronOutput<'_> = GronOutput::with_capacity(100);
        assert!(output.is_empty());
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn test_gron_output_push() {
        let mut output = GronOutput::new();
        output.push(GronLine::borrowed("json", "{}"));
        output.push(GronLine::borrowed("json.x", "1"));

        assert_eq!(output.len(), 2);
        assert!(!output.is_empty());
    }

    #[test]
    fn test_gron_output_iter() {
        let mut output = GronOutput::new();
        output.push(GronLine::borrowed("json", "{}"));
        output.push(GronLine::borrowed("json.a", "1"));
        output.push(GronLine::borrowed("json.b", "2"));

        let paths: Vec<&str> = output.iter().map(|l| &*l.path).collect();
        assert_eq!(paths, vec!["json", "json.a", "json.b"]);
    }

    #[test]
    fn test_gron_output_to_compact_string() {
        let mut output = GronOutput::new();
        output.push(GronLine::borrowed("json", "{}"));
        output.push(GronLine::borrowed("json.name", "\"Alice\""));

        let compact = output.to_compact_string();
        assert!(compact.contains("json={};"));
        assert!(compact.contains("json.name=\"Alice\";"));
    }

    #[test]
    fn test_gron_output_to_standard_string() {
        let mut output = GronOutput::new();
        output.push(GronLine::borrowed("json", "{}"));
        output.push(GronLine::borrowed("json.name", "\"Alice\""));

        let standard = output.to_standard_string();
        assert!(standard.contains("json = {};"));
        assert!(standard.contains("json.name = \"Alice\";"));
    }

    #[test]
    fn test_gron_output_into_owned() {
        let json = r#"{"x": 1, "y": 2}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let owned = output.into_owned();

        // Verify it's 'static lifetime
        let _: GronOutput<'static> = owned;
    }

    #[test]
    fn test_gron_output_debug() {
        let output = GronOutput::new();
        let debug_str = format!("{output:?}");
        assert!(debug_str.contains("GronOutput"));
    }

    #[test]
    fn test_gron_output_default() {
        let output: GronOutput<'_> = GronOutput::default();
        assert!(output.is_empty());
    }

    // =========================================================================
    // IntoIterator Tests
    // =========================================================================

    #[test]
    fn test_into_iterator_owned() {
        let mut output = GronOutput::new();
        output.push(GronLine::borrowed("json", "{}"));
        output.push(GronLine::borrowed("json.x", "1"));

        let lines_count = output.into_iter().count();
        assert_eq!(lines_count, 2);
    }

    #[test]
    fn test_into_iterator_ref() {
        let mut output = GronOutput::new();
        output.push(GronLine::borrowed("json", "{}"));
        output.push(GronLine::borrowed("json.x", "1"));

        let count: usize = (&output).into_iter().count();
        assert_eq!(count, 2);
        // output is still valid
        assert_eq!(output.len(), 2);
    }

    // =========================================================================
    // Value Type Tests
    // =========================================================================

    #[test]
    fn test_integer_values() {
        let json = r#"{"pos": 42, "neg": -10, "zero": 0}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("42"));
        assert!(standard.contains("-10"));
        assert!(standard.contains('0'));
    }

    #[test]
    fn test_large_integer_i64() {
        let json = r#"{"big": -9223372036854775808}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("-9223372036854775808"));
    }

    #[test]
    fn test_large_integer_u64() {
        let json = r#"{"big": 18446744073709551615}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("18446744073709551615"));
    }

    #[test]
    fn test_float_values() {
        let json = r#"{"pi": 1.5, "neg": -1.5, "exp": 1e10}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("1.5"));
        assert!(standard.contains("-1.5"));
    }

    #[test]
    fn test_boolean_values() {
        let json = r#"{"yes": true, "no": false}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("= true;"));
        assert!(standard.contains("= false;"));
    }

    #[test]
    fn test_null_value() {
        let json = r#"{"nothing": null}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("= null;"));
    }

    #[test]
    fn test_string_values() {
        let json = r#"{"msg": "hello world"}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("\"hello world\""));
    }

    #[test]
    fn test_string_with_escapes() {
        let json = r#"{"msg": "line1\nline2"}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("json.msg = "));
    }

    // =========================================================================
    // Complex Structure Tests
    // =========================================================================

    #[test]
    fn test_empty_object() {
        let json = "{}";
        let output = gron_zerocopy(json, "data").unwrap();

        assert_eq!(output.len(), 1);
        assert_eq!(&*output.lines[0].path, "data");
        assert_eq!(&*output.lines[0].value, "{}");
    }

    #[test]
    fn test_empty_array() {
        let json = "[]";
        let output = gron_zerocopy(json, "arr").unwrap();

        assert_eq!(output.len(), 1);
        assert_eq!(&*output.lines[0].value, "[]");
    }

    #[test]
    fn test_nested_arrays() {
        let json = "[[1, 2], [3, 4]]";
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("json = [];"));
        assert!(standard.contains("json[0] = [];"));
        assert!(standard.contains("json[0][0] = 1;"));
        assert!(standard.contains("json[1][1] = 4;"));
    }

    #[test]
    fn test_nested_objects() {
        let json = r#"{"a": {"b": {"c": 1}}}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("json = {};"));
        assert!(standard.contains("json.a = {};"));
        assert!(standard.contains("json.a.b = {};"));
        assert!(standard.contains("json.a.b.c = 1;"));
    }

    #[test]
    fn test_mixed_object_array() {
        let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;
        let output = gron_zerocopy(json, "json").unwrap();
        let standard = output.to_standard_string();

        assert!(standard.contains("json.users = [];"));
        assert!(standard.contains("json.users[0] = {};"));
        assert!(standard.contains("json.users[0].name = \"Alice\";"));
        assert!(standard.contains("json.users[1].name = \"Bob\";"));
    }

    // =========================================================================
    // Error Handling Tests
    // =========================================================================

    #[test]
    fn test_invalid_json() {
        let json = r#"{"invalid": json}"#;
        let result = gron_zerocopy(json, "json");
        assert!(result.is_err());
    }

    #[test]
    fn test_unclosed_object() {
        let json = r#"{"name": "Alice""#;
        let result = gron_zerocopy(json, "json");
        assert!(result.is_err());
    }

    // =========================================================================
    // Custom Prefix Tests
    // =========================================================================

    #[test]
    fn test_custom_prefix() {
        let json = r#"{"x": 1}"#;
        let output = gron_zerocopy(json, "data").unwrap();

        assert_eq!(&*output.lines[0].path, "data");
        assert!(output.to_standard_string().contains("data.x = 1;"));
    }

    #[test]
    fn test_empty_prefix() {
        let json = r#"{"x": 1}"#;
        let output = gron_zerocopy(json, "").unwrap();

        assert!(output.to_standard_string().contains(".x = 1;"));
    }

    // =========================================================================
    // Format Output Tests
    // =========================================================================

    #[test]
    fn test_compact_vs_standard() {
        let json = r#"{"a": 1, "b": 2}"#;
        let output = gron_zerocopy(json, "json").unwrap();

        let standard = output.to_standard_string();
        let compact = output.to_compact_string();

        // Standard has spaces around =
        assert!(standard.contains(" = "));
        // Compact has no spaces
        assert!(compact.contains("json.a=1;"));
        assert!(compact.contains("json.b=2;"));
    }

    // =========================================================================
    // Line Format Tests
    // =========================================================================

    #[test]
    fn test_line_format_with_array_index() {
        let line = GronLine::borrowed("json.items[0]", "\"first\"");
        assert_eq!(line.format_standard(), "json.items[0] = \"first\";");
        assert_eq!(line.format_compact(), "json.items[0]=\"first\";");
    }

    #[test]
    fn test_line_format_with_nested_path() {
        let line = GronLine::borrowed("json.a.b.c.d", "true");
        assert_eq!(line.format_standard(), "json.a.b.c.d = true;");
    }
}
