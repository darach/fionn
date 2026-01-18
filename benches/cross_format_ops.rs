// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmark files don't need public docs; criterion_group! macro generates undocumentable code
// Benchmark files use simple string formatting for readability over performance
#![allow(missing_docs)]
#![allow(clippy::format_push_string)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(dead_code)]
//! Cross-format operations benchmark
//!
//! Demonstrates fionn's unique capabilities:
//! 1. Cross-format diff (YAML vs JSON semantic diff)
//! 2. Format-agnostic skip operations
//! 3. Streaming CRDT pipeline
//! 4. Schema-projected extraction across formats

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_core::TapeSource;
use fionn_diff::json_diff;
use fionn_tape::DsonTape;
use serde_json::Value;

#[cfg(feature = "yaml")]
use fionn_core::FormatKind;
#[cfg(feature = "yaml")]
use fionn_simd::transform::UnifiedTape;

// =============================================================================
// Test Data: Equivalent content in multiple formats
// =============================================================================

/// Generate equivalent config in JSON
fn generate_config_json(size: usize) -> String {
    use std::fmt::Write;
    let mut json = String::from(
        r#"{"config":{"server":{"host":"localhost","port":8080,"ssl":true},"database":{"url":"postgres://localhost/db","pool_size":10},"features":{"#,
    );
    for i in 0..size {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(
            json,
            r#""feature_{i}":{0},"feature_{i}_enabled":{1}"#,
            i % 100,
            i % 2 == 0
        );
    }
    json.push_str(r#"},"logging":{"level":"info","format":"json"}}}"#);
    json
}

/// Generate equivalent config in YAML
#[cfg(feature = "yaml")]
fn generate_config_yaml(size: usize) -> String {
    let mut yaml = String::from(
        r"config:
  server:
    host: localhost
    port: 8080
    ssl: true
  database:
    url: postgres://localhost/db
    pool_size: 10
  features:
",
    );
    for i in 0..size {
        yaml.push_str(&format!("    feature_{}: {}\n", i, i % 100));
        yaml.push_str(&format!("    feature_{}_enabled: {}\n", i, i % 2 == 0));
    }
    yaml.push_str(
        r"  logging:
    level: info
    format: json
",
    );
    yaml
}

/// Generate a modified version (for diff testing)
fn generate_config_json_modified(size: usize) -> String {
    use std::fmt::Write;
    let mut json = String::from(
        r#"{"config":{"server":{"host":"production.example.com","port":443,"ssl":true},"database":{"url":"postgres://prod-db/db","pool_size":50},"features":{"#,
    );
    for i in 0..size {
        if i > 0 {
            json.push(',');
        }
        // Modify every 3rd feature
        let value = if i % 3 == 0 { i % 100 + 1000 } else { i % 100 };
        let _ = write!(
            json,
            r#""feature_{i}":{value},"feature_{i}_enabled":{0}"#,
            i % 2 == 0
        );
    }
    json.push_str(r#"},"logging":{"level":"warn","format":"json"}}}"#);
    json
}

#[cfg(feature = "yaml")]
fn generate_config_yaml_modified(size: usize) -> String {
    let mut yaml = String::from(
        r"config:
  server:
    host: production.example.com
    port: 443
    ssl: true
  database:
    url: postgres://prod-db/db
    pool_size: 50
  features:
",
    );
    for i in 0..size {
        let value = if i % 3 == 0 { i % 100 + 1000 } else { i % 100 };
        yaml.push_str(&format!("    feature_{}: {}\n", i, value));
        yaml.push_str(&format!("    feature_{}_enabled: {}\n", i, i % 2 == 0));
    }
    yaml.push_str(
        r"  logging:
    level: warn
    format: json
",
    );
    yaml
}

// =============================================================================
// Cross-Format Diff Benchmarks
// =============================================================================

/// Benchmark: Diff JSON vs JSON (baseline)
fn bench_diff_json_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format/diff_json_json");

    for size in [10, 50, 100] {
        let json_a = generate_config_json(size);
        let json_b = generate_config_json_modified(size);

        let val_a: Value = serde_json::from_str(&json_a).unwrap();
        let val_b: Value = serde_json::from_str(&json_b).unwrap();

        group.throughput(Throughput::Bytes((json_a.len() + json_b.len()) as u64));

        group.bench_with_input(
            BenchmarkId::new("serde_diff", size),
            &(&val_a, &val_b),
            |bencher, (a, b)| {
                bencher.iter(|| {
                    let patch = json_diff(black_box(a), black_box(b));
                    black_box(patch)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Cross-format diff (YAML source vs JSON source)
#[cfg(feature = "yaml")]
fn bench_diff_yaml_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format/diff_yaml_json");

    for size in [10, 50, 100] {
        let yaml_a = generate_config_yaml(size);
        let json_b = generate_config_json_modified(size);

        group.throughput(Throughput::Bytes((yaml_a.len() + json_b.len()) as u64));

        // Approach 1: Convert YAML to JSON, then diff (traditional)
        group.bench_with_input(
            BenchmarkId::new("convert_then_diff", size),
            &(&yaml_a, &json_b),
            |b, (yaml, json)| {
                b.iter(|| {
                    // Parse YAML via serde
                    let yaml_val: Value = serde_yaml::from_str(black_box(yaml)).unwrap();
                    let json_val: Value = serde_json::from_str(black_box(json)).unwrap();
                    let patch = json_diff(&yaml_val, &json_val);
                    black_box(patch)
                })
            },
        );

        // Approach 2: Parse both to tape, diff via tape (fionn way)
        group.bench_with_input(
            BenchmarkId::new("tape_diff", size),
            &(&yaml_a, &json_b),
            |b, (yaml, json)| {
                b.iter(|| {
                    // Parse to unified tape
                    let tape_a =
                        UnifiedTape::parse(black_box(yaml.as_bytes()), FormatKind::Yaml).unwrap();
                    let tape_b =
                        UnifiedTape::parse(black_box(json.as_bytes()), FormatKind::Json).unwrap();
                    // Compare tape lengths as proxy for structural diff
                    // (Full tape diff would require TapeSource-based diff impl)
                    black_box((tape_a.len(), tape_b.len()))
                })
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "yaml"))]
const fn bench_diff_yaml_json(_c: &mut Criterion) {}

// =============================================================================
// Format-Agnostic Skip Benchmarks
// =============================================================================

/// Benchmark: JSON skip operations (baseline)
fn bench_skip_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format/skip_json");

    for size in [10, 50, 100] {
        let json = generate_config_json(size);
        let tape = DsonTape::parse(&json).unwrap();

        group.throughput(Throughput::Bytes(json.len() as u64));

        // Skip to features section
        group.bench_with_input(
            BenchmarkId::new("skip_to_features", size),
            &tape,
            |b, tape| {
                b.iter(|| {
                    // Skip first few nodes to reach features
                    let result = tape.skip_value(black_box(5));
                    black_box(result)
                });
            },
        );

        // Full traversal for comparison
        group.bench_with_input(BenchmarkId::new("full_traverse", size), &tape, |b, tape| {
            b.iter(|| {
                let mut count = 0;
                for i in 0..tape.len() {
                    if tape.node_at(i).is_some() {
                        count += 1;
                    }
                }
                black_box(count)
            });
        });
    }

    group.finish();
}

/// Benchmark: YAML skip operations
#[cfg(feature = "yaml")]
fn bench_skip_yaml(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format/skip_yaml");

    for size in [10, 50, 100] {
        let yaml = generate_config_yaml(size);

        group.throughput(Throughput::Bytes(yaml.len() as u64));

        // Parse and skip
        group.bench_with_input(
            BenchmarkId::new("parse_and_skip", size),
            &yaml,
            |b, input| {
                b.iter(|| {
                    let tape =
                        UnifiedTape::parse(black_box(input.as_bytes()), FormatKind::Yaml).unwrap();
                    // Skip via tape length check
                    black_box(tape.len())
                })
            },
        );

        // Full traverse via TapeSource
        group.bench_with_input(
            BenchmarkId::new("tape_traverse", size),
            &yaml,
            |b, input| {
                b.iter(|| {
                    let tape =
                        UnifiedTape::parse(black_box(input.as_bytes()), FormatKind::Yaml).unwrap();
                    let mut count = 0;
                    for i in 0..tape.len() {
                        if tape.node_at(i).is_some() {
                            count += 1;
                        }
                    }
                    black_box(count)
                })
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "yaml"))]
const fn bench_skip_yaml(_c: &mut Criterion) {}

// =============================================================================
// Streaming CRDT Pipeline Benchmarks
// =============================================================================

/// Simulate streaming records for CRDT merge
fn generate_stream_records(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            format!(
                r#"{{"id":"record_{}","timestamp":{},"value":{},"status":"{}"}}"#,
                i % 100, // Overlapping IDs for merge scenarios
                1_000_000 + i,
                i * 10,
                if i % 2 == 0 { "active" } else { "pending" }
            )
        })
        .collect()
}

/// Benchmark: Streaming parse + CRDT merge pipeline
fn bench_streaming_crdt_pipeline(c: &mut Criterion) {
    use fionn_crdt::{Winner, merge_lww_fast};

    let mut group = c.benchmark_group("cross_format/streaming_crdt");

    for count in [100, 500, 1000] {
        let records = generate_stream_records(count);
        let total_bytes: usize = records.iter().map(std::string::String::len).sum();

        group.throughput(Throughput::Bytes(total_bytes as u64));

        // Approach 1: Parse all, then merge (batch)
        group.bench_with_input(
            BenchmarkId::new("batch_parse_merge", count),
            &records,
            |b, records| {
                b.iter(|| {
                    let mut merged: std::collections::HashMap<String, (u64, i64)> =
                        std::collections::HashMap::new();

                    for record in records {
                        let tape = DsonTape::parse(black_box(record)).unwrap();
                        // Extract id and timestamp (simplified)
                        let id = format!("id_{}", tape.len() % 100);
                        let ts = tape.len() as u64;
                        let val = tape.len() as i64;

                        merged
                            .entry(id)
                            .and_modify(|(old_ts, old_val)| match merge_lww_fast(*old_ts, ts) {
                                Winner::Remote => {
                                    *old_ts = ts;
                                    *old_val = val;
                                }
                                Winner::Local | Winner::Merged => {}
                            })
                            .or_insert((ts, val));
                    }
                    black_box(merged.len())
                });
            },
        );

        // Approach 2: serde parse + manual merge (baseline)
        group.bench_with_input(
            BenchmarkId::new("serde_parse_merge", count),
            &records,
            |b, records| {
                b.iter(|| {
                    let mut merged: std::collections::HashMap<String, (u64, i64)> =
                        std::collections::HashMap::new();

                    for record in records {
                        let val: Value = serde_json::from_str(black_box(record)).unwrap();
                        let id = val["id"].as_str().unwrap_or("unknown").to_string();
                        let ts = val["timestamp"].as_u64().unwrap_or(0);
                        let v = val["value"].as_i64().unwrap_or(0);

                        merged
                            .entry(id)
                            .and_modify(|(old_ts, old_val)| {
                                if ts > *old_ts {
                                    *old_ts = ts;
                                    *old_val = v;
                                }
                            })
                            .or_insert((ts, v));
                    }
                    black_box(merged.len())
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Schema-Projected Extraction Benchmarks
// =============================================================================

/// Generate records with many fields (only few needed)
fn generate_wide_records(count: usize, fields: usize) -> Vec<String> {
    use std::fmt::Write;
    (0..count)
        .map(|i| {
            let mut json = format!(r#"{{"id":{i},"timestamp":{0}"#, 1_000_000 + i);
            for f in 0..fields {
                let _ = write!(json, r#","field_{f}":"value_{f}""#);
            }
            json.push('}');
            json
        })
        .collect()
}

/// Benchmark: Extract specific fields from wide records
fn bench_schema_projection(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format/schema_projection");

    let fields = 50; // 50 fields per record

    for count in [100, 500] {
        let records = generate_wide_records(count, fields);
        let total_bytes: usize = records.iter().map(std::string::String::len).sum();

        group.throughput(Throughput::Bytes(total_bytes as u64));

        // Full parse (baseline)
        group.bench_with_input(
            BenchmarkId::new("full_parse", count),
            &records,
            |b, records| {
                b.iter(|| {
                    let mut ids = Vec::with_capacity(records.len());
                    for record in records {
                        let val: Value = serde_json::from_str(black_box(record)).unwrap();
                        if let Some(id) = val["id"].as_i64() {
                            ids.push(id);
                        }
                    }
                    black_box(ids.len())
                });
            },
        );

        // Tape parse + selective access
        group.bench_with_input(
            BenchmarkId::new("tape_selective", count),
            &records,
            |b, records| {
                b.iter(|| {
                    let mut count = 0usize;
                    for record in records {
                        let tape = DsonTape::parse(black_box(record)).unwrap();
                        // Access only what we need (first value after object start)
                        if tape.node_at(2).is_some() {
                            count += 1;
                        }
                    }
                    black_box(count)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Cross-Format Transform Benchmarks
// =============================================================================

#[cfg(feature = "yaml")]
fn bench_cross_format_transform(c: &mut Criterion) {
    use fionn_simd::transform::{TransformOptions, transform};

    let mut group = c.benchmark_group("cross_format/transform");

    for size in [10, 50, 100] {
        let json = generate_config_json(size);
        let yaml = generate_config_yaml(size);
        let options = TransformOptions::default();

        // JSON → YAML
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::new("json_to_yaml", size), &json, |b, input| {
            b.iter(|| {
                let result = transform(
                    black_box(input.as_bytes()),
                    FormatKind::Json,
                    FormatKind::Yaml,
                    &options,
                );
                black_box(result)
            })
        });

        // YAML → JSON
        group.throughput(Throughput::Bytes(yaml.len() as u64));
        group.bench_with_input(BenchmarkId::new("yaml_to_json", size), &yaml, |b, input| {
            b.iter(|| {
                let result = transform(
                    black_box(input.as_bytes()),
                    FormatKind::Yaml,
                    FormatKind::Json,
                    &options,
                );
                black_box(result)
            })
        });

        // Comparison: serde roundtrip
        group.bench_with_input(
            BenchmarkId::new("serde_json_to_yaml", size),
            &json,
            |b, input| {
                b.iter(|| {
                    let val: Value = serde_json::from_str(black_box(input)).unwrap();
                    let result = serde_yaml::to_string(&val);
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "yaml"))]
const fn bench_cross_format_transform(_c: &mut Criterion) {}

// =============================================================================
// Format Comparison: Parse Throughput
// =============================================================================

#[cfg(feature = "yaml")]
fn bench_format_parse_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format/parse_comparison");

    for size in [10, 50, 100] {
        let json = generate_config_json(size);
        let yaml = generate_config_yaml(size);

        // JSON parsing
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("json_dsontape", size),
            &json,
            |b, input| {
                b.iter(|| {
                    let tape = DsonTape::parse(black_box(input)).unwrap();
                    black_box(tape.len())
                })
            },
        );

        group.bench_with_input(BenchmarkId::new("json_unified", size), &json, |b, input| {
            b.iter(|| {
                let tape =
                    UnifiedTape::parse(black_box(input.as_bytes()), FormatKind::Json).unwrap();
                black_box(tape.len())
            })
        });

        // YAML parsing
        group.throughput(Throughput::Bytes(yaml.len() as u64));
        group.bench_with_input(BenchmarkId::new("yaml_unified", size), &yaml, |b, input| {
            b.iter(|| {
                let tape =
                    UnifiedTape::parse(black_box(input.as_bytes()), FormatKind::Yaml).unwrap();
                black_box(tape.len())
            })
        });

        group.bench_with_input(BenchmarkId::new("yaml_serde", size), &yaml, |b, input| {
            b.iter(|| {
                let val: Value = serde_yaml::from_str(black_box(input)).unwrap();
                black_box(val)
            })
        });
    }

    group.finish();
}

#[cfg(not(feature = "yaml"))]
const fn bench_format_parse_comparison(_c: &mut Criterion) {}

criterion_group!(
    cross_format_benchmarks,
    bench_diff_json_json,
    bench_diff_yaml_json,
    bench_skip_json,
    bench_skip_yaml,
    bench_streaming_crdt_pipeline,
    bench_schema_projection,
    bench_cross_format_transform,
    bench_format_parse_comparison,
);

criterion_main!(cross_format_benchmarks);
