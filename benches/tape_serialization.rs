// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
use ahash::{AHashMap, AHashSet};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fionn_core::OperationValue;
use fionn_tape::DsonTape;
use std::collections::HashSet as StdHashSet;
use std::hint::black_box;

/// Benchmark tape serialization optimizations
fn tape_serialization_benchmarks(c: &mut Criterion) {
    // Fast path: no modifications
    {
        let mut group = c.benchmark_group("tape_operations");
        let json = r#"{"user":{"name":"Alice","age":30},"active":true,"tags":["dev","admin"]}"#;
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("lazy_vs_eager_serialization", |b| {
            let tape = DsonTape::parse(json).unwrap();
            b.iter(|| {
                black_box(
                    tape.serialize_with_modifications(&AHashMap::new(), &AHashSet::new())
                        .unwrap(),
                );
            });
        });
        group.finish();
    }

    // Modified path: apply modifications during serialization
    {
        let mut group = c.benchmark_group("tape_modifications");
        let json = r#"{"user":{"name":"Alice","age":30},"active":true}"#;
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("modification_overlay_cost", |b| {
            let tape = DsonTape::parse(json).unwrap();
            let mut modifications = AHashMap::new();
            modifications.insert(
                "user.email".to_string(),
                OperationValue::StringRef("alice@example.com".to_string()),
            );
            modifications.insert("user.verified".to_string(), OperationValue::BoolRef(true));
            modifications.insert(
                "last_login".to_string(),
                OperationValue::StringRef("2024-01-01".to_string()),
            );
            let deletions = AHashSet::new();

            b.iter(|| {
                black_box(
                    tape.serialize_with_modifications(&modifications, &deletions)
                        .unwrap(),
                );
            });
        });
        group.finish();
    }

    // Deep nested modification (stress test for inference)
    {
        let mut group = c.benchmark_group("tape_deep_nested");
        let json = r#"{"root":{}}"#;
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("deep_nested_serialization", |b| {
            let tape = DsonTape::parse(json).unwrap();
            let mut modifications = AHashMap::new();
            modifications.insert(
                "root.a.b.c.d.e".to_string(),
                OperationValue::NumberRef("42".to_string()),
            );

            b.iter(|| {
                black_box(
                    tape.serialize_with_modifications(&modifications, &AHashSet::new())
                        .unwrap(),
                );
            });
        });
        group.finish();
    }

    // Zero-copy throughput for unmodified data
    {
        let json = create_medium_json();
        let mut group = c.benchmark_group("tape_zero_copy");
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("zero_copy_throughput", |b| {
            let tape = DsonTape::parse(json.as_str()).unwrap();
            b.iter(|| {
                black_box(
                    tape.serialize_with_modifications(&AHashMap::new(), &AHashSet::new())
                        .unwrap(),
                );
            });
        });
        group.finish();
    }

    // SIMD path resolution benchmark
    {
        let json = create_medium_json();
        let mut group = c.benchmark_group("tape_simd_path");
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("simd_path_resolution", |b| {
            let tape = DsonTape::parse(json.as_str()).unwrap();
            b.iter(|| {
                let _ = black_box(tape.resolve_path("user.profile.skills"));
            });
        });
        group.finish();
    }

    // SIMD string operations benchmark
    {
        let mut group = c.benchmark_group("tape_simd_string");
        let json = r#"{"field1":"test_value","field2":"another_value","field3":"final_value"}"#;
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("simd_string_operations", |b| {
            let tape = DsonTape::parse(json).unwrap();
            b.iter(|| {
                black_box(tape.extract_value_simd(1));
            });
        });
        group.finish();
    }

    // SIMD schema filtering benchmark
    {
        let mut group = c.benchmark_group("tape_simd_schema");
        let json = r#"{"field_0":"value0","field_1":"value1","field_2":"value2","field_3":"value3","field_4":"value4","field_5":"value5"}"#;
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("simd_schema_filtering", |b| {
            let tape = DsonTape::parse(json).unwrap();
            let schema: StdHashSet<String> = (0..5).map(|i| format!("field_{}", i)).collect();

            b.iter(|| {
                black_box(tape.filter_by_schema(&schema).unwrap());
            });
        });
        group.finish();
    }
}

/// Create medium-sized test JSON
fn create_medium_json() -> String {
    r#"{
        "user": {
            "id": 123,
            "name": "Alice Johnson",
            "email": "alice@example.com",
            "profile": {
                "bio": "Software engineer with 5 years experience",
                "skills": ["Rust", "Python", "JavaScript", "SQL"],
                "experience": 5,
                "location": "San Francisco, CA"
            },
            "permissions": ["read", "write", "admin"],
            "stats": {
                "posts": 42,
                "followers": 1234,
                "following": 567
            }
        },
        "metadata": {
            "version": "2.1",
            "created": "2024-01-01T00:00:00Z",
            "updated": "2024-01-15T10:30:00Z"
        },
        "settings": {
            "theme": "dark",
            "notifications": {
                "email": true,
                "push": false,
                "sms": false
            },
            "privacy": {
                "profile_visible": true,
                "activity_visible": false
            }
        }
    }"#
    .to_string()
}

criterion_group!(tape_benches, tape_serialization_benchmarks);
criterion_main!(tape_benches);
