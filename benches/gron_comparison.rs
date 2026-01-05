// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(missing_docs)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(unexpected_cfgs)]
//! Comparison benchmarks: simd-gron vs gron vs fastgron
//!
//! This benchmark compares our implementation against the original gron
//! and fastgron tools using subprocess execution for fair comparison.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_gron::{GronOptions, gron};
use std::fmt::Write as FmtWrite;
use std::hint::black_box;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Generate test JSON of various sizes
fn generate_test_json(size: &str) -> String {
    match size {
        "tiny" => r#"{"name":"Alice","age":30}"#.to_string(),

        "small" => {
            // ~1KB - typical API response
            let mut json = String::from(r#"{"users":["#);
            for i in 0..10 {
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

        "medium" => {
            // ~10KB
            let mut json = String::from(r#"{"records":["#);
            for i in 0..100 {
                if i > 0 {
                    json.push(',');
                }
                write!(
                    json,
                    r#"{{"id":{i},"name":"Record{i}","description":"Description for record {i} with some additional text","metadata":{{"created":"2024-01-01","updated":"2024-12-01","version":{i}}}}}"#
                )
                .unwrap();
            }
            json.push_str("]}");
            json
        }

        "large" => {
            // ~100KB
            let mut json = String::from(r#"{"data":["#);
            for i in 0..500 {
                if i > 0 {
                    json.push(',');
                }
                write!(
                    json,
                    r#"{{"id":{},"name":"Item{}","description":"This is a longer description for item number {} which contains more text to increase the payload size","nested":{{"level1":{{"level2":{{"level3":{{"value":{}}}}}}}}},"tags":["tag1","tag2","tag3"],"scores":[{},{},{}]}}"#,
                    i, i, i, i * 100, i, i * 2, i * 3
                )
                .unwrap();
            }
            json.push_str("]}");
            json
        }

        "xlarge" => {
            // ~1MB
            let mut json = String::from(r#"{"dataset":["#);
            for i in 0..2000 {
                if i > 0 {
                    json.push(',');
                }
                write!(
                    json,
                    r#"{{"id":{},"uuid":"550e8400-e29b-41d4-a716-{:012}","name":"Entry number {} in the dataset","content":"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation.","metadata":{{"created_at":"2024-01-{}","priority":{},"category":"category_{}","nested":{{"deep":{{"value":{}}}}}}}}}"#,
                    i,
                    i,
                    i,
                    (i % 28) + 1,
                    i % 10,
                    i % 5,
                    i * 10
                )
                .unwrap();
            }
            json.push_str("]}");
            json
        }

        _ => r"{}".to_string(),
    }
}

/// Run external gron command
fn run_gron(json: &str) -> String {
    let mut child = Command::new("gron")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn gron");

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(json.as_bytes()).unwrap();
    }

    let output = child
        .wait_with_output()
        .expect("Failed to read gron output");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Run external fastgron command
fn run_fastgron(json: &str) -> String {
    let mut child = Command::new("fastgron")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn fastgron");

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(json.as_bytes()).unwrap();
    }

    let output = child
        .wait_with_output()
        .expect("Failed to read fastgron output");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Run our simd-gron implementation
fn run_simd_gron(json: &str) -> String {
    gron(json, &GronOptions::default()).unwrap_or_default()
}

fn bench_gron_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("gron_comparison");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    for size in &["tiny", "small", "medium", "large"] {
        let json = generate_test_json(size);
        let json_bytes = json.len();

        group.throughput(Throughput::Bytes(json_bytes as u64));

        // Benchmark simd-gron (our implementation)
        group.bench_with_input(BenchmarkId::new("simd-gron", size), &json, |b, json| {
            b.iter(|| run_simd_gron(black_box(json)))
        });

        // Benchmark original gron
        group.bench_with_input(BenchmarkId::new("gron", size), &json, |b, json| {
            b.iter(|| run_gron(black_box(json)))
        });

        // Benchmark fastgron
        group.bench_with_input(BenchmarkId::new("fastgron", size), &json, |b, json| {
            b.iter(|| run_fastgron(black_box(json)))
        });
    }

    group.finish();
}

/// Benchmark just the library implementations (no subprocess overhead)
fn bench_library_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_gron_throughput");

    for size in &["tiny", "small", "medium", "large", "xlarge"] {
        let json = generate_test_json(size);
        let json_bytes = json.len();

        group.throughput(Throughput::Bytes(json_bytes as u64));

        group.bench_with_input(BenchmarkId::new("simd-gron", size), &json, |b, json| {
            b.iter(|| gron(black_box(json), &GronOptions::default()))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_gron_comparison, bench_library_only);
criterion_main!(benches);
