// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Benchmarks comparing pooled vs non-pooled and zero-copy vs allocating APIs.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_diff::{json_diff, json_diff_zerocopy};
use fionn_gron::{GronOptions, gron, gron_zerocopy};
use fionn_pool::{PoolStrategy, SharedPool, TapePool, ThreadLocalPool};
use serde_json::json;
use std::hint::black_box;
use std::sync::Arc;

// =============================================================================
// Test Data
// =============================================================================

const fn small_json() -> &'static str {
    r#"{"name": "Alice", "age": 30}"#
}

const fn medium_json() -> &'static str {
    r#"{
        "users": [
            {"name": "Alice", "age": 30, "email": "alice@example.com"},
            {"name": "Bob", "age": 25, "email": "bob@example.com"},
            {"name": "Charlie", "age": 35, "email": "charlie@example.com"}
        ],
        "metadata": {
            "version": "1.0",
            "generated": "2024-01-01T00:00:00Z",
            "count": 3
        }
    }"#
}

fn large_json() -> String {
    let mut users = Vec::new();
    for i in 0..100 {
        users.push(format!(
            r#"{{"id": {}, "name": "User{}", "email": "user{}@example.com", "active": true, "score": {}}}"#,
            i, i, i, i * 10
        ));
    }
    format!(r#"{{"users": [{}], "total": 100}}"#, users.join(","))
}

// =============================================================================
// Pool Benchmarks
// =============================================================================

fn bench_pool_acquire_release(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_acquire_release");

    // Thread-local pool
    group.bench_function("thread_local_unbounded", |b| {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        b.iter(|| {
            let buf = pool.acquire(1024);
            pool.release(buf);
        });
    });

    group.bench_function("thread_local_size_limited", |b| {
        let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 16 });
        b.iter(|| {
            let buf = pool.acquire(1024);
            pool.release(buf);
        });
    });

    group.bench_function("thread_local_lru", |b| {
        let pool = ThreadLocalPool::new(PoolStrategy::Lru { max_tapes: 16 });
        b.iter(|| {
            let buf = pool.acquire(1024);
            pool.release(buf);
        });
    });

    // Shared pool
    group.bench_function("shared_unbounded", |b| {
        let pool = SharedPool::new(PoolStrategy::Unbounded);
        b.iter(|| {
            let buf = pool.acquire(1024);
            pool.release(buf);
        });
    });

    group.bench_function("shared_size_limited", |b| {
        let pool = SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 16 });
        b.iter(|| {
            let buf = pool.acquire(1024);
            pool.release(buf);
        });
    });

    // Direct allocation (no pool)
    group.bench_function("direct_allocation", |b| {
        b.iter(|| {
            let buf = Vec::<u8>::with_capacity(1024);
            black_box(buf);
        });
    });

    group.finish();
}

fn bench_pool_reuse_pattern(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_reuse_pattern");

    // Simulate realistic usage pattern
    group.bench_function("thread_local_burst", |b| {
        let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 8 });
        b.iter(|| {
            // Acquire several buffers, use them, release
            let bufs: Vec<_> = (0..4).map(|_| pool.acquire(1024)).collect();
            for buf in bufs {
                pool.release(buf);
            }
        });
    });

    group.bench_function("shared_concurrent_simulation", |b| {
        let pool = Arc::new(SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 16 }));
        b.iter(|| {
            // Simulate concurrent access pattern
            let buf1 = pool.acquire(1024);
            let buf2 = pool.acquire(2048);
            pool.release(buf1);
            let buf3 = pool.acquire(1024);
            pool.release(buf2);
            pool.release(buf3);
        });
    });

    group.finish();
}

// =============================================================================
// Gron Zero-Copy Benchmarks
// =============================================================================

fn bench_gron_zerocopy(c: &mut Criterion) {
    let mut group = c.benchmark_group("gron_zerocopy");

    let small = small_json();
    let medium = medium_json();
    let large = large_json();

    // Small JSON
    group.throughput(Throughput::Bytes(small.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("allocating", "small"),
        &small,
        |b, json| {
            b.iter(|| gron(black_box(json), &GronOptions::default()).unwrap());
        },
    );

    group.bench_with_input(BenchmarkId::new("zerocopy", "small"), &small, |b, json| {
        b.iter(|| gron_zerocopy(black_box(json), "json").unwrap());
    });

    // Medium JSON
    group.throughput(Throughput::Bytes(medium.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("allocating", "medium"),
        &medium,
        |b, json| {
            b.iter(|| gron(black_box(json), &GronOptions::default()).unwrap());
        },
    );

    group.bench_with_input(
        BenchmarkId::new("zerocopy", "medium"),
        &medium,
        |b, json| {
            b.iter(|| gron_zerocopy(black_box(json), "json").unwrap());
        },
    );

    // Large JSON
    group.throughput(Throughput::Bytes(large.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("allocating", "large"),
        &large,
        |b, json| {
            b.iter(|| gron(black_box(json), &GronOptions::default()).unwrap());
        },
    );

    group.bench_with_input(BenchmarkId::new("zerocopy", "large"), &large, |b, json| {
        b.iter(|| gron_zerocopy(black_box(json), "json").unwrap());
    });

    group.finish();
}

fn bench_gron_output_formats(c: &mut Criterion) {
    let mut group = c.benchmark_group("gron_output_formats");

    let medium = medium_json();

    group.bench_function("zerocopy_to_standard", |b| {
        let output = gron_zerocopy(medium, "json").unwrap();
        b.iter(|| black_box(output.to_standard_string()));
    });

    group.bench_function("zerocopy_to_compact", |b| {
        let output = gron_zerocopy(medium, "json").unwrap();
        b.iter(|| black_box(output.to_compact_string()));
    });

    group.bench_function("zerocopy_iterate", |b| {
        let output = gron_zerocopy(medium, "json").unwrap();
        b.iter(|| {
            let mut count = 0;
            for line in &output {
                count += line.path.len() + line.value.len();
            }
            black_box(count)
        });
    });

    group.finish();
}

// =============================================================================
// Diff Zero-Copy Benchmarks
// =============================================================================

fn bench_diff_zerocopy(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_zerocopy");

    // Identical documents
    let doc1 = json!({"name": "Alice", "age": 30, "active": true});
    let doc1_clone = doc1.clone();

    group.bench_function("identical/allocating", |b| {
        b.iter(|| json_diff(black_box(&doc1), black_box(&doc1_clone)));
    });

    group.bench_function("identical/zerocopy", |b| {
        b.iter(|| json_diff_zerocopy(black_box(&doc1), black_box(&doc1_clone)));
    });

    // Small change
    let source = json!({"name": "Alice", "age": 30});
    let target = json!({"name": "Alice", "age": 31});

    group.bench_function("small_change/allocating", |b| {
        b.iter(|| json_diff(black_box(&source), black_box(&target)));
    });

    group.bench_function("small_change/zerocopy", |b| {
        b.iter(|| json_diff_zerocopy(black_box(&source), black_box(&target)));
    });

    // Add field
    let source2 = json!({"name": "Alice"});
    let target2 = json!({"name": "Alice", "email": "alice@example.com"});

    group.bench_function("add_field/allocating", |b| {
        b.iter(|| json_diff(black_box(&source2), black_box(&target2)));
    });

    group.bench_function("add_field/zerocopy", |b| {
        b.iter(|| json_diff_zerocopy(black_box(&source2), black_box(&target2)));
    });

    // Nested change
    let source3 = json!({"user": {"name": "Alice", "profile": {"bio": "Hello"}}});
    let target3 = json!({"user": {"name": "Alice", "profile": {"bio": "Updated"}}});

    group.bench_function("nested_change/allocating", |b| {
        b.iter(|| json_diff(black_box(&source3), black_box(&target3)));
    });

    group.bench_function("nested_change/zerocopy", |b| {
        b.iter(|| json_diff_zerocopy(black_box(&source3), black_box(&target3)));
    });

    // Array diff
    let source4 = json!({"items": [1, 2, 3]});
    let target4 = json!({"items": [1, 2, 3, 4, 5]});

    group.bench_function("array_change/allocating", |b| {
        b.iter(|| json_diff(black_box(&source4), black_box(&target4)));
    });

    group.bench_function("array_change/zerocopy", |b| {
        b.iter(|| json_diff_zerocopy(black_box(&source4), black_box(&target4)));
    });

    group.finish();
}

fn bench_diff_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_conversion");

    let source = json!({"a": 1, "b": 2, "c": {"d": 3}});
    let target = json!({"a": 1, "b": 20, "c": {"d": 30}, "e": 4});

    let patch_ref = json_diff_zerocopy(&source, &target);

    group.bench_function("zerocopy_into_owned", |b| {
        b.iter(|| {
            let p = patch_ref.clone();
            black_box(p.into_owned())
        });
    });

    group.bench_function("zerocopy_to_json_patch", |b| {
        b.iter(|| black_box(patch_ref.to_json_patch()));
    });

    group.bench_function("zerocopy_iterate", |b| {
        b.iter(|| {
            let mut count = 0;
            for op in &patch_ref {
                count += op.path().len();
            }
            black_box(count)
        });
    });

    group.finish();
}

// =============================================================================
// Combined Benchmarks
// =============================================================================

fn bench_realistic_workflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_workflow");

    let json_data = medium_json();

    // Workflow: Parse -> Gron -> Process output
    group.bench_function("gron_allocating_workflow", |b| {
        b.iter(|| {
            let output = gron(json_data, &GronOptions::default()).unwrap();
            // Simulate processing
            let count = output.lines().count();
            black_box(count)
        });
    });

    group.bench_function("gron_zerocopy_workflow", |b| {
        b.iter(|| {
            let output = gron_zerocopy(json_data, "json").unwrap();
            // Simulate processing
            let count = output.lines.len();
            black_box(count)
        });
    });

    // Workflow: Diff -> Patch generation
    let source = json!({"users": [{"id": 1}, {"id": 2}]});
    let target = json!({"users": [{"id": 1}, {"id": 2}, {"id": 3}]});

    group.bench_function("diff_allocating_workflow", |b| {
        b.iter(|| {
            let patch = json_diff(&source, &target);
            black_box(patch.operations.len())
        });
    });

    group.bench_function("diff_zerocopy_workflow", |b| {
        b.iter(|| {
            let patch = json_diff_zerocopy(&source, &target);
            black_box(patch.len())
        });
    });

    group.finish();
}

// Register all benchmarks
criterion_group!(
    benches,
    bench_pool_acquire_release,
    bench_pool_reuse_pattern,
    bench_gron_zerocopy,
    bench_gron_output_formats,
    bench_diff_zerocopy,
    bench_diff_conversion,
    bench_realistic_workflow,
);

criterion_main!(benches);
