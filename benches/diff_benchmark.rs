// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Benchmarks for JSON diff/patch/merge with baseline comparisons.
//!
//! Compares fionn diff module against:
//! - json-patch crate (RFC 6902 baseline)
//! - Manual serde_json operations

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_diff::{
    DiffOptions, apply_patch, json_diff, json_diff_with_options, json_merge_patch, merge_many,
    simd_bytes_equal, simd_find_first_difference,
};
use serde_json::{Value, json};
use std::hint::black_box;

// =============================================================================
// Test Data Generation
// =============================================================================

/// Generate source and target JSON pairs for diff benchmarks
fn generate_diff_pairs(scenario: &str) -> (Value, Value) {
    match scenario {
        "identical_small" => {
            let doc = json!({"name": "Alice", "age": 30, "active": true});
            (doc.clone(), doc)
        }

        "identical_medium" => {
            let doc = json!({
                "users": (0..100).map(|i| json!({
                    "id": i,
                    "name": format!("User{}", i),
                    "email": format!("user{}@example.com", i),
                    "active": i % 2 == 0
                })).collect::<Vec<_>>()
            });
            (doc.clone(), doc)
        }

        "small_field_change" => {
            let source = json!({"name": "Alice", "age": 30, "active": true});
            let target = json!({"name": "Alice", "age": 31, "active": true});
            (source, target)
        }

        "medium_field_add" => {
            let source = json!({
                "users": (0..50).map(|i| json!({
                    "id": i,
                    "name": format!("User{}", i)
                })).collect::<Vec<_>>()
            });
            let target = json!({
                "users": (0..50).map(|i| json!({
                    "id": i,
                    "name": format!("User{}", i),
                    "email": format!("user{}@example.com", i)
                })).collect::<Vec<_>>()
            });
            (source, target)
        }

        "array_append" => {
            let source = json!({
                "items": (0..100).collect::<Vec<_>>()
            });
            let target = json!({
                "items": (0..105).collect::<Vec<_>>()
            });
            (source, target)
        }

        "array_reorder" => {
            let source = json!({
                "items": (0..50).collect::<Vec<_>>()
            });
            let mut items: Vec<i32> = (0..50).collect();
            items.reverse();
            let target = json!({
                "items": items
            });
            (source, target)
        }

        "deep_nested_change" => {
            let source = json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "level4": {
                                "level5": {
                                    "value": "original"
                                }
                            }
                        }
                    }
                }
            });
            let target = json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "level4": {
                                "level5": {
                                    "value": "modified"
                                }
                            }
                        }
                    }
                }
            });
            (source, target)
        }

        "large_document" => {
            let source = json!({
                "data": (0..1000).map(|i| json!({
                    "id": i,
                    "name": format!("Item{}", i),
                    "description": format!("Description for item {}", i),
                    "nested": {
                        "field1": i * 10,
                        "field2": format!("value{}", i)
                    }
                })).collect::<Vec<_>>()
            });
            let target = json!({
                "data": (0..1000).map(|i| json!({
                    "id": i,
                    "name": format!("Item{}", i),
                    "description": format!("Description for item {}", i),
                    "nested": {
                        "field1": if i == 500 { i * 100 } else { i * 10 },
                        "field2": format!("value{}", i)
                    }
                })).collect::<Vec<_>>()
            });
            (source, target)
        }

        _ => (json!({}), json!({})),
    }
}

/// Generate merge patch test data
fn generate_merge_data(scenario: &str) -> (Value, Value) {
    match scenario {
        "simple_merge" => {
            let target = json!({"a": 1, "b": 2});
            let patch = json!({"b": 3, "c": 4});
            (target, patch)
        }

        "nested_merge" => {
            let target = json!({
                "user": {
                    "name": "Alice",
                    "settings": {
                        "theme": "light",
                        "notifications": true
                    }
                }
            });
            let patch = json!({
                "user": {
                    "settings": {
                        "theme": "dark"
                    }
                }
            });
            (target, patch)
        }

        "delete_fields" => {
            let target = json!({"a": 1, "b": 2, "c": 3, "d": 4});
            let patch = json!({"b": null, "d": null});
            (target, patch)
        }

        "large_merge" => {
            let target = json!({
                "data": (0..500).map(|i| json!({
                    "id": i,
                    "value": i * 10
                })).collect::<Vec<_>>()
            });
            let patch = json!({
                "data": (500..600).map(|i| json!({
                    "id": i,
                    "value": i * 10
                })).collect::<Vec<_>>()
            });
            (target, patch)
        }

        _ => (json!({}), json!({})),
    }
}

// =============================================================================
// JSON Diff Benchmarks
// =============================================================================

fn bench_json_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_diff");

    for scenario in &[
        "identical_small",
        "identical_medium",
        "small_field_change",
        "medium_field_add",
        "array_append",
        "deep_nested_change",
        "large_document",
    ] {
        let (source, target) = generate_diff_pairs(scenario);
        let source_size = serde_json::to_string(&source).unwrap().len();
        let target_size = serde_json::to_string(&target).unwrap().len();
        let total_size = source_size + target_size;

        group.throughput(Throughput::Bytes(total_size as u64));

        // fionn diff
        group.bench_with_input(
            BenchmarkId::new("simd_dson", scenario),
            &(&source, &target),
            |b, (src, tgt)| {
                b.iter(|| json_diff(black_box(*src), black_box(*tgt)));
            },
        );

        // fionn diff with LCS optimization for arrays
        group.bench_with_input(
            BenchmarkId::new("simd_dson_lcs", scenario),
            &(&source, &target),
            |b, (src, tgt)| {
                let options = DiffOptions::default().with_array_optimization();
                b.iter(|| json_diff_with_options(black_box(*src), black_box(*tgt), &options));
            },
        );

        // Baseline: json-patch crate
        group.bench_with_input(
            BenchmarkId::new("json_patch_crate", scenario),
            &(&source, &target),
            |b, (src, tgt)| {
                b.iter(|| json_patch::diff(black_box(*src), black_box(*tgt)));
            },
        );
    }

    group.finish();
}

// =============================================================================
// JSON Patch Application Benchmarks
// =============================================================================

fn bench_apply_patch(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_patch");

    for scenario in &[
        "small_field_change",
        "medium_field_add",
        "array_append",
        "deep_nested_change",
    ] {
        let (source, target) = generate_diff_pairs(scenario);
        let simd_patch = json_diff(&source, &target);
        let json_patch_patch = json_patch::diff(&source, &target);

        let source_size = serde_json::to_string(&source).unwrap().len();
        group.throughput(Throughput::Bytes(source_size as u64));

        // fionn apply_patch
        group.bench_with_input(
            BenchmarkId::new("simd_dson", scenario),
            &(&source, &simd_patch),
            |b, (src, patch)| {
                b.iter(|| apply_patch(black_box(*src), black_box(patch)));
            },
        );

        // Baseline: json-patch crate
        group.bench_with_input(
            BenchmarkId::new("json_patch_crate", scenario),
            &(&source, &json_patch_patch),
            |b, (src, patch)| {
                let mut doc = (*src).clone();
                b.iter(|| {
                    doc = (*src).clone();
                    json_patch::patch(&mut doc, black_box(patch))
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// JSON Merge Patch Benchmarks
// =============================================================================

fn bench_merge_patch(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge_patch");

    for scenario in &[
        "simple_merge",
        "nested_merge",
        "delete_fields",
        "large_merge",
    ] {
        let (target, patch) = generate_merge_data(scenario);
        let target_size = serde_json::to_string(&target).unwrap().len();
        let patch_size = serde_json::to_string(&patch).unwrap().len();

        group.throughput(Throughput::Bytes((target_size + patch_size) as u64));

        // fionn merge
        group.bench_with_input(
            BenchmarkId::new("simd_dson", scenario),
            &(&target, &patch),
            |b, (tgt, pch)| {
                b.iter(|| json_merge_patch(black_box(*tgt), black_box(*pch)));
            },
        );

        // Baseline: json-patch crate merge_patch
        group.bench_with_input(
            BenchmarkId::new("json_patch_crate", scenario),
            &(&target, &patch),
            |b, (tgt, pch)| {
                let mut doc = (*tgt).clone();
                b.iter(|| {
                    doc = (*tgt).clone();
                    json_patch::merge(&mut doc, black_box(*pch))
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// SIMD Comparison Benchmarks
// =============================================================================

fn bench_simd_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_compare");

    // Test various string sizes
    let sizes = [16, 64, 256, 1024, 4096, 16_384];

    for size in sizes {
        // Identical strings
        let a: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let b = a.clone();

        group.throughput(Throughput::Bytes((size * 2) as u64));

        group.bench_with_input(
            BenchmarkId::new("simd_bytes_equal/identical", size),
            &(&a, &b),
            |bench, (a, b)| {
                bench.iter(|| simd_bytes_equal(black_box(a), black_box(b)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("std_eq/identical", size),
            &(&a, &b),
            |bench, (a, b)| {
                bench.iter(|| black_box(a) == black_box(b));
            },
        );

        // Different at end
        let mut c = a.clone();
        c[size - 1] ^= 0xFF;

        group.bench_with_input(
            BenchmarkId::new("simd_bytes_equal/diff_end", size),
            &(&a, &c),
            |bench, (a, c)| {
                bench.iter(|| simd_bytes_equal(black_box(a), black_box(c)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("std_eq/diff_end", size),
            &(&a, &c),
            |bench, (a, c)| {
                bench.iter(|| black_box(a) == black_box(c));
            },
        );

        // Different at start
        let mut d = a.clone();
        d[0] ^= 0xFF;

        group.bench_with_input(
            BenchmarkId::new("simd_bytes_equal/diff_start", size),
            &(&a, &d),
            |bench, (a, d)| {
                bench.iter(|| simd_bytes_equal(black_box(a), black_box(d)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("std_eq/diff_start", size),
            &(&a, &d),
            |bench, (a, d)| {
                bench.iter(|| black_box(a) == black_box(d));
            },
        );
    }

    group.finish();
}

fn bench_find_first_difference(c: &mut Criterion) {
    let mut group = c.benchmark_group("find_first_difference");

    let sizes = [64, 256, 1024, 4096];

    for size in sizes {
        let a: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();

        // Difference at various positions
        for diff_pos in [size / 4, size / 2, size * 3 / 4] {
            let mut b = a.clone();
            b[diff_pos] ^= 0xFF;

            group.throughput(Throughput::Bytes(size as u64));

            group.bench_with_input(
                BenchmarkId::new(format!("simd/diff_at_{}", diff_pos), size),
                &(&a, &b),
                |bench, (a, b)| {
                    bench.iter(|| simd_find_first_difference(black_box(a), black_box(b)));
                },
            );

            group.bench_with_input(
                BenchmarkId::new(format!("scalar/diff_at_{}", diff_pos), size),
                &(&a, &b),
                |bench, (a, b)| {
                    bench.iter(|| {
                        black_box(a)
                            .iter()
                            .zip(black_box(b).iter())
                            .position(|(x, y)| x != y)
                    });
                },
            );
        }
    }

    group.finish();
}

// =============================================================================
// Merge Many Documents Benchmark
// =============================================================================

fn bench_merge_many(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge_many");

    for count in [2, 5, 10, 20] {
        let documents: Vec<Value> = (0..count)
            .map(|i| {
                json!({
                    format!("field_{}", i): i,
                    "shared": format!("value_{}", i),
                    "nested": {
                        format!("sub_{}", i): i * 10
                    }
                })
            })
            .collect();

        let total_size: usize = documents
            .iter()
            .map(|d| serde_json::to_string(d).unwrap().len())
            .sum();

        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_with_input(
            BenchmarkId::new("simd_dson", count),
            &documents,
            |b, docs| {
                b.iter(|| merge_many(black_box(docs)));
            },
        );

        // Baseline: sequential json-patch merge
        group.bench_with_input(
            BenchmarkId::new("json_patch_sequential", count),
            &documents,
            |b, docs| {
                b.iter(|| {
                    let mut result = docs[0].clone();
                    for doc in docs.iter().skip(1) {
                        json_patch::merge(&mut result, black_box(doc));
                    }
                    result
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Roundtrip Benchmark (diff -> patch -> verify)
// =============================================================================

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_roundtrip");

    for scenario in &[
        "small_field_change",
        "medium_field_add",
        "deep_nested_change",
    ] {
        let (source, target) = generate_diff_pairs(scenario);
        let total_size = serde_json::to_string(&source).unwrap().len()
            + serde_json::to_string(&target).unwrap().len();

        group.throughput(Throughput::Bytes(total_size as u64));

        // fionn roundtrip
        group.bench_with_input(
            BenchmarkId::new("simd_dson", scenario),
            &(&source, &target),
            |b, (src, tgt)| {
                b.iter(|| {
                    let patch = json_diff(black_box(*src), black_box(*tgt));
                    let result = apply_patch(black_box(*src), &patch).unwrap();
                    black_box(result)
                });
            },
        );

        // json-patch crate roundtrip
        group.bench_with_input(
            BenchmarkId::new("json_patch_crate", scenario),
            &(&source, &target),
            |b, (src, tgt)| {
                b.iter(|| {
                    let patch = json_patch::diff(black_box(*src), black_box(*tgt));
                    let mut result = (*src).clone();
                    json_patch::patch(&mut result, &patch).unwrap();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_json_diff,
    bench_apply_patch,
    bench_merge_patch,
    bench_simd_compare,
    bench_find_first_difference,
    bench_merge_many,
    bench_roundtrip,
);

criterion_main!(benches);
