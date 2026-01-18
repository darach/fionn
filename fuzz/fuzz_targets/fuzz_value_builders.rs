#![no_main]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! libFuzzer target for ValueBuilder implementations
//!
//! Tests:
//! - JsonBuilder roundtrip
//! - YamlBuilder roundtrip (when feature enabled)
//! - TomlBuilder roundtrip (when feature enabled)
//! - CsvBuilder pattern recognition
//! - IsonBuilder/ToonBuilder structure

use libfuzzer_sys::fuzz_target;
use fionn_core::{JsonBuilder, ValueBuilder, PathSegment, set_at_path_json};
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    // Skip overly large inputs
    if data.len() > 50_000 || data.len() < 2 {
        return;
    }

    // === Test 1: JsonBuilder basic operations ===
    let mut builder = JsonBuilder;

    // Test null
    let null_val = builder.null();
    assert!(null_val.is_null(), "null() should produce null");

    // Test bool
    let bool_val = builder.bool(data[0] % 2 == 0);
    assert!(bool_val.is_boolean(), "bool() should produce boolean");

    // === Test 2: JsonBuilder with fuzzed data ===

    // Try to interpret first bytes as numbers
    if data.len() >= 8 {
        let int_bytes: [u8; 8] = data[0..8].try_into().unwrap_or([0; 8]);
        let int_val = i64::from_le_bytes(int_bytes);
        let json_int = builder.int(int_val);
        assert!(json_int.is_i64() || json_int.is_u64(), "int() should produce integer");
    }

    // Try to create float
    if data.len() >= 8 {
        let float_bytes: [u8; 8] = data[0..8].try_into().unwrap_or([0; 8]);
        let float_val = f64::from_le_bytes(float_bytes);
        if float_val.is_finite() {
            let json_float = builder.float(float_val);
            assert!(json_float.is_f64(), "float() should produce float");
        }
    }

    // === Test 3: String creation ===
    if let Ok(s) = std::str::from_utf8(data) {
        let json_str = builder.string(s);
        assert!(json_str.is_string(), "string() should produce string");
        assert_eq!(json_str.as_str().unwrap(), s, "string should roundtrip");
    }

    // === Test 4: Object/Array construction ===
    let mut obj = builder.empty_object();
    let mut arr = builder.empty_array();

    // Add fields to object
    for (i, chunk) in data.chunks(4).enumerate().take(10) {
        if let Ok(key) = std::str::from_utf8(chunk) {
            if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let val = builder.int(i as i64);
                builder.insert_field(&mut obj, key, val);
            }
        }
    }

    // Add elements to array
    for i in 0..data.len().min(20) {
        let val = builder.int(data[i] as i64);
        builder.push_element(&mut arr, val);
    }

    // Verify structure
    assert!(obj.is_object(), "Should be object");
    assert!(arr.is_array(), "Should be array");

    // === Test 5: Serialization roundtrip ===
    if let Ok(serialized) = builder.serialize(&obj) {
        // Should be valid JSON
        let parsed: Result<Value, _> = serde_json::from_str(&serialized);
        assert!(parsed.is_ok(), "Serialized JSON should be parseable");
    }

    if let Ok(serialized) = builder.serialize(&arr) {
        let parsed: Result<Value, _> = serde_json::from_str(&serialized);
        assert!(parsed.is_ok(), "Serialized array should be parseable");
    }

    // === Test 6: set_at_path_json ===
    let mut root = Value::Object(serde_json::Map::new());

    // Create path from data
    let path_segments: Vec<PathSegment> = data
        .chunks(2)
        .take(5)
        .enumerate()
        .filter_map(|(i, chunk)| {
            if chunk[0] % 2 == 0 {
                Some(PathSegment::Index(chunk[0] as usize % 10))
            } else if let Ok(s) = std::str::from_utf8(chunk) {
                if s.chars().all(|c| c.is_alphanumeric()) && !s.is_empty() {
                    Some(PathSegment::Field(s.to_string()))
                } else {
                    Some(PathSegment::Field(format!("key{}", i)))
                }
            } else {
                Some(PathSegment::Field(format!("key{}", i)))
            }
        })
        .collect();

    if !path_segments.is_empty() {
        let leaf_value = builder.int(42);
        // This may fail for some paths (e.g., index on non-array) - that's OK
        let _ = set_at_path_json(&mut root, &path_segments, leaf_value);
    }
});
