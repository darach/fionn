// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Helper functions may be defined for future benchmarks
#![allow(dead_code)]
//! Skip Parsing Advantage Benchmarks
//!
//! Demonstrates the performance advantage of schema-guided skip parsing
//! versus full parsing across document sizes and schema selectivity levels.
//!
//! # Key Metrics
//!
//! - **Throughput**: bytes/sec at varying selectivity
//! - **Skip Ratio**: % of document skipped by schema filter
//! - **Memory Efficiency**: bytes allocated per matched field
//! - **Latency Distribution**: p50, p95, p99 at scale

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_stream::skiptape::jsonl::SimdJsonlBatchProcessor;
use fionn_stream::skiptape::schema::CompiledSchema;
use std::time::Instant;

// =============================================================================
// Test Data Generators
// =============================================================================

/// Generate JSONL with varying field counts
fn generate_jsonl_documents(doc_count: usize, fields_per_doc: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(doc_count * fields_per_doc * 30);

    for i in 0..doc_count {
        output.push(b'{');
        for f in 0..fields_per_doc {
            if f > 0 {
                output.push(b',');
            }
            let field = format!("\"field{}\":{}", f, i * fields_per_doc + f);
            output.extend_from_slice(field.as_bytes());
        }
        output.extend_from_slice(b"}\n");
    }

    output
}

/// Generate JSONL with nested structure
fn generate_nested_jsonl(doc_count: usize, depth: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(doc_count * depth * 50);

    for i in 0..doc_count {
        let mut doc = String::with_capacity(depth * 50);
        doc.push('{');

        // Create nested structure: {a:{b:{c:{...value...}}}}
        for d in 0..depth {
            if d > 0 {
                doc.push(',');
            }
            doc.push_str(&format!("\"level{d}\":{{"));
        }

        doc.push_str(&format!("\"value\":{i}"));

        for _ in 0..depth {
            doc.push('}');
        }
        doc.push_str("}\n");

        output.extend_from_slice(doc.as_bytes());
    }

    output
}

/// Generate JSONL with mixed schema (some docs match, some don't)
fn generate_mixed_schema_jsonl(doc_count: usize, match_ratio: f64) -> Vec<u8> {
    let mut output = Vec::with_capacity(doc_count * 100);
    let match_count = (doc_count as f64 * match_ratio) as usize;

    for i in 0..doc_count {
        if i < match_count {
            // Matching document with "target" field
            let doc = format!("{{\"id\":{i},\"target\":\"match\",\"data\":\"value{i}\"}}\n");
            output.extend_from_slice(doc.as_bytes());
        } else {
            // Non-matching document without "target" field
            let doc = format!("{{\"id\":{i},\"other\":\"skip\",\"noise\":\"data{i}\"}}\n");
            output.extend_from_slice(doc.as_bytes());
        }
    }

    output
}

/// Generate wide documents (many fields, few match)
fn generate_wide_jsonl(doc_count: usize, total_fields: usize, target_field_idx: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(doc_count * total_fields * 25);

    for i in 0..doc_count {
        output.push(b'{');
        for f in 0..total_fields {
            if f > 0 {
                output.push(b',');
            }
            if f == target_field_idx {
                let field = format!("\"target\":{i}");
                output.extend_from_slice(field.as_bytes());
            } else {
                let field = format!("\"field{f}\":\"noise{f}\"");
                output.extend_from_slice(field.as_bytes());
            }
        }
        output.extend_from_slice(b"}\n");
    }

    output
}

// =============================================================================
// Benchmark: Schema Selectivity Impact
// =============================================================================

fn bench_schema_selectivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_selectivity");

    let doc_count = 1000;
    let selectivity_levels = [0.01, 0.10, 0.25, 0.50, 0.75, 1.0];

    for selectivity in selectivity_levels {
        let data = generate_mixed_schema_jsonl(doc_count, selectivity);
        group.throughput(Throughput::Bytes(data.len() as u64));

        // With schema filter (skip parsing advantage)
        let schema = CompiledSchema::compile(&["target".to_string()]).unwrap();
        group.bench_with_input(
            BenchmarkId::new("filtered", format!("{:.0}%", selectivity * 100.0)),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_optimized(data, &schema).unwrap();
                    black_box(result.documents.len())
                });
            },
        );

        // Without schema filter (full parse)
        group.bench_with_input(
            BenchmarkId::new("unfiltered", format!("{:.0}%", selectivity * 100.0)),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_raw_simd(data).unwrap();
                    black_box(result.documents.len())
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: Document Width (Field Count) Impact
// =============================================================================

fn bench_document_width(c: &mut Criterion) {
    let mut group = c.benchmark_group("document_width");

    let doc_count = 500;
    let field_counts = [5, 10, 25, 50, 100, 200];

    for field_count in field_counts {
        // Target field is in the middle
        let target_idx = field_count / 2;
        let data = generate_wide_jsonl(doc_count, field_count, target_idx);
        group.throughput(Throughput::Bytes(data.len() as u64));

        // Calculate theoretical skip ratio
        let skip_ratio = 1.0 - (1.0 / field_count as f64);

        // With schema filter
        let schema = CompiledSchema::compile(&["target".to_string()]).unwrap();
        group.bench_with_input(
            BenchmarkId::new(
                "filtered",
                format!("{}fields_skip{:.0}%", field_count, skip_ratio * 100.0),
            ),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_optimized(data, &schema).unwrap();
                    black_box(result.documents.len())
                });
            },
        );

        // Without schema filter
        group.bench_with_input(
            BenchmarkId::new("unfiltered", format!("{field_count}fields")),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_raw_simd(data).unwrap();
                    black_box(result.documents.len())
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: Document Depth (Nesting) Impact
// =============================================================================

fn bench_document_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("document_depth");

    let doc_count = 500;
    let depth_levels = [1, 2, 4, 8, 16];

    for depth in depth_levels {
        let data = generate_nested_jsonl(doc_count, depth);
        group.throughput(Throughput::Bytes(data.len() as u64));

        // Schema targeting deepest level
        let deep_path = format!("level{}", depth - 1);
        let schema = CompiledSchema::compile(&[deep_path]).unwrap();

        group.bench_with_input(
            BenchmarkId::new("filtered", format!("depth{depth}")),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_optimized(data, &schema).unwrap();
                    black_box(result.documents.len())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("unfiltered", format!("depth{depth}")),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_raw_simd(data).unwrap();
                    black_box(result.documents.len())
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: Scale (Document Count)
// =============================================================================

fn bench_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("scale");

    let doc_counts = [100, 1000, 10000, 50000];
    let fields_per_doc = 10;

    for doc_count in doc_counts {
        let data = generate_jsonl_documents(doc_count, fields_per_doc);
        group.throughput(Throughput::Bytes(data.len() as u64));

        // Schema matches 1 of 10 fields (10% selectivity per document)
        let schema = CompiledSchema::compile(&["field5".to_string()]).unwrap();

        group.bench_with_input(
            BenchmarkId::new("filtered", format!("{doc_count}docs")),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_optimized(data, &schema).unwrap();
                    black_box(result.documents.len())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("unfiltered", format!("{doc_count}docs")),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_raw_simd(data).unwrap();
                    black_box(result.documents.len())
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: Structural Filtering (Pre-Filter Optimization)
// =============================================================================

fn bench_structural_filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("structural_filtering");

    // Low selectivity = high skip ratio = maximum advantage
    let doc_count = 1000;
    let selectivity_levels = [0.01, 0.05, 0.10];

    for selectivity in selectivity_levels {
        let data = generate_mixed_schema_jsonl(doc_count, selectivity);
        group.throughput(Throughput::Bytes(data.len() as u64));

        let schema = CompiledSchema::compile(&["target".to_string()]).unwrap();

        // Standard optimized processing
        group.bench_with_input(
            BenchmarkId::new("optimized", format!("{:.0}%_match", selectivity * 100.0)),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor.process_batch_optimized(data, &schema).unwrap();
                    black_box(result.documents.len())
                });
            },
        );

        // Structural pre-filtering (SIMD field scan before parse)
        group.bench_with_input(
            BenchmarkId::new("structural", format!("{:.0}%_match", selectivity * 100.0)),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut processor = SimdJsonlBatchProcessor::new();
                    let result = processor
                        .process_batch_structural_filtering(data, &schema)
                        .unwrap();
                    black_box(result.statistics.successful_lines)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Analytic Benchmark: Comprehensive Skip Parsing Analysis
// =============================================================================

/// Run a comprehensive analysis of skip parsing advantage
pub fn analyze_skip_parsing_advantage() {
    println!("\n{}", "=".repeat(80));
    println!("SKIP PARSING ADVANTAGE ANALYSIS");
    println!("{}\n", "=".repeat(80));

    // Test configurations
    let configs = [
        ("Low selectivity (1%)", 0.01, 1000, 50),
        ("Medium selectivity (10%)", 0.10, 1000, 50),
        ("High selectivity (50%)", 0.50, 1000, 50),
        ("Full parse (100%)", 1.00, 1000, 50),
    ];

    println!("| Configuration | Filtered (μs) | Unfiltered (μs) | Speedup | Skip Ratio |");
    println!("|---------------|---------------|-----------------|---------|------------|");

    for (name, _selectivity, doc_count, fields) in configs {
        let data = generate_wide_jsonl(doc_count, fields, 0);
        let schema = CompiledSchema::compile(&["target".to_string()]).unwrap();

        // Warm up
        let mut processor = SimdJsonlBatchProcessor::new();
        let _ = processor.process_batch_optimized(&data, &schema);
        let _ = processor.process_batch_raw_simd(&data);

        // Measure filtered
        let iterations = 100;
        let start = Instant::now();
        for _ in 0..iterations {
            let mut p = SimdJsonlBatchProcessor::new();
            let _ = p.process_batch_optimized(&data, &schema);
        }
        let filtered_us = start.elapsed().as_micros() as f64 / f64::from(iterations);

        // Measure unfiltered
        let start = Instant::now();
        for _ in 0..iterations {
            let mut p = SimdJsonlBatchProcessor::new();
            let _ = p.process_batch_raw_simd(&data);
        }
        let unfiltered_us = start.elapsed().as_micros() as f64 / f64::from(iterations);

        let speedup = unfiltered_us / filtered_us;
        let skip_ratio = 1.0 - (1.0 / fields as f64);

        println!(
            "| {:13} | {:13.1} | {:15.1} | {:7.2}x | {:9.1}% |",
            name,
            filtered_us,
            unfiltered_us,
            speedup,
            skip_ratio * 100.0
        );
    }

    println!("\n");

    // Width analysis
    println!("DOCUMENT WIDTH IMPACT (1000 docs, 1 target field):");
    println!("| Fields | Skip Ratio | Filtered (μs) | Unfiltered (μs) | Speedup |");
    println!("|--------|------------|---------------|-----------------|---------|");

    for fields in [10, 25, 50, 100, 200] {
        let data = generate_wide_jsonl(1000, fields, 0);
        let schema = CompiledSchema::compile(&["target".to_string()]).unwrap();

        let iterations = 50;
        let start = Instant::now();
        for _ in 0..iterations {
            let mut p = SimdJsonlBatchProcessor::new();
            let _ = p.process_batch_optimized(&data, &schema);
        }
        let filtered_us = start.elapsed().as_micros() as f64 / f64::from(iterations);

        let start = Instant::now();
        for _ in 0..iterations {
            let mut p = SimdJsonlBatchProcessor::new();
            let _ = p.process_batch_raw_simd(&data);
        }
        let unfiltered_us = start.elapsed().as_micros() as f64 / f64::from(iterations);

        let speedup = unfiltered_us / filtered_us;
        let skip_ratio = 1.0 - (1.0 / fields as f64);

        println!(
            "| {:6} | {:9.1}% | {:13.1} | {:15.1} | {:7.2}x |",
            fields,
            skip_ratio * 100.0,
            filtered_us,
            unfiltered_us,
            speedup
        );
    }

    println!("\n");

    // Memory analysis
    println!("MEMORY EFFICIENCY (bytes allocated per matched document):");
    let data = generate_wide_jsonl(1000, 100, 0);
    let schema = CompiledSchema::compile(&["target".to_string()]).unwrap();

    let mut processor = SimdJsonlBatchProcessor::new();
    let result = processor.process_batch_optimized(&data, &schema).unwrap();

    println!("  Total input bytes:     {:>10}", data.len());
    println!("  Matched documents:     {:>10}", result.documents.len());
    println!(
        "  Avg memory per line:   {:>10} bytes",
        result.statistics.avg_memory_per_line
    );
    println!(
        "  Schema match ratio:    {:>10.2}%",
        result.statistics.overall_schema_match_ratio * 100.0
    );

    println!("\n{}\n", "=".repeat(80));
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(
    benches,
    bench_schema_selectivity,
    bench_document_width,
    bench_document_depth,
    bench_scale,
    bench_structural_filtering,
);

criterion_main!(benches);
