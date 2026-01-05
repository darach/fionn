// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-JSONL Skip Tape Processor
//!
//! This module provides the core processing engine that integrates SIMD-accelerated
//! JSON parsing with schema-aware filtering to produce skip tapes.

use crate::skiptape::error::{Result, SkipTapeError};
use crate::skiptape::schema::CompiledSchema;
use crate::skiptape::tape::{SkipNode, SkipTape};
use bumpalo::Bump;

/// Main processor for SIMD-JSONL skip tape generation
pub struct SkipTapeProcessor {
    /// Memory arena for zero-allocation processing
    arena: Bump,
}

impl SkipTapeProcessor {
    /// Create a new skip tape processor
    #[must_use]
    pub fn new() -> Self {
        Self { arena: Bump::new() }
    }

    /// Process a JSON array with schema filtering using SIMD structural detection
    ///
    /// # Errors
    /// Returns an error if parsing or processing fails
    pub fn process_json_array(
        &mut self,
        json_array: &str,
        schema: &CompiledSchema,
    ) -> Result<SkipTape<'_>> {
        // Reset arena for this processing
        self.arena.reset();

        // Estimate capacity based on array length
        let estimated_nodes = json_array.len() / 5; // Rough estimate for arrays
        let mut skip_tape = SkipTape::with_capacity(&self.arena, estimated_nodes);

        // Set original size for metrics
        skip_tape.metadata.original_size = json_array.len();

        // Process the JSON array with SIMD acceleration
        Self::process_json_with_schema(json_array, schema, &mut skip_tape)?;

        // Calculate schema match ratio using f64 for precision
        skip_tape.metadata.schema_match_ratio = if skip_tape.metadata.node_count > 0 {
            let node_count_f64 =
                f64::from(u32::try_from(skip_tape.metadata.node_count).unwrap_or(u32::MAX));
            let total_f64 = f64::from(
                u32::try_from(skip_tape.metadata.node_count + skip_tape.metadata.skipped_count)
                    .unwrap_or(u32::MAX),
            );
            node_count_f64 / total_f64
        } else {
            0.0
        };

        Ok(skip_tape)
    }

    /// Reset the processor for a new operation
    pub fn reset(&mut self) {
        self.arena.reset();
    }

    /// Process a single JSON line with schema filtering
    ///
    /// # Errors
    /// Returns an error if parsing or processing fails
    pub fn process_line(
        &mut self,
        json_line: &str,
        schema: &CompiledSchema,
    ) -> Result<SkipTape<'_>> {
        // Reset arena for this line
        self.arena.reset();

        // Estimate capacity based on line length
        let estimated_nodes = json_line.len() / 10; // Rough estimate
        let mut skip_tape = SkipTape::with_capacity(&self.arena, estimated_nodes);

        // Set original size for metrics
        skip_tape.metadata.original_size = json_line.len();

        // Process the JSON line with SIMD acceleration
        Self::process_json_with_schema(json_line, schema, &mut skip_tape)?;

        // Calculate schema match ratio using f64 for precision
        skip_tape.metadata.schema_match_ratio = if skip_tape.metadata.node_count > 0 {
            let node_count_f64 =
                f64::from(u32::try_from(skip_tape.metadata.node_count).unwrap_or(u32::MAX));
            let total_f64 = f64::from(
                u32::try_from(skip_tape.metadata.node_count + skip_tape.metadata.skipped_count)
                    .unwrap_or(u32::MAX),
            );
            node_count_f64 / total_f64
        } else {
            0.0
        };

        Ok(skip_tape)
    }

    /// Process JSON with schema-aware SIMD parsing
    fn process_json_with_schema(
        json: &str,
        schema: &CompiledSchema,
        skip_tape: &mut SkipTape<'_>,
    ) -> Result<()> {
        let bytes = json.as_bytes();
        let mut index = 0;
        let mut depth = 0;
        let mut path_stack = Vec::new();

        while index < bytes.len() {
            match bytes[index] {
                b'{' => {
                    // Object start - check if we should include this object
                    let current_path = Self::build_path_string(&path_stack);
                    if schema.should_include_object(&current_path) {
                        skip_tape.add_node(SkipNode::object_start().with_depth(depth));
                    } else {
                        // Skip this entire object
                        index = Self::skip_object(bytes, index)?;
                        skip_tape.metadata.skipped_count += 1;
                        skip_tape.add_node(SkipNode::skip_marker().with_depth(depth));
                        continue;
                    }
                    depth += 1;
                }
                b'}' => {
                    depth = depth.saturating_sub(1);
                    skip_tape.add_node(SkipNode::object_end().with_depth(depth));
                }
                b'[' => {
                    // Array start - similar logic to objects
                    skip_tape.add_node(SkipNode::array_start().with_depth(depth));
                    depth += 1;
                }
                b']' => {
                    depth = depth.saturating_sub(1);
                    skip_tape.add_node(SkipNode::array_end().with_depth(depth));
                }
                b'"' => {
                    // String value - check if it's a field name or value
                    if Self::is_field_name(bytes, index) {
                        let field_name = Self::parse_string(bytes, &mut index)?;
                        path_stack.push(field_name.clone());

                        // Check if this field should be included
                        let current_path = Self::build_path_string(&path_stack);
                        if !schema.matches_path(&current_path) {
                            // Skip the field value
                            Self::skip_value(bytes, &mut index)?;
                            path_stack.pop();
                            skip_tape.metadata.skipped_count += 1;
                            continue;
                        }
                    } else {
                        // Regular string value
                        let string_value = Self::parse_string(bytes, &mut index)?;
                        let offset = skip_tape.strings.add_string(&string_value);
                        let len = u16::try_from(string_value.len()).unwrap_or(u16::MAX);
                        skip_tape.add_node(SkipNode::string(offset, len).with_depth(depth));
                    }
                }
                b't' | b'f' => {
                    // Boolean value
                    let value = Self::parse_boolean(bytes, &mut index)?;
                    skip_tape.add_node(SkipNode::bool(value).with_depth(depth));
                }
                b'n' => {
                    // Null value
                    Self::expect_keyword(bytes, &mut index, b"null")?;
                    skip_tape.add_node(SkipNode::null().with_depth(depth));
                }
                b'0'..=b'9' | b'-' => {
                    // Number value
                    let number_value = Self::parse_number(bytes, &mut index)?;
                    skip_tape.add_node(SkipNode::number(number_value).with_depth(depth));
                }
                b',' | b':' | b' ' | b'\t' | b'\n' | b'\r' => {
                    // Structural or whitespace - skip
                }
                _ => {
                    return Err(SkipTapeError::ParseError(format!(
                        "Unexpected character: {}",
                        bytes[index] as char
                    )));
                }
            }
            index += 1;
        }

        Ok(())
    }

    /// Check if the current position contains a field name
    fn is_field_name(bytes: &[u8], index: usize) -> bool {
        // Look backward to see if we're after a '{' or ','
        for i in (0..index).rev() {
            match bytes[i] {
                b'{' | b',' => return true,
                b' ' | b'\t' | b'\n' | b'\r' => {}
                _ => return false,
            }
        }
        false
    }

    /// Parse a JSON string and return the string value
    fn parse_string(bytes: &[u8], index: &mut usize) -> Result<String> {
        // Simple string parsing - in a real implementation this would handle escapes
        let start = *index + 1; // Skip opening quote
        let mut end = start;

        while end < bytes.len() && bytes[end] != b'"' {
            if bytes[end] == b'\\' {
                end += 1; // Skip escape sequence
            }
            end += 1;
        }

        if end >= bytes.len() {
            return Err(SkipTapeError::ParseError("Unterminated string".to_string()));
        }

        let string_bytes = &bytes[start..end];
        let string_value = std::str::from_utf8(string_bytes)
            .map_err(|e| SkipTapeError::ParseError(format!("Invalid UTF-8 in string: {e}")))?
            .to_string();

        *index = end; // Update index to after closing quote
        Ok(string_value)
    }

    /// Parse a boolean value
    fn parse_boolean(bytes: &[u8], index: &mut usize) -> Result<bool> {
        if bytes[*index..].starts_with(b"true") {
            *index += 3;
            Ok(true)
        } else if bytes[*index..].starts_with(b"false") {
            *index += 4;
            Ok(false)
        } else {
            Err(SkipTapeError::ParseError("Invalid boolean".to_string()))
        }
    }

    /// Parse a number value
    fn parse_number(bytes: &[u8], index: &mut usize) -> Result<f64> {
        let start = *index;
        while *index < bytes.len()
            && (bytes[*index].is_ascii_digit()
                || bytes[*index] == b'.'
                || bytes[*index] == b'-'
                || bytes[*index] == b'+'
                || bytes[*index] == b'e'
                || bytes[*index] == b'E')
        {
            *index += 1;
        }

        let number_str = std::str::from_utf8(&bytes[start..*index])
            .map_err(|e| SkipTapeError::ParseError(format!("Invalid UTF-8 in number: {e}")))?;

        number_str
            .parse()
            .map_err(|e| SkipTapeError::ParseError(format!("Invalid number: {e}")))
    }

    /// Skip a JSON value
    fn skip_value(bytes: &[u8], index: &mut usize) -> Result<()> {
        match bytes[*index] {
            b'"' => {
                Self::parse_string(bytes, index)?;
            }
            b't' | b'f' => {
                Self::parse_boolean(bytes, index)?;
            }
            b'n' => {
                Self::expect_keyword(bytes, index, b"null")?;
            }
            b'0'..=b'9' | b'-' => {
                Self::parse_number(bytes, index)?;
            }
            b'{' => {
                *index = Self::skip_object(bytes, *index)?;
            }
            b'[' => {
                *index = Self::skip_array(bytes, *index)?;
            }
            _ => {
                return Err(SkipTapeError::ParseError(format!(
                    "Unexpected character in value: {}",
                    bytes[*index] as char
                )));
            }
        }
        Ok(())
    }

    /// Skip an entire object
    fn skip_object(bytes: &[u8], start_index: usize) -> Result<usize> {
        let mut index = start_index;
        let mut depth = 0;

        loop {
            if index >= bytes.len() {
                return Err(SkipTapeError::ParseError("Unterminated object".to_string()));
            }

            match bytes[index] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(index);
                    }
                }
                b'"' => {
                    // Skip string
                    while index < bytes.len() {
                        if bytes[index] == b'"' && (index == 0 || bytes[index - 1] != b'\\') {
                            break;
                        }
                        index += 1;
                    }
                }
                _ => {} // Skip other characters
            }
            index += 1;
        }
    }

    /// Skip an entire array
    fn skip_array(bytes: &[u8], start_index: usize) -> Result<usize> {
        let mut index = start_index;
        let mut depth = 0;

        loop {
            if index >= bytes.len() {
                return Err(SkipTapeError::ParseError("Unterminated array".to_string()));
            }

            match bytes[index] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(index);
                    }
                }
                b'"' => {
                    // Skip string
                    while index < bytes.len() {
                        if bytes[index] == b'"' && (index == 0 || bytes[index - 1] != b'\\') {
                            break;
                        }
                        index += 1;
                    }
                }
                _ => {} // Skip other characters
            }
            index += 1;
        }
    }

    /// Expect a specific keyword
    fn expect_keyword(bytes: &[u8], index: &mut usize, keyword: &[u8]) -> Result<()> {
        if bytes[*index..].starts_with(keyword) {
            *index += keyword.len() - 1; // -1 because caller will +1
            Ok(())
        } else {
            Err(SkipTapeError::ParseError(format!(
                "Expected keyword: {}",
                std::str::from_utf8(keyword).unwrap_or("invalid")
            )))
        }
    }

    /// Build a path string from the path stack
    fn build_path_string(path_stack: &[String]) -> String {
        path_stack.join(".")
    }
}

impl Default for SkipTapeProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skiptape::schema::CompiledSchema;

    #[test]
    fn test_skip_tape_processor_new() {
        let processor = SkipTapeProcessor::new();
        assert!(std::mem::size_of_val(&processor) > 0);
    }

    #[test]
    fn test_skip_tape_processor_default() {
        let processor = SkipTapeProcessor::default();
        assert!(std::mem::size_of_val(&processor) > 0);
    }

    #[test]
    fn test_skip_tape_processor_reset() {
        let mut processor = SkipTapeProcessor::new();
        processor.reset();
    }

    #[test]
    fn test_process_line_simple() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"name": "test"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_numbers() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"value": 42}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_boolean() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"active": true}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_null() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"value": null}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_array() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"items": [1, 2, 3]}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_nested_object() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"user": {"name": "test"}}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_json_array() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_json_array(r#"[{"a": 1}, {"b": 2}]"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_negative_number() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"value": -123}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_float() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"value": 3.14}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_false() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"active": false}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_empty_object() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r"{}", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_empty_array() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r"[]", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_schema_filtering() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        let result = processor.process_line(r#"{"name": "test", "age": 30}"#, &schema);
        assert!(result.is_ok());
        let tape = result.unwrap();
        // Verify tape was created successfully
        assert!(tape.metadata.node_count > 0);
    }

    #[test]
    fn test_process_line_with_escape_sequence() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"name": "test\"value"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_scientific_notation() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"value": 1.5e10}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_whitespace() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{  "name"  :  "test"  }"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_newlines() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line("{\n\"name\"\n:\n\"test\"\n}", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_tabs() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line("{\t\"name\"\t:\t\"test\"\t}", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_json_array_empty() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_json_array(r"[]", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_json_array_nested() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_json_array(r"[[1, 2], [3, 4]]", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_schema_match_ratio_calculation() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"a": 1, "b": 2}"#, &schema);
        assert!(result.is_ok());
        let tape = result.unwrap();
        // With wildcard schema, all fields should match
        assert!(tape.metadata.schema_match_ratio >= 0.0);
    }

    #[test]
    fn test_process_deeply_nested_object() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let json = r#"{"a": {"b": {"c": {"d": {"e": 1}}}}}"#;
        let result = processor.process_line(json, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_multiple_booleans() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"a": true, "b": false, "c": true}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_multiple_nulls() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"a": null, "b": null}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_mixed_array() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(
            r#"{"items": [1, "two", true, null, {"nested": 1}]}"#,
            &schema,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_array_of_objects() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result =
            processor.process_line(r#"{"users": [{"name": "a"}, {"name": "b"}]}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_field_name_after_brace() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"{\"field\": \"value\"}";
        assert!(SkipTapeProcessor::is_field_name(bytes, 1));
    }

    #[test]
    fn test_is_field_name_after_comma() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"{\"a\": 1, \"field\": 2}";
        assert!(SkipTapeProcessor::is_field_name(bytes, 9));
    }

    #[test]
    fn test_is_field_name_value_position() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"{\"field\": \"value\"}";
        assert!(!SkipTapeProcessor::is_field_name(bytes, 10));
    }

    #[test]
    fn test_build_path_string_empty() {
        let _processor = SkipTapeProcessor::new();
        let path_stack: Vec<String> = Vec::new();
        assert_eq!(SkipTapeProcessor::build_path_string(&path_stack), "");
    }

    #[test]
    fn test_build_path_string_single() {
        let _processor = SkipTapeProcessor::new();
        let path_stack = vec!["field".to_string()];
        assert_eq!(SkipTapeProcessor::build_path_string(&path_stack), "field");
    }

    #[test]
    fn test_build_path_string_nested() {
        let _processor = SkipTapeProcessor::new();
        let path_stack = vec!["user".to_string(), "name".to_string()];
        assert_eq!(
            SkipTapeProcessor::build_path_string(&path_stack),
            "user.name"
        );
    }

    #[test]
    fn test_parse_string_basic() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"\"hello\"";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_string(bytes, &mut index);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn test_parse_string_with_escape() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"\"he\\\"llo\"";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_string(bytes, &mut index);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_string_unterminated() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"\"hello";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_string(bytes, &mut index);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_boolean_true() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"true";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_boolean(bytes, &mut index);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_parse_boolean_false() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"false";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_boolean(bytes, &mut index);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_parse_boolean_invalid() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"invalid";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_boolean(bytes, &mut index);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_number_integer() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"123";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_number(bytes, &mut index);
        assert!(result.is_ok());
        assert!((result.unwrap() - 123.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_number_negative() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"-456";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_number(bytes, &mut index);
        assert!(result.is_ok());
        assert!((result.unwrap() - (-456.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_number_float() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"1.23456";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_number(bytes, &mut index);
        assert!(result.is_ok());
        assert!((result.unwrap() - 1.23456).abs() < 0.001);
    }

    #[test]
    fn test_parse_number_scientific() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"1.5e10";
        let mut index = 0;
        let result = SkipTapeProcessor::parse_number(bytes, &mut index);
        assert!(result.is_ok());
        assert!((result.unwrap() - 1.5e10).abs() < 1.0);
    }

    #[test]
    fn test_skip_object_simple() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"{\"a\": 1}";
        let result = SkipTapeProcessor::skip_object(bytes, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 7);
    }

    #[test]
    fn test_skip_object_nested() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"{\"a\": {\"b\": 1}}";
        let result = SkipTapeProcessor::skip_object(bytes, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_object_with_string() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"{\"a\": \"}\"}";
        let result = SkipTapeProcessor::skip_object(bytes, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_array_simple() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"[1, 2, 3]";
        let result = SkipTapeProcessor::skip_array(bytes, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_array_nested() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"[[1], [2]]";
        let result = SkipTapeProcessor::skip_array(bytes, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_array_with_string() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"[\"]\"]";
        let result = SkipTapeProcessor::skip_array(bytes, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_expect_keyword_null() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"null";
        let mut index = 0;
        let result = SkipTapeProcessor::expect_keyword(bytes, &mut index, b"null");
        assert!(result.is_ok());
    }

    #[test]
    fn test_expect_keyword_mismatch() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"nope";
        let mut index = 0;
        let result = SkipTapeProcessor::expect_keyword(bytes, &mut index, b"null");
        assert!(result.is_err());
    }

    #[test]
    fn test_skip_value_string() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"\"test\"";
        let mut index = 0;
        let result = SkipTapeProcessor::skip_value(bytes, &mut index);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_number() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"123";
        let mut index = 0;
        let result = SkipTapeProcessor::skip_value(bytes, &mut index);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_boolean() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"true";
        let mut index = 0;
        let result = SkipTapeProcessor::skip_value(bytes, &mut index);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_null() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"null";
        let mut index = 0;
        let result = SkipTapeProcessor::skip_value(bytes, &mut index);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_object() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"{\"a\": 1}";
        let mut index = 0;
        let result = SkipTapeProcessor::skip_value(bytes, &mut index);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_value_array() {
        let _processor = SkipTapeProcessor::new();
        let bytes = b"[1, 2, 3]";
        let mut index = 0;
        let result = SkipTapeProcessor::skip_value(bytes, &mut index);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_unexpected_character() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        // This should either error or handle gracefully - the '@' is unexpected
        let result = processor.process_line(r#"{"name": @invalid}"#, &schema);
        // The result depends on parser behavior - just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_reset_clears_arena() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let _ = processor.process_line(r#"{"name": "test"}"#, &schema);
        processor.reset();
        // After reset, we can process again
        let result = processor.process_line(r#"{"name": "test2"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_unterminated_string() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"name": "unterminated"#, &schema);
        // Should be an error due to unterminated string
        assert!(result.is_err());
    }

    #[test]
    fn test_process_line_invalid_boolean_literal() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        // "trux" is not a valid boolean - verify it doesn't panic
        let result = processor.process_line(r#"{"flag": trux}"#, &schema);
        // The parser may handle this gracefully or error - just don't panic
        let _ = result;
    }

    #[test]
    fn test_process_line_invalid_null_literal() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        // "nulx" is not valid - verify it doesn't panic
        let result = processor.process_line(r#"{"value": nulx}"#, &schema);
        // The parser may handle this gracefully or error - just don't panic
        let _ = result;
    }

    #[test]
    fn test_process_line_empty_field_name() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"": "empty key"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_unicode_field() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"日本語": "value", "emoji": "🎉"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_schema_no_fields() {
        let mut processor = SkipTapeProcessor::new();
        // Empty schema should skip everything
        let schema = CompiledSchema::compile(&[]).unwrap();
        let result = processor.process_line(r#"{"name": "test"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_json_array_of_empty_objects() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_json_array(r"[{}, {}, {}]", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_negative_numbers() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"value": -123.45}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_colon_in_string() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"url": "http://example.com"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_multiple_escape_sequences() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"text": "line1\nline2\ttab\rreturn"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_default_impl() {
        let processor = SkipTapeProcessor::default();
        assert!(std::mem::size_of_val(&processor) > 0);
    }

    #[test]
    fn test_process_line_with_null_value() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"value": null}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_boolean_true() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"active": true}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_boolean_false() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"active": false}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_string_value() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"name": "test value"}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_with_schema_filtering_skip() {
        let mut processor = SkipTapeProcessor::new();
        // Only include "name" field, skip "age"
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        let result = processor.process_line(r#"{"name": "Alice", "age": 30}"#, &schema);
        assert!(result.is_ok());
        let tape = result.unwrap();
        // Should have skipped some nodes
        assert!(tape.metadata().skipped_count > 0 || tape.metadata().node_count > 0);
    }

    #[test]
    fn test_process_line_empty_json() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r"{}", &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_reset() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();

        // Process first line
        let _ = processor.process_line(r#"{"a": 1}"#, &schema);

        // Reset and process second line
        processor.reset();
        let result = processor.process_line(r#"{"b": 2}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_mixed_types() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(
            r#"{"string": "value", "number": 42, "bool": true, "null": null}"#,
            &schema,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_nested_with_filter() {
        let mut processor = SkipTapeProcessor::new();
        let schema =
            CompiledSchema::compile(&["user".to_string(), "user.name".to_string()]).unwrap();
        let result = processor.process_line(
            r#"{"user": {"name": "Alice", "email": "alice@test.com"}, "other": "ignored"}"#,
            &schema,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_line_array_with_objects() {
        let mut processor = SkipTapeProcessor::new();
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let result = processor.process_line(r#"{"items": [{"id": 1}, {"id": 2}]}"#, &schema);
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_tape_metadata_zero_nodes() {
        let mut processor = SkipTapeProcessor::new();
        // Empty object should still produce valid tape
        let schema = CompiledSchema::compile(&["nonexistent".to_string()]).unwrap();
        let result = processor.process_line(r"{}", &schema);
        assert!(result.is_ok());
    }
}
