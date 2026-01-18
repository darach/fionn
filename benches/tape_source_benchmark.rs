// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - TapeSource impls conditionally compiled by format features
#![allow(unused)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Benchmarks for TapeSource trait implementations
//!
//! Compares performance of direct operations vs tape-based generic operations.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_core::TapeSource;
use fionn_core::diffable::{DiffableValue, compute_diff};
use fionn_core::patchable::apply_patch;
use fionn_gron::{GronOptions, gron, gron_from_tape};
use fionn_tape::DsonTape;
use std::fmt::Write;
use std::hint::black_box;

/// Generate test JSON of various sizes
fn generate_test_json(size: &str) -> String {
    match size {
        "small" => r#"{"name":"Alice","age":30,"active":true}"#.to_string(),

        "medium" => {
            let mut json = String::from(r#"{"users":["#);
            for i in 0..100 {
                if i > 0 {
                    json.push(',');
                }
                write!(
                    json,
                    r#"{{"id":{},"name":"User{}","email":"user{}@example.com","active":{}}}"#,
                    i,
                    i,
                    i,
                    i % 2 == 0
                )
                .unwrap();
            }
            json.push_str("]}");
            json
        }

        "large" => {
            let mut json = String::from(r#"{"data":["#);
            for i in 0..1000 {
                if i > 0 {
                    json.push(',');
                }
                write!(
                    json,
                    r#"{{"id":{},"name":"Item{}","value":{}}}"#,
                    i,
                    i,
                    i * 100
                )
                .unwrap();
            }
            json.push_str("]}");
            json
        }

        _ => "{}".to_string(),
    }
}

/// Benchmark gron: direct vs tape-based
fn bench_gron_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("gron_comparison");

    for size in ["small", "medium", "large"] {
        let json = generate_test_json(size);
        let bytes = json.len();
        group.throughput(Throughput::Bytes(bytes as u64));

        // Direct gron (existing implementation)
        group.bench_with_input(BenchmarkId::new("direct", size), &json, |b, json| {
            let options = GronOptions::default();
            b.iter(|| black_box(gron(json, &options).unwrap()));
        });

        // Tape-based gron (generic implementation)
        group.bench_with_input(BenchmarkId::new("tape_based", size), &json, |b, json| {
            let options = GronOptions::default();
            b.iter(|| {
                let tape = DsonTape::parse(json).unwrap();
                black_box(gron_from_tape(&tape, &options).unwrap())
            });
        });

        // Tape-based with pre-parsed tape (amortized parsing cost)
        let tape = DsonTape::parse(&json).unwrap();
        group.bench_with_input(
            BenchmarkId::new("tape_based_preparsed", size),
            &tape,
            |b, tape| {
                let options = GronOptions::default();
                b.iter(|| black_box(gron_from_tape(tape, &options).unwrap()));
            },
        );
    }

    group.finish();
}

/// Benchmark TapeSource traversal
fn bench_tape_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_traversal");

    for size in ["small", "medium", "large"] {
        let json = generate_test_json(size);
        let tape = DsonTape::parse(&json).unwrap();

        group.bench_with_input(
            BenchmarkId::new("node_iteration", size),
            &tape,
            |b, tape| {
                b.iter(|| {
                    let mut count = 0usize;
                    for node in tape.iter() {
                        black_box(node);
                        count += 1;
                    }
                    black_box(count)
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("skip_value", size), &tape, |b, tape| {
            b.iter(|| black_box(tape.skip_value(0).unwrap()));
        });

        group.bench_with_input(BenchmarkId::new("len", size), &tape, |b, tape| {
            b.iter(|| black_box(tape.len()));
        });
    }

    group.finish();
}

/// Benchmark generic diff operations
fn bench_diff_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_comparison");

    // Create test data pairs
    let test_cases = vec![
        (
            "small_change",
            r#"{"name":"Alice","age":30}"#,
            r#"{"name":"Alice","age":31}"#,
        ),
        (
            "field_add",
            r#"{"name":"Alice"}"#,
            r#"{"name":"Alice","age":30}"#,
        ),
        (
            "field_remove",
            r#"{"name":"Alice","age":30}"#,
            r#"{"name":"Alice"}"#,
        ),
        (
            "array_change",
            r#"{"items":[1,2,3,4,5]}"#,
            r#"{"items":[1,2,6,4,5]}"#,
        ),
    ];

    for (name, source_json, target_json) in test_cases {
        let source: serde_json::Value = serde_json::from_str(source_json).unwrap();
        let target: serde_json::Value = serde_json::from_str(target_json).unwrap();

        group.bench_with_input(
            BenchmarkId::new("compute_diff", name),
            &(source.clone(), target.clone()),
            |b, (source, target)| {
                b.iter(|| black_box(compute_diff(source, target)));
            },
        );

        // Benchmark apply_patch
        let patch = compute_diff(&source, &target);
        group.bench_with_input(
            BenchmarkId::new("apply_patch", name),
            &(source.clone(), patch.clone()),
            |b, (source, patch)| {
                b.iter(|| {
                    let mut value = source.clone();
                    apply_patch(&mut value, patch).unwrap();
                    black_box(value)
                });
            },
        );

        // Benchmark diff + patch roundtrip
        group.bench_with_input(
            BenchmarkId::new("roundtrip", name),
            &(source.clone(), target.clone()),
            |b, (source, target)| {
                b.iter(|| {
                    let patch = compute_diff(source, target);
                    let mut value = source.clone();
                    apply_patch(&mut value, &patch).unwrap();
                    black_box(value)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark DiffableValue trait operations
fn bench_diffable_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("diffable_ops");

    let test_values = vec![
        ("null", serde_json::json!(null)),
        ("bool", serde_json::json!(true)),
        ("number", serde_json::json!(42)),
        ("string", serde_json::json!("hello world")),
        ("array", serde_json::json!([1, 2, 3, 4, 5])),
        ("object", serde_json::json!({"a": 1, "b": 2, "c": 3})),
        (
            "nested",
            serde_json::json!({"outer": {"inner": {"deep": "value"}}}),
        ),
    ];

    for (name, value) in &test_values {
        group.bench_with_input(BenchmarkId::new("value_kind", name), value, |b, v| {
            b.iter(|| black_box(v.value_kind()));
        });

        group.bench_with_input(BenchmarkId::new("equals_self", name), value, |b, v| {
            b.iter(|| black_box(v.equals(v)));
        });

        group.bench_with_input(BenchmarkId::new("deep_clone", name), value, |b, v| {
            b.iter(|| black_box(v.deep_clone()));
        });
    }

    group.finish();
}

/// Benchmark tape parsing
fn bench_tape_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_parsing");

    for size in ["small", "medium", "large"] {
        let json = generate_test_json(size);
        let bytes = json.len();
        group.throughput(Throughput::Bytes(bytes as u64));

        group.bench_with_input(BenchmarkId::new("dson_tape", size), &json, |b, json| {
            b.iter(|| black_box(DsonTape::parse(json).unwrap()));
        });

        group.bench_with_input(BenchmarkId::new("serde_json", size), &json, |b, json| {
            b.iter(|| black_box(serde_json::from_str::<serde_json::Value>(json).unwrap()));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_gron_comparison,
    bench_tape_traversal,
    bench_diff_comparison,
    bench_diffable_operations,
    bench_tape_parsing,
);
criterion_main!(benches);
