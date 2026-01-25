// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Allow unused results in benchmarks (we just want to measure performance)
#![allow(unused_must_use)]
// Helper functions may be defined for future benchmarks
#![allow(dead_code)]
//! DSON Matrix Benchmarks
//!
//! Full matrix benchmarks for DSON operations across all formats:
//! - Formats: JSONL, ISONL, CSV, YAML, TOML, TOON
//! - Operations: Parse, FieldAdd, FieldModify, FieldDelete
//! - Document sizes: tiny (50B), small (1KB), medium (10KB), large (100KB)
//! - Schema complexity: none, simple, complex, wildcard

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::hint::black_box as hint_black_box;

// =============================================================================
// Test Data Generation
// =============================================================================

mod generators {
    //! Test data generators for DSON matrix benchmarks

    /// Generate JSONL data with specified record count
    pub fn generate_jsonl(record_count: usize) -> Vec<u8> {
        let mut jsonl = String::new();
        for i in 0..record_count {
            jsonl.push_str(&format!(
                r#"{{"id":{},"name":"user{}","email":"user{}@example.com","active":{},"score":{}}}"#,
                i,
                i,
                i,
                i % 2 == 0,
                (i * 17) % 100
            ));
            jsonl.push('\n');
        }
        jsonl.into_bytes()
    }

    /// Generate ISONL data (pipe-delimited with self-contained schema)
    #[cfg(feature = "ison")]
    pub fn generate_isonl(record_count: usize) -> Vec<u8> {
        let mut isonl = String::new();
        for i in 0..record_count {
            // Each line contains the schema and values
            isonl.push_str(&format!(
                "table.users|id:int|name:string|email:string|active:bool|{}|user{}|user{}@example.com|{}\n",
                i,
                i,
                i,
                i % 2 == 0
            ));
        }
        isonl.into_bytes()
    }

    /// Generate CSV data
    #[cfg(feature = "csv")]
    pub fn generate_csv(record_count: usize) -> Vec<u8> {
        let mut csv = String::from("id,name,email,active,score\n");
        for i in 0..record_count {
            csv.push_str(&format!(
                "{},user{},user{}@example.com,{},{}\n",
                i,
                i,
                i,
                i % 2 == 0,
                (i * 17) % 100
            ));
        }
        csv.into_bytes()
    }

    /// Generate YAML data (multi-document)
    #[cfg(feature = "yaml")]
    pub fn generate_yaml(record_count: usize) -> Vec<u8> {
        let mut yaml = String::new();
        for i in 0..record_count {
            yaml.push_str(&format!(
                "id: {}\nname: user{}\nemail: user{}@example.com\nactive: {}\nscore: {}\n---\n",
                i,
                i,
                i,
                i % 2 == 0,
                (i * 17) % 100
            ));
        }
        yaml.into_bytes()
    }

    /// Generate TOML data (single document with sections)
    #[cfg(feature = "toml")]
    pub fn generate_toml(record_count: usize) -> Vec<u8> {
        let mut toml = String::new();
        for i in 0..record_count {
            toml.push_str(&format!(
                "[user_{}]\nid = {}\nname = \"user{}\"\nemail = \"user{}@example.com\"\nactive = {}\nscore = {}\n\n",
                i,
                i,
                i,
                i,
                i % 2 == 0,
                (i * 17) % 100
            ));
        }
        toml.into_bytes()
    }

    /// Generate TOON data (key-value lines)
    #[cfg(feature = "toon")]
    pub fn generate_toon(record_count: usize) -> Vec<u8> {
        let mut toon = String::new();
        for i in 0..record_count {
            toon.push_str(&format!("id: {}\n", i));
            toon.push_str(&format!("name: user{}\n", i));
            toon.push_str(&format!("email: user{}@example.com\n", i));
            toon.push_str(&format!("active: {}\n", i % 2 == 0));
            toon.push_str(&format!("score: {}\n", (i * 17) % 100));
        }
        toon.into_bytes()
    }

    // Size class generators (approximate target sizes)
    pub fn tiny_jsonl() -> Vec<u8> {
        generate_jsonl(1) // ~80 bytes
    }

    pub fn small_jsonl() -> Vec<u8> {
        generate_jsonl(15) // ~1KB
    }

    pub fn medium_jsonl() -> Vec<u8> {
        generate_jsonl(150) // ~10KB
    }

    pub fn large_jsonl() -> Vec<u8> {
        generate_jsonl(1500) // ~100KB
    }

    #[cfg(feature = "ison")]
    pub fn tiny_isonl() -> Vec<u8> {
        generate_isonl(1)
    }
    #[cfg(feature = "ison")]
    pub fn small_isonl() -> Vec<u8> {
        generate_isonl(15)
    }
    #[cfg(feature = "ison")]
    pub fn medium_isonl() -> Vec<u8> {
        generate_isonl(150)
    }
    #[cfg(feature = "ison")]
    pub fn large_isonl() -> Vec<u8> {
        generate_isonl(1500)
    }

    #[cfg(feature = "csv")]
    pub fn tiny_csv() -> Vec<u8> {
        generate_csv(1)
    }
    #[cfg(feature = "csv")]
    pub fn small_csv() -> Vec<u8> {
        generate_csv(30)
    }
    #[cfg(feature = "csv")]
    pub fn medium_csv() -> Vec<u8> {
        generate_csv(300)
    }
    #[cfg(feature = "csv")]
    pub fn large_csv() -> Vec<u8> {
        generate_csv(3000)
    }

    #[cfg(feature = "yaml")]
    pub fn tiny_yaml() -> Vec<u8> {
        generate_yaml(1)
    }
    #[cfg(feature = "yaml")]
    pub fn small_yaml() -> Vec<u8> {
        generate_yaml(12)
    }
    #[cfg(feature = "yaml")]
    pub fn medium_yaml() -> Vec<u8> {
        generate_yaml(120)
    }
    #[cfg(feature = "yaml")]
    pub fn large_yaml() -> Vec<u8> {
        generate_yaml(1200)
    }

    #[cfg(feature = "toml")]
    pub fn tiny_toml() -> Vec<u8> {
        generate_toml(1)
    }
    #[cfg(feature = "toml")]
    pub fn small_toml() -> Vec<u8> {
        generate_toml(10)
    }
    #[cfg(feature = "toml")]
    pub fn medium_toml() -> Vec<u8> {
        generate_toml(100)
    }
    #[cfg(feature = "toml")]
    pub fn large_toml() -> Vec<u8> {
        generate_toml(1000)
    }

    #[cfg(feature = "toon")]
    pub fn tiny_toon() -> Vec<u8> {
        generate_toon(1)
    }
    #[cfg(feature = "toon")]
    pub fn small_toon() -> Vec<u8> {
        generate_toon(3)
    }
    #[cfg(feature = "toon")]
    pub fn medium_toon() -> Vec<u8> {
        generate_toon(30)
    }
    #[cfg(feature = "toon")]
    pub fn large_toon() -> Vec<u8> {
        generate_toon(300)
    }
}

// =============================================================================
// Schema Generators
// =============================================================================

mod schemas {
    use fionn_stream::skiptape::CompiledSchema;

    /// No schema (all fields pass)
    pub fn none() -> CompiledSchema {
        CompiledSchema::compile(&[]).unwrap()
    }

    /// Simple schema (5 fields)
    pub fn simple() -> CompiledSchema {
        CompiledSchema::compile(&[
            "id".to_string(),
            "name".to_string(),
            "email".to_string(),
            "active".to_string(),
            "score".to_string(),
        ])
        .unwrap()
    }

    /// Complex schema (nested paths)
    pub fn complex() -> CompiledSchema {
        CompiledSchema::compile(&[
            "id".to_string(),
            "name".to_string(),
            "user.profile.email".to_string(),
            "user.settings.active".to_string(),
            "metadata.created_at".to_string(),
            "metadata.updated_at".to_string(),
            "stats.views".to_string(),
            "stats.clicks".to_string(),
            "tags".to_string(),
            "categories".to_string(),
        ])
        .unwrap()
    }

    /// Wildcard schema
    pub fn wildcard() -> CompiledSchema {
        CompiledSchema::compile(&["*".to_string()]).unwrap()
    }
}

// =============================================================================
// Operation Generators
// =============================================================================

mod operations {
    use fionn_ops::{DsonOperation, OperationValue};

    /// No operations (parse only)
    pub fn none() -> Vec<DsonOperation> {
        vec![]
    }

    /// FieldAdd operation
    pub fn field_add() -> Vec<DsonOperation> {
        vec![DsonOperation::FieldAdd {
            path: "timestamp".to_string(),
            value: OperationValue::NumberRef("1234567890".to_string()),
        }]
    }

    /// FieldModify operation
    pub fn field_modify() -> Vec<DsonOperation> {
        vec![DsonOperation::FieldModify {
            path: "score".to_string(),
            value: OperationValue::NumberRef("999".to_string()),
        }]
    }

    /// FieldDelete operation
    pub fn field_delete() -> Vec<DsonOperation> {
        vec![DsonOperation::FieldDelete {
            path: "email".to_string(),
        }]
    }

    /// Multiple operations
    pub fn multiple() -> Vec<DsonOperation> {
        vec![
            DsonOperation::FieldAdd {
                path: "processed".to_string(),
                value: OperationValue::BoolRef(true),
            },
            DsonOperation::FieldModify {
                path: "score".to_string(),
                value: OperationValue::NumberRef("100".to_string()),
            },
            DsonOperation::FieldDelete {
                path: "email".to_string(),
            },
        ]
    }
}

// =============================================================================
// JSONL DSON Benchmarks
// =============================================================================

fn bench_jsonl_dson(c: &mut Criterion) {
    use fionn_stream::jsonl_dson::JsonlDsonProcessor;
    use std::collections::HashSet;

    let mut group = c.benchmark_group("dson_matrix/jsonl");

    let sizes = [
        ("tiny", generators::tiny_jsonl()),
        ("small", generators::small_jsonl()),
        ("medium", generators::medium_jsonl()),
    ];

    let schema_types = [
        ("no_schema", schemas::none()),
        ("simple_schema", schemas::simple()),
        ("wildcard", schemas::wildcard()),
    ];

    let ops = [
        ("parse", operations::none()),
        ("field_add", operations::field_add()),
        ("field_modify", operations::field_modify()),
        ("field_delete", operations::field_delete()),
    ];

    for (size_name, data) in &sizes {
        for (schema_name, schema) in &schema_types {
            for (op_name, operations) in &ops {
                let id = format!("{}/{}/{}", size_name, schema_name, op_name);
                group.throughput(Throughput::Bytes(data.len() as u64));

                group.bench_with_input(
                    BenchmarkId::new("process", &id),
                    &(data, schema, operations),
                    |b, (data, schema, ops)| {
                        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());
                        b.iter(|| {
                            let result = processor.process_jsonl_with_operations(
                                black_box(data),
                                black_box(schema),
                                black_box(ops),
                            );
                            hint_black_box(result);
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

// =============================================================================
// ISONL DSON Benchmarks
// =============================================================================

#[cfg(feature = "ison")]
fn bench_isonl_dson(c: &mut Criterion) {
    use fionn_stream::isonl_dson::IsonlDsonProcessor;

    let mut group = c.benchmark_group("dson_matrix/isonl");

    let sizes = [
        ("tiny", generators::tiny_isonl()),
        ("small", generators::small_isonl()),
        ("medium", generators::medium_isonl()),
    ];

    let schema_types = [
        ("no_schema", schemas::none()),
        ("wildcard", schemas::wildcard()),
    ];

    let ops = [
        ("parse", operations::none()),
        ("field_add", operations::field_add()),
    ];

    for (size_name, data) in &sizes {
        for (schema_name, schema) in &schema_types {
            for (op_name, operations) in &ops {
                let id = format!("{}/{}/{}", size_name, schema_name, op_name);
                group.throughput(Throughput::Bytes(data.len() as u64));

                group.bench_with_input(
                    BenchmarkId::new("process", &id),
                    &(data, schema, operations),
                    |b, (data, schema, ops)| {
                        let mut processor = IsonlDsonProcessor::new();
                        b.iter(|| {
                            let result = processor.process(
                                black_box(data),
                                black_box(schema),
                                black_box(ops),
                            );
                            hint_black_box(result);
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

// =============================================================================
// CSV DSON Benchmarks
// =============================================================================

#[cfg(feature = "csv")]
fn bench_csv_dson(c: &mut Criterion) {
    use fionn_stream::csv_dson::CsvDsonProcessor;

    let mut group = c.benchmark_group("dson_matrix/csv");

    let sizes = [
        ("tiny", generators::tiny_csv()),
        ("small", generators::small_csv()),
        ("medium", generators::medium_csv()),
    ];

    let schema_types = [
        ("no_schema", schemas::none()),
        ("wildcard", schemas::wildcard()),
    ];

    let ops = [
        ("parse", operations::none()),
        ("field_add", operations::field_add()),
    ];

    for (size_name, data) in &sizes {
        for (schema_name, schema) in &schema_types {
            for (op_name, operations) in &ops {
                let id = format!("{}/{}/{}", size_name, schema_name, op_name);
                group.throughput(Throughput::Bytes(data.len() as u64));

                group.bench_with_input(
                    BenchmarkId::new("process", &id),
                    &(data, schema, operations),
                    |b, (data, schema, ops)| {
                        let mut processor = CsvDsonProcessor::new();
                        b.iter(|| {
                            let result = processor.process(
                                black_box(data),
                                black_box(schema),
                                black_box(ops),
                            );
                            hint_black_box(result);
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

// =============================================================================
// YAML DSON Benchmarks
// =============================================================================

#[cfg(feature = "yaml")]
fn bench_yaml_dson(c: &mut Criterion) {
    use fionn_stream::yaml_dson::YamlDsonProcessor;

    let mut group = c.benchmark_group("dson_matrix/yaml");

    let sizes = [
        ("tiny", generators::tiny_yaml()),
        ("small", generators::small_yaml()),
        ("medium", generators::medium_yaml()),
    ];

    let schema_types = [
        ("no_schema", schemas::none()),
        ("wildcard", schemas::wildcard()),
    ];

    let ops = [
        ("parse", operations::none()),
        ("field_add", operations::field_add()),
    ];

    for (size_name, data) in &sizes {
        for (schema_name, schema) in &schema_types {
            for (op_name, operations) in &ops {
                let id = format!("{}/{}/{}", size_name, schema_name, op_name);
                group.throughput(Throughput::Bytes(data.len() as u64));

                group.bench_with_input(
                    BenchmarkId::new("process", &id),
                    &(data, schema, operations),
                    |b, (data, schema, ops)| {
                        let mut processor = YamlDsonProcessor::new();
                        b.iter(|| {
                            let result = processor.process(
                                black_box(data),
                                black_box(schema),
                                black_box(ops),
                            );
                            hint_black_box(result);
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

// =============================================================================
// TOML DSON Benchmarks
// =============================================================================

#[cfg(feature = "toml")]
fn bench_toml_dson(c: &mut Criterion) {
    use fionn_stream::toml_dson::TomlDsonProcessor;

    let mut group = c.benchmark_group("dson_matrix/toml");

    let sizes = [
        ("tiny", generators::tiny_toml()),
        ("small", generators::small_toml()),
        ("medium", generators::medium_toml()),
    ];

    let schema_types = [
        ("no_schema", schemas::none()),
        ("wildcard", schemas::wildcard()),
    ];

    let ops = [
        ("parse", operations::none()),
        ("field_add", operations::field_add()),
    ];

    for (size_name, data) in &sizes {
        for (schema_name, schema) in &schema_types {
            for (op_name, operations) in &ops {
                let id = format!("{}/{}/{}", size_name, schema_name, op_name);
                group.throughput(Throughput::Bytes(data.len() as u64));

                group.bench_with_input(
                    BenchmarkId::new("process", &id),
                    &(data, schema, operations),
                    |b, (data, schema, ops)| {
                        let mut processor = TomlDsonProcessor::new();
                        b.iter(|| {
                            let result = processor.process(
                                black_box(data),
                                black_box(schema),
                                black_box(ops),
                            );
                            hint_black_box(result);
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

// =============================================================================
// TOON DSON Benchmarks
// =============================================================================

#[cfg(feature = "toon")]
fn bench_toon_dson(c: &mut Criterion) {
    use fionn_stream::toon_dson::ToonDsonProcessor;

    let mut group = c.benchmark_group("dson_matrix/toon");

    let sizes = [
        ("tiny", generators::tiny_toon()),
        ("small", generators::small_toon()),
        ("medium", generators::medium_toon()),
    ];

    let schema_types = [
        ("no_schema", schemas::none()),
        ("wildcard", schemas::wildcard()),
    ];

    let ops = [
        ("parse", operations::none()),
        ("field_add", operations::field_add()),
    ];

    for (size_name, data) in &sizes {
        for (schema_name, schema) in &schema_types {
            for (op_name, operations) in &ops {
                let id = format!("{}/{}/{}", size_name, schema_name, op_name);
                group.throughput(Throughput::Bytes(data.len() as u64));

                group.bench_with_input(
                    BenchmarkId::new("process", &id),
                    &(data, schema, operations),
                    |b, (data, schema, ops)| {
                        let mut processor = ToonDsonProcessor::new();
                        b.iter(|| {
                            let result = processor.process(
                                black_box(data),
                                black_box(schema),
                                black_box(ops),
                            );
                            hint_black_box(result);
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

// =============================================================================
// CRDT Benchmarks
// =============================================================================

fn bench_crdt_operations(c: &mut Criterion) {
    use fionn_core::format::FormatKind;
    use fionn_stream::format_crdt::FormatCrdtProcessor;
    use fionn_stream::format_dson::FormatBatchResult;

    // Mock processor for CRDT benchmarks
    struct MockProcessor;
    impl fionn_stream::format_dson::FormatBatchProcessor for MockProcessor {
        fn format_kind(&self) -> FormatKind {
            FormatKind::Json
        }
        fn process_batch(
            &mut self,
            _data: &[u8],
            _schema: &fionn_stream::skiptape::CompiledSchema,
        ) -> fionn_core::Result<FormatBatchResult> {
            Ok(FormatBatchResult {
                documents: vec![r#"{"id":1,"name":"test","value":42}"#.to_string()],
                errors: vec![],
                statistics: Default::default(),
            })
        }
        fn process_batch_unfiltered(
            &mut self,
            _data: &[u8],
        ) -> fionn_core::Result<FormatBatchResult> {
            self.process_batch(&[], &schemas::none())
        }
        fn reset(&mut self) {}
    }

    let mut group = c.benchmark_group("dson_matrix/crdt");

    // Vector clock operations
    group.bench_function("vector_clock/increment", |b| {
        use fionn_ops::dson_traits::VectorClock;
        let mut vc = VectorClock::new();
        b.iter(|| {
            vc.increment(black_box("replica_1"));
            hint_black_box(&vc);
        });
    });

    group.bench_function("vector_clock/merge", |b| {
        use fionn_ops::dson_traits::VectorClock;
        let mut vc1 = VectorClock::new();
        let mut vc2 = VectorClock::new();
        vc1.increment("r1");
        vc2.increment("r2");
        vc2.increment("r2");
        b.iter(|| {
            let mut vc = vc1.clone();
            vc.merge(black_box(&vc2));
            hint_black_box(vc);
        });
    });

    group.bench_function("vector_clock/happened_before", |b| {
        use fionn_ops::dson_traits::VectorClock;
        let mut vc1 = VectorClock::new();
        let mut vc2 = VectorClock::new();
        vc1.increment("r1");
        vc2.increment("r1");
        vc2.increment("r1");
        b.iter(|| {
            let result = vc1.happened_before(black_box(&vc2));
            hint_black_box(result);
        });
    });

    // CRDT processor operations
    group.bench_function("crdt_processor/create", |b| {
        b.iter(|| {
            let processor = FormatCrdtProcessor::new(MockProcessor, black_box("replica_1"));
            hint_black_box(processor);
        });
    });

    group.bench_function("crdt_processor/process", |b| {
        let mut processor = FormatCrdtProcessor::new(MockProcessor, "replica_1");
        let schema = schemas::none();
        b.iter(|| {
            let result = processor.process(black_box(b"{}"), black_box(&schema));
            hint_black_box(result);
        });
    });

    // Delta generation
    group.bench_function("crdt_processor/generate_delta", |b| {
        use fionn_ops::dson_traits::{DeltaCrdt, VectorClock};
        let mut processor = FormatCrdtProcessor::new(MockProcessor, "replica_1");
        let schema = schemas::none();
        let _ = processor.process(b"{}", &schema);
        let since = VectorClock::new();
        b.iter(|| {
            let delta = processor.generate_delta(black_box(&since));
            hint_black_box(delta);
        });
    });

    group.finish();
}

// =============================================================================
// Cross-Format Comparison
// =============================================================================

fn bench_cross_format_comparison(c: &mut Criterion) {
    use fionn_stream::jsonl_dson::JsonlDsonProcessor;
    use std::collections::HashSet;

    let mut group = c.benchmark_group("dson_matrix/comparison");

    // Compare throughput across formats with same operations
    let jsonl_data = generators::small_jsonl();
    let schema = schemas::simple();
    let ops = operations::field_add();

    // JSONL baseline
    group.throughput(Throughput::Bytes(jsonl_data.len() as u64));
    group.bench_function("jsonl/small/field_add", |b| {
        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());
        b.iter(|| {
            let result = processor.process_jsonl_with_operations(
                black_box(&jsonl_data),
                black_box(&schema),
                black_box(&ops),
            );
            hint_black_box(result);
        });
    });

    #[cfg(feature = "ison")]
    {
        use fionn_stream::isonl_dson::IsonlDsonProcessor;
        let isonl_data = generators::small_isonl();
        group.throughput(Throughput::Bytes(isonl_data.len() as u64));
        group.bench_function("isonl/small/field_add", |b| {
            let mut processor = IsonlDsonProcessor::new();
            b.iter(|| {
                let result =
                    processor.process(black_box(&isonl_data), black_box(&schema), black_box(&ops));
                hint_black_box(result);
            });
        });
    }

    #[cfg(feature = "csv")]
    {
        use fionn_stream::csv_dson::CsvDsonProcessor;
        let csv_data = generators::small_csv();
        group.throughput(Throughput::Bytes(csv_data.len() as u64));
        group.bench_function("csv/small/field_add", |b| {
            let mut processor = CsvDsonProcessor::new();
            b.iter(|| {
                let result =
                    processor.process(black_box(&csv_data), black_box(&schema), black_box(&ops));
                hint_black_box(result);
            });
        });
    }

    #[cfg(feature = "yaml")]
    {
        use fionn_stream::yaml_dson::YamlDsonProcessor;
        let yaml_data = generators::small_yaml();
        group.throughput(Throughput::Bytes(yaml_data.len() as u64));
        group.bench_function("yaml/small/field_add", |b| {
            let mut processor = YamlDsonProcessor::new();
            b.iter(|| {
                let result =
                    processor.process(black_box(&yaml_data), black_box(&schema), black_box(&ops));
                hint_black_box(result);
            });
        });
    }

    #[cfg(feature = "toml")]
    {
        use fionn_stream::toml_dson::TomlDsonProcessor;
        let toml_data = generators::small_toml();
        group.throughput(Throughput::Bytes(toml_data.len() as u64));
        group.bench_function("toml/small/field_add", |b| {
            let mut processor = TomlDsonProcessor::new();
            b.iter(|| {
                let result =
                    processor.process(black_box(&toml_data), black_box(&schema), black_box(&ops));
                hint_black_box(result);
            });
        });
    }

    #[cfg(feature = "toon")]
    {
        use fionn_stream::toon_dson::ToonDsonProcessor;
        let toon_data = generators::small_toon();
        group.throughput(Throughput::Bytes(toon_data.len() as u64));
        group.bench_function("toon/small/field_add", |b| {
            let mut processor = ToonDsonProcessor::new();
            b.iter(|| {
                let result =
                    processor.process(black_box(&toon_data), black_box(&schema), black_box(&ops));
                hint_black_box(result);
            });
        });
    }

    group.finish();
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(jsonl_benches, bench_jsonl_dson,);

#[cfg(feature = "ison")]
criterion_group!(isonl_benches, bench_isonl_dson,);

#[cfg(feature = "csv")]
criterion_group!(csv_benches, bench_csv_dson,);

#[cfg(feature = "yaml")]
criterion_group!(yaml_benches, bench_yaml_dson,);

#[cfg(feature = "toml")]
criterion_group!(toml_benches, bench_toml_dson,);

#[cfg(feature = "toon")]
criterion_group!(toon_benches, bench_toon_dson,);

criterion_group!(crdt_benches, bench_crdt_operations,);

criterion_group!(comparison_benches, bench_cross_format_comparison,);

// Main entry point with all formats
#[cfg(all(
    feature = "ison",
    feature = "csv",
    feature = "yaml",
    feature = "toml",
    feature = "toon"
))]
criterion_main!(
    jsonl_benches,
    isonl_benches,
    csv_benches,
    yaml_benches,
    toml_benches,
    toon_benches,
    crdt_benches,
    comparison_benches
);

// Fallback for minimal feature set
#[cfg(not(all(
    feature = "ison",
    feature = "csv",
    feature = "yaml",
    feature = "toml",
    feature = "toon"
)))]
criterion_main!(jsonl_benches, comparison_benches);
