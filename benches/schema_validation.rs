// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Schema validation benchmarks
//!
//! Measures overhead of schema-guided operations:
//! - Schema validation during/after parse
//! - Type inference cost
//! - Schema-guided skip optimization

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_core::TapeSource;
use fionn_tape::DsonTape;
use std::hint::black_box;

// ============================================================================
// Test Data Generators
// ============================================================================

/// Generate JSON with a known schema (all fields present, correct types)
fn generate_valid_json(field_count: usize) -> String {
    let mut json = String::from("{");
    for i in 0..field_count {
        if i > 0 {
            json.push(',');
        }
        // Alternate between types to simulate realistic schemas
        match i % 5 {
            0 => json.push_str(&format!(r#""field_{i}": {i}"#)),
            1 => json.push_str(&format!(r#""field_{i}": "value_{i}""#)),
            2 => json.push_str(&format!(r#""field_{i}": {}"#, i % 2 == 0)),
            3 => json.push_str(&format!(r#""field_{i}": {}.{}"#, i, i % 100)),
            _ => json.push_str(&format!(r#""field_{i}": null"#)),
        }
    }
    json.push('}');
    json
}

/// Generate JSON with type violations (simulates invalid data)
fn generate_invalid_json(field_count: usize, error_rate: f32) -> String {
    let mut json = String::from("{");
    for i in 0..field_count {
        if i > 0 {
            json.push(',');
        }
        // Introduce type errors at the specified rate
        let is_error = (i as f32 / field_count as f32) < error_rate;
        match i % 5 {
            0 if is_error => json.push_str(&format!(r#""field_{i}": "should_be_int""#)),
            0 => json.push_str(&format!(r#""field_{i}": {i}"#)),
            1 if is_error => json.push_str(&format!(r#""field_{i}": 12345"#)),
            1 => json.push_str(&format!(r#""field_{i}": "value_{i}""#)),
            2 if is_error => json.push_str(&format!(r#""field_{i}": "not_bool""#)),
            2 => json.push_str(&format!(r#""field_{i}": {}"#, i % 2 == 0)),
            3 if is_error => json.push_str(&format!(r#""field_{i}": "not_float""#)),
            3 => json.push_str(&format!(r#""field_{i}": {}.{}"#, i, i % 100)),
            _ if is_error => json.push_str(&format!(r#""field_{i}": 0"#)),
            _ => json.push_str(&format!(r#""field_{i}": null"#)),
        }
    }
    json.push('}');
    json
}

/// Simple schema representation for benchmarking
#[derive(Clone)]
struct SimpleSchema {
    fields: Vec<(String, FieldType)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FieldType {
    Int,
    String,
    Bool,
    Float,
    Null,
    Any,
}

impl SimpleSchema {
    fn new(field_count: usize) -> Self {
        let fields = (0..field_count)
            .map(|i| {
                let field_type = match i % 5 {
                    0 => FieldType::Int,
                    1 => FieldType::String,
                    2 => FieldType::Bool,
                    3 => FieldType::Float,
                    _ => FieldType::Null,
                };
                (format!("field_{i}"), field_type)
            })
            .collect();
        Self { fields }
    }

    /// Check if a field should be extracted based on schema
    fn should_extract(&self, field_name: &str) -> bool {
        self.fields.iter().any(|(name, _)| name == field_name)
    }

    /// Get expected type for a field
    fn expected_type(&self, field_name: &str) -> Option<FieldType> {
        self.fields
            .iter()
            .find(|(name, _)| name == field_name)
            .map(|(_, t)| *t)
    }
}

// ============================================================================
// Validation Approaches
// ============================================================================

/// Validate after full parse (baseline)
fn validate_after_parse(json: &str, schema: &SimpleSchema) -> Result<(), String> {
    let val: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;

    if let serde_json::Value::Object(map) = val {
        for (field_name, expected_type) in &schema.fields {
            if let Some(value) = map.get(field_name) {
                let actual_type = match value {
                    serde_json::Value::Null => FieldType::Null,
                    serde_json::Value::Bool(_) => FieldType::Bool,
                    serde_json::Value::Number(n) => {
                        if n.is_i64() {
                            FieldType::Int
                        } else {
                            FieldType::Float
                        }
                    }
                    serde_json::Value::String(_) => FieldType::String,
                    _ => FieldType::Any,
                };

                if *expected_type != FieldType::Any && actual_type != *expected_type {
                    return Err(format!(
                        "Type mismatch for {field_name}: expected {:?}",
                        expected_type
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Validate using tape (parse once, validate without materialization)
fn validate_with_tape(json: &str, schema: &SimpleSchema) -> Result<(), String> {
    let tape = DsonTape::parse(json).map_err(|e| e.to_string())?;

    // Walk the tape and validate types
    let mut idx = 0;
    while idx < tape.len() {
        if let Some(node) = tape.node_at(idx) {
            use fionn_core::tape_source::TapeNodeKind;
            if let TapeNodeKind::Key = node.kind {
                if let Some(fionn_core::tape_source::TapeValue::String(key)) = &node.value {
                    if let Some(expected) = schema.expected_type(key) {
                        // Check next node (the value)
                        if let Some(value_node) = tape.node_at(idx + 1) {
                            let actual = match &value_node.value {
                                Some(fionn_core::tape_source::TapeValue::Null) => FieldType::Null,
                                Some(fionn_core::tape_source::TapeValue::Bool(_)) => {
                                    FieldType::Bool
                                }
                                Some(fionn_core::tape_source::TapeValue::Int(_)) => FieldType::Int,
                                Some(fionn_core::tape_source::TapeValue::Float(_)) => {
                                    FieldType::Float
                                }
                                Some(fionn_core::tape_source::TapeValue::String(_)) => {
                                    FieldType::String
                                }
                                Some(fionn_core::tape_source::TapeValue::RawNumber(_)) => {
                                    FieldType::Float
                                }
                                None => FieldType::Any,
                            };

                            if expected != FieldType::Any && actual != expected {
                                return Err(format!("Type mismatch for {key}"));
                            }
                        }
                    }
                }
            }
        }
        idx += 1;
    }
    Ok(())
}

/// Schema-guided selective parse (only extract matching fields)
fn selective_parse_with_schema(json: &str, schema: &SimpleSchema) -> usize {
    let tape = DsonTape::parse(json).unwrap();
    let mut extracted = 0;

    let mut idx = 0;
    while idx < tape.len() {
        if let Some(node) = tape.node_at(idx) {
            use fionn_core::tape_source::TapeNodeKind;
            if let TapeNodeKind::Key = node.kind {
                if let Some(fionn_core::tape_source::TapeValue::String(key)) = &node.value {
                    if schema.should_extract(key) {
                        extracted += 1;
                        idx += 1; // Include the value
                    } else {
                        // Skip value using tape skip
                        idx = tape.skip_value(idx + 1).unwrap_or(idx + 1);
                        continue;
                    }
                }
            }
        }
        idx += 1;
    }
    extracted
}

/// Full parse extracting all fields (baseline for comparison)
fn full_parse_all_fields(json: &str) -> usize {
    let val: serde_json::Value = serde_json::from_str(json).unwrap();
    if let serde_json::Value::Object(map) = val {
        map.len()
    } else {
        0
    }
}

// ============================================================================
// Type Inference
// ============================================================================

/// Infer types from tape (no schema provided)
fn infer_types_from_tape(json: &str) -> Vec<(String, FieldType)> {
    let tape = DsonTape::parse(json).unwrap();
    let mut inferred = Vec::new();

    let mut idx = 0;
    while idx < tape.len() {
        if let Some(node) = tape.node_at(idx) {
            use fionn_core::tape_source::TapeNodeKind;
            if let TapeNodeKind::Key = node.kind {
                if let Some(fionn_core::tape_source::TapeValue::String(key)) = &node.value {
                    if let Some(value_node) = tape.node_at(idx + 1) {
                        let field_type = match &value_node.value {
                            Some(fionn_core::tape_source::TapeValue::Null) => FieldType::Null,
                            Some(fionn_core::tape_source::TapeValue::Bool(_)) => FieldType::Bool,
                            Some(fionn_core::tape_source::TapeValue::Int(_)) => FieldType::Int,
                            Some(fionn_core::tape_source::TapeValue::Float(_)) => FieldType::Float,
                            Some(fionn_core::tape_source::TapeValue::String(_)) => {
                                FieldType::String
                            }
                            Some(fionn_core::tape_source::TapeValue::RawNumber(_)) => {
                                FieldType::Float
                            }
                            None => FieldType::Any,
                        };
                        inferred.push((key.to_string(), field_type));
                    }
                }
            }
        }
        idx += 1;
    }
    inferred
}

/// Infer types from serde_json::Value (baseline)
fn infer_types_from_value(json: &str) -> Vec<(String, FieldType)> {
    let val: serde_json::Value = serde_json::from_str(json).unwrap();
    let mut inferred = Vec::new();

    if let serde_json::Value::Object(map) = val {
        for (key, value) in map {
            let field_type = match value {
                serde_json::Value::Null => FieldType::Null,
                serde_json::Value::Bool(_) => FieldType::Bool,
                serde_json::Value::Number(n) => {
                    if n.is_i64() {
                        FieldType::Int
                    } else {
                        FieldType::Float
                    }
                }
                serde_json::Value::String(_) => FieldType::String,
                _ => FieldType::Any,
            };
            inferred.push((key, field_type));
        }
    }
    inferred
}

// ============================================================================
// Benchmarks
// ============================================================================

/// Benchmark: Validation overhead comparison
fn bench_validation_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema/validation_overhead");

    for field_count in [10, 50, 100, 500] {
        let json = generate_valid_json(field_count);
        let schema = SimpleSchema::new(field_count);

        group.throughput(Throughput::Bytes(json.len() as u64));

        // Baseline: serde parse + validate
        group.bench_with_input(
            BenchmarkId::new("serde_then_validate", field_count),
            &(&json, &schema),
            |b, (json, schema)| b.iter(|| validate_after_parse(black_box(json), black_box(schema))),
        );

        // Tape: parse + validate (no Value materialization)
        group.bench_with_input(
            BenchmarkId::new("tape_validate", field_count),
            &(&json, &schema),
            |b, (json, schema)| b.iter(|| validate_with_tape(black_box(json), black_box(schema))),
        );

        // Parse only (baseline overhead)
        group.bench_with_input(
            BenchmarkId::new("tape_parse_only", field_count),
            &json,
            |b, json| {
                b.iter(|| {
                    let tape = DsonTape::parse(black_box(json)).unwrap();
                    black_box(tape.len())
                })
            },
        );
    }

    group.finish();
}

/// Benchmark: Early exit on invalid data
fn bench_early_exit(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema/early_exit");

    let field_count = 100;

    for error_position in [0.1, 0.5, 0.9] {
        let json = generate_invalid_json(field_count, error_position);
        let schema = SimpleSchema::new(field_count);
        let label = format!("error_at_{:.0}pct", error_position * 100.0);

        group.throughput(Throughput::Bytes(json.len() as u64));

        // serde: must parse all before validation
        group.bench_with_input(
            BenchmarkId::new("serde_validate", &label),
            &(&json, &schema),
            |b, (json, schema)| {
                b.iter(|| {
                    let result = validate_after_parse(black_box(json), black_box(schema));
                    black_box(result.is_err())
                })
            },
        );

        // tape: can exit early on first error
        group.bench_with_input(
            BenchmarkId::new("tape_validate", &label),
            &(&json, &schema),
            |b, (json, schema)| {
                b.iter(|| {
                    let result = validate_with_tape(black_box(json), black_box(schema));
                    black_box(result.is_err())
                })
            },
        );
    }

    group.finish();
}

/// Benchmark: Selective extraction with schema
fn bench_selective_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema/selective_extraction");

    let field_count = 100;
    let json = generate_valid_json(field_count);

    // Create schemas that extract different percentages of fields
    for extract_pct in [10, 25, 50, 100] {
        let schema = SimpleSchema {
            fields: (0..field_count)
                .filter(|i| (*i as usize) < (field_count * extract_pct / 100))
                .map(|i| {
                    let field_type = match i % 5 {
                        0 => FieldType::Int,
                        1 => FieldType::String,
                        2 => FieldType::Bool,
                        3 => FieldType::Float,
                        _ => FieldType::Null,
                    };
                    (format!("field_{i}"), field_type)
                })
                .collect(),
        };

        group.throughput(Throughput::Bytes(json.len() as u64));

        // Schema-guided selective parse
        group.bench_with_input(
            BenchmarkId::new("tape_selective", extract_pct),
            &(&json, &schema),
            |b, (json, schema)| {
                b.iter(|| selective_parse_with_schema(black_box(json), black_box(schema)))
            },
        );

        // Full parse baseline
        group.bench_with_input(
            BenchmarkId::new("serde_full_parse", extract_pct),
            &json,
            |b, json| b.iter(|| full_parse_all_fields(black_box(json))),
        );
    }

    group.finish();
}

/// Benchmark: Type inference overhead
fn bench_type_inference(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema/type_inference");

    for field_count in [10, 50, 100] {
        let json = generate_valid_json(field_count);

        group.throughput(Throughput::Bytes(json.len() as u64));

        // Tape-based inference
        group.bench_with_input(
            BenchmarkId::new("tape_infer", field_count),
            &json,
            |b, json| b.iter(|| infer_types_from_tape(black_box(json))),
        );

        // serde-based inference
        group.bench_with_input(
            BenchmarkId::new("serde_infer", field_count),
            &json,
            |b, json| b.iter(|| infer_types_from_value(black_box(json))),
        );
    }

    group.finish();
}

/// Benchmark: Nested structure validation
fn bench_nested_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema/nested_validation");

    // Generate nested JSON
    fn generate_nested_json(depth: usize, width: usize) -> String {
        if depth == 0 {
            return r#"{"value": 42}"#.to_string();
        }

        let inner = generate_nested_json(depth - 1, width);
        let mut json = String::from("{");
        for i in 0..width {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(r#""level_{depth}_field_{i}": {inner}"#));
        }
        json.push('}');
        json
    }

    for depth in [2, 4, 6] {
        let json = generate_nested_json(depth, 3);

        group.throughput(Throughput::Bytes(json.len() as u64));

        // Tape parse
        group.bench_with_input(BenchmarkId::new("tape_parse", depth), &json, |b, json| {
            b.iter(|| {
                let tape = DsonTape::parse(black_box(json)).unwrap();
                black_box(tape.len())
            })
        });

        // serde parse
        group.bench_with_input(BenchmarkId::new("serde_parse", depth), &json, |b, json| {
            b.iter(|| {
                let val: serde_json::Value = serde_json::from_str(black_box(json)).unwrap();
                black_box(val.is_object())
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_validation_overhead,
    bench_early_exit,
    bench_selective_extraction,
    bench_type_inference,
    bench_nested_validation,
);

criterion_main!(benches);
