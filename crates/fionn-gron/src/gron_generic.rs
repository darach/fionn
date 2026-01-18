// SPDX-License-Identifier: MIT OR Apache-2.0
//! Generic gron implementation using the `TapeSource` trait.
//!
//! This module provides format-agnostic gron transformation that works with
//! any tape type implementing [`TapeSource`], enabling gron output from
//! JSON, YAML, TOML, CSV, ISON, TOON, or any other format with a tape parser.
//!
//! # Example
//!
//! ```rust,ignore
//! use fionn_gron::{gron_from_tape, GronOptions};
//! use fionn_tape::DsonTape;
//!
//! let json = r#"{"name": "Alice", "age": 30}"#;
//! let tape = DsonTape::parse(json)?;
//! let output = gron_from_tape(&tape, &GronOptions::default())?;
//! // json = {};
//! // json.name = "Alice";
//! // json.age = 30;
//! ```

use super::GronOptions;
use super::path_builder::PathBuilder;
use super::simd_utils::escape_json_string;
use fionn_core::tape_source::{TapeNodeKind, TapeSource, TapeValue};
use fionn_core::{DsonError, Result};
use std::io::Write;

/// Convert a tape to gron format.
///
/// This is the generic version of [`gron`](super::gron) that works with any
/// tape type implementing [`TapeSource`].
///
/// # Arguments
///
/// * `tape` - Any tape implementing `TapeSource`
/// * `options` - Gron output options
///
/// # Returns
///
/// The gron output as a String.
///
/// # Errors
///
/// Returns an error if writing fails.
///
/// # Example
///
/// ```rust,ignore
/// use fionn_gron::{gron_from_tape, GronOptions};
/// use fionn_tape::DsonTape;
///
/// let tape = DsonTape::parse(r#"{"key": "value"}"#)?;
/// let output = gron_from_tape(&tape, &GronOptions::default())?;
/// ```
pub fn gron_from_tape<T: TapeSource>(tape: &T, options: &GronOptions) -> Result<String> {
    // Estimate output size: average of 50 bytes per tape node
    let estimated_size = tape.len() * 50;
    let mut output = Vec::with_capacity(estimated_size);
    gron_from_tape_to_writer(tape, options, &mut output)?;
    // Safety: we only write valid UTF-8
    Ok(unsafe { String::from_utf8_unchecked(output) })
}

/// Convert a tape to gron format, writing to a writer.
///
/// # Arguments
///
/// * `tape` - Any tape implementing `TapeSource`
/// * `options` - Gron output options
/// * `writer` - Output writer
///
/// # Returns
///
/// Number of bytes written.
///
/// # Errors
///
/// Returns an error if writing fails.
pub fn gron_from_tape_to_writer<T: TapeSource, W: Write>(
    tape: &T,
    options: &GronOptions,
    writer: &mut W,
) -> Result<usize> {
    if tape.is_empty() {
        return Ok(0);
    }

    let mut path_builder = PathBuilder::new(&options.prefix);
    let mut output_buf = GenericGronWriter::new(writer, options);

    // Traverse the tape
    traverse_tape(tape, 0, &mut path_builder, &mut output_buf)?;

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
struct GenericGronWriter<'a, W: Write> {
    writer: &'a mut W,
    buffer: Vec<u8>,
    bytes_written: usize,
    options: &'a GronOptions,
}

impl<'a, W: Write> GenericGronWriter<'a, W> {
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

/// Traverse a tape and generate gron output.
///
/// This function handles the recursive traversal of the tape structure,
/// generating gron lines for each value.
fn traverse_tape<T: TapeSource, W: Write>(
    tape: &T,
    index: usize,
    path: &mut PathBuilder,
    out: &mut GenericGronWriter<'_, W>,
) -> Result<usize> {
    if index >= tape.len() {
        return Ok(index);
    }

    let node = tape
        .node_at(index)
        .ok_or_else(|| DsonError::InvalidField(format!("No node at index {index}")))?;

    match node.kind {
        TapeNodeKind::ObjectStart { count } => {
            // Output object initialization
            out.write_line(path.current_path(), b"{}")?;

            let mut idx = index + 1;
            for _ in 0..count {
                if idx >= tape.len() {
                    break;
                }

                // Get key node
                let key_node = tape.node_at(idx);
                let key = match key_node {
                    Some(ref n) if matches!(n.kind, TapeNodeKind::Key) => {
                        if let Some(TapeValue::String(ref s)) = n.value {
                            s.as_ref()
                        } else {
                            idx += 1;
                            continue;
                        }
                    }
                    _ => {
                        // No key node, skip
                        idx += 1;
                        continue;
                    }
                };
                idx += 1; // Move past key

                // Push field onto path
                path.push_field(key);

                // Recurse into value
                idx = traverse_tape(tape, idx, path, out)?;

                // Pop field from path
                path.pop();
            }

            // Skip ObjectEnd if present
            if idx < tape.len()
                && let Some(n) = tape.node_at(idx)
                && matches!(n.kind, TapeNodeKind::ObjectEnd)
            {
                idx += 1;
            }

            Ok(idx)
        }

        TapeNodeKind::ArrayStart { count } => {
            // Output array initialization
            out.write_line(path.current_path(), b"[]")?;

            let mut idx = index + 1;
            for i in 0..count {
                if idx >= tape.len() {
                    break;
                }

                // Push index onto path
                path.push_index(i);

                // Recurse into element
                idx = traverse_tape(tape, idx, path, out)?;

                // Pop index from path
                path.pop();
            }

            // Skip ArrayEnd if present
            if idx < tape.len()
                && let Some(n) = tape.node_at(idx)
                && matches!(n.kind, TapeNodeKind::ArrayEnd)
            {
                idx += 1;
            }

            Ok(idx)
        }

        TapeNodeKind::Value => {
            // Output the value
            let value = node.value.ok_or_else(|| {
                DsonError::InvalidField(format!("Value node at {index} has no value"))
            })?;

            write_value(path.current_path(), &value, out)?;
            Ok(index + 1)
        }

        TapeNodeKind::Key => {
            // Standalone key - shouldn't happen in well-formed traversal
            // but handle gracefully by skipping
            Ok(index + 1)
        }

        TapeNodeKind::ObjectEnd | TapeNodeKind::ArrayEnd => {
            // End markers - skip
            Ok(index + 1)
        }
    }
}

/// Write a tape value to the gron output.
fn write_value<W: Write>(
    path: &str,
    value: &TapeValue<'_>,
    out: &mut GenericGronWriter<'_, W>,
) -> Result<()> {
    match value {
        TapeValue::Null => {
            out.write_line(path, b"null")?;
        }
        TapeValue::Bool(true) => {
            out.write_line(path, b"true")?;
        }
        TapeValue::Bool(false) => {
            out.write_line(path, b"false")?;
        }
        TapeValue::Int(n) => {
            let mut buf = itoa::Buffer::new();
            let s = buf.format(*n);
            out.write_line(path, s.as_bytes())?;
        }
        TapeValue::Float(n) => {
            if n.is_nan() || n.is_infinite() {
                // JSON doesn't support NaN/Infinity, output as null
                out.write_line(path, b"null")?;
            } else {
                let mut buf = ryu::Buffer::new();
                let s = buf.format(*n);
                out.write_line(path, s.as_bytes())?;
            }
        }
        TapeValue::String(s) => {
            let mut value_buf = Vec::with_capacity(s.len() + 2);
            escape_json_string(s, &mut value_buf);
            out.write_line(path, &value_buf)?;
        }
        TapeValue::RawNumber(s) => {
            out.write_line(path, s.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fionn_core::format::FormatKind;
    use fionn_core::tape_source::TapeNodeRef;
    use std::borrow::Cow;

    // Mock tape for testing
    struct MockTape {
        nodes: Vec<(TapeNodeKind, Option<TapeValue<'static>>)>,
    }

    impl MockTape {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn push(&mut self, kind: TapeNodeKind, value: Option<TapeValue<'static>>) {
            self.nodes.push((kind, value));
        }
    }

    impl TapeSource for MockTape {
        fn format(&self) -> FormatKind {
            FormatKind::Json
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_at(&self, index: usize) -> Option<TapeNodeRef<'_>> {
            self.nodes.get(index).map(|(kind, value)| TapeNodeRef {
                kind: *kind,
                value: value.clone(),
                format: FormatKind::Json,
            })
        }

        fn skip_value(&self, start_index: usize) -> Result<usize> {
            let node = self
                .node_at(start_index)
                .ok_or_else(|| DsonError::InvalidField("Index out of bounds".to_string()))?;

            match node.kind {
                TapeNodeKind::ObjectStart { count } => {
                    let mut idx = start_index + 1;
                    for _ in 0..count {
                        idx += 1; // key
                        idx = self.skip_value(idx)?; // value
                    }
                    Ok(idx + 1) // ObjectEnd
                }
                TapeNodeKind::ArrayStart { count } => {
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
            Ok(None)
        }
    }

    #[test]
    fn test_empty_tape() {
        let tape = MockTape::new();
        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_simple_object() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 2 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("name"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("Alice"))),
        );
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("age"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(30)));
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json = {};"));
        assert!(result.contains(r#"json.name = "Alice";"#));
        assert!(result.contains("json.age = 30;"));
    }

    #[test]
    fn test_nested_object() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("user"))),
        );
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("name"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("Bob"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json = {};"));
        assert!(result.contains("json.user = {};"));
        assert!(result.contains(r#"json.user.name = "Bob";"#));
    }

    #[test]
    fn test_array() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("items"))),
        );
        tape.push(TapeNodeKind::ArrayStart { count: 3 }, None);
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(1)));
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(2)));
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(3)));
        tape.push(TapeNodeKind::ArrayEnd, None);
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json = {};"));
        assert!(result.contains("json.items = [];"));
        assert!(result.contains("json.items[0] = 1;"));
        assert!(result.contains("json.items[1] = 2;"));
        assert!(result.contains("json.items[2] = 3;"));
    }

    #[test]
    fn test_array_of_objects() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ArrayStart { count: 2 }, None);
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("name"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("Alice"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("name"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("Bob"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);
        tape.push(TapeNodeKind::ArrayEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json = [];"));
        assert!(result.contains("json[0] = {};"));
        assert!(result.contains(r#"json[0].name = "Alice";"#));
        assert!(result.contains("json[1] = {};"));
        assert!(result.contains(r#"json[1].name = "Bob";"#));
    }

    #[test]
    fn test_all_value_types() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 6 }, None);

        // Null
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("null_val"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Null));

        // Bool true
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("bool_true"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Bool(true)));

        // Bool false
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("bool_false"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Bool(false)));

        // Int
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("int_val"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(-42)));

        // Float
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("float_val"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Float(1.5)));

        // String
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("string_val"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("hello"))),
        );

        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json.null_val = null;"));
        assert!(result.contains("json.bool_true = true;"));
        assert!(result.contains("json.bool_false = false;"));
        assert!(result.contains("json.int_val = -42;"));
        assert!(result.contains("json.float_val = 1.5;"));
        assert!(result.contains(r#"json.string_val = "hello";"#));
    }

    #[test]
    fn test_string_escaping() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("msg"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("line1\nline2"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains(r#"json.msg = "line1\nline2";"#));
    }

    #[test]
    fn test_special_field_names() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 2 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("field.with.dots"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("v1"))),
        );
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("field[0]"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("v2"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains(r#"json["field.with.dots"] = "v1";"#));
        assert!(result.contains(r#"json["field[0]"] = "v2";"#));
    }

    #[test]
    fn test_custom_prefix() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("key"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("value"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        let options = GronOptions::with_prefix("data");
        let result = gron_from_tape(&tape, &options).unwrap();
        assert!(result.contains("data = {};"));
        assert!(result.contains(r#"data.key = "value";"#));
    }

    #[test]
    fn test_compact_mode() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("key"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("value"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        let options = GronOptions::default().compact();
        let result = gron_from_tape(&tape, &options).unwrap();
        assert!(result.contains("json={};"));
        assert!(result.contains(r#"json.key="value";"#));
    }

    #[test]
    fn test_paths_only() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("key"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("value"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        let options = GronOptions::default().paths_only();
        let result = gron_from_tape(&tape, &options).unwrap();
        assert!(result.contains("json\n"));
        assert!(result.contains("json.key\n"));
        assert!(!result.contains("value"));
        assert!(!result.contains('='));
    }

    #[test]
    fn test_values_only() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("key"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("value"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        let options = GronOptions::default().values_only();
        let result = gron_from_tape(&tape, &options).unwrap();
        assert!(result.contains("{}\n"));
        assert!(result.contains("\"value\"\n"));
        assert!(!result.contains("json"));
        assert!(!result.contains('='));
    }

    #[test]
    fn test_nan_infinity() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 2 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("nan"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Float(f64::NAN)));
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("inf"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Float(f64::INFINITY)));
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        // NaN and Infinity should be output as null
        assert!(result.contains("json.nan = null;"));
        assert!(result.contains("json.inf = null;"));
    }

    #[test]
    fn test_raw_number() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("raw"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::RawNumber(Cow::Borrowed("12345678901234567890"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json.raw = 12345678901234567890;"));
    }

    #[test]
    fn test_deeply_nested() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("a"))),
        );
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("b"))),
        );
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("c"))),
        );
        tape.push(
            TapeNodeKind::Value,
            Some(TapeValue::String(Cow::Borrowed("deep"))),
        );
        tape.push(TapeNodeKind::ObjectEnd, None);
        tape.push(TapeNodeKind::ObjectEnd, None);
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains(r#"json.a.b.c = "deep";"#));
    }

    #[test]
    fn test_empty_object() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 0 }, None);
        tape.push(TapeNodeKind::ObjectEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json = {};"));
        assert_eq!(result.lines().count(), 1);
    }

    #[test]
    fn test_empty_array() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ArrayStart { count: 0 }, None);
        tape.push(TapeNodeKind::ArrayEnd, None);

        let result = gron_from_tape(&tape, &GronOptions::default()).unwrap();
        assert!(result.contains("json = [];"));
        assert_eq!(result.lines().count(), 1);
    }

    #[test]
    fn test_to_writer() {
        let mut tape = MockTape::new();
        tape.push(TapeNodeKind::ObjectStart { count: 1 }, None);
        tape.push(
            TapeNodeKind::Key,
            Some(TapeValue::String(Cow::Borrowed("key"))),
        );
        tape.push(TapeNodeKind::Value, Some(TapeValue::Int(42)));
        tape.push(TapeNodeKind::ObjectEnd, None);

        let mut output = Vec::new();
        let bytes = gron_from_tape_to_writer(&tape, &GronOptions::default(), &mut output).unwrap();
        assert!(bytes > 0);
        assert_eq!(bytes, output.len());

        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("json.key = 42;"));
    }
}
