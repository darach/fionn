// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(missing_docs)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(unexpected_cfgs)]
//! Benchmarks for gron optimization strategies.
//!
//! This benchmark compares:
//! - Original gron (scalar escaping)
//! - SIMD-accelerated escaping
//! - Parallel processing for large arrays

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_gron::{
    GronOptions, GronParallelOptions, escape_json_string, escape_json_string_simd, gron,
    gron_parallel, unescape_json_string_simd,
};
use std::hint::black_box;

/// Generate test JSON of varying sizes.
fn generate_json(size: &str) -> String {
    match size {
        "tiny" => r#"{"name": "Alice", "age": 30}"#.to_string(),
        "small" => {
            let items: Vec<String> = (0..10)
                .map(|i| {
                    format!(r#"{{"id": {i}, "name": "User{i}", "email": "user{i}@example.com"}}"#)
                })
                .collect();
            format!(r#"{{"users": [{}]}}"#, items.join(","))
        }
        "medium" => {
            let items: Vec<String> = (0..500)
                .map(|i| {
                    format!(
                        r#"{{"id": {i}, "name": "Record{i}", "description": "Description for record {i} with more text", "meta": {{"version": {i}}}}}"#
                    )
                })
                .collect();
            format!(r#"{{"records": [{}]}}"#, items.join(","))
        }
        "large" => {
            let items: Vec<String> = (0..2500)
                .map(|i| {
                    format!(
                        r#"{{"id": {i}, "name": "Item{i}", "content": "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod", "nested": {{"a": {{"b": {{"value": {i}}}}}}}}}"#
                    )
                })
                .collect();
            format!(r#"{{"data": [{}]}}"#, items.join(","))
        }
        "xlarge" => {
            let items: Vec<String> = (0..5000)
                .map(|i| {
                    format!(
                        r#"{{"id": {i}, "uuid": "550e8400-e29b-41d4-a716-{i:012}", "name": "Entry {i}", "content": "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore", "meta": {{"timestamp": "2024-01-01", "priority": {}, "category": "cat_{}"}} }}"#,
                        i % 10,
                        i % 5
                    )
                })
                .collect();
            format!(r#"{{"dataset": [{}]}}"#, items.join(","))
        }
        _ => panic!("Unknown size: {size}"),
    }
}

/// Generate strings for escape benchmarks.
fn generate_escape_strings() -> Vec<(&'static str, String)> {
    vec![
        ("clean_short", "hello world".to_string()),
        ("clean_medium", "a".repeat(100)),
        ("clean_long", "a".repeat(1000)),
        (
            "dirty_quotes",
            format!("{}\"{}\"", "a".repeat(50), "b".repeat(50)),
        ),
        (
            "dirty_newlines",
            format!("{}\n{}\n{}", "a".repeat(30), "b".repeat(30), "c".repeat(30)),
        ),
        (
            "dirty_mixed",
            format!(
                "{}\"\\{}\n{}",
                "a".repeat(25),
                "b".repeat(25),
                "c".repeat(25)
            ),
        ),
        ("many_escapes", (0..100).map(|_| "\"").collect::<String>()),
        ("unicode", "Hello 世界! 🎉 Emoji and UTF-8".to_string()),
    ]
}

/// Benchmark string escaping: scalar vs SIMD.
fn bench_escape(c: &mut Criterion) {
    let mut group = c.benchmark_group("escape_comparison");

    for (name, input) in generate_escape_strings() {
        let size = input.len();
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("scalar", name), &input, |b, s| {
            b.iter(|| {
                let mut out = Vec::with_capacity(s.len() + 10);
                escape_json_string(black_box(s), &mut out);
                out
            });
        });

        group.bench_with_input(BenchmarkId::new("simd", name), &input, |b, s| {
            b.iter(|| {
                let mut out = Vec::with_capacity(s.len() + 10);
                escape_json_string_simd(black_box(s), &mut out);
                out
            });
        });
    }

    group.finish();
}

/// Benchmark gron: sequential vs parallel.
fn bench_gron_modes(c: &mut Criterion) {
    let mut group = c.benchmark_group("gron_modes");

    for size in ["tiny", "small", "medium", "large"] {
        let json = generate_json(size);
        let json_size = json.len();
        group.throughput(Throughput::Bytes(json_size as u64));

        group.bench_with_input(BenchmarkId::new("sequential", size), &json, |b, j| {
            b.iter(|| gron(black_box(j), &GronOptions::default()).unwrap());
        });

        // Only benchmark parallel for medium and large
        if size == "medium" || size == "large" {
            let options = GronParallelOptions::default().with_threshold(100);
            group.bench_with_input(BenchmarkId::new("parallel", size), &json, |b, j| {
                b.iter(|| gron_parallel(black_box(j), &options).unwrap());
            });
        }
    }

    group.finish();
}

/// Benchmark parallel thresholds.
fn bench_parallel_thresholds(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_thresholds");

    // Use large file for threshold testing
    let json = generate_json("large");
    let json_size = json.len();
    group.throughput(Throughput::Bytes(json_size as u64));

    for threshold in [10, 50, 100, 250, 500, 1000, 2500] {
        let options = GronParallelOptions::default().with_threshold(threshold);
        group.bench_with_input(BenchmarkId::new("threshold", threshold), &json, |b, j| {
            b.iter(|| gron_parallel(black_box(j), &options).unwrap());
        });
    }

    group.finish();
}

/// Benchmark throughput scaling with file size.
fn bench_throughput_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_scaling");

    for size in ["tiny", "small", "medium", "large", "xlarge"] {
        let json = generate_json(size);
        let json_size = json.len();
        group.throughput(Throughput::Bytes(json_size as u64));

        group.bench_with_input(BenchmarkId::new("gron", size), &json, |b, j| {
            b.iter(|| gron(black_box(j), &GronOptions::default()).unwrap());
        });
    }

    group.finish();
}

/// Benchmark strings with high escape density.
fn bench_escape_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("escape_density");

    // Generate strings with varying escape character density
    let densities = [0, 1, 5, 10, 25, 50, 100]; // percentage

    for density in densities {
        let input = generate_string_with_density(1000, density);
        let size = input.len();
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("simd", format!("{density}%")),
            &input,
            |b, s| {
                b.iter(|| {
                    let mut out = Vec::with_capacity(s.len() * 2);
                    escape_json_string_simd(black_box(s), &mut out);
                    out
                });
            },
        );
    }

    group.finish();
}

/// Generate a string with specified escape character density.
fn generate_string_with_density(len: usize, density_percent: usize) -> String {
    let mut result = String::with_capacity(len);
    for i in 0..len {
        if density_percent > 0 && (i * 100 / len).is_multiple_of(100 / density_percent.max(1)) {
            // Add an escape character
            match i % 4 {
                0 => result.push('"'),
                1 => result.push('\\'),
                2 => result.push('\n'),
                _ => result.push('\t'),
            }
        } else {
            result.push('a');
        }
    }
    result
}

/// Benchmark SIMD unescape.
fn bench_unescape(c: &mut Criterion) {
    let mut group = c.benchmark_group("unescape");

    // Pre-escape the strings to benchmark unescaping
    let test_cases: Vec<(&str, Vec<u8>)> = generate_escape_strings()
        .into_iter()
        .map(|(name, input)| {
            let mut escaped = Vec::with_capacity(input.len() + 10);
            escape_json_string_simd(&input, &mut escaped);
            // Strip surrounding quotes
            let inner = escaped[1..escaped.len() - 1].to_vec();
            (name, inner)
        })
        .collect();

    for (name, input) in &test_cases {
        let size = input.len();
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("simd", *name), input, |b, s| {
            b.iter(|| unescape_json_string_simd(black_box(s)).unwrap());
        });
    }

    group.finish();
}

/// Benchmark unescape with varying escape densities.
fn bench_unescape_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("unescape_density");

    let densities = [0, 1, 5, 10, 25, 50, 100];

    for density in densities {
        // Generate escaped string
        let original = generate_string_with_density(1000, density);
        let mut escaped = Vec::with_capacity(original.len() + 10);
        escape_json_string_simd(&original, &mut escaped);
        let inner = escaped[1..escaped.len() - 1].to_vec();
        let size = inner.len();

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("simd", format!("{density}%")),
            &inner,
            |b, s| {
                b.iter(|| unescape_json_string_simd(black_box(s)).unwrap());
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_escape,
    bench_unescape,
    bench_gron_modes,
    bench_parallel_thresholds,
    bench_throughput_scaling,
    bench_escape_density,
    bench_unescape_density,
);

criterion_main!(benches);
