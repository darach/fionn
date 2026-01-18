// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Benchmarks for simd-gron implementation
//!
//! Compares performance against baseline and measures throughput.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_gron::{GronOptions, gron, ungron_to_value};
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
                    r#"{{"id":{},"name":"Item{}","description":"This is a longer description for item number {} which contains more text","nested":{{"level1":{{"level2":{{"value":{}}}}}}}}}"#,
                    i, i, i, i * 100
                ).unwrap();
            }
            json.push_str("]}");
            json
        }

        "deep" => {
            // Deeply nested structure
            let mut json = String::new();
            for _ in 0..50 {
                json.push_str(r#"{"level":"#);
            }
            json.push_str(r#""bottom""#);
            for _ in 0..50 {
                json.push('}');
            }
            json
        }

        "wide" => {
            // Many sibling fields
            let mut json = String::from("{");
            for i in 0..500 {
                if i > 0 {
                    json.push(',');
                }
                write!(json, r#""field_{i}":"value_{i}""#).unwrap();
            }
            json.push('}');
            json
        }

        "special" => {
            // Fields requiring bracket notation
            let mut json = String::from("{");
            for i in 0..100 {
                if i > 0 {
                    json.push(',');
                }
                write!(
                    json,
                    r#""field.{i}.name":"value{i}","field[{i}]":"bracket{i}""#
                )
                .unwrap();
            }
            json.push('}');
            json
        }

        _ => r"{}".to_string(),
    }
}

fn bench_gron(c: &mut Criterion) {
    let mut group = c.benchmark_group("gron");

    for size in &["small", "medium", "large", "deep", "wide", "special"] {
        let json = generate_test_json(size);
        let json_bytes = json.len();

        group.throughput(Throughput::Bytes(json_bytes as u64));

        group.bench_with_input(BenchmarkId::new("standard", size), &json, |b, json| {
            b.iter(|| gron(black_box(json), &GronOptions::default()));
        });

        group.bench_with_input(BenchmarkId::new("compact", size), &json, |b, json| {
            b.iter(|| gron(black_box(json), &GronOptions::default().compact()));
        });

        group.bench_with_input(BenchmarkId::new("paths_only", size), &json, |b, json| {
            b.iter(|| gron(black_box(json), &GronOptions::default().paths_only()));
        });
    }

    group.finish();
}

fn bench_ungron(c: &mut Criterion) {
    let mut group = c.benchmark_group("ungron");

    for size in &["small", "medium", "large"] {
        let json = generate_test_json(size);
        let gron_output = gron(&json, &GronOptions::default()).unwrap();
        let gron_bytes = gron_output.len();

        group.throughput(Throughput::Bytes(gron_bytes as u64));

        group.bench_with_input(BenchmarkId::new("parse", size), &gron_output, |b, gron| {
            b.iter(|| ungron_to_value(black_box(gron)));
        });
    }

    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    for size in &["small", "medium", "large"] {
        let json = generate_test_json(size);
        let json_bytes = json.len();

        group.throughput(Throughput::Bytes(json_bytes as u64));

        group.bench_with_input(BenchmarkId::new("gron_ungron", size), &json, |b, json| {
            b.iter(|| {
                let gron_output = gron(black_box(json), &GronOptions::default()).unwrap();
                ungron_to_value(&gron_output)
            });
        });
    }

    group.finish();
}

fn bench_path_building(c: &mut Criterion) {
    use fionn_gron::PathBuilder;

    let mut group = c.benchmark_group("path_builder");

    group.bench_function("push_pop_simple", |b| {
        let mut builder = PathBuilder::new("json");
        b.iter(|| {
            builder.push_field("users");
            builder.push_index(0);
            builder.push_field("name");
            let _ = black_box(builder.current_path());
            builder.pop();
            builder.pop();
            builder.pop();
        });
    });

    group.bench_function("push_pop_special", |b| {
        let mut builder = PathBuilder::new("json");
        b.iter(|| {
            builder.push_field("field.with.dots");
            builder.push_index(123);
            builder.push_field("normal");
            let _ = black_box(builder.current_path());
            builder.pop();
            builder.pop();
            builder.pop();
        });
    });

    group.bench_function("deep_nesting", |b| {
        let mut builder = PathBuilder::new("json");
        b.iter(|| {
            for i in 0..20 {
                builder.push_field("level");
                builder.push_index(i);
            }
            let _ = black_box(builder.current_path());
            for _ in 0..40 {
                builder.pop();
            }
        });
    });

    group.finish();
}

fn bench_simd_utils(c: &mut Criterion) {
    use fionn_gron::{needs_escape, needs_quoting};

    let mut group = c.benchmark_group("simd_utils");

    // Clean strings (no escaping needed)
    let clean_short = "simple_field_name";
    let clean_long = "a".repeat(200);

    // Dirty strings (escaping needed)
    let dirty_short = "field.with.dots";
    let dirty_long = format!("{}.", "a".repeat(199));

    group.bench_function("needs_quoting/clean_short", |b| {
        b.iter(|| needs_quoting(black_box(clean_short.as_bytes())));
    });

    group.bench_function("needs_quoting/clean_long", |b| {
        b.iter(|| needs_quoting(black_box(clean_long.as_bytes())));
    });

    group.bench_function("needs_quoting/dirty_short", |b| {
        b.iter(|| needs_quoting(black_box(dirty_short.as_bytes())));
    });

    group.bench_function("needs_quoting/dirty_long", |b| {
        b.iter(|| needs_quoting(black_box(dirty_long.as_bytes())));
    });

    let escape_clean = "This is a clean string without special chars";
    let escape_dirty = "This string has \"quotes\" and \\ backslashes";

    group.bench_function("needs_escape/clean", |b| {
        b.iter(|| needs_escape(black_box(escape_clean.as_bytes())));
    });

    group.bench_function("needs_escape/dirty", |b| {
        b.iter(|| needs_escape(black_box(escape_dirty.as_bytes())));
    });

    group.finish();
}

fn bench_extended_path(c: &mut Criterion) {
    use fionn_gron::parse_extended_path;

    let mut group = c.benchmark_group("extended_path");

    let simple = "json.users.name";
    let bracket = r#"json["field"].items[0]"#;
    let mixed = r#"json["field.with.dots"].items[0].data["key"]"#;
    let deep = "a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p";

    group.bench_function("simple", |b| {
        b.iter(|| parse_extended_path(black_box(simple)));
    });

    group.bench_function("bracket", |b| {
        b.iter(|| parse_extended_path(black_box(bracket)));
    });

    group.bench_function("mixed", |b| {
        b.iter(|| parse_extended_path(black_box(mixed)));
    });

    group.bench_function("deep", |b| {
        b.iter(|| parse_extended_path(black_box(deep)));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_gron,
    bench_ungron,
    bench_roundtrip,
    bench_path_building,
    bench_simd_utils,
    bench_extended_path,
);

criterion_main!(benches);
