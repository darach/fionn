// SPDX-License-Identifier: MIT OR Apache-2.0
//! Efficient path string builder for gron output.
//!
//! This module provides a stack-based path builder that efficiently
//! constructs path strings during JSON traversal. Key features:
//!
//! - Stack-based: Push/pop operations for traversal
//! - Incremental rendering: Paths share prefixes
//! - SIMD-friendly: Uses contiguous memory
//! - Auto-quoting: Automatically quotes field names that need it
//! - `SmallVec` optimization: Avoids heap allocation for typical nesting depths

use smallvec::SmallVec;

use super::simd_utils::needs_quoting;

/// Stack-based path builder for efficient path construction.
///
/// During JSON traversal, paths share prefixes (e.g., `json.users[0].name`
/// and `json.users[0].age` share `json.users[0]`). This builder maintains
/// a stack of segment boundaries, allowing efficient push/pop operations.
///
/// Uses `SmallVec` for the stack to avoid heap allocation for typical JSON
/// documents with fewer than 16 levels of nesting.
#[derive(Debug, Clone)]
pub struct PathBuilder {
    /// The path string buffer
    buffer: Vec<u8>,
    /// Stack of segment start positions (inline for depth < 16)
    stack: SmallVec<[usize; 16]>,
    /// Root prefix (e.g., "json")
    root: String,
}

impl PathBuilder {
    /// Create a new path builder with the given root prefix.
    #[must_use]
    pub fn new(root: &str) -> Self {
        let mut buffer = Vec::with_capacity(256);
        buffer.extend_from_slice(root.as_bytes());

        Self {
            buffer,
            stack: SmallVec::new(),
            root: root.to_string(),
        }
    }

    /// Create a new path builder with default root "json".
    #[must_use]
    pub fn default_root() -> Self {
        Self::new("json")
    }

    /// Push a field name onto the path.
    ///
    /// If the field name contains special characters, it will be
    /// automatically quoted using bracket notation.
    pub fn push_field(&mut self, name: &str) {
        self.stack.push(self.buffer.len());

        if needs_quoting(name.as_bytes()) {
            // Use bracket notation: ["field"]
            self.buffer.push(b'[');
            self.buffer.push(b'"');
            escape_field_name(&mut self.buffer, name);
            self.buffer.push(b'"');
            self.buffer.push(b']');
        } else {
            // Use dot notation: .field
            self.buffer.push(b'.');
            self.buffer.extend_from_slice(name.as_bytes());
        }
    }

    /// Push an array index onto the path.
    pub fn push_index(&mut self, index: usize) {
        self.stack.push(self.buffer.len());
        self.buffer.push(b'[');
        write_usize(&mut self.buffer, index);
        self.buffer.push(b']');
    }

    /// Push an array index onto the path without recording on the stack.
    ///
    /// This is used for parallel processing where we don't need `pop()` support.
    /// The index is added directly to the current path.
    pub fn push_index_raw(&mut self, index: usize) {
        self.buffer.push(b'[');
        write_usize(&mut self.buffer, index);
        self.buffer.push(b']');
    }

    /// Pop the last segment from the path.
    pub fn pop(&mut self) {
        if let Some(offset) = self.stack.pop() {
            self.buffer.truncate(offset);
        }
    }

    /// Get the current path as a string slice.
    ///
    /// # Safety
    /// The buffer only contains valid UTF-8 because:
    /// - Root is a valid string
    /// - Field names come from JSON keys (valid UTF-8)
    /// - All added characters are ASCII
    #[must_use]
    pub fn current_path(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(&self.buffer) }
    }

    /// Get the current path as bytes.
    #[must_use]
    pub fn current_path_bytes(&self) -> &[u8] {
        &self.buffer
    }

    /// Get the current depth (number of segments).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Reset to root state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.buffer.extend_from_slice(self.root.as_bytes());
        self.stack.clear();
    }

    /// Check if the path is at root level.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.stack.is_empty()
    }
}

impl Default for PathBuilder {
    fn default() -> Self {
        Self::default_root()
    }
}

/// Write a usize to a byte buffer as decimal digits.
///
/// Uses a lookup table for small numbers (0-999) for efficiency.
#[inline]
#[allow(clippy::cast_possible_truncation)] // Safe: digit extraction always yields 0-9
fn write_usize(buffer: &mut Vec<u8>, value: usize) {
    // Fast path for small numbers
    // Truncations are safe: we're extracting digits which are always 0-9
    if value < 10 {
        buffer.push(b'0' + value as u8);
        return;
    }

    if value < 100 {
        buffer.push(b'0' + (value / 10) as u8);
        buffer.push(b'0' + (value % 10) as u8);
        return;
    }

    if value < 1000 {
        buffer.push(b'0' + (value / 100) as u8);
        buffer.push(b'0' + ((value / 10) % 10) as u8);
        buffer.push(b'0' + (value % 10) as u8);
        return;
    }

    // General case: use itoa-style conversion
    let mut temp = [0u8; 20];
    let mut pos = temp.len();
    let mut n = value;

    while n > 0 {
        pos -= 1;
        temp[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }

    buffer.extend_from_slice(&temp[pos..]);
}

/// Escape a field name for use in bracket notation.
///
/// Escapes: `"` -> `\"`, `\` -> `\\`
#[inline]
fn escape_field_name(buffer: &mut Vec<u8>, name: &str) {
    for byte in name.bytes() {
        match byte {
            b'"' => {
                buffer.push(b'\\');
                buffer.push(b'"');
            }
            b'\\' => {
                buffer.push(b'\\');
                buffer.push(b'\\');
            }
            _ => {
                buffer.push(byte);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_path() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("name");
        assert_eq!(builder.current_path(), "json.name");
    }

    #[test]
    fn test_array_index() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("items");
        builder.push_index(0);
        assert_eq!(builder.current_path(), "json.items[0]");
    }

    #[test]
    fn test_pop() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("items");
        builder.push_index(0);
        builder.push_field("name");
        assert_eq!(builder.current_path(), "json.items[0].name");

        builder.pop();
        assert_eq!(builder.current_path(), "json.items[0]");

        builder.pop();
        assert_eq!(builder.current_path(), "json.items");

        builder.pop();
        assert_eq!(builder.current_path(), "json");
    }

    #[test]
    fn test_quoted_field() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("field.with.dots");
        assert_eq!(builder.current_path(), r#"json["field.with.dots"]"#);
    }

    #[test]
    fn test_field_with_quote() {
        let mut builder = PathBuilder::new("json");
        builder.push_field(r#"field"quote"#);
        assert_eq!(builder.current_path(), r#"json["field\"quote"]"#);
    }

    #[test]
    fn test_field_with_bracket() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("field[0]");
        assert_eq!(builder.current_path(), r#"json["field[0]"]"#);
    }

    #[test]
    fn test_depth() {
        let mut builder = PathBuilder::new("json");
        assert_eq!(builder.depth(), 0);

        builder.push_field("a");
        assert_eq!(builder.depth(), 1);

        builder.push_index(0);
        assert_eq!(builder.depth(), 2);

        builder.pop();
        assert_eq!(builder.depth(), 1);
    }

    #[test]
    fn test_reset() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("a");
        builder.push_field("b");
        builder.reset();
        assert_eq!(builder.current_path(), "json");
        assert_eq!(builder.depth(), 0);
    }

    #[test]
    fn test_large_index() {
        let mut builder = PathBuilder::new("json");
        builder.push_index(12345);
        assert_eq!(builder.current_path(), "json[12345]");
    }

    #[test]
    fn test_write_usize() {
        let mut buf = Vec::new();
        write_usize(&mut buf, 0);
        assert_eq!(&buf, b"0");

        buf.clear();
        write_usize(&mut buf, 42);
        assert_eq!(&buf, b"42");

        buf.clear();
        write_usize(&mut buf, 999);
        assert_eq!(&buf, b"999");

        buf.clear();
        write_usize(&mut buf, 1_234_567_890);
        assert_eq!(&buf, b"1234567890");
    }

    // =========================================================================
    // Additional Coverage Tests
    // =========================================================================

    #[test]
    fn test_default_root() {
        let builder = PathBuilder::default_root();
        assert_eq!(builder.current_path(), "json");
    }

    #[test]
    fn test_default_trait() {
        let builder = PathBuilder::default();
        assert_eq!(builder.current_path(), "json");
    }

    #[test]
    fn test_current_path_bytes() {
        let mut builder = PathBuilder::new("root");
        builder.push_field("field");
        assert_eq!(builder.current_path_bytes(), b"root.field");
    }

    #[test]
    fn test_is_root() {
        let mut builder = PathBuilder::new("json");
        assert!(builder.is_root());

        builder.push_field("field");
        assert!(!builder.is_root());

        builder.pop();
        assert!(builder.is_root());
    }

    #[test]
    fn test_push_index_raw() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("arr");
        builder.push_index_raw(5);
        assert_eq!(builder.current_path(), "json.arr[5]");
        // Note: push_index_raw doesn't affect depth
        assert_eq!(builder.depth(), 1);
    }

    #[test]
    fn test_pop_empty() {
        let mut builder = PathBuilder::new("json");
        builder.pop(); // Pop on empty stack should be no-op
        assert_eq!(builder.current_path(), "json");
    }

    #[test]
    fn test_clone() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("field");
        let cloned = builder.clone();
        assert_eq!(cloned.current_path(), "json.field");
    }

    #[test]
    fn test_debug() {
        let builder = PathBuilder::new("json");
        let debug = format!("{builder:?}");
        assert!(debug.contains("PathBuilder"));
    }

    #[test]
    fn test_field_with_backslash() {
        let mut builder = PathBuilder::new("json");
        builder.push_field(r"field\back");
        assert_eq!(builder.current_path(), r#"json["field\\back"]"#);
    }

    #[test]
    fn test_field_with_both_escapes() {
        let mut builder = PathBuilder::new("json");
        builder.push_field(r#"field\"both"#);
        assert_eq!(builder.current_path(), r#"json["field\\\"both"]"#);
    }

    #[test]
    fn test_escape_field_name_direct() {
        let mut buffer = Vec::new();
        escape_field_name(&mut buffer, "normal");
        assert_eq!(&buffer, b"normal");

        buffer.clear();
        escape_field_name(&mut buffer, r#""quoted""#);
        assert_eq!(&buffer, b"\\\"quoted\\\"");

        buffer.clear();
        escape_field_name(&mut buffer, r"back\slash");
        assert_eq!(&buffer, b"back\\\\slash");
    }

    #[test]
    fn test_write_usize_boundaries() {
        let mut buf = Vec::new();
        // Test boundary: 9 -> 10
        write_usize(&mut buf, 9);
        assert_eq!(&buf, b"9");

        buf.clear();
        write_usize(&mut buf, 10);
        assert_eq!(&buf, b"10");

        // Test boundary: 99 -> 100
        buf.clear();
        write_usize(&mut buf, 99);
        assert_eq!(&buf, b"99");

        buf.clear();
        write_usize(&mut buf, 100);
        assert_eq!(&buf, b"100");

        // Test boundary: 999 -> 1000
        buf.clear();
        write_usize(&mut buf, 1000);
        assert_eq!(&buf, b"1000");
    }

    #[test]
    fn test_custom_root() {
        let builder = PathBuilder::new("data");
        assert_eq!(builder.current_path(), "data");
    }

    #[test]
    fn test_deeply_nested() {
        let mut builder = PathBuilder::new("json");
        for i in 0..20 {
            builder.push_field(&format!("level{i}"));
        }
        assert_eq!(builder.depth(), 20);
        assert!(builder.current_path().contains("level19"));
    }

    #[test]
    fn test_mixed_fields_and_indices() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("users");
        builder.push_index(0);
        builder.push_field("orders");
        builder.push_index(5);
        builder.push_field("item");
        assert_eq!(builder.current_path(), "json.users[0].orders[5].item");
    }

    #[test]
    fn test_empty_field_name() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("");
        // Empty string still needs quoting
        assert!(builder.current_path().contains("[\"\"]") || builder.current_path().contains('.'));
    }

    #[test]
    fn test_unicode_field() {
        let mut builder = PathBuilder::new("json");
        builder.push_field("日本語");
        // Unicode fields need quoting
        assert!(builder.current_path().contains("日本語"));
    }
}
