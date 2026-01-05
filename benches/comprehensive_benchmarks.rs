// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(missing_docs)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(unexpected_cfgs)]
//! Comprehensive Benchmarks for SIMD-DSON
//!
//! This benchmark suite provides complete comparisons between:
//! - `serde_json` vs SIMD-JSON vs SIMD-DSON (parsing performance)
//! - DSON vs SIMD-DSON (CRDT operations and merge performance)
//! - CRDT merge operations and conflict resolution

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use fionn_core::{DsonOperation, OperationValue};
use fionn_crdt::dot_store::{CausalContext, CausalDotStore, Dot, VecDotStore};
use fionn_crdt::observed_remove::ConcurrentResolver;
use fionn_tape::DsonTape;
use std::hint::black_box;

// Test data
const SMALL_JSON: &str = r#"{"name":"Alice","age":30,"active":true}"#;
const MEDIUM_JSON: &str = r#"{"users":[{"name":"Alice","profile":{"city":"NYC","skills":["rust","python"]},"active":true},{"name":"Bob","profile":{"city":"LA","skills":["go","typescript"]},"active":false}],"total":2}"#;
const LARGE_JSON: &str = r#"{
  "users": [
    {"id": 1, "name": "Alice", "email": "alice@example.com", "profile": {"city": "NYC", "country": "USA", "skills": ["rust", "python", "javascript"], "experience": 5}, "active": true, "score": 95.5, "tags": ["developer", "team-lead"]},
    {"id": 2, "name": "Bob", "email": "bob@example.com", "profile": {"city": "LA", "country": "USA", "skills": ["go", "typescript", "docker"], "experience": 3}, "active": false, "score": 87.2, "tags": ["developer", "backend"]},
    {"id": 3, "name": "Charlie", "email": "charlie@example.com", "profile": {"city": "Chicago", "country": "USA", "skills": ["java", "spring", "kubernetes"], "experience": 7}, "active": true, "score": 92.1, "tags": ["developer", "architect"]},
    {"id": 4, "name": "Diana", "email": "diana@example.com", "profile": {"city": "Seattle", "country": "USA", "skills": ["csharp", "dotnet", "azure"], "experience": 4}, "active": true, "score": 89.8, "tags": ["developer", "cloud"]},
    {"id": 5, "name": "Eve", "email": "eve@example.com", "profile": {"city": "Austin", "country": "USA", "skills": ["python", "django", "postgresql"], "experience": 6}, "active": false, "score": 91.3, "tags": ["developer", "fullstack"]}
  ],
  "metadata": {
    "version": "1.0",
    "created": "2024-01-01T00:00:00Z",
    "total_users": 5,
    "active_users": 3,
    "average_score": 91.18,
    "tags_distribution": {
      "developer": 5,
      "team-lead": 1,
      "backend": 1,
      "architect": 1,
      "cloud": 1,
      "fullstack": 1
    }
  },
  "settings": {
    "max_users": 1000,
    "features": ["registration", "authentication", "profiles", "messaging"],
    "limits": {"max_skills": 10, "max_tags": 5, "max_score": 100}
  }
}"#;

// =============================================================================
// Serde vs SIMD-JSON vs SIMD-DSON Parsing Benchmarks
// =============================================================================

fn bench_parsing_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing/small");

    group.bench_function("serde_json", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(SMALL_JSON)).unwrap();
        });
    });

    group.bench_function("simd_json", |b| {
        b.iter(|| {
            let mut bytes = SMALL_JSON.as_bytes().to_vec();
            let tape = simd_json::to_tape(black_box(&mut bytes)).unwrap();
            black_box(tape);
        });
    });

    group.bench_function("sonic_rs", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(SMALL_JSON)).unwrap();
        });
    });

    group.bench_function("fionn", |b| {
        b.iter(|| {
            let tape = DsonTape::parse(black_box(SMALL_JSON)).unwrap();
            black_box(tape);
        });
    });

    group.finish();
}

fn bench_parsing_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing/medium");

    group.bench_function("serde_json", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(MEDIUM_JSON)).unwrap();
        });
    });

    group.bench_function("simd_json", |b| {
        b.iter(|| {
            let mut bytes = MEDIUM_JSON.as_bytes().to_vec();
            let tape = simd_json::to_tape(black_box(&mut bytes)).unwrap();
            black_box(tape);
        });
    });

    group.bench_function("sonic_rs", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(MEDIUM_JSON)).unwrap();
        });
    });

    group.bench_function("fionn", |b| {
        b.iter(|| {
            let tape = DsonTape::parse(black_box(MEDIUM_JSON)).unwrap();
            black_box(tape);
        });
    });

    group.finish();
}

fn bench_parsing_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing/large");

    group.bench_function("serde_json", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(LARGE_JSON)).unwrap();
        });
    });

    group.bench_function("simd_json", |b| {
        b.iter(|| {
            let mut bytes = LARGE_JSON.as_bytes().to_vec();
            let tape = simd_json::to_tape(black_box(&mut bytes)).unwrap();
            black_box(tape);
        });
    });

    group.bench_function("sonic_rs", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(LARGE_JSON)).unwrap();
        });
    });

    group.bench_function("fionn", |b| {
        b.iter(|| {
            let tape = DsonTape::parse(black_box(LARGE_JSON)).unwrap();
            black_box(tape);
        });
    });

    group.finish();
}

// =============================================================================
// CRDT Merge Operation Benchmarks
// =============================================================================

fn bench_crdt_merge_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("crdt/merge");

    // Create test data for merge operations
    let mut store1 = VecDotStore::new();
    let mut store2 = VecDotStore::new();

    for i in 0..100 {
        store1.add_dot(Dot::new(1, i));
        store2.add_dot(Dot::new(2, i));
    }

    let causal_store1 = CausalDotStore::new(store1);
    let causal_store2 = CausalDotStore::new(store2);

    group.bench_function("causal_dot_store_join", |b| {
        b.iter(|| {
            let result = black_box(causal_store1.clone())
                .join(black_box(causal_store2.clone()))
                .unwrap();
            black_box(result);
        });
    });

    group.bench_function("concurrent_resolver_merge", |b| {
        b.iter(|| {
            let mut resolver = ConcurrentResolver::new();

            let local_ops = vec![DsonOperation::FieldAdd {
                path: "user.alice".to_string(),
                value: OperationValue::StringRef("active".to_string()),
            }];

            let remote_ops = vec![DsonOperation::FieldAdd {
                path: "user.bob".to_string(),
                value: OperationValue::StringRef("active".to_string()),
            }];

            let (local_resolved, remote_resolved) =
                resolver.resolve_concurrent_operations(&local_ops, &remote_ops);

            black_box((local_resolved, remote_resolved));
        });
    });

    group.finish();
}

fn bench_crdt_merge_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("crdt/merge_scaling");

    for merge_count in &[10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(merge_count),
            merge_count,
            |b, &merge_count| {
                b.iter(|| {
                    let mut stores = Vec::new();

                    // Create multiple causal stores to merge
                    for i in 0..merge_count {
                        let mut store = VecDotStore::new();
                        for j in 0..10 {
                            store.add_dot(Dot::new(i as u64, j));
                        }
                        stores.push(CausalDotStore::new(store));
                    }

                    // Perform sequential merges
                    let mut result = stores[0].clone();
                    for store in stores.iter().skip(1) {
                        result = result.join(store.clone()).unwrap();
                    }

                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// DSON vs SIMD-DSON Complete CRDT Comparison
// =============================================================================

fn bench_dson_vs_simd_dson_crdt_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("dson_vs_simd_dson/crdt_operations");

    // SIMD-DSON operations (measured)
    group.bench_function("simd_dson/dot_store_1000", |b| {
        b.iter(|| {
            let mut store = VecDotStore::new();
            for i in 0..1000 {
                store.add_dot(Dot::new(1, i as u64));
            }
            black_box(store);
        });
    });

    group.bench_function("simd_dson/causal_context_1000", |b| {
        b.iter(|| {
            let mut context = CausalContext::new();
            for i in 0..1000 {
                context.observe(Dot::new(i % 10, i));
            }
            black_box(context);
        });
    });

    group.bench_function("simd_dson/join_operation", |b| {
        b.iter(|| {
            let mut store1 = VecDotStore::new();
            let mut store2 = VecDotStore::new();

            for i in 0..50 {
                store1.add_dot(Dot::new(1, i));
                store2.add_dot(Dot::new(2, i));
            }

            let causal_store1 = CausalDotStore::new(store1);
            let causal_store2 = CausalDotStore::new(store2);

            let result = causal_store1.join(causal_store2).unwrap();
            black_box(result);
        });
    });

    group.bench_function("simd_dson/happened_before_1000", |b| {
        b.iter(|| {
            let mut ctx1 = CausalContext::new();
            let mut ctx2 = CausalContext::new();

            // Set up causal relationship
            for i in 0..500 {
                ctx1.observe(Dot::new(1, i));
                ctx2.observe(Dot::new(1, i));
            }
            ctx2.observe(Dot::new(2, 1000));

            for _ in 0..1000 {
                black_box(ctx1.happened_before(&ctx2));
            }
        });
    });

    group.finish();
}

// =============================================================================
// Configuration
// =============================================================================

criterion_group!(
    parsing_benches,
    bench_parsing_small,
    bench_parsing_medium,
    bench_parsing_large,
);

criterion_group!(
    crdt_merge_benches,
    bench_crdt_merge_operations,
    bench_crdt_merge_scaling,
);

criterion_group!(
    crdt_comparison_benches,
    bench_dson_vs_simd_dson_crdt_operations,
);

criterion_main!(parsing_benches, crdt_merge_benches, crdt_comparison_benches);
