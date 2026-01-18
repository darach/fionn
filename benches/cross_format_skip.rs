// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - format-specific generators/benches conditionally compiled by features
#![allow(unused)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Cross-format skip selectivity benchmarks
//!
//! Compares skip performance across all supported formats to understand
//! format-specific overhead and optimization opportunities.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_core::{FormatKind, TapeSource};
use fionn_tape::DsonTape;

/// Generate equivalent test data in multiple formats
fn generate_test_data_json(fields: usize) -> String {
    let mut json = String::from("{");
    for i in 0..fields {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            "\"field_{}\": {{\"id\": {}, \"name\": \"value_{}\", \"active\": {}, \"score\": {}.{}}}",
            i,
            i,
            i,
            i % 2 == 0,
            i * 10,
            i
        ));
    }
    json.push('}');
    json
}

#[cfg(feature = "yaml")]
fn generate_test_data_yaml(fields: usize) -> String {
    let mut yaml = String::new();
    for i in 0..fields {
        yaml.push_str(&format!(
            "field_{}:\n  id: {}\n  name: value_{}\n  active: {}\n  score: {}.{}\n",
            i,
            i,
            i,
            i % 2 == 0,
            i * 10,
            i
        ));
    }
    yaml
}

#[cfg(feature = "toml")]
fn generate_test_data_toml(fields: usize) -> String {
    let mut toml = String::new();
    for i in 0..fields {
        toml.push_str(&format!(
            "[field_{}]\nid = {}\nname = \"value_{}\"\nactive = {}\nscore = {}.{}\n\n",
            i,
            i,
            i,
            i % 2 == 0,
            i * 10,
            i
        ));
    }
    toml
}

#[cfg(feature = "csv")]
fn generate_test_data_csv(rows: usize) -> String {
    let mut csv = String::from("id,name,active,score\n");
    for i in 0..rows {
        csv.push_str(&format!(
            "{},value_{},{},{}.{}\n",
            i,
            i,
            i % 2 == 0,
            i * 10,
            i
        ));
    }
    csv
}

/// Benchmark: JSON skip at various selectivity levels
fn bench_json_skip_selectivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format_skip/json");

    for fields in [10, 50, 100, 500] {
        let json = generate_test_data_json(fields);
        group.throughput(Throughput::Bytes(json.len() as u64));

        // Parse once
        let tape = DsonTape::parse(&json).unwrap();

        // Skip 25% (first quarter)
        let skip_target = fields / 4;
        group.bench_with_input(
            BenchmarkId::new("skip_25pct", fields),
            &(&tape, skip_target),
            |b, (tape, target)| {
                b.iter(|| {
                    let mut idx = 0;
                    let mut skipped = 0;
                    while idx < tape.len() && skipped < *target {
                        if let Ok(next) = tape.skip_value(idx) {
                            idx = next;
                            skipped += 1;
                        } else {
                            break;
                        }
                    }
                    black_box(idx)
                })
            },
        );

        // Skip 50%
        let skip_target = fields / 2;
        group.bench_with_input(
            BenchmarkId::new("skip_50pct", fields),
            &(&tape, skip_target),
            |b, (tape, target)| {
                b.iter(|| {
                    let mut idx = 0;
                    let mut skipped = 0;
                    while idx < tape.len() && skipped < *target {
                        if let Ok(next) = tape.skip_value(idx) {
                            idx = next;
                            skipped += 1;
                        } else {
                            break;
                        }
                    }
                    black_box(idx)
                })
            },
        );

        // Skip 75%
        let skip_target = (fields * 3) / 4;
        group.bench_with_input(
            BenchmarkId::new("skip_75pct", fields),
            &(&tape, skip_target),
            |b, (tape, target)| {
                b.iter(|| {
                    let mut idx = 0;
                    let mut skipped = 0;
                    while idx < tape.len() && skipped < *target {
                        if let Ok(next) = tape.skip_value(idx) {
                            idx = next;
                            skipped += 1;
                        } else {
                            break;
                        }
                    }
                    black_box(idx)
                })
            },
        );
    }

    group.finish();
}

/// Benchmark: YAML skip selectivity (feature-gated)
#[cfg(feature = "yaml")]
fn bench_yaml_skip_selectivity(c: &mut Criterion) {
    use fionn_simd::transform::UnifiedTape;

    let mut group = c.benchmark_group("cross_format_skip/yaml");

    for fields in [10, 50, 100] {
        let yaml = generate_test_data_yaml(fields);
        group.throughput(Throughput::Bytes(yaml.len() as u64));

        if let Ok(tape) = UnifiedTape::parse(yaml.as_bytes(), FormatKind::Yaml) {
            let skip_target = fields / 2;
            group.bench_with_input(
                BenchmarkId::new("skip_50pct", fields),
                &(&tape, skip_target),
                |b, (tape, target)| {
                    b.iter(|| {
                        let mut idx = 0;
                        let mut skipped = 0;
                        while idx < tape.len() && skipped < *target {
                            if let Ok(next) = tape.skip_value(idx) {
                                idx = next;
                                skipped += 1;
                            } else {
                                break;
                            }
                        }
                        black_box(idx)
                    })
                },
            );
        }
    }

    group.finish();
}

#[cfg(not(feature = "yaml"))]
fn bench_yaml_skip_selectivity(_c: &mut Criterion) {}

/// Benchmark: TOML skip selectivity (feature-gated)
#[cfg(feature = "toml")]
fn bench_toml_skip_selectivity(c: &mut Criterion) {
    use fionn_simd::transform::UnifiedTape;

    let mut group = c.benchmark_group("cross_format_skip/toml");

    for fields in [10, 50, 100] {
        let toml_data = generate_test_data_toml(fields);
        group.throughput(Throughput::Bytes(toml_data.len() as u64));

        if let Ok(tape) = UnifiedTape::parse(toml_data.as_bytes(), FormatKind::Toml) {
            let skip_target = fields / 2;
            group.bench_with_input(
                BenchmarkId::new("skip_50pct", fields),
                &(&tape, skip_target),
                |b, (tape, target)| {
                    b.iter(|| {
                        let mut idx = 0;
                        let mut skipped = 0;
                        while idx < tape.len() && skipped < *target {
                            if let Ok(next) = tape.skip_value(idx) {
                                idx = next;
                                skipped += 1;
                            } else {
                                break;
                            }
                        }
                        black_box(idx)
                    })
                },
            );
        }
    }

    group.finish();
}

#[cfg(not(feature = "toml"))]
fn bench_toml_skip_selectivity(_c: &mut Criterion) {}

/// Benchmark: CSV row skip (feature-gated)
#[cfg(feature = "csv")]
fn bench_csv_skip_selectivity(c: &mut Criterion) {
    use fionn_simd::transform::UnifiedTape;

    let mut group = c.benchmark_group("cross_format_skip/csv");

    for rows in [100, 500, 1000] {
        let csv = generate_test_data_csv(rows);
        group.throughput(Throughput::Bytes(csv.len() as u64));

        if let Ok(tape) = UnifiedTape::parse(csv.as_bytes(), FormatKind::Csv) {
            // Skip 50% of rows
            let skip_target = rows / 2;
            group.bench_with_input(
                BenchmarkId::new("skip_50pct_rows", rows),
                &(&tape, skip_target),
                |b, (tape, target)| {
                    b.iter(|| {
                        let mut idx = 0;
                        let mut skipped = 0;
                        while idx < tape.len() && skipped < *target {
                            if let Ok(next) = tape.skip_value(idx) {
                                idx = next;
                                skipped += 1;
                            } else {
                                break;
                            }
                        }
                        black_box(idx)
                    })
                },
            );
        }
    }

    group.finish();
}

#[cfg(not(feature = "csv"))]
fn bench_csv_skip_selectivity(_c: &mut Criterion) {}

/// Benchmark: Compare skip cost per byte across formats
fn bench_skip_cost_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format_skip/cost_per_byte");

    // Generate ~10KB of data in each format
    let json = generate_test_data_json(100);

    #[cfg(feature = "yaml")]
    let yaml = generate_test_data_yaml(100);

    #[cfg(feature = "toml")]
    let toml_data = generate_test_data_toml(100);

    // JSON baseline
    group.throughput(Throughput::Bytes(json.len() as u64));
    let json_tape = DsonTape::parse(&json).unwrap();
    group.bench_function("json", |b| {
        b.iter(|| {
            let mut idx = 0;
            while idx < json_tape.len() {
                if let Ok(next) = json_tape.skip_value(idx) {
                    if next == idx {
                        break;
                    }
                    idx = next;
                } else {
                    break;
                }
            }
            black_box(idx)
        })
    });

    #[cfg(feature = "yaml")]
    {
        use fionn_simd::transform::UnifiedTape;
        if let Ok(yaml_tape) = UnifiedTape::parse(yaml.as_bytes(), FormatKind::Yaml) {
            group.throughput(Throughput::Bytes(yaml.len() as u64));
            group.bench_function("yaml", |b| {
                b.iter(|| {
                    let mut idx = 0;
                    while idx < yaml_tape.len() {
                        if let Ok(next) = yaml_tape.skip_value(idx) {
                            if next == idx {
                                break;
                            }
                            idx = next;
                        } else {
                            break;
                        }
                    }
                    black_box(idx)
                })
            });
        }
    }

    #[cfg(feature = "toml")]
    {
        use fionn_simd::transform::UnifiedTape;
        if let Ok(toml_tape) = UnifiedTape::parse(toml_data.as_bytes(), FormatKind::Toml) {
            group.throughput(Throughput::Bytes(toml_data.len() as u64));
            group.bench_function("toml", |b| {
                b.iter(|| {
                    let mut idx = 0;
                    while idx < toml_tape.len() {
                        if let Ok(next) = toml_tape.skip_value(idx) {
                            if next == idx {
                                break;
                            }
                            idx = next;
                        } else {
                            break;
                        }
                    }
                    black_box(idx)
                })
            });
        }
    }

    group.finish();
}

/// Benchmark: Skip performance on nested structures
fn bench_nested_skip(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format_skip/nested");

    // Generate deeply nested JSON
    fn nested_json(depth: usize) -> String {
        let mut s = String::new();
        for i in 0..depth {
            s.push_str(&format!("{{\"level_{}\": ", i));
        }
        s.push_str("\"deep_value\"");
        for _ in 0..depth {
            s.push('}');
        }
        s
    }

    for depth in [5, 10, 20, 50] {
        let json = nested_json(depth);
        group.throughput(Throughput::Bytes(json.len() as u64));

        let tape = DsonTape::parse(&json).unwrap();
        group.bench_with_input(BenchmarkId::new("json_depth", depth), &tape, |b, tape| {
            b.iter(|| {
                // Skip from root (should skip entire nested structure)
                black_box(tape.skip_value(0))
            })
        });
    }

    group.finish();
}

/// Benchmark: Skip performance on arrays vs objects
fn bench_structure_type_skip(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format_skip/structure_type");

    // Large array
    let array_json: String = format!(
        "[{}]",
        (0..1000)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    // Large object
    let object_json: String = format!(
        "{{{}}}",
        (0..1000)
            .map(|i| format!("\"k{}\": {}", i, i))
            .collect::<Vec<_>>()
            .join(",")
    );

    // Array benchmark
    group.throughput(Throughput::Bytes(array_json.len() as u64));
    let array_tape = DsonTape::parse(&array_json).unwrap();
    group.bench_function("array_1000", |b| {
        b.iter(|| black_box(array_tape.skip_value(0)))
    });

    // Object benchmark
    group.throughput(Throughput::Bytes(object_json.len() as u64));
    let object_tape = DsonTape::parse(&object_json).unwrap();
    group.bench_function("object_1000", |b| {
        b.iter(|| black_box(object_tape.skip_value(0)))
    });

    group.finish();
}

/// Benchmark: String-heavy vs number-heavy content
fn bench_content_type_skip(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_format_skip/content_type");

    // String-heavy (long strings)
    let string_json = format!(
        "{{{}}}",
        (0..100)
            .map(|i| format!("\"k{}\": \"{}\"", i, "x".repeat(100)))
            .collect::<Vec<_>>()
            .join(",")
    );

    // Number-heavy
    let number_json = format!(
        "{{{}}}",
        (0..100)
            .map(|i| format!("\"k{}\": {}.{}", i, i * 12345, i))
            .collect::<Vec<_>>()
            .join(",")
    );

    // Boolean-heavy
    let bool_json = format!(
        "{{{}}}",
        (0..100)
            .map(|i| format!("\"k{}\": {}", i, i % 2 == 0))
            .collect::<Vec<_>>()
            .join(",")
    );

    group.throughput(Throughput::Bytes(string_json.len() as u64));
    let string_tape = DsonTape::parse(&string_json).unwrap();
    group.bench_function("string_heavy", |b| {
        b.iter(|| black_box(string_tape.skip_value(0)))
    });

    group.throughput(Throughput::Bytes(number_json.len() as u64));
    let number_tape = DsonTape::parse(&number_json).unwrap();
    group.bench_function("number_heavy", |b| {
        b.iter(|| black_box(number_tape.skip_value(0)))
    });

    group.throughput(Throughput::Bytes(bool_json.len() as u64));
    let bool_tape = DsonTape::parse(&bool_json).unwrap();
    group.bench_function("bool_heavy", |b| {
        b.iter(|| black_box(bool_tape.skip_value(0)))
    });

    group.finish();
}

criterion_group!(
    cross_format_benchmarks,
    bench_json_skip_selectivity,
    bench_yaml_skip_selectivity,
    bench_toml_skip_selectivity,
    bench_csv_skip_selectivity,
    bench_skip_cost_comparison,
    bench_nested_skip,
    bench_structure_type_skip,
    bench_content_type_skip,
);

criterion_main!(cross_format_benchmarks);
