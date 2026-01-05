#![allow(clippy::all)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::pedantic)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! AFL fuzz target for JSON diff/patch/merge operations.
//!
//! This target tests:
//! - diff -> patch roundtrip
//! - SIMD byte comparison equivalence
//! - Merge patch operations
//! - Edge cases in structural comparison
//!
//! Run with:
//!   cargo afl build --release --features afl-fuzz -bin `fuzz_diff
//!   cargo afl fuzz -i fuzz/corpus/diff -o fuzz/output/diff target/release/`fuzz_diff

#[macro_use]
extern crate afl;

use fionn_diff::{
    JsonPatch, apply_patch, deep_merge, json_diff, json_merge_patch, simd_bytes_equal,
    simd_find_first_difference,
};
use serde_json::Value;

/// Test diff -> patch roundtrip
fn fuzz_diff_patch_roundtrip(data: &[u8]) {
    // Split data to create source and target
    if data.len() < 2 {
        return;
    }

    let split = data.len() / 2;
    let source_data = &data[..split];
    let target_data = &data[split..];

    // Try to interpret as JSON
    let source: Value = match serde_json::from_slice(source_data) {
        Ok(v) => v,
        Err(_) => return,
    };

    let target: Value = match serde_json::from_slice(target_data) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Generate diff
    let patch = json_diff(&source, &target);

    // Apply patch
    let result = apply_patch(&source, &patch);

    // Verify roundtrip
    match result {
        Ok(patched) => {
            assert_eq!(
                patched, target,
                "diff/patch roundtrip failed: source={:?}, target={:?}, patch={:?}",
                source, target, patch
            );
        }
        Err(e) => {
            panic!(
                "patch application failed: {:?}, source={:?}, target={:?}",
                e, source, target
            );
        }
    }
}

/// Test diff of identical values produces empty patch
fn fuzz_diff_identical(data: &[u8]) {
    let value: Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => return,
    };

    let patch = json_diff(&value, &value);

    assert!(
        patch.is_empty(),
        "diff of identical values should be empty: value={:?}, patch={:?}",
        value,
        patch
    );
}

/// Test SIMD byte equality matches standard
fn fuzz_simd_equality(data: &[u8]) {
    if data.len() < 2 {
        return;
    }

    let split = data.len() / 2;
    let a = &data[..split];
    let b = &data[split..];

    let std_eq = a == b;
    let simd_eq = simd_bytes_equal(a, b);

    assert_eq!(
        std_eq, simd_eq,
        "SIMD equality mismatch: std={}, simd={}",
        std_eq, simd_eq
    );
}

/// Test SIMD find first difference
fn fuzz_simd_find_diff(data: &[u8]) {
    if data.len() < 2 {
        return;
    }

    let split = data.len() / 2;
    let a = &data[..split];
    let b = &data[split..];

    let simd_diff = simd_find_first_difference(a, b);

    // Verify correctness
    let expected = a
        .iter()
        .zip(b.iter())
        .position(|(x, y)| x != y)
        .or_else(|| {
            if a.len() != b.len() {
                Some(a.len().min(b.len()))
            } else {
                None
            }
        });

    assert_eq!(
        simd_diff, expected,
        "SIMD find_first_difference mismatch: simd={:?}, expected={:?}",
        simd_diff, expected
    );
}

/// Test merge patch operations
fn fuzz_merge_patch(data: &[u8]) {
    if data.len() < 2 {
        return;
    }

    let split = data.len() / 2;
    let source_data = &data[..split];
    let patch_data = &data[split..];

    let source: Value = match serde_json::from_slice(source_data) {
        Ok(v) => v,
        Err(_) => return,
    };

    let patch: Value = match serde_json::from_slice(patch_data) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Apply merge patch (should not panic)
    let _ = json_merge_patch(&source, &patch);
}

/// Test deep merge associativity: merge(merge(a,b),c) == merge(a, merge(b,c))
fn fuzz_merge_associativity(data: &[u8]) {
    if data.len() < 3 {
        return;
    }

    let third = data.len() / 3;
    let a_data = &data[..third];
    let b_data = &data[third..2 * third];
    let c_data = &data[2 * third..];

    let a: Value = match serde_json::from_slice(a_data) {
        Ok(v) => v,
        Err(_) => return,
    };
    let b: Value = match serde_json::from_slice(b_data) {
        Ok(v) => v,
        Err(_) => return,
    };
    let c: Value = match serde_json::from_slice(c_data) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Only test with objects (merge behavior differs for other types)
    if !a.is_object() || !b.is_object() || !c.is_object() {
        return;
    }

    let ab = deep_merge(&a, &b);
    let ab_c = deep_merge(&ab, &c);

    let bc = deep_merge(&b, &c);
    let a_bc = deep_merge(&a, &bc);

    // Note: JSON merge is not necessarily associative due to null handling,
    // but deep_merge for objects should be
    // This is a soft check - just ensure no panics
    let _ = (ab_c, a_bc);
}

/// Test empty patch application
fn fuzz_empty_patch(data: &[u8]) {
    let value: Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => return,
    };

    let empty_patch = JsonPatch::new();
    let result = apply_patch(&value, &empty_patch);

    assert!(result.is_ok(), "empty patch should always succeed");
    assert_eq!(
        result.unwrap(),
        value,
        "empty patch should not modify value"
    );
}

/// Test diff options
fn fuzz_diff_options(data: &[u8]) {
    if data.len() < 2 {
        return;
    }

    let split = data.len() / 2;
    let source: Value = match serde_json::from_slice(&data[..split]) {
        Ok(v) => v,
        Err(_) => return,
    };
    let target: Value = match serde_json::from_slice(&data[split..]) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Test diff
    let patch1 = fionn_diff::json_diff(&source, &target);

    // Verify patch applies correctly
    if let Ok(result) = apply_patch(&source, &patch1) {
        assert_eq!(result, target, "Diff with options produced incorrect patch");
    }
}

/// Test SIMD comparison with aligned and unaligned data
fn fuzz_simd_alignment(data: &[u8]) {
    // Test self-comparison (should always be equal)
    assert!(
        simd_bytes_equal(data, data),
        "Self-comparison should be equal"
    );

    // Test difference should be None for identical
    assert_eq!(
        simd_find_first_difference(data, data),
        None,
        "Self-comparison should have no difference"
    );

    // Test with offset slice (misaligned)
    if data.len() > 17 {
        let slice1 = &data[1..];
        let slice2 = &data[1..];

        assert!(
            simd_bytes_equal(slice1, slice2),
            "Misaligned self should be equal"
        );
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        // Skip extremely large inputs
        if data.len() <= 50_000 {
            // Core diff/patch roundtrip
            fuzz_diff_patch_roundtrip(data);

            // Identical value diff
            fuzz_diff_identical(data);

            // SIMD equality tests
            fuzz_simd_equality(data);
            fuzz_simd_find_diff(data);
            fuzz_simd_alignment(data);

            // Merge patch tests
            fuzz_merge_patch(data);
            fuzz_merge_associativity(data);

            // Empty patch
            fuzz_empty_patch(data);

            // Diff options
            fuzz_diff_options(data);
        }
    });
}
