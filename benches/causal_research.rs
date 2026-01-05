// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(missing_docs)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(unexpected_cfgs)]
//! Benchmarks for Causal Dot Store and Observed-Remove Research
//!
//! This benchmark suite compares DSON and SIMD-DSON performance for CRDT operations,
//! measuring the overhead of causal tracking and observed-remove semantics.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
#[cfg(feature = "dhat-heap")]
use dhat::{Dhat, DhatAlloc};
use fionn_core::{DsonOperation, OperationValue};
use fionn_crdt::dot_store::{CausalContext, CausalDotStore, Dot, DotStore, VecDotStore};
use fionn_crdt::observed_remove::{ConcurrentResolver, ObservedRemoveProcessor};
use std::hint::black_box;

// =============================================================================
// Causal Dot Store Benchmarks
// =============================================================================

fn bench_causal_dot_store_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("causal_dot_store/creation");

    for size in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut store = VecDotStore::new();
                for i in 0..size {
                    store.add_dot(Dot::new(1, i as u64));
                }
                let causal_store = CausalDotStore::new(store);
                black_box(causal_store);
            });
        });
    }
    group.finish();
}

fn bench_causal_context_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("causal_context");

    group.bench_function("observe_dots/1000", |b| {
        b.iter(|| {
            let mut context = CausalContext::new();
            for i in 0..1000 {
                context.observe(Dot::new(i % 10, i));
            }
            black_box(context);
        });
    });

    group.bench_function("happened_before_check/100", |b| {
        let mut ctx1 = CausalContext::new();
        let mut ctx2 = CausalContext::new();

        // Set up causal relationship
        for i in 0..50 {
            ctx1.observe(Dot::new(1, i));
            ctx2.observe(Dot::new(1, i));
        }
        ctx2.observe(Dot::new(2, 100)); // Additional event

        b.iter(|| {
            black_box(ctx1.happened_before(&ctx2));
        });
    });

    group.finish();
}

fn bench_causal_join_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("causal_join");

    for size in [10, 100].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                // Create two causal dot stores with overlapping dots
                let mut store1 = VecDotStore::new();
                let mut store2 = VecDotStore::new();

                for i in 0..size {
                    store1.add_dot(Dot::new(1, i));
                    if i % 2 == 0 {
                        store2.add_dot(Dot::new(1, i));
                    } else {
                        store2.add_dot(Dot::new(2, i));
                    }
                }

                let cds1 = CausalDotStore::new(store1);
                let cds2 = CausalDotStore::new(store2);

                let result = cds1.join(cds2).unwrap();
                black_box(result);
            });
        });
    }
    group.finish();
}

// =============================================================================
// Observed-Remove Benchmarks
// =============================================================================

fn bench_observed_remove_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("observed_remove/processing");

    for ops_count in [100, 1000].iter() {
        group.throughput(Throughput::Elements(*ops_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(ops_count),
            ops_count,
            |b, &count| {
                b.iter(|| {
                    let mut processor = ObservedRemoveProcessor::new();

                    // Generate a sequence of add/modify/delete operations
                    for i in 0..count {
                        let op = match i % 3 {
                            0 => DsonOperation::FieldAdd {
                                path: format!("field_{}", i % 10),
                                value: OperationValue::NumberRef(i.to_string()),
                            },
                            1 => DsonOperation::FieldModify {
                                path: format!("field_{}", i % 10),
                                old_value: Some(OperationValue::NumberRef(i.to_string())),
                                new_value: OperationValue::NumberRef((i * 2).to_string()),
                            },
                            _ => DsonOperation::FieldDelete {
                                path: format!("field_{}", i % 10),
                            },
                        };

                        let _result = processor.process_operation(&op).unwrap();
                    }

                    black_box(processor);
                });
            },
        );
    }
    group.finish();
}

fn bench_concurrent_resolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_resolution");

    group.bench_function("update_vs_delete_conflict/100", |b| {
        b.iter(|| {
            let mut resolver = ConcurrentResolver::new();

            // Simulate concurrent update vs delete scenario
            let local_ops = vec![
                DsonOperation::FieldAdd {
                    path: "shared_field".to_string(),
                    value: OperationValue::StringRef("local_value".to_string()),
                },
                DsonOperation::FieldModify {
                    path: "shared_field".to_string(),
                    old_value: Some(OperationValue::StringRef("local_value".to_string())),
                    new_value: OperationValue::StringRef("local_update".to_string()),
                },
            ];

            let remote_ops = vec![
                DsonOperation::FieldAdd {
                    path: "shared_field".to_string(),
                    value: OperationValue::StringRef("remote_value".to_string()),
                },
                DsonOperation::FieldDelete {
                    path: "shared_field".to_string(),
                },
            ];

            for _ in 0..50 {
                // Repeat to get meaningful timing
                let (_local_resolved, _remote_resolved) =
                    resolver.resolve_concurrent_operations(&local_ops, &remote_ops);
            }

            black_box(resolver);
        });
    });

    group.finish();
}

// =============================================================================
// DSON vs SIMD-DSON Performance Comparison
// =============================================================================

fn bench_dson_vs_simd_dson_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("dson_vs_simd_dson");

    // Test data
    let json_data = r#"{"users":[{"id":1,"name":"Alice","email":"alice@test.com"},{"id":2,"name":"Bob","email":"bob@test.com"}]}"#;

    group.bench_function("simd_dson_baseline", |b| {
        b.iter(|| {
            // Standard SIMD-DSON processing
            let tape = fionn_tape::DsonTape::parse(black_box(json_data)).unwrap();
            black_box(tape);
        });
    });

    group.bench_function("simd_dson_with_causal_tracking", |b| {
        b.iter(|| {
            // SIMD-DSON with causal dot store overhead
            let tape = fionn_tape::DsonTape::parse(black_box(json_data)).unwrap();

            // Add causal tracking overhead
            let mut store = VecDotStore::new();
            store.add_dot(Dot::new(1, 1)); // Simulate causal dot
            let causal_store = CausalDotStore::new(store);

            black_box((tape, causal_store));
        });
    });

    group.bench_function("simd_dson_with_observed_remove", |b| {
        b.iter(|| {
            // SIMD-DSON with observed-remove processing
            let tape = fionn_tape::DsonTape::parse(black_box(json_data)).unwrap();

            let mut processor = ObservedRemoveProcessor::new();
            // Simulate some operations
            let add_op = DsonOperation::FieldAdd {
                path: "test.field".to_string(),
                value: OperationValue::StringRef("test".to_string()),
            };
            let _result = processor.process_operation(&add_op).unwrap();

            black_box((tape, processor));
        });
    });

    group.finish();
}

// Performance scaling comparison
fn generate_test_json(size: usize) -> String {
    let mut json = String::from("{\"data\":[");
    for i in 0..size {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("{{\"id\":{},\"value\":\"test_{}\"}}", i, i));
    }
    json.push_str("]}");
    json
}

fn bench_scaling_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling_comparison");

    for size in [1, 10, 100].iter() {
        let json_data = generate_test_json(*size);

        group.bench_with_input(BenchmarkId::new("simd_dson", size), size, |b, _size| {
            b.iter(|| {
                let tape = fionn_tape::DsonTape::parse(black_box(&json_data)).unwrap();
                black_box(tape);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("causal_simd_dson", size),
            size,
            |b, _size| {
                b.iter(|| {
                    let tape = fionn_tape::DsonTape::parse(black_box(&json_data)).unwrap();

                    // Add causal tracking proportional to data size
                    let mut store = VecDotStore::new();
                    for i in 0..*size {
                        store.add_dot(Dot::new(1, i as u64));
                    }
                    let causal_store = CausalDotStore::new(store);

                    black_box((tape, causal_store));
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Configuration
// =============================================================================

criterion_group!(
    causal_dot_store_benches,
    bench_causal_dot_store_creation,
    bench_causal_context_operations,
    bench_causal_join_operations,
);

criterion_group!(
    observed_remove_benches,
    bench_observed_remove_processing,
    bench_concurrent_resolution,
);

criterion_group!(
    comparative_benches,
    bench_dson_vs_simd_dson_overhead,
    bench_scaling_comparison,
);

// =============================================================================
// Memory Profiling Benchmarks
// =============================================================================

#[cfg(feature = "dhat-heap")]
fn bench_memory_usage_causal_dot_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/causal_dot_store");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("memory_usage", size), size, |b, &size| {
            b.iter(|| {
                // Use dhat to profile memory usage
                let _profiler = Dhat::start_heap_profiling();

                let mut store = VecDotStore::new();
                for i in 0..size {
                    store.add_dot(Dot::new(i % 10, i as u64));
                }

                let causal_store = CausalDotStore::new(store);

                // Force allocation to stay alive
                black_box(causal_store);
            });
        });
    }
    group.finish();
}

#[cfg(feature = "dhat-heap")]
fn bench_memory_usage_observed_remove(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/observed_remove");

    for ops_count in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("memory_usage", ops_count),
            ops_count,
            |b, &count| {
                b.iter(|| {
                    let _profiler = Dhat::start_heap_profiling();

                    let mut processor = ObservedRemoveProcessor::new();

                    // Generate operations that will create observed state
                    for i in 0..count {
                        let op = match i % 4 {
                            0 => DsonOperation::FieldAdd {
                                path: format!("field_{}", i % 50),
                                value: OperationValue::NumberRef(i.to_string()),
                            },
                            1 => DsonOperation::FieldModify {
                                path: format!("field_{}", i % 50),
                                old_value: Some(OperationValue::NumberRef(i.to_string())),
                                new_value: OperationValue::NumberRef((i * 2).to_string()),
                            },
                            2 => DsonOperation::FieldDelete {
                                path: format!("field_{}", i % 50),
                            },
                            _ => DsonOperation::FieldAdd {
                                path: format!("new_field_{}", i),
                                value: OperationValue::StringRef(format!("value_{}", i)),
                            },
                        };

                        let _result = processor.process_operation(&op);
                    }

                    black_box(processor);
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// Concurrent Operations Benchmarks
// =============================================================================

use std::sync::{Arc, Mutex};
use std::thread;

fn bench_concurrent_readers_writers(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent/operations");

    for num_threads in [2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("readers_writers", num_threads),
            num_threads,
            |b, &num_threads| {
                b.iter(|| {
                    // Shared state for concurrent operations
                    let causal_store =
                        Arc::new(Mutex::new(CausalDotStore::new(VecDotStore::new())));
                    let observed_processor = Arc::new(Mutex::new(ObservedRemoveProcessor::new()));

                    let mut handles = vec![];

                    // Spawn reader threads
                    for thread_id in 0..num_threads {
                        let store_clone = Arc::clone(&causal_store);
                        let processor_clone = Arc::clone(&observed_processor);

                        let handle = thread::spawn(move || {
                            for i in 0..100 {
                                let dot_id = thread_id * 100 + i;

                                // Reader operations
                                {
                                    let store = store_clone.lock().unwrap();
                                    let _dots = store.store.dots();
                                }

                                {
                                    let processor = processor_clone.lock().unwrap();
                                    let _fields = processor.observed_fields();
                                }

                                // Writer operations
                                {
                                    let mut store = store_clone.lock().unwrap();
                                    store
                                        .store
                                        .add_dot(Dot::new(thread_id as u64, dot_id as u64));
                                }

                                {
                                    let mut processor = processor_clone.lock().unwrap();
                                    let op = DsonOperation::FieldAdd {
                                        path: format!("field_{}_{}", thread_id, i),
                                        value: OperationValue::NumberRef(dot_id.to_string()),
                                    };
                                    let _result = processor.process_operation(&op);
                                }
                            }
                        });

                        handles.push(handle);
                    }

                    // Wait for all threads to complete
                    for handle in handles {
                        handle.join().unwrap();
                    }

                    // Final state check
                    let final_store = causal_store.lock().unwrap();
                    let final_processor = observed_processor.lock().unwrap();

                    black_box((
                        final_store.store.dots().len(),
                        final_processor.observed_fields().len(),
                    ));
                });
            },
        );
    }
    group.finish();
}

fn bench_concurrent_conflict_resolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent/conflicts");

    for num_pairs in [5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::new("conflict_resolution", num_pairs),
            num_pairs,
            |b, &num_pairs| {
                b.iter(|| {
                    let resolver = Arc::new(Mutex::new(ConcurrentResolver::new()));
                    let mut handles = vec![];

                    // Spawn pairs of threads that create conflicts
                    for pair_id in 0..num_pairs {
                        let resolver_clone = Arc::clone(&resolver);

                        let handle = thread::spawn(move || {
                            let local_ops = vec![
                                DsonOperation::FieldAdd {
                                    path: format!("shared_{}", pair_id),
                                    value: OperationValue::StringRef(format!("local_{}", pair_id)),
                                },
                                DsonOperation::FieldModify {
                                    path: format!("shared_{}", pair_id),
                                    old_value: Some(OperationValue::StringRef(format!(
                                        "local_{}",
                                        pair_id
                                    ))),
                                    new_value: OperationValue::StringRef(format!(
                                        "local_update_{}",
                                        pair_id
                                    )),
                                },
                            ];

                            let remote_ops = vec![
                                DsonOperation::FieldAdd {
                                    path: format!("shared_{}", pair_id),
                                    value: OperationValue::StringRef(format!("remote_{}", pair_id)),
                                },
                                DsonOperation::FieldDelete {
                                    path: format!("shared_{}", pair_id),
                                },
                            ];

                            let mut resolver = resolver_clone.lock().unwrap();
                            let (_local_resolved, _remote_resolved) =
                                resolver.resolve_concurrent_operations(&local_ops, &remote_ops);

                            black_box((_local_resolved, _remote_resolved));
                        });

                        handles.push(handle);
                    }

                    // Wait for all conflict resolution to complete
                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// Updated Criterion Main
// =============================================================================

#[cfg(feature = "dhat-heap")]
criterion_group!(
    memory_benches,
    bench_memory_usage_causal_dot_store,
    bench_memory_usage_observed_remove,
);

criterion_group!(
    concurrent_benches,
    bench_concurrent_readers_writers,
    bench_concurrent_conflict_resolution,
);

#[cfg(feature = "dhat-heap")]
criterion_main!(
    causal_dot_store_benches,
    observed_remove_benches,
    comparative_benches,
    memory_benches,
    concurrent_benches
);

#[cfg(not(feature = "dhat-heap"))]
criterion_main!(
    causal_dot_store_benches,
    observed_remove_benches,
    comparative_benches,
    concurrent_benches
);
