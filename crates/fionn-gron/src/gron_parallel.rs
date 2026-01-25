// SPDX-License-Identifier: MIT OR Apache-2.0
//! Parallel gron implementation using rayon for large files.
//!
//! This module provides a parallelized version of gron that processes
//! large arrays concurrently, providing significant speedup on multi-core systems.

use super::path_builder::PathBuilder;
use super::simd_escape::escape_json_string_simd;
use fionn_core::{DsonError, Result};
use rayon::prelude::*;
use simd_json::value::tape::Node;

/// Options for parallel gron output.
#[derive(Debug, Clone)]
pub struct GronParallelOptions {
    /// Root prefix for paths (default: "json")
    pub prefix: String,
    /// Minimum array size to parallelize (default: 1000)
    pub parallel_threshold: usize,
    /// Number of threads to use (default: rayon default)
    pub num_threads: Option<usize>,
}

impl Default for GronParallelOptions {
    fn default() -> Self {
        Self {
            prefix: "json".to_string(),
            parallel_threshold: 1000,
            num_threads: None,
        }
    }
}

impl GronParallelOptions {
    /// Create options with custom prefix.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // String::to_string() is not const
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            ..Default::default()
        }
    }

    /// Set the parallel threshold.
    #[must_use]
    pub const fn with_threshold(mut self, threshold: usize) -> Self {
        self.parallel_threshold = threshold;
        self
    }
}

/// Convert JSON to gron format using parallel processing for large arrays.
///
/// This function uses rayon to parallelize processing of arrays that exceed
/// the parallel threshold, providing significant speedup for large files.
///
/// # Errors
/// Returns an error if JSON parsing fails.
pub fn gron_parallel(json: &str, options: &GronParallelOptions) -> Result<String> {
    // Parse JSON using simd-json
    let mut bytes = json.as_bytes().to_vec();
    let tape = simd_json::to_tape(&mut bytes)
        .map_err(|e| DsonError::ParseError(format!("JSON parse error: {e}")))?;

    let nodes = &tape.0;
    if nodes.is_empty() {
        return Ok(String::new());
    }

    // Estimate output size (paths expand significantly)
    let estimated_size = json.len() * 3;
    let mut output = Vec::with_capacity(estimated_size);

    let mut path_builder = PathBuilder::new(&options.prefix);

    // Traverse with parallel processing for large arrays
    traverse_parallel(nodes, 0, &mut path_builder, &mut output, options)?;

    // Safety: we only write valid UTF-8
    Ok(unsafe { String::from_utf8_unchecked(output) })
}

/// Traverse the tape with parallel processing for large arrays.
fn traverse_parallel(
    nodes: &[Node<'_>],
    index: usize,
    path: &mut PathBuilder,
    out: &mut Vec<u8>,
    options: &GronParallelOptions,
) -> Result<usize> {
    if index >= nodes.len() {
        return Ok(index);
    }

    let node = &nodes[index];

    match node {
        Node::Object { len, count: _ } => {
            // Output object initialization
            write_line(out, path.current_path(), b"{}");

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

                // Push field onto path
                path.push_field(key);

                // Recurse into value
                idx = traverse_parallel(nodes, idx, path, out, options)?;

                // Pop field from path
                path.pop();
            }

            Ok(idx)
        }

        Node::Array { len, count: _ } => {
            // Output array initialization
            write_line(out, path.current_path(), b"[]");

            let array_len = *len;

            // Check if array is large enough to parallelize
            if array_len >= options.parallel_threshold {
                // Parallel processing for large arrays
                let current_path = path.current_path().to_string();

                // Collect array element indices
                let mut element_indices = Vec::with_capacity(array_len);
                let mut idx = index + 1;
                for i in 0..array_len {
                    if idx >= nodes.len() {
                        break;
                    }
                    element_indices.push((i, idx));
                    idx = skip_value(nodes, idx);
                }

                // Process elements in parallel
                let results: Vec<Vec<u8>> = element_indices
                    .par_iter()
                    .map(|(i, elem_idx)| {
                        let mut local_path = PathBuilder::new(&current_path);
                        // Don't add the root again, just the index
                        local_path.push_index_raw(*i);

                        let mut local_out = Vec::with_capacity(1024);
                        let _ =
                            traverse_sequential(nodes, *elem_idx, &mut local_path, &mut local_out);
                        local_out
                    })
                    .collect();

                // Merge results in order
                for result in results {
                    out.extend_from_slice(&result);
                }

                Ok(idx)
            } else {
                // Sequential processing for small arrays
                let mut idx = index + 1;
                for i in 0..array_len {
                    if idx >= nodes.len() {
                        break;
                    }

                    path.push_index(i);
                    idx = traverse_parallel(nodes, idx, path, out, options)?;
                    path.pop();
                }
                Ok(idx)
            }
        }

        Node::String(s) => {
            let mut value_buf = Vec::with_capacity(s.len() + 2);
            escape_json_string_simd(s, &mut value_buf);
            write_line(out, path.current_path(), &value_buf);
            Ok(index + 1)
        }

        Node::Static(static_node) => {
            use simd_json::StaticNode;
            let value = match static_node {
                StaticNode::Null => b"null".as_slice(),
                StaticNode::Bool(true) => b"true".as_slice(),
                StaticNode::Bool(false) => b"false".as_slice(),
                StaticNode::I64(n) => {
                    let mut buf = itoa::Buffer::new();
                    let s = buf.format(*n);
                    write_line(out, path.current_path(), s.as_bytes());
                    return Ok(index + 1);
                }
                StaticNode::U64(n) => {
                    let mut buf = itoa::Buffer::new();
                    let s = buf.format(*n);
                    write_line(out, path.current_path(), s.as_bytes());
                    return Ok(index + 1);
                }
                StaticNode::F64(n) => {
                    let mut buf = ryu::Buffer::new();
                    let s = buf.format(*n);
                    write_line(out, path.current_path(), s.as_bytes());
                    return Ok(index + 1);
                }
            };
            write_line(out, path.current_path(), value);
            Ok(index + 1)
        }
    }
}

/// Sequential traversal for parallel array elements.
fn traverse_sequential(
    nodes: &[Node<'_>],
    index: usize,
    path: &mut PathBuilder,
    out: &mut Vec<u8>,
) -> usize {
    if index >= nodes.len() {
        return index;
    }

    let node = &nodes[index];

    match node {
        Node::Object { len, count: _ } => {
            write_line(out, path.current_path(), b"{}");

            let mut idx = index + 1;
            for _ in 0..*len {
                if idx >= nodes.len() {
                    break;
                }

                let key = match &nodes[idx] {
                    Node::String(s) => *s,
                    _ => continue,
                };
                idx += 1;

                path.push_field(key);
                idx = traverse_sequential(nodes, idx, path, out);
                path.pop();
            }

            idx
        }

        Node::Array { len, count: _ } => {
            write_line(out, path.current_path(), b"[]");

            let mut idx = index + 1;
            for i in 0..*len {
                if idx >= nodes.len() {
                    break;
                }

                path.push_index(i);
                idx = traverse_sequential(nodes, idx, path, out);
                path.pop();
            }

            idx
        }

        Node::String(s) => {
            let mut value_buf = Vec::with_capacity(s.len() + 2);
            escape_json_string_simd(s, &mut value_buf);
            write_line(out, path.current_path(), &value_buf);
            index + 1
        }

        Node::Static(static_node) => {
            use simd_json::StaticNode;
            let value = match static_node {
                StaticNode::Null => b"null".as_slice(),
                StaticNode::Bool(true) => b"true".as_slice(),
                StaticNode::Bool(false) => b"false".as_slice(),
                StaticNode::I64(n) => {
                    let mut buf = itoa::Buffer::new();
                    let s = buf.format(*n);
                    write_line(out, path.current_path(), s.as_bytes());
                    return index + 1;
                }
                StaticNode::U64(n) => {
                    let mut buf = itoa::Buffer::new();
                    let s = buf.format(*n);
                    write_line(out, path.current_path(), s.as_bytes());
                    return index + 1;
                }
                StaticNode::F64(n) => {
                    let mut buf = ryu::Buffer::new();
                    let s = buf.format(*n);
                    write_line(out, path.current_path(), s.as_bytes());
                    return index + 1;
                }
            };
            write_line(out, path.current_path(), value);
            index + 1
        }
    }
}

/// Skip a value in the tape, returning the index after the value.
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

/// Write a gron line to the output buffer.
#[inline]
fn write_line(out: &mut Vec<u8>, path: &str, value: &[u8]) {
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(b" = ");
    out.extend_from_slice(value);
    out.extend_from_slice(b";\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // GronParallelOptions Tests
    // =========================================================================

    #[test]
    fn test_options_default() {
        let opts = GronParallelOptions::default();
        assert_eq!(opts.prefix, "json");
        assert_eq!(opts.parallel_threshold, 1000);
        assert!(opts.num_threads.is_none());
    }

    #[test]
    fn test_options_with_prefix() {
        let opts = GronParallelOptions::with_prefix("data");
        assert_eq!(opts.prefix, "data");
        assert_eq!(opts.parallel_threshold, 1000);
    }

    #[test]
    fn test_options_with_threshold() {
        let opts = GronParallelOptions::default().with_threshold(500);
        assert_eq!(opts.parallel_threshold, 500);
    }

    #[test]
    fn test_options_debug() {
        let opts = GronParallelOptions::default();
        let debug_str = format!("{opts:?}");
        assert!(debug_str.contains("GronParallelOptions"));
        assert!(debug_str.contains("json"));
    }

    #[test]
    fn test_options_clone() {
        let opts = GronParallelOptions::with_prefix("test").with_threshold(100);
        #[allow(clippy::redundant_clone)] // Test verifies Clone impl correctness
        let cloned = opts.clone();
        // Verify cloned values match original
        assert_eq!(cloned.prefix, opts.prefix);
        assert_eq!(cloned.parallel_threshold, opts.parallel_threshold);
    }

    // =========================================================================
    // gron_parallel Basic Tests
    // =========================================================================

    #[test]
    fn test_parallel_simple() {
        let json = r#"{"name": "Alice", "age": 30}"#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = {};"));
        assert!(output.contains(r#"json.name = "Alice";"#));
        assert!(output.contains("json.age = 30;"));
    }

    #[test]
    fn test_parallel_empty_object() {
        let json = "{}";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = {};"));
    }

    #[test]
    fn test_parallel_empty_array() {
        let json = "[]";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = [];"));
    }

    #[test]
    fn test_parallel_null() {
        let json = "null";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = null;"));
    }

    #[test]
    fn test_parallel_bool_true() {
        let json = "true";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = true;"));
    }

    #[test]
    fn test_parallel_bool_false() {
        let json = "false";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = false;"));
    }

    #[test]
    fn test_parallel_integer() {
        let json = "42";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = 42;"));
    }

    #[test]
    fn test_parallel_negative_integer() {
        let json = "-123";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = -123;"));
    }

    #[test]
    fn test_parallel_float() {
        let json = "1.5";
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json = 1.5;"));
    }

    #[test]
    fn test_parallel_string() {
        let json = r#""hello world""#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains(r#"json = "hello world";"#));
    }

    #[test]
    fn test_parallel_invalid_json() {
        let json = "{ invalid }";
        let result = gron_parallel(json, &GronParallelOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_parallel_custom_prefix() {
        let json = r#"{"key": "value"}"#;
        let opts = GronParallelOptions::with_prefix("root");
        let output = gron_parallel(json, &opts).unwrap();
        assert!(output.contains("root = {};"));
        assert!(output.contains(r#"root.key = "value";"#));
    }

    // =========================================================================
    // Array Processing Tests
    // =========================================================================

    #[test]
    fn test_parallel_small_array() {
        let json = r#"{"items": [1, 2, 3]}"#;
        let options = GronParallelOptions::default().with_threshold(10);
        let output = gron_parallel(json, &options).unwrap();
        assert!(output.contains("json.items = [];"));
        assert!(output.contains("json.items[0] = 1;"));
        assert!(output.contains("json.items[1] = 2;"));
        assert!(output.contains("json.items[2] = 3;"));
    }

    #[test]
    fn test_parallel_large_array() {
        // Create array large enough to trigger parallel processing
        let items: Vec<String> = (0..100).map(|i| format!("{{\"id\": {i}}}")).collect();
        let json = format!(r#"{{"data": [{}]}}"#, items.join(","));

        let options = GronParallelOptions::default().with_threshold(10);
        let output = gron_parallel(&json, &options).unwrap();

        assert!(output.contains("json.data = [];"));
        assert!(output.contains("json.data[0] = {};"));
        assert!(output.contains("json.data[0].id = 0;"));
        assert!(output.contains("json.data[99] = {};"));
        assert!(output.contains("json.data[99].id = 99;"));
    }

    #[test]
    fn test_parallel_large_array_strings() {
        let items: Vec<String> = (0..50).map(|i| format!(r#""item{i}""#)).collect();
        let json = format!("[{}]", items.join(","));

        let options = GronParallelOptions::default().with_threshold(10);
        let output = gron_parallel(&json, &options).unwrap();

        assert!(output.contains(r#"json[0] = "item0";"#));
        assert!(output.contains(r#"json[49] = "item49";"#));
    }

    #[test]
    fn test_parallel_large_array_mixed_types() {
        let json = r#"[1, "two", true, null, 5.5, {"nested": "obj"}, [1,2]]"#;
        let options = GronParallelOptions::default().with_threshold(2);
        let output = gron_parallel(json, &options).unwrap();

        assert!(output.contains("json[0] = 1;"));
        assert!(output.contains(r#"json[1] = "two";"#));
        assert!(output.contains("json[2] = true;"));
        assert!(output.contains("json[3] = null;"));
        assert!(output.contains("json[4] = 5.5;"));
        assert!(output.contains("json[5] = {};"));
        assert!(output.contains(r#"json[5].nested = "obj";"#));
        assert!(output.contains("json[6] = [];"));
    }

    #[test]
    fn test_parallel_nested() {
        let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;
        let options = GronParallelOptions::default().with_threshold(1);
        let output = gron_parallel(json, &options).unwrap();

        assert!(output.contains("json.users[0] = {};"));
        assert!(output.contains(r#"json.users[0].name = "Alice";"#));
        assert!(output.contains("json.users[1] = {};"));
        assert!(output.contains(r#"json.users[1].name = "Bob";"#));
    }

    #[test]
    fn test_parallel_preserves_order() {
        // Verify that parallel processing preserves array order
        let items: Vec<String> = (0..50).map(|i| i.to_string()).collect();
        let json = format!("[{}]", items.join(","));

        let options = GronParallelOptions::default().with_threshold(10);
        let output = gron_parallel(&json, &options).unwrap();

        // Check that elements appear in order
        for i in 0..50 {
            let expected = format!("json[{i}] = {i};");
            assert!(
                output.contains(&expected),
                "Missing or misordered: {expected}"
            );
        }

        // Verify relative ordering in output
        let pos_0 = output.find("json[0] = 0;").unwrap();
        let pos_49 = output.find("json[49] = 49;").unwrap();
        assert!(pos_0 < pos_49, "Elements out of order");
    }

    // =========================================================================
    // Nested Structure Tests
    // =========================================================================

    #[test]
    fn test_parallel_deeply_nested() {
        let json = r#"{"a": {"b": {"c": {"d": 1}}}}"#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json.a.b.c.d = 1;"));
    }

    #[test]
    fn test_parallel_array_of_arrays() {
        let json = "[[1,2],[3,4],[5,6]]";
        let options = GronParallelOptions::default().with_threshold(2);
        let output = gron_parallel(json, &options).unwrap();

        assert!(output.contains("json[0][0] = 1;"));
        assert!(output.contains("json[0][1] = 2;"));
        assert!(output.contains("json[2][1] = 6;"));
    }

    #[test]
    fn test_parallel_object_in_array() {
        let items: Vec<String> = (0..20)
            .map(|i| format!(r#"{{"id": {i}, "name": "item{i}"}}"#))
            .collect();
        let json = format!("[{}]", items.join(","));

        let options = GronParallelOptions::default().with_threshold(5);
        let output = gron_parallel(&json, &options).unwrap();

        assert!(output.contains("json[0].id = 0;"));
        assert!(output.contains(r#"json[0].name = "item0";"#));
        assert!(output.contains("json[19].id = 19;"));
    }

    // =========================================================================
    // Large Number Tests
    // =========================================================================

    #[test]
    fn test_parallel_large_u64() {
        let json = r#"{"big": 9223372036854775807}"#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json.big = 9223372036854775807;"));
    }

    #[test]
    fn test_parallel_large_negative_i64() {
        let json = r#"{"big": -9223372036854775808}"#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json.big = -9223372036854775808;"));
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_parallel_escaped_string() {
        let json = r#"{"text": "hello\nworld"}"#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json.text = "));
    }

    #[test]
    fn test_parallel_unicode() {
        let json = r#"{"emoji": "🎉", "japanese": "日本語"}"#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        assert!(output.contains("json.emoji = "));
        assert!(output.contains("json.japanese = "));
    }

    #[test]
    fn test_parallel_special_field_names() {
        let json = r#"{"with space": 1, "with.dot": 2}"#;
        let output = gron_parallel(json, &GronParallelOptions::default()).unwrap();
        // These should be escaped in some way in the output
        assert!(output.contains("= 1;"));
        assert!(output.contains("= 2;"));
    }

    #[test]
    fn test_parallel_threshold_boundary() {
        // Test exactly at threshold
        let items: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        let json = format!("[{}]", items.join(","));

        let options = GronParallelOptions::default().with_threshold(10);
        let output = gron_parallel(&json, &options).unwrap();

        for i in 0..10 {
            let expected = format!("json[{i}] = {i};");
            assert!(output.contains(&expected));
        }
    }

    #[test]
    fn test_parallel_threshold_just_above() {
        // Test just above threshold to trigger parallel path
        let items: Vec<String> = (0..11).map(|i| i.to_string()).collect();
        let json = format!("[{}]", items.join(","));

        let options = GronParallelOptions::default().with_threshold(10);
        let output = gron_parallel(&json, &options).unwrap();

        for i in 0..11 {
            let expected = format!("json[{i}] = {i};");
            assert!(output.contains(&expected));
        }
    }
}
