// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - some CRDT operations conditionally compiled by features
#![allow(unused)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Comprehensive benchmarks for diff, patch, merge, and CRDT operations
//!
//! Covers:
//! - JSON Patch (RFC 6902) diff/apply
//! - JSON Merge Patch (RFC 7396)
//! - Optimized CRDT merge strategies (LWW, Max, Min, Additive)
//! - Multi-document merge operations
//! - Roundtrip performance (diff then apply)

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_core::{MergeStrategy, OperationValue};
use fionn_crdt::merge::{
    MergeTable, OptimizedMergeProcessor, PreParsedValue, StrategyBatches, Winner,
    merge_additive_i64, merge_lww_fast, merge_max_i64, merge_min_i64,
};
use fionn_diff::{
    DiffOptions, apply_patch, deep_merge, json_diff, json_diff_with_options, json_merge_patch,
    merge_many, merge_patch_to_value,
};
use serde_json::{Value, json};

// =============================================================================
// Test Data Generation
// =============================================================================

/// Generate a flat object with N fields
fn generate_flat_object(fields: usize) -> Value {
    let mut map = serde_json::Map::new();
    for i in 0..fields {
        map.insert(format!("field_{i}"), json!(format!("value_{i}")));
    }
    Value::Object(map)
}

/// Generate a nested object with depth D and breadth B
fn generate_nested_object(depth: usize, breadth: usize) -> Value {
    if depth == 0 {
        return json!("leaf_value");
    }
    let mut map = serde_json::Map::new();
    for i in 0..breadth {
        map.insert(
            format!("level_{depth}_field_{i}"),
            generate_nested_object(depth - 1, breadth),
        );
    }
    Value::Object(map)
}

/// Generate an array of objects (typical API response)
fn generate_array_of_objects(count: usize) -> Value {
    let arr: Vec<Value> = (0..count)
        .map(|i| {
            json!({
                "id": i,
                "name": format!("user_{}", i),
                "email": format!("user{}@example.com", i),
                "active": i % 2 == 0,
                "score": i * 10,
                "tags": ["tag1", "tag2", "tag3"]
            })
        })
        .collect();
    Value::Array(arr)
}

/// Generate a document with small change (single field update)
fn generate_small_change(original: &Value) -> Value {
    let mut changed = original.clone();
    if let Some(obj) = changed.as_object_mut() {
        obj.insert("field_0".to_string(), json!("changed_value"));
    }
    changed
}

/// Generate a document with many changes
fn generate_many_changes(original: &Value, change_ratio: f64) -> Value {
    let mut changed = original.clone();
    if let Some(obj) = changed.as_object_mut() {
        let change_count = (obj.len() as f64 * change_ratio) as usize;
        for i in 0..change_count {
            obj.insert(format!("field_{i}"), json!(format!("changed_{i}")));
        }
        // Add some new fields
        for i in 0..change_count / 2 {
            obj.insert(format!("new_field_{i}"), json!(i * 100));
        }
    }
    changed
}

// =============================================================================
// JSON Diff Benchmarks (RFC 6902)
// =============================================================================

fn bench_json_diff_identical(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/diff_identical");

    for fields in [10, 50, 100, 500] {
        let doc = generate_flat_object(fields);
        let json_str = serde_json::to_string(&doc).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(BenchmarkId::new("flat_object", fields), &doc, |b, doc| {
            b.iter(|| black_box(json_diff(doc, doc)))
        });
    }

    group.finish();
}

fn bench_json_diff_small_change(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/diff_small_change");

    for fields in [10, 50, 100, 500] {
        let original = generate_flat_object(fields);
        let changed = generate_small_change(&original);
        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("flat_object", fields),
            &(&original, &changed),
            |b, (original, changed)| b.iter(|| black_box(json_diff(original, changed))),
        );
    }

    group.finish();
}

fn bench_json_diff_many_changes(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/diff_many_changes");

    for fields in [50, 100, 500] {
        let original = generate_flat_object(fields);
        let changed = generate_many_changes(&original, 0.5);
        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("50pct_changed", fields),
            &(&original, &changed),
            |b, (original, changed)| b.iter(|| black_box(json_diff(original, changed))),
        );
    }

    group.finish();
}

fn bench_json_diff_nested(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/diff_nested");

    for depth in [3, 5, 7] {
        let original = generate_nested_object(depth, 3);
        // Change a deep leaf
        let mut changed = original.clone();
        fn modify_deep(v: &mut Value, remaining: usize) {
            if remaining == 0 {
                *v = json!("modified_leaf");
                return;
            }
            if let Some(obj) = v.as_object_mut() {
                if let Some((_, first_val)) = obj.iter_mut().next() {
                    modify_deep(first_val, remaining - 1);
                }
            }
        }
        modify_deep(&mut changed, depth);

        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("depth", depth),
            &(&original, &changed),
            |b, (original, changed)| b.iter(|| black_box(json_diff(original, changed))),
        );
    }

    group.finish();
}

fn bench_json_diff_array(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/diff_array");

    for count in [10, 50, 100] {
        let original = generate_array_of_objects(count);
        // Modify middle element
        let mut changed = original.clone();
        if let Some(arr) = changed.as_array_mut() {
            if let Some(elem) = arr.get_mut(count / 2) {
                if let Some(obj) = elem.as_object_mut() {
                    obj.insert("name".to_string(), json!("MODIFIED"));
                }
            }
        }

        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        // Simple diff
        group.bench_with_input(
            BenchmarkId::new("simple", count),
            &(&original, &changed),
            |b, (original, changed)| b.iter(|| black_box(json_diff(original, changed))),
        );

        // Optimized diff (LCS)
        let opts = DiffOptions::default().with_array_optimization();
        group.bench_with_input(
            BenchmarkId::new("optimized_lcs", count),
            &(&original, &changed, &opts),
            |b, (original, changed, opts)| {
                b.iter(|| black_box(json_diff_with_options(original, changed, opts)))
            },
        );
    }

    group.finish();
}

// =============================================================================
// JSON Patch Apply Benchmarks
// =============================================================================

fn bench_patch_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/patch_apply");

    for fields in [10, 50, 100, 500] {
        let original = generate_flat_object(fields);
        let changed = generate_many_changes(&original, 0.3);
        let patch = json_diff(&original, &changed);

        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("30pct_changes", fields),
            &(&original, &patch),
            |b, (original, patch)| b.iter(|| black_box(apply_patch(original, patch))),
        );
    }

    group.finish();
}

fn bench_diff_patch_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/roundtrip");

    for fields in [10, 50, 100] {
        let original = generate_flat_object(fields);
        let target = generate_many_changes(&original, 0.5);

        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("diff_then_apply", fields),
            &(&original, &target),
            |b, (original, target)| {
                b.iter(|| {
                    let patch = json_diff(original, target);
                    black_box(apply_patch(original, &patch))
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// JSON Merge Patch Benchmarks (RFC 7396)
// =============================================================================

fn bench_merge_patch_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/merge_patch_apply");

    for fields in [10, 50, 100, 500] {
        let original = generate_flat_object(fields);
        // Create a merge patch (modify some, add some, remove some)
        let patch = json!({
            "field_0": "changed",
            "field_1": null,  // delete
            "new_field": "added"
        });

        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("small_patch", fields),
            &(&original, &patch),
            |b, (original, patch)| b.iter(|| black_box(json_merge_patch(original, patch))),
        );
    }

    group.finish();
}

fn bench_merge_patch_generate(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/merge_patch_generate");

    for fields in [10, 50, 100] {
        let original = generate_flat_object(fields);
        let target = generate_many_changes(&original, 0.3);

        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("30pct_changed", fields),
            &(&original, &target),
            |b, (original, target)| b.iter(|| black_box(merge_patch_to_value(original, target))),
        );
    }

    group.finish();
}

fn bench_merge_many(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/merge_many");

    for doc_count in [2, 5, 10, 20] {
        let docs: Vec<Value> = (0..doc_count)
            .map(|i| {
                json!({
                    format!("field_{}", i): format!("value_{}", i),
                    "shared": format!("from_doc_{}", i)
                })
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("documents", doc_count),
            &docs,
            |b, docs| b.iter(|| black_box(merge_many(docs))),
        );
    }

    group.finish();
}

fn bench_deep_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/deep_merge");

    for depth in [3, 5, 7] {
        let base = generate_nested_object(depth, 2);
        let overlay = generate_nested_object(depth, 2);

        let json_str = serde_json::to_string(&base).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("depth", depth),
            &(&base, &overlay),
            |b, (base, overlay)| b.iter(|| black_box(deep_merge(base, overlay))),
        );
    }

    group.finish();
}

// =============================================================================
// CRDT Merge Strategy Benchmarks
// =============================================================================

fn bench_crdt_lww_fast(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/crdt_lww");

    // Single LWW comparison
    group.bench_function("single_comparison", |b| {
        b.iter(|| black_box(merge_lww_fast(100, 200)))
    });

    // Batched LWW
    for batch_size in [10, 100, 1000] {
        let mut batches = StrategyBatches::new();
        for i in 0..batch_size {
            batches.lww.push((i as u64, 100 + i as u64, 200 + i as u64));
        }

        group.bench_with_input(
            BenchmarkId::new("batch", batch_size),
            &batches,
            |b, batches| b.iter(|| black_box(batches.process_lww_batch())),
        );
    }

    group.finish();
}

fn bench_crdt_max_min(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/crdt_max_min");

    // Single max/min
    group.bench_function("max_i64_single", |b| {
        b.iter(|| black_box(merge_max_i64(100, 200)))
    });

    group.bench_function("min_i64_single", |b| {
        b.iter(|| black_box(merge_min_i64(100, 200)))
    });

    // Batched max
    for batch_size in [10, 100, 1000] {
        let mut batches = StrategyBatches::new();
        for i in 0..batch_size {
            batches.max_numeric.push((
                i as u64,
                PreParsedValue::Integer(i as i64 * 10),
                PreParsedValue::Integer(i as i64 * 15),
            ));
        }

        group.bench_with_input(
            BenchmarkId::new("max_batch", batch_size),
            &batches,
            |b, batches| b.iter(|| black_box(batches.process_max_batch())),
        );
    }

    group.finish();
}

fn bench_crdt_additive(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/crdt_additive");

    // Single additive
    group.bench_function("additive_i64_single", |b| {
        b.iter(|| black_box(merge_additive_i64(100, 200)))
    });

    // Batched additive
    for batch_size in [10, 100, 1000] {
        let mut batches = StrategyBatches::new();
        for i in 0..batch_size {
            batches.additive_numeric.push((
                i as u64,
                PreParsedValue::Integer(i as i64),
                PreParsedValue::Integer(i as i64 * 2),
            ));
        }

        group.bench_with_input(
            BenchmarkId::new("batch", batch_size),
            &batches,
            |b, batches| b.iter(|| black_box(batches.process_additive_batch())),
        );
    }

    group.finish();
}

fn bench_crdt_merge_table(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/crdt_merge_table");

    for entry_count in [10, 100, 1000] {
        // Build table
        let mut table = MergeTable::with_capacity(entry_count);
        for i in 0..entry_count {
            table.add_entry(
                &format!("path.to.field_{i}"),
                MergeStrategy::LastWriteWins,
                &OperationValue::NumberRef(i.to_string()),
                i as u64,
            );
        }

        // Lookup benchmark
        group.bench_with_input(
            BenchmarkId::new("lookup", entry_count),
            &table,
            |b, table| {
                b.iter(|| {
                    // Lookup middle entry
                    black_box(table.get(&format!("path.to.field_{}", entry_count / 2)))
                })
            },
        );

        // Iteration benchmark
        group.bench_with_input(
            BenchmarkId::new("iterate", entry_count),
            &table,
            |b, table| {
                b.iter(|| {
                    let count = table.iter().count();
                    black_box(count)
                })
            },
        );
    }

    group.finish();
}

fn bench_crdt_optimized_processor(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/crdt_processor");

    for field_count in [10, 50, 100] {
        // Setup processor with local values
        let mut processor = OptimizedMergeProcessor::new();
        processor.set_default_strategy(MergeStrategy::LastWriteWins);

        let local_entries: Vec<_> = (0..field_count)
            .map(|i| {
                (
                    format!("field_{i}"),
                    OperationValue::NumberRef(i.to_string()),
                    100u64,
                )
            })
            .collect();

        processor.init_local(local_entries.into_iter());

        // Merge single value
        group.bench_with_input(
            BenchmarkId::new("merge_single", field_count),
            &processor,
            |b, processor| {
                b.iter(|| {
                    black_box(processor.merge_value(
                        "field_0",
                        &OperationValue::NumberRef("999".to_string()),
                        200,
                    ))
                })
            },
        );

        // Batch merge
        let remote_entries: Vec<_> = (0..field_count)
            .map(|i| {
                (
                    format!("field_{i}"),
                    OperationValue::NumberRef((i * 2).to_string()),
                    200u64,
                )
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("merge_batch", field_count),
            &(&processor, &remote_entries),
            |b, (processor, entries)| {
                b.iter(|| black_box(processor.merge_batch(entries.iter().cloned())))
            },
        );

        // Parallel batch merge
        group.bench_with_input(
            BenchmarkId::new("merge_parallel", field_count),
            &(&processor, &remote_entries),
            |b, (processor, entries)| b.iter(|| black_box(processor.merge_batch_parallel(entries))),
        );
    }

    group.finish();
}

// =============================================================================
// Pre-Parsed Value Benchmarks
// =============================================================================

fn bench_preparsed_value(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/preparsed_value");

    // Integer parsing
    group.bench_function("parse_integer", |b| {
        let op = OperationValue::NumberRef("12345".to_string());
        b.iter(|| black_box(PreParsedValue::from_operation_value(&op)))
    });

    // Float parsing
    group.bench_function("parse_float", |b| {
        let op = OperationValue::NumberRef("123.456".to_string());
        b.iter(|| black_box(PreParsedValue::from_operation_value(&op)))
    });

    // String (no parsing)
    group.bench_function("parse_string", |b| {
        let op = OperationValue::StringRef("hello world".to_string());
        b.iter(|| black_box(PreParsedValue::from_operation_value(&op)))
    });

    // Numeric conversion
    group.bench_function("as_i64", |b| {
        let pre = PreParsedValue::Integer(12345);
        b.iter(|| black_box(pre.as_i64()))
    });

    group.bench_function("as_f64", |b| {
        let pre = PreParsedValue::Float(123.456);
        b.iter(|| black_box(pre.as_f64()))
    });

    group.finish();
}

// =============================================================================
// Comparison: json-patch crate vs fionn
// =============================================================================

fn bench_comparison_json_patch_crate(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_patch_merge/comparison");

    for fields in [10, 50, 100] {
        let original = generate_flat_object(fields);
        let target = generate_many_changes(&original, 0.3);

        let json_str = serde_json::to_string(&original).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));

        // fionn diff
        group.bench_with_input(
            BenchmarkId::new("fionn_diff", fields),
            &(&original, &target),
            |b, (original, target)| b.iter(|| black_box(json_diff(original, target))),
        );

        // json-patch crate diff
        group.bench_with_input(
            BenchmarkId::new("jsonpatch_diff", fields),
            &(&original, &target),
            |b, (original, target)| b.iter(|| black_box(json_patch::diff(original, target))),
        );
    }

    group.finish();
}

criterion_group!(
    diff_patch_merge_benchmarks,
    // Diff benchmarks
    bench_json_diff_identical,
    bench_json_diff_small_change,
    bench_json_diff_many_changes,
    bench_json_diff_nested,
    bench_json_diff_array,
    // Patch benchmarks
    bench_patch_apply,
    bench_diff_patch_roundtrip,
    // Merge patch benchmarks
    bench_merge_patch_apply,
    bench_merge_patch_generate,
    bench_merge_many,
    bench_deep_merge,
    // CRDT benchmarks
    bench_crdt_lww_fast,
    bench_crdt_max_min,
    bench_crdt_additive,
    bench_crdt_merge_table,
    bench_crdt_optimized_processor,
    bench_preparsed_value,
    // Comparison
    bench_comparison_json_patch_crate,
);

criterion_main!(diff_patch_merge_benchmarks);
