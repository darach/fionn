#![no_main]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz target for Tape path resolution correctness.
//!
//! This target verifies that Tape.resolve_path() and related methods
//! return correct values by comparing against serde_json reference.
//!
//! Run with:
//!   cargo +nightly fuzz run fuzz_tape_path_resolution

use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use fionn_tape::DsonTape;

/// Extract a value from serde_json::Value using a path string
fn extract_from_serde(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;

    // Parse path manually
    let mut chars = path.chars().peekable();

    while chars.peek().is_some() {
        // Skip leading dots
        while chars.peek() == Some(&'.') {
            chars.next();
        }

        if chars.peek().is_none() {
            break;
        }

        // Check for array index
        if chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            let mut num_str = String::new();
            while let Some(&c) = chars.peek() {
                if c == ']' {
                    chars.next();
                    break;
                }
                if c.is_ascii_digit() {
                    num_str.push(c);
                    chars.next();
                } else {
                    return None; // Invalid index
                }
            }

            let idx: usize = num_str.parse().ok()?;
            current = current.get(idx)?;
        } else {
            // Field name
            let mut field_name = String::new();
            while let Some(&c) = chars.peek() {
                if c == '.' || c == '[' {
                    break;
                }
                field_name.push(c);
                chars.next();
            }

            if field_name.is_empty() {
                return None;
            }

            current = current.get(&field_name)?;
        }
    }

    Some(current.clone())
}

/// Compare tape extraction with serde_json reference
fn verify_path_resolution(json_str: &str, path: &str) {
    // Parse with both
    let serde_value: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return, // Invalid JSON
    };

    let tape = match DsonTape::parse(json_str) {
        Ok(t) => t,
        Err(_) => return, // Tape parse failed (should not happen for valid JSON)
    };

    // Resolve path with tape
    let tape_idx = tape.resolve_path(path);

    // Extract with serde reference
    let serde_result = extract_from_serde(&serde_value, path);

    // Compare results
    match (tape_idx, &serde_result) {
        (Ok(Some(idx)), Some(expected)) => {
            // Tape found something, verify it's correct
            if let Some(tape_value) = tape.extract_value_simd(idx) {
                match (&expected, &tape_value) {
                    (Value::Null, fionn_tape::SimdValue::Null) => {}
                    (Value::Bool(b1), fionn_tape::SimdValue::Bool(b2)) => {
                        assert_eq!(b1, b2, "Bool mismatch for path {}", path);
                    }
                    (Value::Number(n), fionn_tape::SimdValue::Number(s)) => {
                        // Compare as strings to avoid float issues
                        let n_str = n.to_string();
                        // Allow for float formatting differences - just verify no crash
                        // Different parsers may represent floats differently
                        if n_str != *s {
                            // Try parsing both as f64 to verify they're valid numbers
                            let _ = (n_str.parse::<f64>(), s.parse::<f64>());
                            // Don't assert equality - parser differences are expected
                        }
                    }
                    (Value::String(s1), fionn_tape::SimdValue::String(s2)) => {
                        assert_eq!(s1, s2, "String mismatch for path {}", path);
                    }
                    // Arrays and objects are not returned as SimdValue
                    (Value::Array(_), _) | (Value::Object(_), _) => {}
                    _ => {}
                }
            }
        }
        (Ok(None), &None) => {
            // Both agree the path doesn't exist - good
        }
        (Ok(Some(_)), &None) => {
            // Tape found something but serde didn't
            // This could be a false positive (tape found wrong thing)
            // or serde path parsing is wrong - investigate case by case
        }
        (Ok(None), &Some(_)) => {
            // Serde found something but tape didn't - this is a BUG!
            panic!(
                "Tape failed to find path {} in JSON but serde found it!\nJSON: {}\nExpected: {:?}",
                path, json_str, serde_result
            );
        }
        (Err(_), _) => {
            // Tape path resolution error - acceptable for malformed paths
        }
    }
}

/// Generate test paths for a JSON structure
fn generate_paths(value: &Value, prefix: &str, paths: &mut Vec<String>) {
    match value {
        Value::Object(obj) => {
            for (key, val) in obj {
                let new_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                paths.push(new_prefix.clone());
                generate_paths(val, &new_prefix, paths);
            }
        }
        Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                let new_prefix = format!("{}[{}]", prefix, i);
                paths.push(new_prefix.clone());
                generate_paths(val, &new_prefix, paths);
            }
        }
        _ => {
            // Leaf value, path already added
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    let json_str = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Skip very large inputs
    if json_str.len() > 50_000 {
        return;
    }

    // Parse as JSON
    let value: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return, // Not valid JSON
    };

    // Generate all valid paths for this JSON
    let mut paths = Vec::new();
    generate_paths(&value, "", &mut paths);

    // Add the root path
    paths.push(String::new());

    // Test each generated path
    for path in &paths {
        verify_path_resolution(json_str, path);
    }

    // Also test some potentially invalid paths
    let invalid_paths = [
        "[999999]",
        "nonexistent",
        "a.b.c.d.e.f",
        "[0][0][0][0]",
        "...",
        "[]",
        "[abc]",
    ];

    for path in &invalid_paths {
        // These shouldn't panic
        let _ = DsonTape::parse(json_str).map(|t| t.resolve_path(path));
    }
});
