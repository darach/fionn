// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Benchmark for SIMD-JSONL Skip Tape performance

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fionn_stream::skiptape::CompiledSchema;
use fionn_stream::skiptape::jsonl::SimdJsonlBatchProcessor;
use std::hint::black_box;

// Test data - JSONL format
const JSONL_DATA: &str = r#"{"user":"alice","age":30,"active":true}
{"user":"bob","age":25,"active":false,"tags":["dev","rust"]}
{"user":"charlie","age":35,"active":true,"profile":{"bio":"Developer"}}
{"user":"diana","age":28,"active":false}
{"user":"eve","age":32,"active":true,"stats":{"posts":42}}
"#;

fn bench_simd_jsonl_processing(c: &mut Criterion) {
    let schema = CompiledSchema::compile(&["user".to_string(), "age".to_string()]).unwrap();

    // Small data benchmarks
    {
        let mut group = c.benchmark_group("simd_jsonl_small");
        group.throughput(Throughput::Bytes(JSONL_DATA.len() as u64));

        group.bench_function("process_batch_optimized", |b| {
            let mut processor = SimdJsonlBatchProcessor::new();
            let data = JSONL_DATA.as_bytes();

            b.iter(|| {
                let result = processor
                    .process_batch_optimized(black_box(data), &schema)
                    .unwrap();
                black_box(result);
            });
        });

        group.bench_function("process_batch_raw_simd", |b| {
            let mut processor = SimdJsonlBatchProcessor::new();
            let data = JSONL_DATA.as_bytes();

            b.iter(|| {
                let result = processor.process_batch_raw_simd(black_box(data)).unwrap();
                black_box(result);
            });
        });

        drop(group);
    }

    // Large data benchmark
    {
        let large_jsonl = create_large_jsonl(100);
        let mut group = c.benchmark_group("simd_jsonl_large");
        group.throughput(Throughput::Bytes(large_jsonl.len() as u64));

        group.bench_function("process_batch_optimized", |b| {
            let mut processor = SimdJsonlBatchProcessor::new();
            let data = large_jsonl.as_bytes();

            b.iter(|| {
                let result = processor
                    .process_batch_optimized(black_box(data), &schema)
                    .unwrap();
                black_box(result);
            });
        });

        group.bench_function("process_batch_raw_simd", |b| {
            let mut processor = SimdJsonlBatchProcessor::new();
            let data = large_jsonl.as_bytes();

            b.iter(|| {
                let result = processor.process_batch_raw_simd(black_box(data)).unwrap();
                black_box(result);
            });
        });

        drop(group);
    }
}

fn create_large_jsonl(num_lines: usize) -> String {
    let mut jsonl = String::new();
    for i in 0..num_lines {
        jsonl.push_str(&format!(
            r#"{{"user":"user_{}","age":{},"active":{},"id":{}}}"#,
            i,
            20 + (i % 50),
            i % 2 == 0,
            i
        ));
        jsonl.push('\n');
    }
    jsonl
}

criterion_group!(simd_jsonl_benches, bench_simd_jsonl_processing);
criterion_main!(simd_jsonl_benches);
