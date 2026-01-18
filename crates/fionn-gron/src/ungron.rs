// SPDX-License-Identifier: MIT OR Apache-2.0
//! Ungron: reconstruct JSON from gron output.
//!
//! This module parses gron-format lines and reconstructs the original JSON
//! document. This is the inverse of the gron transformation.

use super::path_extended::{ExtendedPathComponent, parse_extended_path};
use fionn_core::{DsonError, Result};
use serde_json::{Map, Value};

/// Maximum array index allowed to prevent OOM from malicious input.
/// This limits arrays to ~10 million elements which is ~80MB for nulls.
const MAX_ARRAY_INDEX: usize = 10_000_000;

/// Convert gron output back to JSON string.
///
/// # Errors
/// Returns an error if parsing fails.
///
/// # Example
/// ```rust,ignore
/// let gron_text = r#"json = {};
/// json.name = "Alice";
/// json.age = 30;
/// "#;
/// let json = ungron(gron_text)?;
/// // {"name":"Alice","age":30}
/// ```
pub fn ungron(input: &str) -> Result<String> {
    let value = ungron_to_value(input)?;
    serde_json::to_string(&value).map_err(|e| DsonError::SerializationError(e.to_string()))
}

/// Convert gron output to a `serde_json` Value.
///
/// # Errors
/// Returns an error if parsing fails.
pub fn ungron_to_value(input: &str) -> Result<Value> {
    let mut root = Value::Null;

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse gron line: path = value;
        let (path_str, value_str) = parse_gron_line(line)?;

        // Parse the path using extended parser
        let components = parse_extended_path(path_str);

        // Skip root component (e.g., "json")
        let path_components: Vec<_> = if components.len() > 1 {
            components.into_iter().skip(1).collect()
        } else {
            // Root assignment
            vec![]
        };

        // Parse the value
        let value = parse_json_value(value_str)?;

        // Set the value at the path
        set_path(&mut root, &path_components, value);
    }

    Ok(root)
}

/// Parse a gron line into path and value parts.
fn parse_gron_line(line: &str) -> Result<(&str, &str)> {
    // Handle both compact (=) and standard ( = ) formats
    // Also handle trailing semicolon

    let line = line.trim_end_matches(';').trim();

    // Find the = separator
    let eq_pos = line
        .find(" = ")
        .map(|p| (p, p + 3))
        .or_else(|| line.find('=').map(|p| (p, p + 1)));

    let (path_end, value_start) =
        eq_pos.ok_or_else(|| DsonError::ParseError(format!("Invalid gron line: {line}")))?;

    let path = line[..path_end].trim();
    let value = line[value_start..].trim();

    Ok((path, value))
}

/// Parse a JSON value string.
fn parse_json_value(s: &str) -> Result<Value> {
    // Handle special cases
    match s {
        "{}" => return Ok(Value::Object(Map::new())),
        "[]" => return Ok(Value::Array(Vec::new())),
        "null" => return Ok(Value::Null),
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        _ => {}
    }

    // Try parsing as JSON
    serde_json::from_str(s)
        .map_err(|e| DsonError::ParseError(format!("Invalid JSON value '{s}': {e}")))
}

/// Set a value at the given path in the JSON tree.
fn set_path(root: &mut Value, path: &[ExtendedPathComponent], value: Value) {
    if path.is_empty() {
        *root = value;
        return;
    }

    // Ensure root is the right type
    if *root == Value::Null {
        *root = match &path[0] {
            ExtendedPathComponent::Field(_) => Value::Object(Map::new()),
            ExtendedPathComponent::ArrayIndex(_) => Value::Array(Vec::new()),
        };
    }

    let mut current = root;

    for (i, component) in path.iter().enumerate() {
        let is_last = i == path.len() - 1;

        match component {
            ExtendedPathComponent::Field(key) => {
                // Ensure current is an object
                if !current.is_object() {
                    *current = Value::Object(Map::new());
                }

                let obj = current.as_object_mut().unwrap();

                if is_last {
                    obj.insert(key.clone(), value);
                    return;
                }

                // Look ahead to determine what type the next level should be
                let next_type = match &path[i + 1] {
                    ExtendedPathComponent::Field(_) => Value::Object(Map::new()),
                    ExtendedPathComponent::ArrayIndex(_) => Value::Array(Vec::new()),
                };

                // Navigate into or create the child
                if !obj.contains_key(key) {
                    obj.insert(key.clone(), next_type);
                }
                current = obj.get_mut(key).unwrap();
            }

            ExtendedPathComponent::ArrayIndex(index) => {
                // Prevent OOM from excessively large array indices
                if *index > MAX_ARRAY_INDEX {
                    return;
                }

                // Ensure current is an array
                if !current.is_array() {
                    *current = Value::Array(Vec::new());
                }

                let arr = current.as_array_mut().unwrap();

                // Extend array if needed
                while arr.len() <= *index {
                    arr.push(Value::Null);
                }

                if is_last {
                    arr[*index] = value;
                    return;
                }

                // Look ahead to determine what type the next level should be
                let next_type = match &path[i + 1] {
                    ExtendedPathComponent::Field(_) => Value::Object(Map::new()),
                    ExtendedPathComponent::ArrayIndex(_) => Value::Array(Vec::new()),
                };

                // Ensure the element has the right type
                if arr[*index] == Value::Null {
                    arr[*index] = next_type;
                }
                current = &mut arr[*index];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_ungron() {
        let input = r#"
json = {};
json.name = "Alice";
json.age = 30;
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["name"], "Alice");
        assert_eq!(value["age"], 30);
    }

    #[test]
    fn test_nested_ungron() {
        let input = r#"
json = {};
json.user = {};
json.user.name = "Bob";
json.user.active = true;
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["user"]["name"], "Bob");
        assert_eq!(value["user"]["active"], true);
    }

    #[test]
    fn test_array_ungron() {
        let input = r"
json = {};
json.items = [];
json.items[0] = 1;
json.items[1] = 2;
json.items[2] = 3;
";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["items"][0], 1);
        assert_eq!(value["items"][1], 2);
        assert_eq!(value["items"][2], 3);
    }

    #[test]
    fn test_array_of_objects_ungron() {
        let input = r#"
json = {};
json.users = [];
json.users[0] = {};
json.users[0].name = "Alice";
json.users[1] = {};
json.users[1].name = "Bob";
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["users"][0]["name"], "Alice");
        assert_eq!(value["users"][1]["name"], "Bob");
    }

    #[test]
    fn test_bracket_notation_ungron() {
        let input = r#"
json = {};
json["field.with.dots"] = "value";
json["field[0]"] = "value2";
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["field.with.dots"], "value");
        assert_eq!(value["field[0]"], "value2");
    }

    #[test]
    fn test_mixed_notation_ungron() {
        let input = r#"
json = {};
json["key"].items = [];
json["key"].items[0] = "value";
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["key"]["items"][0], "value");
    }

    #[test]
    fn test_compact_format_ungron() {
        let input = r#"
json={};
json.name="Alice";
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["name"], "Alice");
    }

    #[test]
    fn test_null_value_ungron() {
        let input = r"
json = {};
json.value = null;
";
        let value = ungron_to_value(input).unwrap();
        assert!(value["value"].is_null());
    }

    #[test]
    fn test_boolean_values_ungron() {
        let input = r"
json = {};
json.active = true;
json.deleted = false;
";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["active"], true);
        assert_eq!(value["deleted"], false);
    }

    #[test]
    fn test_float_values_ungron() {
        let input = r"
json = {};
json.price = 19.99;
";
        let value = ungron_to_value(input).unwrap();
        assert!((value["price"].as_f64().unwrap() - 19.99).abs() < 0.00001);
    }

    #[test]
    fn test_escaped_string_ungron() {
        let input = r#"
json = {};
json.message = "line1\nline2";
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["message"], "line1\nline2");
    }

    #[test]
    fn test_roundtrip() {
        use super::super::gron_core::{GronOptions, gron};

        let original_json = r#"{"name":"Alice","items":[1,2,3],"nested":{"key":"value"}}"#;

        // gron -> ungron roundtrip
        let gron_output = gron(original_json, &GronOptions::default()).unwrap();
        let reconstructed = ungron_to_value(&gron_output).unwrap();

        // Parse original for comparison
        let original: Value = serde_json::from_str(original_json).unwrap();

        assert_eq!(reconstructed, original);
    }

    #[test]
    fn test_roundtrip_special_fields() {
        use super::super::gron_core::{GronOptions, gron};

        let original_json = r#"{"field.with.dots":"value","field[0]":"value2"}"#;

        let gron_output = gron(original_json, &GronOptions::default()).unwrap();
        let reconstructed = ungron_to_value(&gron_output).unwrap();

        let original: Value = serde_json::from_str(original_json).unwrap();

        assert_eq!(reconstructed, original);
    }

    #[test]
    fn test_empty_object_ungron() {
        let input = "json = {};";
        let value = ungron_to_value(input).unwrap();
        assert!(value.is_object());
        assert!(value.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_empty_array_ungron() {
        let input = "json = [];";
        let value = ungron_to_value(input).unwrap();
        assert!(value.is_array());
        assert!(value.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_deep_nesting_ungron() {
        let input = r#"
json = {};
json.a = {};
json.a.b = {};
json.a.b.c = {};
json.a.b.c.d = "value";
"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["a"]["b"]["c"]["d"], "value");
    }

    // =========================================================================
    // Additional Coverage Tests
    // =========================================================================

    #[test]
    fn test_ungron_string_output() {
        let input = "json = {};\njson.name = \"test\";";
        let result = ungron(input);
        assert!(result.is_ok());
        let json_str = result.unwrap();
        assert!(json_str.contains("name"));
        assert!(json_str.contains("test"));
    }

    #[test]
    fn test_parse_gron_line_invalid() {
        let result = parse_gron_line("no equals sign here");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gron_line_with_semicolon() {
        let result = parse_gron_line("json.key = \"value\";");
        assert!(result.is_ok());
        let (path, value) = result.unwrap();
        assert_eq!(path, "json.key");
        assert_eq!(value, "\"value\"");
    }

    #[test]
    fn test_parse_gron_line_compact() {
        let result = parse_gron_line("json.key=\"value\"");
        assert!(result.is_ok());
        let (path, value) = result.unwrap();
        assert_eq!(path, "json.key");
        assert_eq!(value, "\"value\"");
    }

    #[test]
    fn test_parse_json_value_number() {
        let result = parse_json_value("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Number(42.into()));
    }

    #[test]
    fn test_parse_json_value_string() {
        let result = parse_json_value("\"hello\"");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hello".to_string()));
    }

    #[test]
    fn test_parse_json_value_invalid() {
        let result = parse_json_value("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_root_assignment_only() {
        // When path has only root component
        let input = "json = 42;";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_root_array_assignment() {
        let input = "json = [];\njson[0] = 1;\njson[1] = 2;";
        let value = ungron_to_value(input).unwrap();
        assert!(value.is_array());
        assert_eq!(value[0], 1);
        assert_eq!(value[1], 2);
    }

    #[test]
    fn test_array_with_gaps() {
        // Test extending array with nulls
        let input = "json = [];\njson[5] = \"value\";";
        let value = ungron_to_value(input).unwrap();
        assert!(value.is_array());
        let arr = value.as_array().unwrap();
        assert_eq!(arr.len(), 6);
        assert!(arr[0].is_null());
        assert_eq!(arr[5], "value");
    }

    #[test]
    fn test_overwrite_null_with_object() {
        // Test overwriting null root with object
        let input = "json.key = \"value\";";
        let value = ungron_to_value(input).unwrap();
        assert!(value.is_object());
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn test_overwrite_null_with_array() {
        // Test overwriting null root with array
        let input = "json[0] = \"value\";";
        let value = ungron_to_value(input).unwrap();
        assert!(value.is_array());
        assert_eq!(value[0], "value");
    }

    #[test]
    fn test_nested_array_of_arrays() {
        let input = r"
json = [];
json[0] = [];
json[0][0] = 1;
json[0][1] = 2;
json[1] = [];
json[1][0] = 3;
";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value[0][0], 1);
        assert_eq!(value[0][1], 2);
        assert_eq!(value[1][0], 3);
    }

    #[test]
    fn test_empty_lines_ignored() {
        let input = "\n\njson = {};\n\njson.key = \"value\";\n\n";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn test_whitespace_lines_ignored() {
        let input = "   \njson = {};\n   \njson.key = \"value\";\n   ";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn test_error_on_invalid_line() {
        let input = "this is not valid gron";
        let result = ungron_to_value(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_negative_number() {
        let input = "json = {};\njson.val = -42;";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["val"], -42);
    }

    #[test]
    fn test_scientific_notation() {
        let input = "json = {};\njson.val = 1.5e10;";
        let value = ungron_to_value(input).unwrap();
        assert!((value["val"].as_f64().unwrap() - 1.5e10).abs() < 1.0);
    }

    #[test]
    fn test_string_with_equals() {
        // String containing = character
        let input = r#"json = {};
json.formula = "a = b";"#;
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["formula"], "a = b");
    }

    #[test]
    fn test_field_becomes_object() {
        // If a path goes through a field that wasn't explicitly set as object
        let input = "json.a.b = \"value\";";
        let value = ungron_to_value(input).unwrap();
        assert!(value["a"].is_object());
        assert_eq!(value["a"]["b"], "value");
    }

    #[test]
    fn test_field_becomes_array() {
        // If a path goes through a field that needs to become an array
        let input = "json.arr[0] = \"value\";";
        let value = ungron_to_value(input).unwrap();
        assert!(value["arr"].is_array());
        assert_eq!(value["arr"][0], "value");
    }

    #[test]
    fn test_array_element_becomes_object() {
        let input = "json[0].key = \"value\";";
        let value = ungron_to_value(input).unwrap();
        assert!(value[0].is_object());
        assert_eq!(value[0]["key"], "value");
    }

    #[test]
    fn test_array_element_becomes_array() {
        let input = "json[0][0] = \"value\";";
        let value = ungron_to_value(input).unwrap();
        assert!(value[0].is_array());
        assert_eq!(value[0][0], "value");
    }

    #[test]
    fn test_unicode_string() {
        let input = "json = {};\njson.emoji = \"😀\";";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["emoji"], "😀");
    }

    #[test]
    fn test_quoted_empty_string() {
        let input = "json = {};\njson.empty = \"\";";
        let value = ungron_to_value(input).unwrap();
        assert_eq!(value["empty"], "");
    }

    #[test]
    fn test_large_array_index_rejected() {
        // This should not OOM - large indices are rejected
        let input = "json[999999999999] = \"value\";";
        let value = ungron_to_value(input).unwrap();
        // The value should be an empty array since the large index was rejected
        assert!(value.is_array());
        assert!(value.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_max_array_index_boundary() {
        // Index at MAX_ARRAY_INDEX + 1 should be rejected
        let input = format!("json[{}] = \"value\";", super::MAX_ARRAY_INDEX + 1);
        let value = ungron_to_value(&input).unwrap();
        // The value should be an empty array since the large index was rejected
        assert!(value.is_array());
        assert!(value.as_array().unwrap().is_empty());
    }
}
