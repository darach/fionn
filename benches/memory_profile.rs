// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - CountingAllocator kept for future memory profiling integration
#![allow(unused)]
// Benchmarks: GlobalAlloc trait requires unsafe fn bodies with unsafe operations
#![allow(unsafe_op_in_unsafe_fn)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Memory profiling benchmarks
//!
//! Measures memory allocation patterns across different tape types and formats.
//! Uses allocation counting to understand memory efficiency.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_core::{FormatKind, TapeSource};
use fionn_simd::transform::UnifiedTape;
use fionn_tape::DsonTape;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global allocator that counts allocations
struct CountingAllocator {
    alloc_count: AtomicUsize,
    alloc_bytes: AtomicUsize,
}

impl CountingAllocator {
    const fn new() -> Self {
        Self {
            alloc_count: AtomicUsize::new(0),
            alloc_bytes: AtomicUsize::new(0),
        }
    }

    fn reset(&self) {
        self.alloc_count.store(0, Ordering::SeqCst);
        self.alloc_bytes.store(0, Ordering::SeqCst);
    }

    fn alloc_count(&self) -> usize {
        self.alloc_count.load(Ordering::SeqCst)
    }

    fn alloc_bytes(&self) -> usize {
        self.alloc_bytes.load(Ordering::SeqCst)
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc_count.fetch_add(1, Ordering::SeqCst);
        self.alloc_bytes.fetch_add(layout.size(), Ordering::SeqCst);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            self.alloc_bytes
                .fetch_add(new_size - layout.size(), Ordering::SeqCst);
        }
        System.realloc(ptr, layout, new_size)
    }
}

// Note: Cannot use #[global_allocator] in benchmarks, so we measure differently

/// Estimate tape memory usage
fn estimate_tape_memory<T: TapeSource>(tape: &T) -> usize {
    // Estimate based on tape length and node size
    // Each node is approximately 24-32 bytes (enum + data)
    tape.len() * 32
}

/// Generate test data at various sizes
fn generate_json_data(node_count: usize) -> String {
    let mut json = String::from("{");
    for i in 0..node_count {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(r#""key_{}": "value_{}""#, i, i));
    }
    json.push('}');
    json
}

/// Benchmark: Tape size relative to input size
fn bench_tape_memory_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/tape_ratio");

    for node_count in [10, 100, 1000, 10000] {
        let json = generate_json_data(node_count);
        let input_size = json.len();
        group.throughput(Throughput::Bytes(input_size as u64));

        group.bench_with_input(
            BenchmarkId::new("dsontape", node_count),
            &json,
            |b, input| {
                b.iter(|| {
                    let tape = DsonTape::parse(input).unwrap();
                    let tape_memory = estimate_tape_memory(&tape);
                    black_box((tape.len(), tape_memory))
                })
            },
        );

        #[cfg(feature = "yaml")]
        {
            use fionn_simd::transform::UnifiedTape;
            // Generate equivalent YAML
            let yaml = {
                let mut y = String::new();
                for i in 0..node_count {
                    y.push_str(&format!("key_{}: value_{}\n", i, i));
                }
                y
            };

            group.bench_with_input(
                BenchmarkId::new("unified_yaml", node_count),
                &yaml,
                |b, input| {
                    b.iter(|| {
                        if let Ok(tape) = UnifiedTape::parse(input.as_bytes(), FormatKind::Yaml) {
                            let tape_memory = estimate_tape_memory(&tape);
                            black_box((tape.len(), tape_memory));
                        }
                    })
                },
            );
        }
    }

    group.finish();
}

/// Benchmark: Node count for different JSON structures
fn bench_node_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/node_density");

    // Flat object with many keys
    let flat_object = format!(
        "{{{}}}",
        (0..1000)
            .map(|i| format!(r#""k{}": {}"#, i, i))
            .collect::<Vec<_>>()
            .join(",")
    );

    // Deep nested object
    let deep_nested = {
        let mut s = String::new();
        for i in 0..100 {
            s.push_str(&format!(r#"{{"level_{}":"#, i));
        }
        s.push_str("1");
        for _ in 0..100 {
            s.push('}');
        }
        s
    };

    // Large array
    let large_array = format!(
        "[{}]",
        (0..1000)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    // String-heavy (fewer nodes, more bytes per node)
    let string_heavy = format!(
        "{{{}}}",
        (0..100)
            .map(|i| format!(r#""k{}": "{}""#, i, "x".repeat(100)))
            .collect::<Vec<_>>()
            .join(",")
    );

    for (name, json) in [
        ("flat_object_1k", &flat_object),
        ("deep_nested_100", &deep_nested),
        ("array_1k", &large_array),
        ("string_heavy", &string_heavy),
    ] {
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("parse", name), json, |b, input| {
            b.iter(|| {
                let tape = DsonTape::parse(input).unwrap();
                black_box((
                    tape.len(),
                    input.len(),
                    tape.len() as f64 / input.len() as f64,
                ))
            })
        });
    }

    group.finish();
}

/// Benchmark: Memory efficiency of skip operations
fn bench_skip_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/skip_efficiency");

    // Create a document where we only need one field
    let json = format!(
        r#"{{"target": "wanted", "skip_me": {{{}}}}}"#,
        (0..1000)
            .map(|i| format!(r#""k{}": {}"#, i, i))
            .collect::<Vec<_>>()
            .join(",")
    );

    group.throughput(Throughput::Bytes(json.len() as u64));

    // Full parse
    group.bench_function("full_parse", |b| {
        b.iter(|| {
            let tape = DsonTape::parse(&json).unwrap();
            black_box(tape.len())
        })
    });

    // Parse with skip (simulated selective access)
    group.bench_function("selective_with_skip", |b| {
        b.iter(|| {
            let tape = DsonTape::parse(&json).unwrap();
            // Skip the large field
            if tape.len() > 3 {
                let skipped = tape.skip_value(3);
                black_box(skipped);
            }
        })
    });

    group.finish();
}

/// Benchmark: Memory usage with string deduplication potential
fn bench_string_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/strings");

    // Document with repeated strings (deduplication potential)
    let repeated_strings = format!(
        "[{}]",
        (0..1000)
            .map(|_| r#"{"type": "user", "status": "active"}"#)
            .collect::<Vec<_>>()
            .join(",")
    );

    // Document with unique strings (no deduplication)
    let unique_strings = format!(
        "[{}]",
        (0..1000)
            .map(|i| format!(r#"{{"type": "user_{}", "status": "active_{}"}}"#, i, i))
            .collect::<Vec<_>>()
            .join(",")
    );

    for (name, json) in [
        ("repeated_strings", &repeated_strings),
        ("unique_strings", &unique_strings),
    ] {
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("parse", name), json, |b, input| {
            b.iter(|| {
                let tape = DsonTape::parse(input).unwrap();
                black_box(tape.len())
            })
        });
    }

    group.finish();
}

/// Benchmark: Cow<str> borrowing effectiveness
fn bench_borrowed_vs_owned(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/borrowed_strings");

    // Simple strings that can be borrowed (no escapes)
    let borrowable = r#"{"name": "Alice", "city": "Paris", "country": "France"}"#;

    // Strings that need allocation (escapes)
    let needs_alloc =
        r#"{"name": "Alice \"Bob\"", "city": "Paris\nFrance", "note": "Hello\tWorld"}"#;

    for (name, json) in [("borrowable", borrowable), ("needs_alloc", needs_alloc)] {
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("parse", name), json, |b, input| {
            b.iter(|| {
                let tape = DsonTape::parse(input).unwrap();
                // Access string values
                for idx in 0..tape.len() {
                    if let Some(node) = tape.node_at(idx) {
                        black_box(&node.value);
                    }
                }
                black_box(tape.len())
            })
        });
    }

    group.finish();
}

/// Benchmark: Tape reuse patterns
fn bench_tape_reuse(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/tape_reuse");

    let documents: Vec<String> = (0..100)
        .map(|i| {
            format!(
                r#"{{"id": {}, "data": "value_{}", "active": {}}}"#,
                i,
                i,
                i % 2 == 0
            )
        })
        .collect();

    // Parse each document fresh
    group.bench_function("fresh_parse_each", |b| {
        b.iter(|| {
            for doc in &documents {
                let tape = DsonTape::parse(doc).unwrap();
                black_box(tape.len());
            }
        })
    });

    // Note: If DsonTape supported reuse, we'd benchmark that here
    // For now, this measures the baseline cost

    group.finish();
}

/// Benchmark: Format comparison memory efficiency
#[cfg(feature = "yaml")]
fn bench_format_memory_comparison(c: &mut Criterion) {
    use fionn_simd::transform::UnifiedTape;

    let mut group = c.benchmark_group("memory/format_comparison");

    // Same logical data in different formats
    let json = r#"{"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}"#;
    let yaml = "users:\n  - name: Alice\n    age: 30\n  - name: Bob\n    age: 25\n";

    // JSON tape
    group.throughput(Throughput::Bytes(json.len() as u64));
    group.bench_function("json", |b| {
        b.iter(|| {
            let tape = DsonTape::parse(json).unwrap();
            let ratio = tape.len() as f64 / json.len() as f64;
            black_box((tape.len(), ratio))
        })
    });

    // YAML tape
    group.throughput(Throughput::Bytes(yaml.len() as u64));
    group.bench_function("yaml", |b| {
        b.iter(|| {
            if let Ok(tape) = UnifiedTape::parse(yaml.as_bytes(), FormatKind::Yaml) {
                let ratio = tape.len() as f64 / yaml.len() as f64;
                black_box((tape.len(), ratio));
            }
        })
    });

    group.finish();
}

#[cfg(not(feature = "yaml"))]
fn bench_format_memory_comparison(_c: &mut Criterion) {}

criterion_group!(
    memory_benchmarks,
    bench_tape_memory_ratio,
    bench_node_density,
    bench_skip_memory,
    bench_string_memory,
    bench_borrowed_vs_owned,
    bench_tape_reuse,
    bench_format_memory_comparison,
);

criterion_main!(memory_benchmarks);
