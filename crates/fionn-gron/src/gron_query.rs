// SPDX-License-Identifier: MIT OR Apache-2.0
//! Query-filtered gron output.
//!
//! Applies a query filter during gron traversal for efficient filtered output.

use super::gron_core::GronOptions;
use super::path_builder::PathBuilder;
use super::query::{MatchPotential, Query};
use super::simd_utils::escape_json_string;
use fionn_core::{DsonError, Result};
use simd_json::value::tape::Node;
use std::io::Write;

/// Options for query-filtered gron.
#[derive(Debug, Clone, Default)]
pub struct GronQueryOptions {
    /// Base gron options
    pub gron: GronOptions,
    /// Maximum matches (0 = unlimited)
    pub max_matches: usize,
    /// Include parent container declarations
    pub include_containers: bool,
}

impl GronQueryOptions {
    /// Set maximum number of matches.
    #[must_use]
    pub const fn max_matches(mut self, max: usize) -> Self {
        self.max_matches = max;
        self
    }

    /// Include container declarations for matching paths.
    #[must_use]
    pub const fn include_containers(mut self) -> Self {
        self.include_containers = true;
        self
    }

    /// Set compact output.
    #[must_use]
    pub fn compact(mut self) -> Self {
        self.gron = self.gron.compact();
        self
    }
}

/// Query-filtered gron output.
///
/// # Errors
/// Returns an error if JSON parsing fails.
pub fn gron_query(json: &str, query: &Query, options: &GronQueryOptions) -> Result<String> {
    let mut output = Vec::with_capacity(json.len());
    gron_query_to_writer(json, query, options, &mut output)?;
    Ok(unsafe { String::from_utf8_unchecked(output) })
}

/// Query-filtered gron output to a writer.
///
/// # Errors
/// Returns an error if JSON parsing or writing fails.
pub fn gron_query_to_writer<W: Write>(
    json: &str,
    query: &Query,
    options: &GronQueryOptions,
    writer: &mut W,
) -> Result<usize> {
    let mut bytes = json.as_bytes().to_vec();
    let tape = simd_json::to_tape(&mut bytes)
        .map_err(|e| DsonError::ParseError(format!("JSON parse error: {e}")))?;

    let nodes = &tape.0;
    if nodes.is_empty() {
        return Ok(0);
    }

    let mut path_builder = PathBuilder::new(&options.gron.prefix);
    let mut out = QueryGronWriter::new(writer, &options.gron, query, options.max_matches);

    traverse_query(nodes, 0, &mut path_builder, &mut out, query, options)?;

    Ok(out.bytes_written())
}

/// Writer that filters output based on query.
struct QueryGronWriter<'a, W: Write> {
    writer: &'a mut W,
    buffer: Vec<u8>,
    bytes_written: usize,
    options: &'a GronOptions,
    query: &'a Query,
    match_count: usize,
    max_matches: usize,
}

impl<'a, W: Write> QueryGronWriter<'a, W> {
    fn new(
        writer: &'a mut W,
        options: &'a GronOptions,
        query: &'a Query,
        max_matches: usize,
    ) -> Self {
        Self {
            writer,
            buffer: Vec::with_capacity(65536),
            bytes_written: 0,
            options,
            query,
            match_count: 0,
            max_matches,
        }
    }

    const fn should_stop(&self) -> bool {
        self.max_matches > 0 && self.match_count >= self.max_matches
    }

    fn write_if_matches(&mut self, path: &str, value: &[u8]) -> Result<bool> {
        if self.should_stop() {
            return Ok(false);
        }

        if self.query.matches(path) {
            self.match_count += 1;
            self.write_line(path, value)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn write_line(&mut self, path: &str, value: &[u8]) -> Result<()> {
        if self.options.values_only {
            self.buffer.extend_from_slice(value);
            self.buffer.push(b'\n');
        } else if self.options.paths_only {
            self.buffer.extend_from_slice(path.as_bytes());
            self.buffer.push(b'\n');
        } else if self.options.compact {
            self.buffer.extend_from_slice(path.as_bytes());
            self.buffer.push(b'=');
            self.buffer.extend_from_slice(value);
            self.buffer.extend_from_slice(b";\n");
        } else {
            self.buffer.extend_from_slice(path.as_bytes());
            self.buffer.extend_from_slice(b" = ");
            self.buffer.extend_from_slice(value);
            self.buffer.extend_from_slice(b";\n");
        }

        if self.buffer.len() >= 60000 {
            self.flush()?;
        }

        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if !self.buffer.is_empty() {
            self.writer
                .write_all(&self.buffer)
                .map_err(DsonError::IoError)?;
            self.bytes_written += self.buffer.len();
            self.buffer.clear();
        }
        Ok(())
    }

    fn bytes_written(&mut self) -> usize {
        let _ = self.flush();
        self.bytes_written
    }
}

/// Traverse tape with query filtering.
fn traverse_query<W: Write>(
    nodes: &[Node<'_>],
    index: usize,
    path: &mut PathBuilder,
    out: &mut QueryGronWriter<'_, W>,
    query: &Query,
    options: &GronQueryOptions,
) -> Result<usize> {
    if index >= nodes.len() || out.should_stop() {
        return Ok(index);
    }

    let current_path = path.current_path().to_string();

    // Check if we should even traverse this subtree
    let potential = query.match_potential(&current_path);
    if potential == MatchPotential::NoMatch && !query.has_recursive() {
        // Skip this subtree entirely
        return Ok(skip_value(nodes, index));
    }

    let node = &nodes[index];

    match node {
        Node::Object { len, count: _ } => {
            // Check if this object path matches
            if options.include_containers {
                out.write_if_matches(&current_path, b"{}")?;
            }

            let mut idx = index + 1;
            for _ in 0..*len {
                if idx >= nodes.len() || out.should_stop() {
                    break;
                }

                let key = match &nodes[idx] {
                    Node::String(s) => *s,
                    _ => continue,
                };
                idx += 1;

                path.push_field(key);
                idx = traverse_query(nodes, idx, path, out, query, options)?;
                path.pop();
            }

            Ok(idx)
        }

        Node::Array { len, count: _ } => {
            if options.include_containers {
                out.write_if_matches(&current_path, b"[]")?;
            }

            let mut idx = index + 1;
            for i in 0..*len {
                if idx >= nodes.len() || out.should_stop() {
                    break;
                }

                path.push_index(i);
                idx = traverse_query(nodes, idx, path, out, query, options)?;
                path.pop();
            }

            Ok(idx)
        }

        Node::String(s) => {
            let mut value_buf = Vec::with_capacity(s.len() + 2);
            escape_json_string(s, &mut value_buf);
            out.write_if_matches(&current_path, &value_buf)?;
            Ok(index + 1)
        }

        Node::Static(static_node) => {
            use simd_json::StaticNode;
            match static_node {
                StaticNode::Null => {
                    out.write_if_matches(&current_path, b"null")?;
                }
                StaticNode::Bool(true) => {
                    out.write_if_matches(&current_path, b"true")?;
                }
                StaticNode::Bool(false) => {
                    out.write_if_matches(&current_path, b"false")?;
                }
                StaticNode::I64(n) => {
                    let mut buf = itoa::Buffer::new();
                    let s = buf.format(*n);
                    out.write_if_matches(&current_path, s.as_bytes())?;
                }
                StaticNode::U64(n) => {
                    let mut buf = itoa::Buffer::new();
                    let s = buf.format(*n);
                    out.write_if_matches(&current_path, s.as_bytes())?;
                }
                StaticNode::F64(n) => {
                    let mut buf = ryu::Buffer::new();
                    let s = buf.format(*n);
                    out.write_if_matches(&current_path, s.as_bytes())?;
                }
            }
            Ok(index + 1)
        }
    }
}

/// Skip over a value in the tape without processing.
fn skip_value(nodes: &[Node<'_>], index: usize) -> usize {
    if index >= nodes.len() {
        return index;
    }

    match &nodes[index] {
        Node::Object { len, count: _ } => {
            let mut idx = index + 1;
            for _ in 0..*len {
                if idx >= nodes.len() {
                    break;
                }
                // Skip key
                idx += 1;
                // Skip value
                idx = skip_value(nodes, idx);
            }
            idx
        }
        Node::Array { len, count: _ } => {
            let mut idx = index + 1;
            for _ in 0..*len {
                if idx >= nodes.len() {
                    break;
                }
                idx = skip_value(nodes, idx);
            }
            idx
        }
        _ => index + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_gron(json: &str, query_str: &str) -> String {
        let query = Query::parse(query_str).unwrap();
        gron_query(json, &query, &GronQueryOptions::default()).unwrap()
    }

    #[test]
    fn test_simple_field_query() {
        let json = r#"{"name": "Alice", "age": 30}"#;
        let output = query_gron(json, ".name");

        assert!(output.contains(r#"json.name = "Alice";"#));
        assert!(!output.contains("age"));
    }

    #[test]
    fn test_nested_field_query() {
        let json = r#"{"user": {"name": "Alice", "email": "alice@example.com"}}"#;
        let output = query_gron(json, ".user.name");

        assert!(output.contains(r#"json.user.name = "Alice";"#));
        assert!(!output.contains("email"));
    }

    #[test]
    fn test_array_index_query() {
        let json = r#"{"items": [1, 2, 3]}"#;
        let output = query_gron(json, ".items[0]");

        assert!(output.contains("json.items[0] = 1;"));
        assert!(!output.contains("items[1]"));
        assert!(!output.contains("items[2]"));
    }

    #[test]
    fn test_wildcard_query() {
        let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;
        let output = query_gron(json, ".users[*].name");

        assert!(output.contains(r#"json.users[0].name = "Alice";"#));
        assert!(output.contains(r#"json.users[1].name = "Bob";"#));
        // Should not include the user objects themselves
        assert!(!output.contains("json.users[0] = {};"));
    }

    #[test]
    fn test_recursive_query() {
        let json = r#"{"a": {"error": "x"}, "b": {"c": {"error": "y"}}}"#;
        let output = query_gron(json, "..error");

        assert!(output.contains(r#"json.a.error = "x";"#));
        assert!(output.contains(r#"json.b.c.error = "y";"#));
    }

    #[test]
    fn test_max_matches() {
        let json = r#"{"items": [1, 2, 3, 4, 5]}"#;
        let query = Query::parse(".items[*]").unwrap();
        let options = GronQueryOptions::default().max_matches(2);
        let output = gron_query(json, &query, &options).unwrap();

        assert!(output.contains("json.items[0] = 1;"));
        assert!(output.contains("json.items[1] = 2;"));
        assert!(!output.contains("items[2]"));
    }

    #[test]
    fn test_include_containers() {
        let json = r#"{"users": [{"name": "Alice"}]}"#;
        let query = Query::parse(".users[*].name").unwrap();
        let options = GronQueryOptions::default().include_containers();
        let output = gron_query(json, &query, &options).unwrap();

        // Should include matching containers
        assert!(output.contains("json.users[0].name"));
    }

    #[test]
    fn test_complex_nested_query() {
        let json = r#"{
            "data": [
                {"users": [{"name": "Alice"}, {"name": "Bob"}]},
                {"users": [{"name": "Charlie"}]}
            ]
        }"#;
        let output = query_gron(json, ".data[*].users[*].name");

        assert!(output.contains(r#"json.data[0].users[0].name = "Alice";"#));
        assert!(output.contains(r#"json.data[0].users[1].name = "Bob";"#));
        assert!(output.contains(r#"json.data[1].users[0].name = "Charlie";"#));
    }

    #[test]
    fn test_no_matches() {
        let json = r#"{"name": "Alice"}"#;
        let output = query_gron(json, ".nonexistent");

        assert!(output.is_empty());
    }

    #[test]
    fn test_paths_only_with_query() {
        let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;
        let query = Query::parse(".users[*].name").unwrap();
        let mut options = GronQueryOptions::default();
        options.gron = options.gron.paths_only();
        let output = gron_query(json, &query, &options).unwrap();

        assert!(output.contains("json.users[0].name\n"));
        assert!(output.contains("json.users[1].name\n"));
        assert!(!output.contains("Alice"));
    }

    #[test]
    fn test_values_only_with_query() {
        let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;
        let query = Query::parse(".users[*].name").unwrap();
        let mut options = GronQueryOptions::default();
        options.gron = options.gron.values_only();
        let output = gron_query(json, &query, &options).unwrap();

        assert!(output.contains("\"Alice\"\n"));
        assert!(output.contains("\"Bob\"\n"));
        assert!(!output.contains("json.users"));
    }

    // =========================================================================
    // GronQueryOptions Builder Tests
    // =========================================================================

    #[test]
    fn test_options_default() {
        let options = GronQueryOptions::default();
        assert_eq!(options.max_matches, 0);
        assert!(!options.include_containers);
        assert!(!options.gron.compact);
    }

    #[test]
    fn test_options_max_matches() {
        let options = GronQueryOptions::default().max_matches(10);
        assert_eq!(options.max_matches, 10);
    }

    #[test]
    fn test_options_include_containers() {
        let options = GronQueryOptions::default().include_containers();
        assert!(options.include_containers);
    }

    #[test]
    fn test_options_compact() {
        let options = GronQueryOptions::default().compact();
        assert!(options.gron.compact);
    }

    #[test]
    fn test_options_chained() {
        let options = GronQueryOptions::default()
            .max_matches(5)
            .include_containers()
            .compact();
        assert_eq!(options.max_matches, 5);
        assert!(options.include_containers);
        assert!(options.gron.compact);
    }

    #[test]
    fn test_options_debug() {
        let options = GronQueryOptions::default().max_matches(3);
        let debug_str = format!("{options:?}");
        assert!(debug_str.contains("GronQueryOptions"));
        assert!(debug_str.contains("max_matches"));
    }

    #[test]
    fn test_options_clone() {
        let options = GronQueryOptions::default()
            .max_matches(7)
            .include_containers();
        #[allow(clippy::redundant_clone)] // Test verifies Clone impl correctness
        let cloned = options.clone();
        assert_eq!(cloned.max_matches, options.max_matches);
        assert_eq!(cloned.include_containers, options.include_containers);
    }

    // =========================================================================
    // Compact Output Tests
    // =========================================================================

    #[test]
    fn test_compact_output() {
        let json = r#"{"name": "Alice"}"#;
        let query = Query::parse(".name").unwrap();
        let options = GronQueryOptions::default().compact();
        let output = gron_query(json, &query, &options).unwrap();

        // Compact format uses = without spaces and ;
        assert!(output.contains("json.name=\"Alice\";"));
    }

    #[test]
    fn test_compact_vs_non_compact() {
        let json = r#"{"value": 42}"#;
        let query = Query::parse(".value").unwrap();

        let compact_options = GronQueryOptions::default().compact();
        let compact_output = gron_query(json, &query, &compact_options).unwrap();

        let normal_options = GronQueryOptions::default();
        let normal_output = gron_query(json, &query, &normal_options).unwrap();

        assert!(compact_output.contains("json.value=42;"));
        assert!(normal_output.contains("json.value = 42;"));
    }

    // =========================================================================
    // Static Node Type Tests
    // =========================================================================

    #[test]
    fn test_query_null_value() {
        let json = r#"{"value": null}"#;
        let output = query_gron(json, ".value");
        assert!(output.contains("json.value = null;"));
    }

    #[test]
    fn test_query_bool_true() {
        let json = r#"{"active": true}"#;
        let output = query_gron(json, ".active");
        assert!(output.contains("json.active = true;"));
    }

    #[test]
    fn test_query_bool_false() {
        let json = r#"{"active": false}"#;
        let output = query_gron(json, ".active");
        assert!(output.contains("json.active = false;"));
    }

    #[test]
    fn test_query_integer() {
        let json = r#"{"count": 42}"#;
        let output = query_gron(json, ".count");
        assert!(output.contains("json.count = 42;"));
    }

    #[test]
    fn test_query_negative_integer() {
        let json = r#"{"temp": -10}"#;
        let output = query_gron(json, ".temp");
        assert!(output.contains("json.temp = -10;"));
    }

    #[test]
    fn test_query_large_integer() {
        let json = r#"{"big": 9223372036854775807}"#;
        let output = query_gron(json, ".big");
        assert!(output.contains("json.big = 9223372036854775807;"));
    }

    #[test]
    fn test_query_float() {
        let json = r#"{"pi": 1.5}"#;
        let output = query_gron(json, ".pi");
        assert!(output.contains("json.pi = 1.5;"));
    }

    // =========================================================================
    // Writer and Writer Integration Tests
    // =========================================================================

    #[test]
    fn test_gron_query_to_writer() {
        let json = r#"{"name": "Alice"}"#;
        let query = Query::parse(".name").unwrap();
        let options = GronQueryOptions::default();
        let mut output = Vec::new();

        let bytes = gron_query_to_writer(json, &query, &options, &mut output).unwrap();

        assert!(bytes > 0);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains(r#"json.name = "Alice";"#));
    }

    #[test]
    fn test_empty_json() {
        let json = r"{}";
        let output = query_gron(json, ".anything");
        assert!(output.is_empty());
    }

    #[test]
    fn test_empty_array() {
        let json = r#"{"items": []}"#;
        let output = query_gron(json, ".items[*]");
        assert!(output.is_empty());
    }

    // =========================================================================
    // Skip Value Tests
    // =========================================================================

    #[test]
    fn test_skip_nested_object() {
        // Query that doesn't match should skip entire subtrees
        let json = r#"{"a": {"deep": {"nested": {"value": 1}}}, "b": 2}"#;
        let output = query_gron(json, ".b");
        assert!(output.contains("json.b = 2;"));
        assert!(!output.contains("deep"));
        assert!(!output.contains("nested"));
    }

    #[test]
    fn test_skip_nested_array() {
        let json = r#"{"arr": [[[1, 2], [3, 4]], [[5, 6]]], "target": "found"}"#;
        let output = query_gron(json, ".target");
        assert!(output.contains(r#"json.target = "found";"#));
    }

    // =========================================================================
    // Include Containers Tests
    // =========================================================================

    #[test]
    fn test_include_containers_object() {
        let json = r#"{"user": {"name": "Alice"}}"#;
        let query = Query::parse(".user.name").unwrap();
        let options = GronQueryOptions::default().include_containers();
        let output = gron_query(json, &query, &options).unwrap();

        // With include_containers, should include parent containers if they match
        assert!(output.contains("json.user.name"));
    }

    #[test]
    fn test_include_containers_array() {
        let json = r#"{"items": [1, 2, 3]}"#;
        let query = Query::parse(".items[0]").unwrap();
        let options = GronQueryOptions::default().include_containers();
        let output = gron_query(json, &query, &options).unwrap();

        assert!(output.contains("json.items[0]"));
    }

    // =========================================================================
    // Max Matches Edge Cases
    // =========================================================================

    #[test]
    fn test_max_matches_zero_means_unlimited() {
        let json = r#"{"items": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]}"#;
        let query = Query::parse(".items[*]").unwrap();
        let options = GronQueryOptions::default().max_matches(0);
        let output = gron_query(json, &query, &options).unwrap();

        // max_matches=0 means unlimited
        assert!(output.contains("items[9]"));
    }

    #[test]
    fn test_max_matches_one() {
        let json = r#"{"items": [1, 2, 3]}"#;
        let query = Query::parse(".items[*]").unwrap();
        let options = GronQueryOptions::default().max_matches(1);
        let output = gron_query(json, &query, &options).unwrap();

        assert!(output.contains("json.items[0] = 1;"));
        assert!(!output.contains("items[1]"));
    }

    #[test]
    fn test_max_matches_exact() {
        let json = r#"{"items": [1, 2, 3]}"#;
        let query = Query::parse(".items[*]").unwrap();
        let options = GronQueryOptions::default().max_matches(3);
        let output = gron_query(json, &query, &options).unwrap();

        assert!(output.contains("items[0]"));
        assert!(output.contains("items[1]"));
        assert!(output.contains("items[2]"));
    }

    // =========================================================================
    // String Escape Tests
    // =========================================================================

    #[test]
    fn test_query_string_with_escapes() {
        let json = r#"{"msg": "hello\nworld"}"#;
        let output = query_gron(json, ".msg");
        assert!(output.contains("json.msg = "));
    }

    #[test]
    fn test_query_string_with_quotes() {
        let json = r#"{"msg": "say \"hello\""}"#;
        let output = query_gron(json, ".msg");
        assert!(output.contains("json.msg = "));
    }

    // =========================================================================
    // Recursive Query Tests
    // =========================================================================

    #[test]
    fn test_recursive_query_deep() {
        let json = r#"{"a": {"b": {"c": {"target": 1}}}}"#;
        let output = query_gron(json, "..target");
        assert!(output.contains("json.a.b.c.target = 1;"));
    }

    #[test]
    fn test_recursive_query_multiple_matches() {
        let json = r#"{"x": {"id": 1}, "y": {"z": {"id": 2}}, "id": 3}"#;
        let output = query_gron(json, "..id");
        assert!(output.contains("json.x.id = 1;"));
        assert!(output.contains("json.y.z.id = 2;"));
        assert!(output.contains("json.id = 3;"));
    }

    // =========================================================================
    // Complex Query Tests
    // =========================================================================

    #[test]
    fn test_wildcard_in_middle() {
        let json = r#"{"users": [{"profile": {"name": "A"}}, {"profile": {"name": "B"}}]}"#;
        let output = query_gron(json, ".users[*].profile.name");
        assert!(output.contains(r#"json.users[0].profile.name = "A";"#));
        assert!(output.contains(r#"json.users[1].profile.name = "B";"#));
    }

    #[test]
    fn test_multiple_array_wildcards() {
        let json = r#"{"matrix": [[1, 2], [3, 4]]}"#;
        let output = query_gron(json, ".matrix[*][*]");
        assert!(output.contains("json.matrix[0][0] = 1;"));
        assert!(output.contains("json.matrix[0][1] = 2;"));
        assert!(output.contains("json.matrix[1][0] = 3;"));
        assert!(output.contains("json.matrix[1][1] = 4;"));
    }

    // =========================================================================
    // Error Handling Tests
    // =========================================================================

    #[test]
    fn test_invalid_json() {
        let json = r#"{"invalid": json"#;
        let query = Query::parse(".field").unwrap();
        let result = gron_query(json, &query, &GronQueryOptions::default());
        assert!(result.is_err());
    }

    // =========================================================================
    // Bytes Written Tests
    // =========================================================================

    #[test]
    fn test_bytes_written_count() {
        let json = r#"{"name": "Alice", "age": 30}"#;
        let query = Query::parse(".name").unwrap();
        let options = GronQueryOptions::default();
        let mut output = Vec::new();

        let bytes = gron_query_to_writer(json, &query, &options, &mut output).unwrap();
        assert_eq!(bytes, output.len());
    }

    #[test]
    fn test_bytes_written_zero_no_matches() {
        let json = r#"{"name": "Alice"}"#;
        let query = Query::parse(".nonexistent").unwrap();
        let options = GronQueryOptions::default();
        let mut output = Vec::new();

        let bytes = gron_query_to_writer(json, &query, &options, &mut output).unwrap();
        assert_eq!(bytes, 0);
        assert!(output.is_empty());
    }

    // =========================================================================
    // Large Output Buffer Flush Tests
    // =========================================================================

    #[test]
    fn test_large_output_triggers_flush() {
        // Create JSON with many items to trigger buffer flush
        let items: Vec<String> = (0..1000).map(|i| format!("\"item{i}\"")).collect();
        let json = format!("{{\"items\": [{}]}}", items.join(","));
        let query = Query::parse(".items[*]").unwrap();
        let options = GronQueryOptions::default();

        let output = gron_query(&json, &query, &options).unwrap();
        assert!(!output.is_empty());
    }
}
