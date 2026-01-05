// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(missing_docs)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(unexpected_cfgs)]
//! Schema Selectivity Benchmarks
//!
//! This benchmark demonstrates fionn's key value proposition:
//! **Parse only what you need, skip the rest.**
//!
//! Compares:
//! - Full DOM parsing (serde_json, simd-json, sonic-rs)
//! - fionn SIMD skip operations at various depths/widths
//! - Schema-filtered JSONL batch processing
//!
//! Run with: cargo bench --bench schema_selectivity

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

use fionn_simd::{Avx2Skip, JsonSkiSkip, LangdaleSkip, ScalarSkip, Skip, SkipStrategy};
use fionn_stream::skiptape::CompiledSchema;
use fionn_stream::skiptape::jsonl::SimdJsonlBatchProcessor;

// =============================================================================
// Test Data Generators
// =============================================================================

/// Create a large JSON document with N fields
fn create_wide_json(num_fields: usize) -> String {
    let mut json = String::from("{");
    for i in 0..num_fields {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            "\"field_{i}\":{{\"value\":{i},\"name\":\"item_{i}\",\"active\":{}}}",
            i % 2 == 0
        ));
    }
    json.push('}');
    json
}

/// Create a deeply nested JSON document
fn create_nested_json(depth: usize) -> String {
    let mut json = String::new();
    for i in 0..depth {
        json.push_str(&format!("{{\"level_{i}\":"));
    }
    json.push_str("\"deepest_value\"");
    for _ in 0..depth {
        json.push('}');
    }
    json
}

/// Create a JSON document representing typical API response
fn create_api_response(num_users: usize) -> String {
    let mut json =
        String::from(r#"{"metadata":{"version":"1.0","timestamp":1704067200,"page":1,"total":"#);
    json.push_str(&num_users.to_string());
    json.push_str(r#"},"users":["#);

    for i in 0..num_users {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"id":{i},"name":"user_{i}","email":"user{i}@example.com","profile":{{"bio":"Bio for user {i}","avatar":"https://example.com/avatar/{i}.png","settings":{{"theme":"dark","notifications":true,"language":"en"}}}},"stats":{{"posts":{0},"followers":{1},"following":{2}}}}}"#,
            i * 10, i * 100, i * 50
        ));
    }

    json.push_str(r#"],"pagination":{"next":"/api/users?page=2","prev":null}}"#);
    json
}

// =============================================================================
// Full Parse Benchmarks (Baseline)
// =============================================================================

fn bench_full_parse_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("selectivity/full_parse");

    // 100-field document (~15KB)
    let json_100 = create_api_response(100);
    group.throughput(Throughput::Bytes(json_100.len() as u64));

    group.bench_function("serde_json/100_users", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(&json_100)).unwrap();
        });
    });

    group.bench_function("simd_json/100_users", |b| {
        b.iter(|| {
            let mut bytes = json_100.as_bytes().to_vec();
            let _ = simd_json::to_tape(black_box(&mut bytes)).unwrap();
        });
    });

    group.bench_function("sonic_rs/100_users", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(&json_100)).unwrap();
        });
    });

    group.finish();
}

// =============================================================================
// SIMD Skip Performance (THE KEY VALUE PROPOSITION)
// =============================================================================

fn bench_skip_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("skip/strategies");

    // Test skip strategies on nested objects
    let json_nested = create_nested_json(50);
    let bytes = json_nested.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Skip from after opening brace
    let slice = &bytes[1..];

    group.bench_function("scalar/nested_50", |b| {
        let skip = ScalarSkip;
        b.iter(|| black_box(skip.skip_object(black_box(slice))));
    });

    group.bench_function("langdale/nested_50", |b| {
        let skip = LangdaleSkip::new();
        b.iter(|| black_box(skip.skip_object(black_box(slice))));
    });

    group.bench_function("jsonski/nested_50", |b| {
        let skip = JsonSkiSkip::new();
        b.iter(|| black_box(skip.skip_object(black_box(slice))));
    });

    group.bench_function("avx2/nested_50", |b| {
        let skip = Avx2Skip::new();
        b.iter(|| black_box(skip.skip_object(black_box(slice))));
    });

    group.finish();
}

fn bench_skip_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("skip/scaling");

    // Use AVX2 (best available) for scaling tests
    let skip = Avx2Skip::new();

    // Nested objects at various depths
    for depth in [10, 50, 100, 200] {
        let json = create_nested_json(depth);
        let bytes = json.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        let slice = &bytes[1..];

        group.bench_with_input(BenchmarkId::new("avx2_nested", depth), slice, |b, s| {
            b.iter(|| black_box(skip.skip_object(black_box(s))));
        });
    }

    // Wide objects with many fields
    for fields in [10, 100, 500, 1000] {
        let json = create_wide_json(fields);
        let bytes = json.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        let slice = &bytes[1..];

        group.bench_with_input(BenchmarkId::new("avx2_wide", fields), slice, |b, s| {
            b.iter(|| black_box(skip.skip_object(black_box(s))));
        });
    }

    group.finish();
}

// =============================================================================
// Skip vs Full Parse Comparison
// =============================================================================

fn bench_skip_vs_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("selectivity/skip_vs_parse");

    // Large API response (realistic workload)
    let json = create_api_response(1000);
    group.throughput(Throughput::Bytes(json.len() as u64));

    // Full parse with serde_json
    group.bench_function("serde_json/full_parse", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(&json)).unwrap();
        });
    });

    // Full parse with sonic-rs
    group.bench_function("sonic_rs/full_parse", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(&json)).unwrap();
        });
    });

    // SIMD skip (skip entire document without parsing)
    group.bench_function("fionn_avx2/skip_only", |b| {
        let skip = Avx2Skip::new();
        let bytes = json.as_bytes();
        let slice = &bytes[1..]; // after opening {
        b.iter(|| black_box(skip.skip_object(black_box(slice))));
    });

    // JsonSki skip
    group.bench_function("fionn_jsonski/skip_only", |b| {
        let skip = JsonSkiSkip::new();
        let bytes = json.as_bytes();
        let slice = &bytes[1..];
        b.iter(|| black_box(skip.skip_object(black_box(slice))));
    });

    group.finish();
}

// =============================================================================
// JSONL Streaming with Schema (batch processing)
// =============================================================================

fn bench_jsonl_schema_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("selectivity/jsonl_streaming");

    // Create JSONL data (1000 lines)
    let mut jsonl = String::new();
    for i in 0..1000 {
        jsonl.push_str(&format!(
            r#"{{"id":{i},"name":"user_{i}","email":"user{i}@example.com","data":{{"field1":{0},"field2":{1},"field3":{2}}}}}"#,
            i * 10, i * 20, i * 30
        ));
        jsonl.push('\n');
    }

    group.throughput(Throughput::Bytes(jsonl.len() as u64));

    // Full parse each line with serde_json
    group.bench_function("serde_json/line_by_line", |b| {
        b.iter(|| {
            for line in jsonl.lines() {
                let _: serde_json::Value = serde_json::from_str(black_box(line)).unwrap();
            }
        });
    });

    // Schema-filtered batch processing with fionn
    let schema = CompiledSchema::compile(&["id".to_string(), "name".to_string()]).unwrap();

    group.bench_function("fionn/batch_filtered", |b| {
        let mut processor = SimdJsonlBatchProcessor::new();
        let data = jsonl.as_bytes();

        b.iter(|| {
            let _ = processor
                .process_batch_optimized(black_box(data), &schema)
                .unwrap();
        });
    });

    group.finish();
}

// =============================================================================
// Best Strategy Selection
// =============================================================================

fn bench_best_strategy(c: &mut Criterion) {
    let mut group = c.benchmark_group("skip/best_strategy");

    let json = create_api_response(500);
    let bytes = json.as_bytes();
    let slice = &bytes[1..];
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Use runtime-selected best strategy
    group.bench_function("runtime_best", |b| {
        let strategy = SkipStrategy::best_simd();
        let skipper = strategy.skipper();
        b.iter(|| black_box(skipper.skip_object(black_box(slice))));
    });

    // Compare with explicit AVX2
    group.bench_function("explicit_avx2", |b| {
        let skip = Avx2Skip::new();
        b.iter(|| black_box(skip.skip_object(black_box(slice))));
    });

    group.finish();
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(
    selectivity_benches,
    bench_full_parse_comparison,
    bench_skip_strategies,
    bench_skip_scaling,
    bench_skip_vs_parse,
    bench_jsonl_schema_streaming,
    bench_best_strategy,
);

criterion_main!(selectivity_benches);
