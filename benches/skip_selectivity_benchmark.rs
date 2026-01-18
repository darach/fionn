// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - selectivity test generators conditionally compiled
#![allow(unused)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Skip Selectivity Benchmarks
//!
//! Measures how performance scales as we skip increasing portions of content.
//! Key questions:
//! 1. What's the pure skip throughput (bytes/sec)?
//! 2. How does selectivity (% extracted) affect total time?
//! 3. How does skip position (early vs late) affect performance?
//! 4. How do different formats compare for skip operations?

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_core::{FormatKind, TapeSource};
use fionn_simd::transform::UnifiedTape;
use fionn_tape::DsonTape;

// =============================================================================
// Test Data Generation
// =============================================================================

/// Generate a flat object with N fields of specified value size
fn generate_flat_object(num_fields: usize, value_size: usize) -> String {
    let value = "x".repeat(value_size);
    let mut fields = Vec::with_capacity(num_fields);
    for i in 0..num_fields {
        fields.push(format!("\"field_{i:04}\":\"{value}\""));
    }
    format!("{{{}}}", fields.join(","))
}

/// Generate an array with N elements of specified value size
fn generate_flat_array(num_elements: usize, value_size: usize) -> String {
    let value = format!("\"{}\"", "x".repeat(value_size));
    let elements: Vec<_> = (0..num_elements).map(|_| value.clone()).collect();
    format!("[{}]", elements.join(","))
}

/// Generate nested object with specified depth and values at each level
fn generate_nested_object(depth: usize, values_per_level: usize) -> String {
    if depth == 0 {
        return "42".to_string();
    }

    let mut fields = Vec::with_capacity(values_per_level + 1);
    for i in 0..values_per_level {
        fields.push(format!("\"val_{i}\":\"data\""));
    }
    fields.push(format!(
        "\"nested\":{}",
        generate_nested_object(depth - 1, values_per_level)
    ));
    format!("{{{}}}", fields.join(","))
}

/// Generate JSON with a "needle" field at a specific position
/// Returns (json, needle_path, total_size)
fn generate_needle_in_haystack(
    needle_position_pct: usize,
    total_fields: usize,
    field_size: usize,
) -> (String, String, usize) {
    let needle_idx = (total_fields * needle_position_pct) / 100;
    let value = "x".repeat(field_size);

    let mut fields = Vec::with_capacity(total_fields);
    for i in 0..total_fields {
        if i == needle_idx {
            fields.push(format!("\"NEEDLE\":\"found_it\""));
        } else {
            fields.push(format!("\"field_{i:04}\":\"{value}\""));
        }
    }

    let json = format!("{{{}}}", fields.join(","));
    let path = format!("/NEEDLE");
    let size = json.len();
    (json, path, size)
}

// =============================================================================
// SIMD Skip Benchmarks
// =============================================================================

/// Pure skip throughput - how fast can we skip N bytes?
fn bench_pure_skip_throughput(c: &mut Criterion) {
    use fionn_simd::x86::skip::Avx2Skip;

    let skipper = Avx2Skip::new();
    let mut group = c.benchmark_group("pure_skip_throughput");

    for size in [1024, 4096, 16_384, 65_536, 262_144] {
        // Generate content to skip (object with many small fields)
        let json = generate_flat_object(size / 20, 10);
        let bytes = json.as_bytes();

        // Skip content starts after opening brace
        let skip_content = &bytes[1..];

        group.throughput(Throughput::Bytes(skip_content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("skip_object", format!("{:.0}KB", size as f64 / 1024.0)),
            skip_content,
            |b, data| {
                b.iter(|| {
                    let result = skipper.skip_object(black_box(data));
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Skip vs full parse comparison
fn bench_skip_vs_parse(c: &mut Criterion) {
    use fionn_simd::x86::skip::Avx2Skip;

    let skipper = Avx2Skip::new();
    let mut group = c.benchmark_group("skip_vs_parse");

    for num_fields in [10, 50, 100, 500, 1000] {
        let json = generate_flat_object(num_fields, 50);
        let bytes = json.as_bytes();

        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // Full parse
        group.bench_with_input(
            BenchmarkId::new("full_parse", num_fields),
            bytes,
            |b, data| {
                b.iter(|| {
                    let tape = DsonTape::parse(black_box(std::str::from_utf8(data).unwrap()));
                    black_box(tape)
                });
            },
        );

        // Skip entire value
        group.bench_with_input(
            BenchmarkId::new("skip_entire", num_fields),
            bytes,
            |b, data| {
                b.iter(|| {
                    let result = skipper.skip_value(black_box(data));
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Selectivity Benchmarks
// =============================================================================

/// Measure performance at different selectivity levels
/// (what percentage of the document do we actually read vs skip?)
fn bench_selectivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("selectivity");

    let total_fields = 100;
    let field_size = 100;
    let json = generate_flat_object(total_fields, field_size);

    // Different selectivity levels: extract 10%, 25%, 50%, 75%, 100% of fields
    for selectivity_pct in [10, 25, 50, 75, 100] {
        let fields_to_extract = (total_fields * selectivity_pct) / 100;

        group.throughput(Throughput::Elements(fields_to_extract as u64));
        group.bench_with_input(
            BenchmarkId::new("extract_pct", selectivity_pct),
            &json,
            |b, data| {
                b.iter(|| {
                    let tape = DsonTape::parse(data).unwrap();
                    let mut extracted = 0;

                    // Extract every Nth field to hit the target selectivity
                    let _step = if fields_to_extract > 0 {
                        total_fields / fields_to_extract
                    } else {
                        usize::MAX
                    };

                    let mut idx = 0;
                    while idx < tape.len() {
                        if let Some(node) = tape.node_at(idx) {
                            if extracted < fields_to_extract {
                                // "Read" this value
                                black_box(&node);
                                extracted += 1;
                            }
                            // Skip to next field
                            idx = tape.skip_value(idx).unwrap_or(idx + 1);
                        } else {
                            idx += 1;
                        }
                    }
                    black_box(extracted)
                });
            },
        );
    }

    group.finish();
}

/// Measure skip performance based on position in document
fn bench_skip_position(c: &mut Criterion) {
    use fionn_simd::x86::skip::Avx2Skip;

    let skipper = Avx2Skip::new();
    let mut group = c.benchmark_group("skip_position");

    // Generate a large document
    let total_fields = 500;
    let field_size = 100;

    for needle_pct in [10, 25, 50, 75, 90] {
        let (json, _path, _) = generate_needle_in_haystack(needle_pct, total_fields, field_size);
        let bytes = json.as_bytes();

        // Calculate approximate byte position of needle
        let approx_needle_pos = (bytes.len() * needle_pct) / 100;

        group.throughput(Throughput::Bytes(approx_needle_pos as u64));
        group.bench_with_input(
            BenchmarkId::new("needle_at_pct", needle_pct),
            bytes,
            |b, data| {
                b.iter(|| {
                    // Skip until we find NEEDLE
                    let mut pos = 1; // Skip opening brace
                    let mut found = false;

                    while pos < data.len() && !found {
                        // Skip whitespace
                        while pos < data.len() && data[pos].is_ascii_whitespace() {
                            pos += 1;
                        }

                        // Check for key
                        if pos < data.len() && data[pos] == b'"' {
                            // Check if this is our needle
                            if pos + 8 <= data.len() && &data[pos..pos + 8] == b"\"NEEDLE\"" {
                                found = true;
                                break;
                            }

                            // Skip key string
                            if let Some(result) = skipper.skip_string(&data[pos + 1..]) {
                                pos += 1 + result.consumed;
                            } else {
                                break;
                            }

                            // Skip colon and whitespace
                            while pos < data.len()
                                && (data[pos].is_ascii_whitespace() || data[pos] == b':')
                            {
                                pos += 1;
                            }

                            // Skip value
                            if let Some(result) = skipper.skip_value(&data[pos..]) {
                                pos += result.consumed;
                            } else {
                                break;
                            }

                            // Skip comma
                            while pos < data.len()
                                && (data[pos].is_ascii_whitespace() || data[pos] == b',')
                            {
                                pos += 1;
                            }
                        } else {
                            break;
                        }
                    }

                    black_box(found)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Nesting Depth Impact
// =============================================================================

/// Measure if nesting depth affects skip performance
fn bench_nesting_depth(c: &mut Criterion) {
    use fionn_simd::x86::skip::Avx2Skip;

    let skipper = Avx2Skip::new();
    let mut group = c.benchmark_group("nesting_depth");

    // Keep total size roughly constant, vary depth
    for depth in [1, 2, 5, 10, 20, 50] {
        // Adjust values per level to keep size similar
        let values_per_level = 100 / depth.max(1);
        let json = generate_nested_object(depth, values_per_level);
        let bytes = json.as_bytes();

        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::new("depth", depth), bytes, |b, data| {
            b.iter(|| {
                let result = skipper.skip_value(black_box(data));
                black_box(result)
            });
        });
    }

    group.finish();
}

/// Measure skip throughput per byte across different content types
fn bench_content_type_skip(c: &mut Criterion) {
    use fionn_simd::x86::skip::Avx2Skip;

    let skipper = Avx2Skip::new();
    let mut group = c.benchmark_group("content_type_skip");

    let size = 10_000; // ~10KB each

    // Dense object (many small fields)
    let dense_obj = generate_flat_object(500, 10);
    group.throughput(Throughput::Bytes(dense_obj.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("type", "dense_object"),
        dense_obj.as_bytes(),
        |b, data| {
            b.iter(|| black_box(skipper.skip_value(black_box(data))));
        },
    );

    // Sparse object (few large fields)
    let sparse_obj = generate_flat_object(10, size / 10);
    group.throughput(Throughput::Bytes(sparse_obj.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("type", "sparse_object"),
        sparse_obj.as_bytes(),
        |b, data| {
            b.iter(|| black_box(skipper.skip_value(black_box(data))));
        },
    );

    // Dense array
    let dense_arr = generate_flat_array(500, 10);
    group.throughput(Throughput::Bytes(dense_arr.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("type", "dense_array"),
        dense_arr.as_bytes(),
        |b, data| {
            b.iter(|| black_box(skipper.skip_value(black_box(data))));
        },
    );

    // Long string
    let long_string = format!("\"{}\"", "x".repeat(size));
    group.throughput(Throughput::Bytes(long_string.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("type", "long_string"),
        long_string.as_bytes(),
        |b, data| {
            b.iter(|| black_box(skipper.skip_value(black_box(data))));
        },
    );

    // String with escapes
    let escaped_string = format!("\"{}\"", "hello\\nworld\\t".repeat(size / 15));
    group.throughput(Throughput::Bytes(escaped_string.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("type", "escaped_string"),
        escaped_string.as_bytes(),
        |b, data| {
            b.iter(|| black_box(skipper.skip_value(black_box(data))));
        },
    );

    group.finish();
}

// =============================================================================
// TapeSource Skip Comparison
// =============================================================================

/// Compare skip_value across tape implementations
fn bench_tape_skip_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("tape_skip_comparison");

    for num_fields in [10, 50, 100, 500] {
        let json = generate_flat_object(num_fields, 50);

        // DsonTape skip
        group.bench_with_input(
            BenchmarkId::new("DsonTape", num_fields),
            &json,
            |b, data| {
                let tape = DsonTape::parse(data).unwrap();
                b.iter(|| {
                    // Skip entire root value
                    let result = tape.skip_value(0);
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Multi-Format Skip Comparison
// =============================================================================

#[cfg(feature = "yaml")]
fn generate_yaml_flat(num_fields: usize, value_size: usize) -> String {
    let value = "x".repeat(value_size);
    let mut lines = Vec::with_capacity(num_fields);
    for i in 0..num_fields {
        lines.push(format!("field_{i:04}: \"{value}\""));
    }
    lines.join("\n")
}

#[cfg(feature = "toml")]
fn generate_toml_flat(num_fields: usize, value_size: usize) -> String {
    let value = "x".repeat(value_size);
    let mut lines = Vec::with_capacity(num_fields);
    for i in 0..num_fields {
        lines.push(format!("field_{i:04} = \"{value}\""));
    }
    lines.join("\n")
}

#[cfg(all(feature = "yaml", feature = "toml"))]
fn bench_format_skip_comparison(c: &mut Criterion) {
    use fionn_simd::transform::UnifiedTape;

    let mut group = c.benchmark_group("format_skip_comparison");

    let num_fields = 100;
    let value_size = 50;

    // JSON
    let json = generate_flat_object(num_fields, value_size);
    group.throughput(Throughput::Bytes(json.len() as u64));
    group.bench_with_input(BenchmarkId::new("format", "json"), &json, |b, data| {
        let tape = DsonTape::parse(data).unwrap();
        b.iter(|| {
            let result = tape.skip_value(0);
            black_box(result)
        });
    });

    // YAML
    let yaml = generate_yaml_flat(num_fields, value_size);
    group.throughput(Throughput::Bytes(yaml.len() as u64));
    group.bench_with_input(BenchmarkId::new("format", "yaml"), &yaml, |b, data| {
        let tape = UnifiedTape::parse(data.as_bytes(), FormatKind::Yaml).unwrap();
        b.iter(|| {
            let result = tape.skip_value(0);
            black_box(result)
        });
    });

    // TOML
    let toml = generate_toml_flat(num_fields, value_size);
    group.throughput(Throughput::Bytes(toml.len() as u64));
    group.bench_with_input(BenchmarkId::new("format", "toml"), &toml, |b, data| {
        let tape = UnifiedTape::parse(data.as_bytes(), FormatKind::Toml).unwrap();
        b.iter(|| {
            let result = tape.skip_value(0);
            black_box(result)
        });
    });

    group.finish();
}

// =============================================================================
// Incremental Skip Gains
// =============================================================================

/// Measure skip gains as we skip increasing portions of a document
fn bench_incremental_skip(c: &mut Criterion) {
    let mut group = c.benchmark_group("incremental_skip");

    let total_fields = 100;
    let field_size = 100;
    let json = generate_flat_object(total_fields, field_size);

    // Baseline: traverse everything
    group.bench_with_input(BenchmarkId::new("traverse", "full"), &json, |b, data| {
        b.iter(|| {
            let tape = DsonTape::parse(data).unwrap();
            let mut count = 0;
            for i in 0..tape.len() {
                if let Some(node) = tape.node_at(i) {
                    black_box(&node);
                    count += 1;
                }
            }
            black_box(count)
        });
    });

    // Skip first N% of fields, traverse rest
    for skip_pct in [25, 50, 75, 90] {
        group.bench_with_input(
            BenchmarkId::new("skip_first", format!("{}%", skip_pct)),
            &json,
            |b, data| {
                b.iter(|| {
                    let tape = DsonTape::parse(data).unwrap();
                    let fields_to_skip = (total_fields * skip_pct) / 100;
                    let mut idx = 1; // Skip root ObjectStart
                    let mut skipped = 0;

                    // Skip first N fields
                    while idx < tape.len() && skipped < fields_to_skip {
                        idx += 1; // Skip key
                        idx = tape.skip_value(idx).unwrap_or(idx + 1); // Skip value
                        skipped += 1;
                    }

                    // Traverse the rest
                    let mut traversed = 0;
                    while idx < tape.len() {
                        if let Some(node) = tape.node_at(idx) {
                            black_box(&node);
                            traversed += 1;
                        }
                        idx += 1;
                    }

                    black_box((skipped, traversed))
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(
    skip_benchmarks,
    bench_pure_skip_throughput,
    bench_skip_vs_parse,
    bench_selectivity,
    bench_skip_position,
    bench_nesting_depth,
    bench_content_type_skip,
    bench_tape_skip_comparison,
    bench_incremental_skip,
);

#[cfg(all(feature = "yaml", feature = "toml"))]
criterion_group!(format_skip_benchmarks, bench_format_skip_comparison,);

#[cfg(all(feature = "yaml", feature = "toml"))]
criterion_main!(skip_benchmarks, format_skip_benchmarks);

#[cfg(not(all(feature = "yaml", feature = "toml")))]
criterion_main!(skip_benchmarks);
