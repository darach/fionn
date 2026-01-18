// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Benchmark for SIMD-JSONL Skip Tape performance
//!
//! Tests the performance of SIMD-accelerated tape navigation and value extraction.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

use fionn_tape::DsonTape;

fn bench_simd_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_operations");

    // Setup test data
    let json_dataset = create_large_json_dataset();
    let tape = DsonTape::parse(&json_dataset).expect("Failed to parse test dataset");

    group.throughput(Throughput::Bytes(json_dataset.len() as u64));

    // internal helper to finding indices for benchmarking
    let mut string_indices = Vec::new();
    let mut number_indices = Vec::new();
    let nodes = tape.nodes();
    for (i, node) in nodes.iter().enumerate() {
        match node {
            simd_json::value::tape::Node::String(_) => string_indices.push(i),
            simd_json::value::tape::Node::Static(simd_json::StaticNode::I64(_))
            | simd_json::value::tape::Node::Static(simd_json::StaticNode::U64(_))
            | simd_json::value::tape::Node::Static(simd_json::StaticNode::F64(_)) => {
                number_indices.push(i)
            }
            _ => {}
        }
    }

    // Pick a few representative indices to avoid bench overhead of iterating all
    let test_string_idx = if !string_indices.is_empty() {
        string_indices[string_indices.len() / 2]
    } else {
        0
    };
    let test_number_idx = if !number_indices.is_empty() {
        number_indices[number_indices.len() / 2]
    } else {
        0
    };

    // Benchmark 1: extract_value_simd (String)
    group.bench_function("extract_value_simd_string", |b| {
        b.iter(|| {
            black_box(tape.extract_value_simd(test_string_idx));
        });
    });

    // Benchmark 2: extract_value_simd (Number)
    group.bench_function("extract_value_simd_number", |b| {
        b.iter(|| {
            black_box(tape.extract_value_simd(test_number_idx));
        });
    });

    // Benchmark 3: skip_value
    // Find an object or array start to skip
    let complex_idx = nodes
        .iter()
        .position(|n| {
            matches!(
                n,
                simd_json::value::tape::Node::Object { .. }
                    | simd_json::value::tape::Node::Array { .. }
            )
        })
        .unwrap_or(0);

    group.bench_function("skip_value_complex", |b| {
        b.iter(|| {
            // Cloning tape for skip is not right, skip_value takes mutable reference usually or index?
            // Checking signature: public fn skip_value(&self, index: usize) -> Result<usize>
            // It's immutable on self, so we can call it repeatedly.
            black_box(tape.skip_value(complex_idx).unwrap());
        });
    });

    // Benchmark 4: Resolve path (SIMD path navigation)
    group.bench_function("resolve_path", |b| {
        // Path depends on the dataset structure
        b.iter(|| {
            let _ = black_box(tape.resolve_path("users[5].profile.name"));
        });
    });

    group.finish();
}

fn bench_simd_jsonl_processor(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_jsonl_processor");
    let json_dataset = create_large_json_dataset();

    // We benchmark the raw parsing throughput which uses our new AVX2 Structural Detector
    group.throughput(Throughput::Bytes(json_dataset.len() as u64));

    group.bench_function("parse_json_raw_simd", |b| {
        // Reuse processor to avoid allocation overhead in bench loop
        let mut processor = fionn_stream::skiptape::jsonl::SimdJsonlBatchProcessor::new();
        b.iter(|| {
            // benchmarking the raw parse which uses structural_detector.find_structural_characters
            black_box(processor.parse_json_raw(black_box(&json_dataset))).unwrap();
        });
    });

    group.finish();
}

fn create_large_json_dataset() -> String {
    let mut json = String::with_capacity(1024 * 1024);
    json.push_str("{\"users\": [");
    // Use fewer iterations to keep dataset size manageable for cache effects if desired
    // But large enough to show SIMD benefits
    for i in 0..1000 {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"id": {}, "name": "User {}", "profile": {{"bio": "Bio for user {}", "age": {}, "active": {}}}, "tags": ["a", "b", "c"]}}"#, 
            i, i, i, 20 + (i % 50), i % 2 == 0
        ));
    }
    json.push_str("]}");
    json
}

criterion_group!(
    simd_benches,
    bench_simd_operations,
    bench_simd_jsonl_processor
);
criterion_main!(simd_benches);
