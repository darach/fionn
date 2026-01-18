// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - streaming format parsers conditionally compiled by features
#![allow(unused)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Streaming format benchmarks
//!
//! Measures streaming processing performance across different scenarios:
//! - JSONL batch processing at various sizes
//! - Schema-guided selective extraction
//! - Streaming throughput with backpressure simulation
//! - Line-by-line vs batch processing comparison

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_stream::skiptape::CompiledSchema;
use fionn_stream::skiptape::jsonl::SimdJsonlBatchProcessor;
use fionn_stream::streaming::StreamingProcessor;

// =============================================================================
// Test Data Generation
// =============================================================================

/// Generate JSONL data with N lines
fn generate_jsonl(num_lines: usize) -> String {
    let mut jsonl = String::new();
    for i in 0..num_lines {
        jsonl.push_str(&format!(
            r#"{{"id":{},"user":"user_{}","email":"user{}@example.com","age":{},"active":{},"score":{}.{},"tags":["tag1","tag2"]}}"#,
            i,
            i,
            i,
            20 + (i % 50),
            i % 2 == 0,
            i * 10,
            i % 100
        ));
        jsonl.push('\n');
    }
    jsonl
}

/// Generate JSONL with varying line sizes
fn generate_variable_jsonl(num_lines: usize) -> String {
    let mut jsonl = String::new();
    for i in 0..num_lines {
        let extra_data = if i % 10 == 0 {
            // Every 10th line has extra nested data
            format!(
                r#","metadata":{{"created":"2024-01-01","updated":"2024-01-02","version":{},"nested":{{"a":1,"b":2,"c":3}}}}"#,
                i
            )
        } else if i % 5 == 0 {
            // Every 5th line has a longer tags array
            r#","tags":["tag1","tag2","tag3","tag4","tag5","tag6","tag7","tag8"]"#.to_string()
        } else {
            String::new()
        };

        jsonl.push_str(&format!(
            r#"{{"id":{},"user":"user_{}","active":{}{}}}"#,
            i,
            i,
            i % 2 == 0,
            extra_data
        ));
        jsonl.push('\n');
    }
    jsonl
}

/// Generate JSONL with deep nesting
fn generate_nested_jsonl(num_lines: usize, depth: usize) -> String {
    let mut jsonl = String::new();
    for i in 0..num_lines {
        let mut nested = format!("\"value\":{}", i);
        for d in 0..depth {
            nested = format!("\"level_{}\":{{{}}}", d, nested);
        }
        jsonl.push_str(&format!(r#"{{"id":{},{}}}"#, i, nested));
        jsonl.push('\n');
    }
    jsonl
}

// =============================================================================
// JSONL Batch Processing Benchmarks
// =============================================================================

/// Benchmark: JSONL batch sizes
fn bench_jsonl_batch_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/jsonl_batch_size");

    let schema = CompiledSchema::compile(&["id".to_string(), "user".to_string()]).unwrap();

    for num_lines in [10, 100, 1000, 10000] {
        let jsonl = generate_jsonl(num_lines);
        let bytes = jsonl.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("optimized", num_lines),
            bytes,
            |b, data| {
                let mut processor = SimdJsonlBatchProcessor::new();
                b.iter(|| {
                    let result = processor.process_batch_optimized(black_box(data), &schema);
                    black_box(result)
                })
            },
        );

        group.bench_with_input(BenchmarkId::new("raw_simd", num_lines), bytes, |b, data| {
            let mut processor = SimdJsonlBatchProcessor::new();
            b.iter(|| {
                let result = processor.process_batch_raw_simd(black_box(data));
                black_box(result)
            })
        });
    }

    group.finish();
}

/// Benchmark: Schema selectivity impact
fn bench_schema_selectivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/schema_selectivity");

    let jsonl = generate_jsonl(1000);
    let bytes = jsonl.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Different schema sizes (number of fields to extract)
    for num_fields in [1, 2, 4, 6] {
        let fields: Vec<String> = ["id", "user", "email", "age", "active", "score"]
            .iter()
            .take(num_fields)
            .map(|s| s.to_string())
            .collect();

        let schema = CompiledSchema::compile(&fields).unwrap();

        group.bench_with_input(BenchmarkId::new("fields", num_fields), bytes, |b, data| {
            let mut processor = SimdJsonlBatchProcessor::new();
            b.iter(|| {
                let result = processor.process_batch_optimized(black_box(data), &schema);
                black_box(result)
            })
        });
    }

    group.finish();
}

/// Benchmark: Variable line sizes
fn bench_variable_line_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/variable_lines");

    let schema = CompiledSchema::compile(&["id".to_string(), "user".to_string()]).unwrap();

    // Uniform lines
    let uniform = generate_jsonl(1000);
    group.throughput(Throughput::Bytes(uniform.len() as u64));

    group.bench_function("uniform_1k", |b| {
        let mut processor = SimdJsonlBatchProcessor::new();
        let data = uniform.as_bytes();
        b.iter(|| {
            let result = processor.process_batch_optimized(black_box(data), &schema);
            black_box(result)
        })
    });

    // Variable lines (some with extra data)
    let variable = generate_variable_jsonl(1000);
    group.throughput(Throughput::Bytes(variable.len() as u64));

    group.bench_function("variable_1k", |b| {
        let mut processor = SimdJsonlBatchProcessor::new();
        let data = variable.as_bytes();
        b.iter(|| {
            let result = processor.process_batch_optimized(black_box(data), &schema);
            black_box(result)
        })
    });

    group.finish();
}

/// Benchmark: Nested JSONL processing
fn bench_nested_jsonl(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/nested_jsonl");

    let schema = CompiledSchema::compile(&["id".to_string()]).unwrap();

    for depth in [1, 3, 5, 10] {
        let jsonl = generate_nested_jsonl(500, depth);
        let bytes = jsonl.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("depth", depth), bytes, |b, data| {
            let mut processor = SimdJsonlBatchProcessor::new();
            b.iter(|| {
                let result = processor.process_batch_raw_simd(black_box(data));
                black_box(result)
            })
        });
    }

    group.finish();
}

// =============================================================================
// Streaming Pipeline Benchmarks
// =============================================================================

/// Benchmark: Streaming processor buffer sizes
fn bench_streaming_buffer_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/buffer_size");

    for buffer_size in [64, 256, 1024, 4096] {
        group.bench_with_input(
            BenchmarkId::new("create", buffer_size),
            &buffer_size,
            |b, &size| {
                b.iter(|| {
                    let processor = StreamingProcessor::new(size);
                    black_box(processor)
                })
            },
        );
    }

    group.finish();
}

/// Benchmark: Throughput at different chunk sizes
fn bench_chunk_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/chunk_throughput");

    let schema = CompiledSchema::compile(&["id".to_string(), "user".to_string()]).unwrap();

    // Generate a large dataset
    let large_jsonl = generate_jsonl(10000);

    for chunk_lines in [100, 500, 1000, 2000] {
        // Split into chunks
        let lines: Vec<&str> = large_jsonl.lines().collect();
        let chunks: Vec<String> = lines
            .chunks(chunk_lines)
            .map(|chunk| chunk.join("\n") + "\n")
            .collect();

        let total_bytes: usize = chunks.iter().map(|c| c.len()).sum();
        group.throughput(Throughput::Bytes(total_bytes as u64));

        group.bench_with_input(
            BenchmarkId::new("chunk_size", chunk_lines),
            &chunks,
            |b, chunks| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let mut total_results = 0;
                    for chunk in chunks {
                        if let Ok(results) =
                            processor.process_batch_optimized(chunk.as_bytes(), &schema)
                        {
                            total_results += results.documents.len();
                        }
                    }
                    black_box(total_results)
                })
            },
        );
    }

    group.finish();
}

/// Benchmark: Line counting (preprocessing step)
fn bench_line_counting(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/line_counting");

    for num_lines in [100, 1000, 10000] {
        let jsonl = generate_jsonl(num_lines);
        let bytes = jsonl.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // Simple newline counting
        group.bench_with_input(
            BenchmarkId::new("memchr_count", num_lines),
            bytes,
            |b, data| {
                b.iter(|| {
                    let count = black_box(data).iter().filter(|&&b| b == b'\n').count();
                    black_box(count)
                })
            },
        );

        // Iterator-based counting
        group.bench_with_input(
            BenchmarkId::new("iter_count", num_lines),
            bytes,
            |b, data| {
                b.iter(|| {
                    let count = black_box(data).iter().filter(|&&b| b == b'\n').count();
                    black_box(count)
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// Memory Efficiency Benchmarks
// =============================================================================

/// Benchmark: Memory allocation patterns during streaming
fn bench_streaming_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/allocation");

    let schema = CompiledSchema::compile(&["id".to_string(), "user".to_string()]).unwrap();

    // Pre-allocated processor vs fresh processor
    let jsonl = generate_jsonl(1000);
    let bytes = jsonl.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Fresh processor each iteration
    group.bench_function("fresh_processor", |b| {
        b.iter(|| {
            let mut processor = SimdJsonlBatchProcessor::new();
            let result = processor.process_batch_optimized(black_box(bytes), &schema);
            black_box(result)
        })
    });

    // Reused processor
    group.bench_function("reused_processor", |b| {
        let mut processor = SimdJsonlBatchProcessor::new();
        b.iter(|| {
            let result = processor.process_batch_optimized(black_box(bytes), &schema);
            black_box(result)
        })
    });

    group.finish();
}

// =============================================================================
// Comparison Benchmarks
// =============================================================================

/// Benchmark: SIMD vs standard processing
fn bench_simd_vs_standard(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming/simd_comparison");

    let schema = CompiledSchema::compile(&["id".to_string(), "user".to_string()]).unwrap();

    for num_lines in [100, 1000] {
        let jsonl = generate_jsonl(num_lines);
        let bytes = jsonl.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // SIMD batch processing
        group.bench_with_input(
            BenchmarkId::new("simd_batch", num_lines),
            bytes,
            |b, data| {
                let mut processor = SimdJsonlBatchProcessor::new();
                b.iter(|| {
                    let result = processor.process_batch_optimized(black_box(data), &schema);
                    black_box(result)
                })
            },
        );

        // Standard line-by-line with serde_json (baseline)
        group.bench_with_input(
            BenchmarkId::new("serde_lines", num_lines),
            &jsonl,
            |b, data| {
                b.iter(|| {
                    let mut results = Vec::new();
                    for line in data.lines() {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                            if let Some(id) = value.get("id") {
                                results.push(id.clone());
                            }
                        }
                    }
                    black_box(results)
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    streaming_benchmarks,
    bench_jsonl_batch_sizes,
    bench_schema_selectivity,
    bench_variable_line_sizes,
    bench_nested_jsonl,
    bench_streaming_buffer_sizes,
    bench_chunk_throughput,
    bench_line_counting,
    bench_streaming_allocation,
    bench_simd_vs_standard,
);

criterion_main!(streaming_benchmarks);
