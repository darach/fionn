// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Benchmarks for Optimized Merge Strategies
//!
//! This benchmark suite compares:
//! - Raw merge functions (direct, no overhead)
//! - OptimizedMergeProcessor (table lookup + merge)
//! - PreParsedValue conversion overhead
//!
//! Run with: cargo bench --bench optimized_merge

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

use fionn_core::{MergeStrategy, OperationValue};
use fionn_crdt::merge::{
    MergeTable, OptimizedMergeProcessor, PreParsedValue, merge_additive_i64, merge_lww_fast,
    merge_max_i64,
};

// =============================================================================
// Raw Merge Function Benchmarks (minimal overhead)
// =============================================================================

fn bench_raw_lww(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/raw/lww");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let pairs: Vec<_> = (0..count).map(|i| (i as u64, (i + 1) as u64)).collect();

            b.iter(|| {
                for &(local_ts, remote_ts) in &pairs {
                    black_box(merge_lww_fast(local_ts, remote_ts));
                }
            });
        });
    }
    drop(group);
}

fn bench_raw_max_i64(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/raw/max_i64");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let pairs: Vec<_> = (0..count)
                .map(|i| ((i * 2) as i64, (i * 3) as i64))
                .collect();

            b.iter(|| {
                for &(local, remote) in &pairs {
                    black_box(merge_max_i64(local, remote));
                }
            });
        });
    }
    drop(group);
}

fn bench_raw_additive_i64(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/raw/additive_i64");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let pairs: Vec<_> = (0..count).map(|i| (i as i64, (i * 2) as i64)).collect();

            b.iter(|| {
                for &(local, remote) in &pairs {
                    black_box(merge_additive_i64(local, remote));
                }
            });
        });
    }
    drop(group);
}

// =============================================================================
// OptimizedMergeProcessor Benchmarks
// =============================================================================

fn bench_processor_merge_value(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/processor/merge_value");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let mut processor = OptimizedMergeProcessor::new();
            processor.set_default_strategy(MergeStrategy::LastWriteWins);

            let paths: Vec<_> = (0..count).map(|i| format!("field_{}", i)).collect();
            let local_entries = paths.iter().enumerate().map(|(i, path)| {
                (
                    path.clone(),
                    OperationValue::NumberRef(i.to_string()),
                    i as u64,
                )
            });
            processor.init_local(local_entries);

            let remote_entries: Vec<_> = paths
                .iter()
                .enumerate()
                .map(|(i, path)| {
                    (
                        path.clone(),
                        OperationValue::NumberRef((i + 100).to_string()),
                        (i + 1) as u64,
                    )
                })
                .collect();

            b.iter(|| {
                for (path, value, ts) in &remote_entries {
                    black_box(processor.merge_value(path, value, *ts));
                }
            });
        });
    }
    drop(group);
}

fn bench_processor_merge_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/processor/merge_batch");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let mut processor = OptimizedMergeProcessor::new();
            processor.set_default_strategy(MergeStrategy::LastWriteWins);
            processor.set_path_strategy("score", MergeStrategy::Max);
            processor.set_path_strategy("count", MergeStrategy::Additive);

            let local_entries: Vec<_> = (0..count)
                .map(|i| {
                    let path = format!("field_{}", i);
                    let value = OperationValue::NumberRef(i.to_string());
                    (path, value, i as u64)
                })
                .collect();

            processor.init_local(local_entries.into_iter());

            let remote_entries: Vec<_> = (0..count)
                .map(|i| {
                    let path = format!("field_{}", i);
                    let value = OperationValue::NumberRef((i + 100).to_string());
                    (path, value, (i + 1) as u64)
                })
                .collect();

            b.iter(|| {
                black_box(processor.merge_batch(remote_entries.clone().into_iter()));
            });
        });
    }
    drop(group);
}

// =============================================================================
// PreParsedValue Conversion Benchmarks
// =============================================================================

fn bench_preparsed_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/preparsed/conversion");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let values: Vec<_> = (0..count)
                .map(|i| OperationValue::NumberRef(i.to_string()))
                .collect();

            b.iter(|| {
                for value in &values {
                    black_box(PreParsedValue::from_operation_value(value));
                }
            });
        });
    }
    drop(group);
}

fn bench_merge_table_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/table/lookup");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let mut table = MergeTable::with_capacity(count);
            let paths: Vec<_> = (0..count).map(|i| format!("field_{}", i)).collect();

            for (i, path) in paths.iter().enumerate() {
                table.add_entry(
                    path,
                    MergeStrategy::LastWriteWins,
                    &OperationValue::NumberRef(i.to_string()),
                    i as u64,
                );
            }

            b.iter(|| {
                for path in &paths {
                    black_box(table.get(path));
                }
            });
        });
    }
    drop(group);
}

// =============================================================================
// Comparison: Raw vs Processor (1000 elements)
// =============================================================================

fn bench_comparison_lww_1000(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/comparison/lww_1000");

    // Raw: direct timestamp comparison
    group.bench_function("raw", |b| {
        let pairs: Vec<_> = (0..1000).map(|i| (i as u64, (i + 1) as u64)).collect();

        b.iter(|| {
            for &(local_ts, remote_ts) in &pairs {
                black_box(merge_lww_fast(local_ts, remote_ts));
            }
        });
    });

    // Processor: includes table lookup overhead
    group.bench_function("processor", |b| {
        let mut processor = OptimizedMergeProcessor::new();
        processor.set_default_strategy(MergeStrategy::LastWriteWins);

        let paths: Vec<_> = (0..1000).map(|i| format!("field_{}", i)).collect();
        let local_entries = paths.iter().enumerate().map(|(i, path)| {
            (
                path.clone(),
                OperationValue::NumberRef(i.to_string()),
                i as u64,
            )
        });
        processor.init_local(local_entries);

        let remote_entries: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                (
                    path.clone(),
                    OperationValue::NumberRef((i + 100).to_string()),
                    (i + 1) as u64,
                )
            })
            .collect();

        b.iter(|| {
            for (path, value, ts) in &remote_entries {
                black_box(processor.merge_value(path, value, *ts));
            }
        });
    });

    drop(group);
}

fn bench_comparison_max_1000(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/comparison/max_1000");

    // Raw: direct integer comparison
    group.bench_function("raw", |b| {
        let pairs: Vec<_> = (0..1000)
            .map(|i| ((i * 2) as i64, (i * 3) as i64))
            .collect();

        b.iter(|| {
            for &(local, remote) in &pairs {
                black_box(merge_max_i64(local, remote));
            }
        });
    });

    // Processor with Max strategy
    group.bench_function("processor", |b| {
        let mut processor = OptimizedMergeProcessor::new();
        processor.set_default_strategy(MergeStrategy::Max);

        let paths: Vec<_> = (0..1000).map(|i| format!("field_{}", i)).collect();
        let local_entries = paths.iter().enumerate().map(|(i, path)| {
            (
                path.clone(),
                OperationValue::NumberRef((i * 2).to_string()),
                i as u64,
            )
        });
        processor.init_local(local_entries);

        let remote_entries: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                (
                    path.clone(),
                    OperationValue::NumberRef((i * 3).to_string()),
                    1_u64,
                )
            })
            .collect();

        b.iter(|| {
            for (path, value, ts) in &remote_entries {
                black_box(processor.merge_value(path, value, *ts));
            }
        });
    });

    drop(group);
}

// =============================================================================
// Scaling Benchmarks
// =============================================================================

fn bench_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge/scaling");
    group.sample_size(50);

    for count in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*count as u64));

        group.bench_with_input(BenchmarkId::new("raw_max", count), count, |b, &count| {
            let pairs: Vec<_> = (0..count)
                .map(|i| ((i * 2) as i64, (i * 3) as i64))
                .collect();

            b.iter(|| {
                for &(local, remote) in &pairs {
                    black_box(merge_max_i64(local, remote));
                }
            });
        });

        group.bench_with_input(
            BenchmarkId::new("processor_max", count),
            count,
            |b, &count| {
                let mut processor = OptimizedMergeProcessor::new();
                processor.set_default_strategy(MergeStrategy::Max);

                let paths: Vec<_> = (0..count).map(|i| format!("field_{}", i)).collect();
                let local_entries = paths.iter().enumerate().map(|(i, path)| {
                    (
                        path.clone(),
                        OperationValue::NumberRef((i * 2).to_string()),
                        i as u64,
                    )
                });
                processor.init_local(local_entries);

                let remote_entries: Vec<_> = paths
                    .iter()
                    .enumerate()
                    .map(|(i, path)| {
                        (
                            path.clone(),
                            OperationValue::NumberRef((i * 3).to_string()),
                            1_u64,
                        )
                    })
                    .collect();

                b.iter(|| {
                    for (path, value, ts) in &remote_entries {
                        black_box(processor.merge_value(path, value, *ts));
                    }
                });
            },
        );
    }
    drop(group);
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(
    raw_benches,
    bench_raw_lww,
    bench_raw_max_i64,
    bench_raw_additive_i64,
);

criterion_group!(
    processor_benches,
    bench_processor_merge_value,
    bench_processor_merge_batch,
    bench_preparsed_conversion,
    bench_merge_table_lookup,
);

criterion_group!(
    comparison_benches,
    bench_comparison_lww_1000,
    bench_comparison_max_1000,
    bench_scaling,
);

criterion_main!(raw_benches, processor_benches, comparison_benches);
