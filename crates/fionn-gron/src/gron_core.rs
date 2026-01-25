// SPDX-License-Identifier: MIT OR Apache-2.0
//! Core gron implementation using simd-json tape.
//!
//! This module provides the main gron transformation that converts JSON
//! into greppable line-oriented output.

use super::path_builder::PathBuilder;
use super::simd_utils::escape_json_string;
use fionn_core::{DsonError, Result};
use simd_json::value::tape::Node;
use std::io::Write;

/// Options for gron output.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // Configuration options are independent boolean flags
pub struct GronOptions {
    /// Root prefix for paths (default: "json")
    pub prefix: String,
    /// Compact output (no spaces around =)
    pub compact: bool,
    /// Sort object keys alphabetically
    pub sort_keys: bool,
    /// Output paths only (no values)
    pub paths_only: bool,
    /// Output values only (no paths)
    pub values_only: bool,
    /// Include type annotations in output
    pub show_types: bool,
    /// Colorized output for terminal
    pub color: bool,
}

impl Default for GronOptions {
    fn default() -> Self {
        Self {
            prefix: "json".to_string(),
            compact: false,
            sort_keys: false,
            paths_only: false,
            values_only: false,
            show_types: false,
            color: false,
        }
    }
}

impl GronOptions {
    /// Create new options with custom prefix.
    #[must_use]
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            ..Default::default()
        }
    }

    /// Set compact mode.
    #[must_use]
    pub const fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Set paths-only mode.
    #[must_use]
    pub const fn paths_only(mut self) -> Self {
        self.paths_only = true;
        self
    }

    /// Set values-only mode.
    #[must_use]
    pub const fn values_only(mut self) -> Self {
        self.values_only = true;
        self
    }

    /// Set color mode for terminal output.
    #[must_use]
    pub const fn color(mut self) -> Self {
        self.color = true;
        self
    }
}

/// Output format for gron.
pub enum GronOutput {
    /// Return as String
    String(String),
    /// Write to a writer
    Written(usize),
}

/// Convert JSON to gron format.
///
/// # Errors
/// Returns an error if JSON parsing fails.
///
/// # Example
/// ```rust,ignore
/// let json = r#"{"name": "Alice", "age": 30}"#;
/// let output = gron(json, &GronOptions::default())?;
/// ```
pub fn gron(json: &str, options: &GronOptions) -> Result<String> {
    let mut output = Vec::with_capacity(json.len() * 2);
    gron_to_writer(json, options, &mut output)?;
    // Safety: we only write valid UTF-8
    Ok(unsafe { String::from_utf8_unchecked(output) })
}

/// Convert JSON to gron format, writing to a writer.
///
/// # Errors
/// Returns an error if JSON parsing or writing fails.
pub fn gron_to_writer<W: Write>(
    json: &str,
    options: &GronOptions,
    writer: &mut W,
) -> Result<usize> {
    // Parse JSON using simd-json
    let mut bytes = json.as_bytes().to_vec();
    let tape = simd_json::to_tape(&mut bytes)
        .map_err(|e| DsonError::ParseError(format!("JSON parse error: {e}")))?;

    let nodes = &tape.0;
    if nodes.is_empty() {
        return Ok(0);
    }

    let mut path_builder = PathBuilder::new(&options.prefix);
    let mut output_buf = GronWriter::new(writer, options);

    // Traverse the tape
    traverse_gron(nodes, 0, &mut path_builder, &mut output_buf)?;

    Ok(output_buf.bytes_written())
}

// ANSI color codes
const COLOR_PATH: &[u8] = b"\x1b[36m"; // Cyan for paths
const COLOR_EQUALS: &[u8] = b"\x1b[90m"; // Gray for = and ;
const COLOR_STRING: &[u8] = b"\x1b[32m"; // Green for strings
const COLOR_NUMBER: &[u8] = b"\x1b[33m"; // Yellow for numbers
const COLOR_BOOL: &[u8] = b"\x1b[35m"; // Magenta for booleans
const COLOR_NULL: &[u8] = b"\x1b[31m"; // Red for null
const COLOR_BRACKET: &[u8] = b"\x1b[90m"; // Gray for {} []
const COLOR_RESET: &[u8] = b"\x1b[0m"; // Reset

/// Buffered writer for gron output.
struct GronWriter<'a, W: Write> {
    writer: &'a mut W,
    buffer: Vec<u8>,
    bytes_written: usize,
    options: &'a GronOptions,
}

impl<'a, W: Write> GronWriter<'a, W> {
    fn new(writer: &'a mut W, options: &'a GronOptions) -> Self {
        Self {
            writer,
            buffer: Vec::with_capacity(65536), // 64KB buffer
            bytes_written: 0,
            options,
        }
    }

    fn write_line(&mut self, path: &str, value: &[u8]) -> Result<()> {
        if self.options.values_only {
            if self.options.color {
                self.write_colored_value(value);
            } else {
                self.buffer.extend_from_slice(value);
            }
            self.buffer.push(b'\n');
        } else if self.options.paths_only {
            if self.options.color {
                self.buffer.extend_from_slice(COLOR_PATH);
                self.buffer.extend_from_slice(path.as_bytes());
                self.buffer.extend_from_slice(COLOR_RESET);
            } else {
                self.buffer.extend_from_slice(path.as_bytes());
            }
            self.buffer.push(b'\n');
        } else if self.options.compact {
            if self.options.color {
                self.buffer.extend_from_slice(COLOR_PATH);
                self.buffer.extend_from_slice(path.as_bytes());
                self.buffer.extend_from_slice(COLOR_EQUALS);
                self.buffer.push(b'=');
                self.buffer.extend_from_slice(COLOR_RESET);
                self.write_colored_value(value);
                self.buffer.extend_from_slice(COLOR_EQUALS);
                self.buffer.push(b';');
                self.buffer.extend_from_slice(COLOR_RESET);
            } else {
                self.buffer.extend_from_slice(path.as_bytes());
                self.buffer.push(b'=');
                self.buffer.extend_from_slice(value);
                self.buffer.push(b';');
            }
            self.buffer.push(b'\n');
        } else if self.options.color {
            self.buffer.extend_from_slice(COLOR_PATH);
            self.buffer.extend_from_slice(path.as_bytes());
            self.buffer.extend_from_slice(COLOR_RESET);
            self.buffer.extend_from_slice(COLOR_EQUALS);
            self.buffer.extend_from_slice(b" = ");
            self.buffer.extend_from_slice(COLOR_RESET);
            self.write_colored_value(value);
            self.buffer.extend_from_slice(COLOR_EQUALS);
            self.buffer.push(b';');
            self.buffer.extend_from_slice(COLOR_RESET);
            self.buffer.push(b'\n');
        } else {
            self.buffer.extend_from_slice(path.as_bytes());
            self.buffer.extend_from_slice(b" = ");
            self.buffer.extend_from_slice(value);
            self.buffer.extend_from_slice(b";\n");
        }

        // Flush if buffer is getting full
        if self.buffer.len() >= 60000 {
            self.flush()?;
        }

        Ok(())
    }

    fn write_colored_value(&mut self, value: &[u8]) {
        // Determine value type and apply appropriate color
        if value.is_empty() {
            return;
        }

        let color = match value[0] {
            b'"' => COLOR_STRING,
            b't' | b'f' => COLOR_BOOL, // true/false
            b'n' => COLOR_NULL,        // null
            b'{' | b'}' | b'[' | b']' => COLOR_BRACKET,
            b'0'..=b'9' | b'-' => COLOR_NUMBER,
            _ => COLOR_RESET,
        };

        self.buffer.extend_from_slice(color);
        self.buffer.extend_from_slice(value);
        self.buffer.extend_from_slice(COLOR_RESET);
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
        // Flush remaining
        let _ = self.flush();
        self.bytes_written
    }
}

/// Traverse the tape and generate gron output.
fn traverse_gron<W: Write>(
    nodes: &[Node<'_>],
    index: usize,
    path: &mut PathBuilder,
    out: &mut GronWriter<'_, W>,
) -> Result<usize> {
    if index >= nodes.len() {
        return Ok(index);
    }

    let node = &nodes[index];

    match node {
        Node::Object { len, count: _ } => {
            // Output object initialization
            out.write_line(path.current_path(), b"{}")?;

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
                idx = traverse_gron(nodes, idx, path, out)?;

                // Pop field from path
                path.pop();
            }

            Ok(idx)
        }

        Node::Array { len, count: _ } => {
            // Output array initialization
            out.write_line(path.current_path(), b"[]")?;

            let mut idx = index + 1;
            for i in 0..*len {
                if idx >= nodes.len() {
                    break;
                }

                // Push index onto path
                path.push_index(i);

                // Recurse into element
                idx = traverse_gron(nodes, idx, path, out)?;

                // Pop index from path
                path.pop();
            }

            Ok(idx)
        }

        Node::String(s) => {
            let mut value_buf = Vec::with_capacity(s.len() + 2);
            escape_json_string(s, &mut value_buf);
            out.write_line(path.current_path(), &value_buf)?;
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
                    out.write_line(path.current_path(), s.as_bytes())?;
                    return Ok(index + 1);
                }
                StaticNode::U64(n) => {
                    let mut buf = itoa::Buffer::new();
                    let s = buf.format(*n);
                    out.write_line(path.current_path(), s.as_bytes())?;
                    return Ok(index + 1);
                }
                StaticNode::F64(n) => {
                    let mut buf = ryu::Buffer::new();
                    let s = buf.format(*n);
                    out.write_line(path.current_path(), s.as_bytes())?;
                    return Ok(index + 1);
                }
            };
            out.write_line(path.current_path(), value)?;
            Ok(index + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_object() {
        let json = r#"{"name": "Alice", "age": 30}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json = {};"));
        assert!(output.contains(r#"json.name = "Alice";"#));
        assert!(output.contains("json.age = 30;"));
    }

    #[test]
    fn test_nested_object() {
        let json = r#"{"user": {"name": "Bob", "active": true}}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json = {};"));
        assert!(output.contains("json.user = {};"));
        assert!(output.contains(r#"json.user.name = "Bob";"#));
        assert!(output.contains("json.user.active = true;"));
    }

    #[test]
    fn test_array() {
        let json = r#"{"items": [1, 2, 3]}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json = {};"));
        assert!(output.contains("json.items = [];"));
        assert!(output.contains("json.items[0] = 1;"));
        assert!(output.contains("json.items[1] = 2;"));
        assert!(output.contains("json.items[2] = 3;"));
    }

    #[test]
    fn test_array_of_objects() {
        let json = r#"{"users": [{"name": "Alice"}, {"name": "Bob"}]}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json.users[0] = {};"));
        assert!(output.contains(r#"json.users[0].name = "Alice";"#));
        assert!(output.contains("json.users[1] = {};"));
        assert!(output.contains(r#"json.users[1].name = "Bob";"#));
    }

    #[test]
    fn test_special_field_names() {
        let json = r#"{"field.with.dots": "value", "field[0]": "value2"}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains(r#"json["field.with.dots"] = "value";"#));
        assert!(output.contains(r#"json["field[0]"] = "value2";"#));
    }

    #[test]
    fn test_custom_prefix() {
        let json = r#"{"name": "test"}"#;
        let options = GronOptions::with_prefix("data");
        let output = gron(json, &options).unwrap();
        assert!(output.contains("data = {};"));
        assert!(output.contains(r#"data.name = "test";"#));
    }

    #[test]
    fn test_compact_mode() {
        let json = r#"{"name": "test"}"#;
        let options = GronOptions::default().compact();
        let output = gron(json, &options).unwrap();
        assert!(output.contains("json={};"));
        assert!(output.contains(r#"json.name="test";"#));
    }

    #[test]
    fn test_paths_only() {
        let json = r#"{"name": "test", "age": 30}"#;
        let options = GronOptions::default().paths_only();
        let output = gron(json, &options).unwrap();
        assert!(output.contains("json\n"));
        assert!(output.contains("json.name\n"));
        assert!(output.contains("json.age\n"));
        assert!(!output.contains("test"));
    }

    #[test]
    fn test_null_value() {
        let json = r#"{"value": null}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json.value = null;"));
    }

    #[test]
    fn test_boolean_values() {
        let json = r#"{"active": true, "deleted": false}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json.active = true;"));
        assert!(output.contains("json.deleted = false;"));
    }

    #[test]
    fn test_string_escaping() {
        let json = r#"{"message": "line1\nline2"}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains(r#"json.message = "line1\nline2";"#));
    }

    #[test]
    fn test_empty_object() {
        let json = "{}";
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json = {};"));
    }

    #[test]
    fn test_empty_array() {
        let json = "[]";
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json = [];"));
    }

    #[test]
    fn test_deep_nesting() {
        let json = r#"{"a": {"b": {"c": {"d": "value"}}}}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains(r#"json.a.b.c.d = "value";"#));
    }

    #[test]
    fn test_float_values() {
        let json = r#"{"pi": 1.5, "e": 2.71828}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json.pi = "));
        assert!(output.contains("json.e = "));
    }

    #[test]
    fn test_negative_numbers() {
        let json = r#"{"temp": -10, "balance": -123.45}"#;
        let output = gron(json, &GronOptions::default()).unwrap();
        assert!(output.contains("json.temp = -10;"));
    }
}
