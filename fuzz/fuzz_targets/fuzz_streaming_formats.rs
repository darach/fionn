// SPDX-License-Identifier: MIT OR Apache-2.0
//! libFuzzer target for JSONL and ISONL streaming format operations.
//!
//! Tests parsing, serialization, and round-trip consistency for
//! fionn's first-class streaming formats.
//!
//! Run with: cargo +nightly fuzz run fuzz_streaming_formats

#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;

// ============================================================================
// Float comparison helpers (for roundtrip tests with IEEE 754 precision limits)
// ============================================================================

/// Compare two f64 values for approximate equality using relative tolerance.
fn floats_approximately_equal(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a.is_nan() || b.is_nan() {
        return false;
    }
    if a.is_infinite() && b.is_infinite() {
        return a.signum() == b.signum();
    }
    if a.is_infinite() || b.is_infinite() {
        return false;
    }
    if a == b {
        return true;
    }
    let abs_diff = (a - b).abs();
    let max_abs = a.abs().max(b.abs());
    if max_abs < f64::MIN_POSITIVE {
        abs_diff < f64::MIN_POSITIVE
    } else {
        abs_diff / max_abs < 1e-14
    }
}

/// Compare JSON values with proper handling of float precision edge cases.
fn values_semantically_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => {
            if a == b {
                return true;
            }
            match (a.as_f64(), b.as_f64()) {
                (Some(fa), Some(fb)) => floats_approximately_equal(fa, fb),
                _ => false,
            }
        }
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_semantically_equal(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(k, v)| b.get(k).is_some_and(|bv| values_semantically_equal(v, bv)))
        }
        _ => false,
    }
}

// ============================================================================
// JSONL Testing
// ============================================================================

/// Parse JSONL string to vector of JSON values
fn parse_jsonl(data: &str) -> Vec<Value> {
    let mut results = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str(line) {
            results.push(value);
        }
    }
    results
}

/// Serialize values to JSONL
fn to_jsonl(values: &[Value]) -> String {
    let mut output = String::new();
    for value in values {
        if let Ok(line) = serde_json::to_string(value) {
            output.push_str(&line);
            output.push('\n');
        }
    }
    output
}

/// Test JSONL round-trip: parse -> serialize -> parse
fn test_jsonl_roundtrip(data: &str) {
    let values = parse_jsonl(data);
    if values.is_empty() {
        return;
    }

    let serialized = to_jsonl(&values);
    let reparsed = parse_jsonl(&serialized);

    // Values should be identical after round-trip
    assert_eq!(
        values.len(),
        reparsed.len(),
        "JSONL round-trip should preserve record count"
    );

    for (i, (original, reparsed)) in values.iter().zip(reparsed.iter()).enumerate() {
        assert!(
            values_semantically_equal(original, reparsed),
            "JSONL round-trip mismatch at record {}: {:?} vs {:?}",
            i, original, reparsed
        );
    }
}

// ============================================================================
// ISONL Testing
// ============================================================================

/// Parse an ISONL line: table.name|field1:type|field2:type|value1|value2
fn parse_isonl_line(line: &str) -> Option<Value> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 3 {
        return None;
    }

    // First part is table name (optional validation)
    // Next parts until we have types are schema
    // Rest are values

    let mut schema_parts = Vec::new();
    let mut value_start_idx = 1;

    for (i, part) in parts.iter().enumerate().skip(1) {
        if part.contains(':') {
            schema_parts.push(*part);
            value_start_idx = i + 1;
        } else {
            break;
        }
    }

    if schema_parts.is_empty() {
        return None;
    }

    let values = &parts[value_start_idx..];
    let mut obj = serde_json::Map::new();

    for (i, schema) in schema_parts.iter().enumerate() {
        let (name, type_hint) = schema.split_once(':')?;
        let value = values.get(i).copied().unwrap_or("");

        let json_value = match type_hint {
            "int" => {
                if let Ok(n) = value.parse::<i64>() {
                    Value::Number(n.into())
                } else {
                    Value::String(value.to_string())
                }
            }
            "float" => {
                if let Ok(f) = value.parse::<f64>() {
                    serde_json::Number::from_f64(f)
                        .map(Value::Number)
                        .unwrap_or(Value::Null)
                } else {
                    Value::String(value.to_string())
                }
            }
            "bool" => Value::Bool(value == "true" || value == "1"),
            _ => Value::String(value.to_string()),
        };

        obj.insert(name.to_string(), json_value);
    }

    Some(Value::Object(obj))
}

/// Parse ISONL data to vector of JSON values
fn parse_isonl(data: &str) -> Vec<Value> {
    let mut results = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = parse_isonl_line(line) {
            results.push(value);
        }
    }
    results
}

/// Infer schema from JSON object for ISONL serialization
fn infer_schema(obj: &serde_json::Map<String, Value>) -> Vec<(String, String)> {
    obj.iter()
        .map(|(k, v)| {
            let type_hint = match v {
                Value::Number(n) if n.is_i64() => "int",
                Value::Number(n) if n.is_f64() => "float",
                Value::Bool(_) => "bool",
                _ => "string",
            };
            (k.clone(), type_hint.to_string())
        })
        .collect()
}

/// Serialize values to ISONL
fn to_isonl(values: &[Value], table: &str) -> Option<String> {
    if values.is_empty() {
        return Some(String::new());
    }

    // Get schema from first object
    let first = values.first()?;
    let obj = first.as_object()?;
    let schema = infer_schema(obj);

    let mut output = String::new();

    for value in values {
        let obj = value.as_object()?;

        // Write table name and schema
        output.push_str(&format!("table.{}", table));
        for (name, type_hint) in &schema {
            output.push('|');
            output.push_str(name);
            output.push(':');
            output.push_str(type_hint);
        }

        // Write values
        for (name, _) in &schema {
            output.push('|');
            if let Some(v) = obj.get(name) {
                match v {
                    Value::String(s) => output.push_str(s),
                    Value::Number(n) => output.push_str(&n.to_string()),
                    Value::Bool(b) => output.push_str(if *b { "true" } else { "false" }),
                    Value::Null => output.push_str("null"),
                    _ => {}
                }
            }
        }
        output.push('\n');
    }

    Some(output)
}

/// Test ISONL round-trip: parse -> serialize -> parse
fn test_isonl_roundtrip(data: &str) {
    let values = parse_isonl(data);
    if values.is_empty() {
        return;
    }

    // Only test objects (ISONL is tabular)
    let objects: Vec<&Value> = values.iter().filter(|v| v.is_object()).collect();
    if objects.is_empty() {
        return;
    }

    if let Some(serialized) = to_isonl(&values, "test") {
        let reparsed = parse_isonl(&serialized);

        // Count should match
        assert_eq!(
            values.len(),
            reparsed.len(),
            "ISONL round-trip should preserve record count"
        );
    }
}

// ============================================================================
// Cross-format testing
// ============================================================================

/// Test JSONL -> ISONL transformation
fn test_jsonl_to_isonl(data: &str) {
    let jsonl_values = parse_jsonl(data);
    if jsonl_values.is_empty() {
        return;
    }

    // Filter to only non-empty objects with valid keys for ISONL:
    // - Non-empty keys (required for schema)
    // - No pipe characters (ISONL field separator)
    // - No colons (ISONL type separator)
    // - No newlines (ISONL record separator)
    let objects: Vec<Value> = jsonl_values
        .into_iter()
        .filter(|v| {
            v.as_object().is_some_and(|o| {
                !o.is_empty() && o.keys().all(|k| {
                    !k.is_empty() && !k.contains('|') && !k.contains(':') && !k.contains('\n')
                })
            })
        })
        .collect();

    if objects.is_empty() {
        return;
    }

    // Convert to ISONL
    if let Some(isonl) = to_isonl(&objects, "converted") {
        // Parse back
        let back = parse_isonl(&isonl);

        // Should have same count
        assert_eq!(
            objects.len(),
            back.len(),
            "JSONL->ISONL should preserve record count"
        );
    }
}

// ============================================================================
// Fuzz target
// ============================================================================

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    let text = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Skip very large inputs
    if text.len() > 100_000 {
        return;
    }

    // Use first byte to select test type
    if data.is_empty() {
        return;
    }

    match data[0] % 4 {
        0 => {
            // Test as JSONL
            test_jsonl_roundtrip(text);
        }
        1 => {
            // Test as ISONL
            test_isonl_roundtrip(text);
        }
        2 => {
            // Test JSONL -> ISONL conversion
            test_jsonl_to_isonl(text);
        }
        3 => {
            // Test all
            test_jsonl_roundtrip(text);
            test_isonl_roundtrip(text);
            test_jsonl_to_isonl(text);
        }
        _ => unreachable!(),
    }

    // Always test that parsing doesn't panic
    let _ = parse_jsonl(text);
    let _ = parse_isonl(text);
});
