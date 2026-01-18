// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - tape transformations conditionally compiled by format features
#![allow(unused)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Tape-to-tape transformation benchmarks
//!
//! Measures the performance of direct tape transformations between formats,
//! comparing against traditional parse-serialize pipelines.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_core::FormatKind;
use fionn_simd::transform::{TransformOptions, UnifiedTape, transform};

/// Generate structured JSON test data
fn generate_json(size: &str) -> String {
    match size {
        "small" => r#"{"id": 1, "name": "test", "active": true}"#.to_string(),
        "medium" => {
            let mut json = String::from(r#"{"users": ["#);
            for i in 0..100 {
                if i > 0 {
                    json.push(',');
                }
                json.push_str(&format!(
                    r#"{{"id": {}, "name": "user_{}", "email": "user{}@example.com", "active": {}}}"#,
                    i,
                    i,
                    i,
                    i % 2 == 0
                ));
            }
            json.push_str("]}");
            json
        }
        "large" => {
            let mut json = String::from(r#"{"data": {"records": ["#);
            for i in 0..1000 {
                if i > 0 {
                    json.push(',');
                }
                json.push_str(&format!(
                    r#"{{"id": {}, "timestamp": "{}", "value": {}.{}, "tags": ["tag1", "tag2", "tag3"], "metadata": {{"source": "sensor_{}", "version": "1.0"}}}}"#,
                    i, "2024-01-01T00:00:00Z", i * 100, i, i
                ));
            }
            json.push_str("]}}");
            json
        }
        _ => r"{}".to_string(),
    }
}

/// Benchmark: JSON to JSON identity transform (baseline)
fn bench_json_identity(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/identity");
    let options = TransformOptions::default();

    for size in ["small", "medium", "large"] {
        let json = generate_json(size);
        let bytes = json.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("json_to_json", size), bytes, |b, input| {
            b.iter(|| {
                black_box(transform(
                    input,
                    FormatKind::Json,
                    FormatKind::Json,
                    &options,
                ));
            });
        });
    }

    group.finish();
}

/// Benchmark: JSON to YAML transformation
#[cfg(feature = "yaml")]
fn bench_json_to_yaml(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/json_to_yaml");
    let options = TransformOptions::default();

    for size in ["small", "medium", "large"] {
        let json = generate_json(size);
        let bytes = json.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // Tape-based transformation
        group.bench_with_input(
            BenchmarkId::new("tape_transform", size),
            bytes,
            |b, input| {
                b.iter(|| {
                    black_box(transform(
                        input,
                        FormatKind::Json,
                        FormatKind::Yaml,
                        &options,
                    ))
                })
            },
        );

        // Traditional parse-serialize (serde_json -> serde_yaml)
        group.bench_with_input(
            BenchmarkId::new("serde_roundtrip", size),
            &json,
            |b, input| {
                b.iter(|| {
                    let value: serde_json::Value = serde_json::from_str(input).unwrap();
                    black_box(serde_yaml::to_string(&value).unwrap())
                })
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "yaml"))]
fn bench_json_to_yaml(_c: &mut Criterion) {}

/// Benchmark: YAML to JSON transformation
#[cfg(feature = "yaml")]
fn bench_yaml_to_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/yaml_to_json");
    let options = TransformOptions::default();

    // Generate YAML equivalents
    let yaml_small = "id: 1\nname: test\nactive: true\n";
    let yaml_medium = {
        let mut yaml = String::from("users:\n");
        for i in 0..100 {
            yaml.push_str(&format!(
                "  - id: {}\n    name: user_{}\n    email: user{}@example.com\n    active: {}\n",
                i,
                i,
                i,
                i % 2 == 0
            ));
        }
        yaml
    };

    for (size, yaml) in [("small", yaml_small.to_string()), ("medium", yaml_medium)] {
        let bytes = yaml.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // Tape-based transformation
        group.bench_with_input(
            BenchmarkId::new("tape_transform", size),
            bytes,
            |b, input| {
                b.iter(|| {
                    black_box(transform(
                        input,
                        FormatKind::Yaml,
                        FormatKind::Json,
                        &options,
                    ))
                })
            },
        );

        // Traditional parse-serialize
        group.bench_with_input(
            BenchmarkId::new("serde_roundtrip", size),
            &yaml,
            |b, input| {
                b.iter(|| {
                    let value: serde_yaml::Value = serde_yaml::from_str(input).unwrap();
                    black_box(serde_json::to_string(&value).unwrap())
                })
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "yaml"))]
fn bench_yaml_to_json(_c: &mut Criterion) {}

/// Benchmark: JSON to TOML transformation
#[cfg(feature = "toml")]
fn bench_json_to_toml(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/json_to_toml");
    let options = TransformOptions::default();

    // TOML requires top-level object, use config-like structure
    let json_config = r#"{"package": {"name": "test", "version": "1.0.0"}, "dependencies": {"serde": "1.0", "tokio": "1.0"}, "features": {"default": ["std"], "async": ["tokio"]}}"#;

    let bytes = json_config.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Tape-based transformation
    group.bench_function("tape_transform", |b| {
        b.iter(|| {
            black_box(transform(
                bytes,
                FormatKind::Json,
                FormatKind::Toml,
                &options,
            ))
        })
    });

    // Traditional parse-serialize
    group.bench_with_input("serde_roundtrip", &json_config, |b, input| {
        b.iter(|| {
            let value: serde_json::Value = serde_json::from_str(input).unwrap();
            black_box(toml::to_string(&value).unwrap())
        })
    });

    group.finish();
}

#[cfg(not(feature = "toml"))]
fn bench_json_to_toml(_c: &mut Criterion) {}

/// Benchmark: TOML to JSON transformation
#[cfg(feature = "toml")]
fn bench_toml_to_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/toml_to_json");
    let options = TransformOptions::default();

    let toml_config = r#"
[package]
name = "test"
version = "1.0.0"

[dependencies]
serde = "1.0"
tokio = "1.0"

[features]
default = ["std"]
async = ["tokio"]
"#;

    let bytes = toml_config.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Tape-based transformation
    group.bench_function("tape_transform", |b| {
        b.iter(|| {
            black_box(transform(
                bytes,
                FormatKind::Toml,
                FormatKind::Json,
                &options,
            ))
        })
    });

    // Traditional parse-serialize
    group.bench_with_input("serde_roundtrip", &toml_config, |b, input| {
        b.iter(|| {
            let value: toml::Value = toml::from_str(input).unwrap();
            black_box(serde_json::to_string(&value).unwrap())
        })
    });

    group.finish();
}

#[cfg(not(feature = "toml"))]
fn bench_toml_to_json(_c: &mut Criterion) {}

/// Benchmark: JSON to ISON transformation (LLM-optimized)
#[cfg(feature = "ison")]
fn bench_json_to_ison(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/json_to_ison");
    let options = TransformOptions::default();

    for size in ["small", "medium", "large"] {
        let json = generate_json(size);
        let bytes = json.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("tape_transform", size),
            bytes,
            |b, input| {
                b.iter(|| {
                    black_box(transform(
                        input,
                        FormatKind::Json,
                        FormatKind::Ison,
                        &options,
                    ))
                })
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "ison"))]
fn bench_json_to_ison(_c: &mut Criterion) {}

/// Benchmark: JSON to TOON transformation
#[cfg(feature = "toon")]
fn bench_json_to_toon(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/json_to_toon");
    let options = TransformOptions::default();

    for size in ["small", "medium"] {
        let json = generate_json(size);
        let bytes = json.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("tape_transform", size),
            bytes,
            |b, input| {
                b.iter(|| {
                    black_box(transform(
                        input,
                        FormatKind::Json,
                        FormatKind::Toon,
                        &options,
                    ))
                })
            },
        );
    }

    group.finish();
}

#[cfg(not(feature = "toon"))]
fn bench_json_to_toon(_c: &mut Criterion) {}

/// Benchmark: Pre-parsed tape transformation (no parse overhead)
fn bench_preparsed_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/preparsed");

    for size in ["small", "medium", "large"] {
        let json = generate_json(size);

        // Pre-parse the tape
        let tape = UnifiedTape::parse(json.as_bytes(), FormatKind::Json).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        // Emit only (no parsing)
        #[cfg(feature = "yaml")]
        {
            use fionn_simd::transform::{Emitter, YamlEmitter};
            let options = TransformOptions::default();
            let emitter = YamlEmitter::new(&options);
            group.bench_with_input(
                BenchmarkId::new("emit_yaml_only", size),
                &tape,
                |b, tape| b.iter(|| black_box(emitter.emit(tape))),
            );
        }

        // Compare with full transform
        #[cfg(feature = "yaml")]
        {
            let options = TransformOptions::default();
            group.bench_with_input(
                BenchmarkId::new("full_transform", size),
                &json,
                |b, input| {
                    b.iter(|| {
                        black_box(transform(
                            input.as_bytes(),
                            FormatKind::Json,
                            FormatKind::Yaml,
                            &options,
                        ))
                    })
                },
            );
        }
    }

    group.finish();
}

/// Benchmark: Transformation with pretty printing
fn bench_pretty_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/pretty");

    let json = generate_json("medium");
    let bytes = json.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    // Compact output
    let compact_opts = TransformOptions::default();
    group.bench_function("json_compact", |b| {
        b.iter(|| {
            black_box(transform(
                bytes,
                FormatKind::Json,
                FormatKind::Json,
                &compact_opts,
            ))
        })
    });

    // Pretty output
    let pretty_opts = TransformOptions::default().with_pretty(true);
    group.bench_function("json_pretty", |b| {
        b.iter(|| {
            black_box(transform(
                bytes,
                FormatKind::Json,
                FormatKind::Json,
                &pretty_opts,
            ))
        })
    });

    group.finish();
}

/// Benchmark: Transformation with zero-allocation (pre-allocated buffer)
fn bench_zero_alloc_transform(c: &mut Criterion) {
    use fionn_simd::transform::transform_into;

    let mut group = c.benchmark_group("tape_to_tape/zero_alloc");

    for size in ["small", "medium", "large"] {
        let json = generate_json(size);
        let bytes = json.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        let options = TransformOptions::default();

        // Allocating transform
        group.bench_with_input(BenchmarkId::new("allocating", size), bytes, |b, input| {
            b.iter(|| {
                black_box(transform(
                    input,
                    FormatKind::Json,
                    FormatKind::Json,
                    &options,
                ))
            })
        });

        // Zero-allocation transform (reuse buffer)
        let mut output_buf = Vec::with_capacity(bytes.len() * 2);
        group.bench_with_input(BenchmarkId::new("zero_alloc", size), bytes, |b, input| {
            b.iter(|| {
                black_box(transform_into(
                    input,
                    FormatKind::Json,
                    FormatKind::Json,
                    &options,
                    &mut output_buf,
                ))
            })
        });
    }

    group.finish();
}

/// Benchmark: Cross-format roundtrip fidelity check
#[cfg(all(feature = "yaml", feature = "toml"))]
fn bench_roundtrip_fidelity(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_to_tape/roundtrip");

    let json = r#"{"name": "test", "version": "1.0", "enabled": true}"#;
    let bytes = json.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    let options = TransformOptions::default();

    // JSON -> YAML -> JSON roundtrip
    group.bench_function("json_yaml_json", |b| {
        b.iter(|| {
            let (yaml, _) = transform(bytes, FormatKind::Json, FormatKind::Yaml, &options).unwrap();
            let (json2, _) =
                transform(&yaml, FormatKind::Yaml, FormatKind::Json, &options).unwrap();
            black_box(json2)
        })
    });

    // JSON -> TOML -> JSON roundtrip
    group.bench_function("json_toml_json", |b| {
        b.iter(|| {
            let (toml_data, _) =
                transform(bytes, FormatKind::Json, FormatKind::Toml, &options).unwrap();
            let (json2, _) =
                transform(&toml_data, FormatKind::Toml, FormatKind::Json, &options).unwrap();
            black_box(json2)
        })
    });

    group.finish();
}

#[cfg(not(all(feature = "yaml", feature = "toml")))]
fn bench_roundtrip_fidelity(_c: &mut Criterion) {}

criterion_group!(
    tape_to_tape_benchmarks,
    bench_json_identity,
    bench_json_to_yaml,
    bench_yaml_to_json,
    bench_json_to_toml,
    bench_toml_to_json,
    bench_json_to_ison,
    bench_json_to_toon,
    bench_preparsed_transform,
    bench_pretty_transform,
    bench_zero_alloc_transform,
    bench_roundtrip_fidelity,
);

criterion_main!(tape_to_tape_benchmarks);
