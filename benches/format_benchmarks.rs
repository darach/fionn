// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Helper functions may be defined for future benchmarks
#![allow(dead_code)]
//! Format Parser Benchmarks
//!
//! Comprehensive throughput and latency benchmarks for multi-format parsing:
//! - YAML, TOML, CSV, ISON, TOON format parsers
//! - Size classes: tiny (<100B), small (100B-1KB), medium (1KB-10KB), large (10KB-100KB)
//! - Structural complexity: flat, nested, wide, deep
//! - Format-specific features: anchors, sections, tabular data, references

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::hint::black_box as hint_black_box;

// =============================================================================
// Test Data Generation
// =============================================================================

mod generators {
    //! Test data generators for equivalent cross-format benchmarks

    /// Generate JSON test data of specified size and structure
    pub fn generate_json_flat(field_count: usize) -> String {
        let mut json = String::from("{");
        for i in 0..field_count {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!("\"field{}\":{}", i, i * 10));
        }
        json.push('}');
        json
    }

    #[allow(dead_code)] // Benchmark helper used selectively
    pub fn generate_json_nested(depth: usize, width: usize) -> String {
        fn nested_object(depth: usize, width: usize) -> String {
            if depth == 0 {
                return "\"leaf\"".to_string();
            }
            let mut obj = String::from("{");
            for i in 0..width {
                if i > 0 {
                    obj.push(',');
                }
                obj.push_str(&format!("\"k{}\":{}", i, nested_object(depth - 1, width)));
            }
            obj.push('}');
            obj
        }
        nested_object(depth, width)
    }

    #[allow(dead_code)] // Benchmark helper used selectively
    pub fn generate_json_wide(field_count: usize) -> String {
        let mut json = String::from("{");
        for i in 0..field_count {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "\"field_with_longer_name_{}\":\"value_with_some_content_{}\"",
                i, i
            ));
        }
        json.push('}');
        json
    }

    pub fn generate_json_array(count: usize) -> String {
        let mut json = String::from("[");
        for i in 0..count {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{{\"id\":{},\"name\":\"user{}\",\"active\":{}}}",
                i,
                i,
                i % 2 == 0
            ));
        }
        json.push(']');
        json
    }

    /// Generate YAML test data equivalent to JSON
    pub fn generate_yaml_flat(field_count: usize) -> String {
        let mut yaml = String::new();
        for i in 0..field_count {
            yaml.push_str(&format!("field{}: {}\n", i, i * 10));
        }
        yaml
    }

    pub fn generate_yaml_nested(depth: usize, width: usize) -> String {
        fn nested_yaml(depth: usize, width: usize, indent: usize) -> String {
            if depth == 0 {
                return "leaf".to_string();
            }
            let mut yaml = String::new();
            let spaces = " ".repeat(indent);
            for i in 0..width {
                yaml.push_str(&format!(
                    "{}k{}:\n{}\n",
                    spaces,
                    i,
                    nested_yaml(depth - 1, width, indent + 2)
                ));
            }
            yaml
        }
        nested_yaml(depth, width, 0)
    }

    pub fn generate_yaml_with_anchors(record_count: usize) -> String {
        let mut yaml =
            String::from("defaults: &defaults\n  active: true\n  role: user\n\nusers:\n");
        for i in 0..record_count {
            yaml.push_str(&format!(
                "  - id: {}\n    name: user{}\n    <<: *defaults\n",
                i, i
            ));
        }
        yaml
    }

    /// Generate TOML test data equivalent to JSON
    pub fn generate_toml_flat(field_count: usize) -> String {
        let mut toml = String::new();
        for i in 0..field_count {
            toml.push_str(&format!("field{} = {}\n", i, i * 10));
        }
        toml
    }

    pub fn generate_toml_nested(section_count: usize, field_count: usize) -> String {
        let mut toml = String::new();
        for s in 0..section_count {
            toml.push_str(&format!("[section{}]\n", s));
            for f in 0..field_count {
                toml.push_str(&format!("field{} = {}\n", f, s * 100 + f));
            }
            toml.push('\n');
        }
        toml
    }

    pub fn generate_toml_with_inline_tables(count: usize) -> String {
        let mut toml = String::new();
        for i in 0..count {
            toml.push_str(&format!(
                "record{} = {{ id = {}, name = \"user{}\", active = {} }}\n",
                i,
                i,
                i,
                i % 2 == 0
            ));
        }
        toml
    }

    /// Generate CSV test data
    pub fn generate_csv_records(row_count: usize, col_count: usize) -> String {
        let mut csv = String::new();
        // Header
        for c in 0..col_count {
            if c > 0 {
                csv.push(',');
            }
            csv.push_str(&format!("col{}", c));
        }
        csv.push('\n');
        // Data rows
        for r in 0..row_count {
            for c in 0..col_count {
                if c > 0 {
                    csv.push(',');
                }
                csv.push_str(&format!("val_{}_{}", r, c));
            }
            csv.push('\n');
        }
        csv
    }

    pub fn generate_csv_quoted(row_count: usize) -> String {
        let mut csv = String::from("id,name,description\n");
        for r in 0..row_count {
            csv.push_str(&format!(
                "{},\"User {}\",\"Description with, comma and \"\"quotes\"\"\"\n",
                r, r
            ));
        }
        csv
    }

    /// Generate ISON test data
    pub fn generate_ison_table(record_count: usize) -> String {
        let mut ison = String::from("table.users\nid:int name:string email:string active:bool\n");
        for i in 0..record_count {
            ison.push_str(&format!(
                "{} user{} user{}@example.com {}\n",
                i,
                i,
                i,
                i % 2 == 0
            ));
        }
        ison
    }

    pub fn generate_ison_with_refs(record_count: usize) -> String {
        let mut ison = String::from("table.users\nid:int name:string\n");
        for i in 0..record_count {
            ison.push_str(&format!("{} user{}\n", i, i));
        }
        ison.push_str("\ntable.posts\nid:int author:user title:string\n");
        for i in 0..record_count {
            ison.push_str(&format!(
                "{} :user:{} \"Post {}\"\n",
                i,
                i % record_count,
                i
            ));
        }
        ison
    }

    /// Generate TOON test data
    pub fn generate_toon_basic(field_count: usize) -> String {
        let mut toon = String::new();
        for i in 0..field_count {
            toon.push_str(&format!("field{}: {}\n", i, i * 10));
        }
        toon
    }

    pub fn generate_toon_tabular(row_count: usize, col_count: usize) -> String {
        let mut toon = format!("data[{}]{{", row_count);
        for c in 0..col_count {
            if c > 0 {
                toon.push(',');
            }
            toon.push_str(&format!("col{}", c));
        }
        toon.push_str("}:\n");
        for r in 0..row_count {
            toon.push_str("  ");
            for c in 0..col_count {
                if c > 0 {
                    toon.push(',');
                }
                toon.push_str(&format!("val{}_{}", r, c));
            }
            toon.push('\n');
        }
        toon
    }

    pub fn generate_toon_folded_keys(depth: usize, width: usize) -> String {
        let mut toon = String::new();
        for w in 0..width {
            let path: String = (0..depth)
                .map(|d| format!("level{}", d))
                .collect::<Vec<_>>()
                .join(".");
            toon.push_str(&format!("{}.field{}: value{}\n", path, w, w));
        }
        toon
    }

    /// Size class generators
    pub fn tiny_json() -> String {
        r#"{"name":"test","value":42}"#.to_string()
    }

    pub fn small_json() -> String {
        generate_json_flat(20)
    }

    pub fn medium_json() -> String {
        generate_json_array(100)
    }

    pub fn large_json() -> String {
        generate_json_array(1000)
    }

    pub fn tiny_yaml() -> String {
        "name: test\nvalue: 42\n".to_string()
    }

    pub fn small_yaml() -> String {
        generate_yaml_flat(20)
    }

    pub fn medium_yaml() -> String {
        generate_yaml_with_anchors(50)
    }

    pub fn large_yaml() -> String {
        generate_yaml_with_anchors(500)
    }

    pub fn tiny_toml() -> String {
        "name = \"test\"\nvalue = 42\n".to_string()
    }

    pub fn small_toml() -> String {
        generate_toml_flat(20)
    }

    pub fn medium_toml() -> String {
        generate_toml_nested(10, 10)
    }

    pub fn large_toml() -> String {
        generate_toml_nested(50, 20)
    }

    pub fn tiny_csv() -> String {
        "a,b,c\n1,2,3\n".to_string()
    }

    pub fn small_csv() -> String {
        generate_csv_records(20, 5)
    }

    pub fn medium_csv() -> String {
        generate_csv_records(100, 10)
    }

    pub fn large_csv() -> String {
        generate_csv_records(1000, 10)
    }

    pub fn tiny_ison() -> String {
        "object.test\nname value\ntest 42\n".to_string()
    }

    pub fn small_ison() -> String {
        generate_ison_table(20)
    }

    pub fn medium_ison() -> String {
        generate_ison_table(100)
    }

    pub fn large_ison() -> String {
        generate_ison_with_refs(500)
    }

    pub fn tiny_toon() -> String {
        "name: test\nvalue: 42\n".to_string()
    }

    pub fn small_toon() -> String {
        generate_toon_basic(20)
    }

    pub fn medium_toon() -> String {
        generate_toon_tabular(50, 10)
    }

    pub fn large_toon() -> String {
        generate_toon_folded_keys(5, 100)
    }
}

// =============================================================================
// JSON Baseline Benchmarks (Reference)
// =============================================================================

fn bench_json_baseline(c: &mut Criterion) {
    use fionn_tape::DsonTape;

    let mut group = c.benchmark_group("format/json/baseline");

    let test_cases = [
        ("tiny", generators::tiny_json()),
        ("small", generators::small_json()),
        ("medium", generators::medium_json()),
        ("large", generators::large_json()),
    ];

    for (name, data) in &test_cases {
        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_with_input(BenchmarkId::new("fionn", name), data, |b, data| {
            b.iter(|| {
                let tape = DsonTape::parse(black_box(data)).unwrap();
                hint_black_box(tape);
            });
        });

        group.bench_with_input(BenchmarkId::new("serde_json", name), data, |b, data| {
            b.iter(|| {
                let _: serde_json::Value = serde_json::from_str(black_box(data)).unwrap();
            });
        });
    }

    group.finish();
}

// =============================================================================
// YAML Format Benchmarks
// =============================================================================

#[cfg(feature = "yaml")]
fn bench_yaml_parsing(c: &mut Criterion) {
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::YamlParser;

    let mut group = c.benchmark_group("format/yaml/parsing");

    let test_cases = [
        ("tiny", generators::tiny_yaml()),
        ("small", generators::small_yaml()),
        ("medium", generators::medium_yaml()),
        ("large", generators::large_yaml()),
    ];

    for (name, data) in &test_cases {
        let bytes = data.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // SIMD chunk scanning
        group.bench_with_input(BenchmarkId::new("simd_scan", name), bytes, |b, bytes| {
            let parser = YamlParser::new();
            b.iter(|| {
                let mut pos = 0;
                while pos + 64 <= bytes.len() {
                    let chunk: &[u8; 64] = bytes[pos..pos + 64].try_into().unwrap();
                    let mask = parser.scan_chunk(chunk);
                    hint_black_box(mask);
                    pos += 64;
                }
            });
        });

        // Structural parsing
        group.bench_with_input(BenchmarkId::new("structural", name), bytes, |b, bytes| {
            let parser = YamlParser::new();
            b.iter(|| {
                let positions = parser.parse_structural(black_box(bytes)).unwrap();
                hint_black_box(positions);
            });
        });

        // Indentation detection
        group.bench_with_input(BenchmarkId::new("indent", name), bytes, |b, bytes| {
            b.iter(|| {
                let mut indent_sum = 0usize;
                for line in bytes.split(|&b| b == b'\n') {
                    indent_sum += YamlParser::count_indent(line);
                }
                hint_black_box(indent_sum);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "yaml")]
fn bench_yaml_features(c: &mut Criterion) {
    use fionn_simd::formats::YamlParser;

    let mut group = c.benchmark_group("format/yaml/features");

    // Anchor detection benchmark
    let yaml_with_anchors = generators::generate_yaml_with_anchors(100);
    let bytes = yaml_with_anchors.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    group.bench_function("anchor_detection", |b| {
        b.iter(|| {
            let mut anchors = Vec::new();
            for (i, &byte) in bytes.iter().enumerate() {
                if byte == b'&' {
                    if let Some(name) = YamlParser::detect_anchor(bytes, i) {
                        anchors.push(name);
                    }
                }
            }
            hint_black_box(anchors);
        });
    });

    group.bench_function("alias_detection", |b| {
        b.iter(|| {
            let mut aliases = Vec::new();
            for (i, &byte) in bytes.iter().enumerate() {
                if byte == b'*' {
                    if let Some(name) = YamlParser::detect_alias(bytes, i) {
                        aliases.push(name);
                    }
                }
            }
            hint_black_box(aliases);
        });
    });

    group.bench_function("merge_key_detection", |b| {
        b.iter(|| {
            let mut merge_count = 0;
            for line in bytes.split(|&b| b == b'\n') {
                if YamlParser::detect_merge_key(line) {
                    merge_count += 1;
                }
            }
            hint_black_box(merge_count);
        });
    });

    group.finish();
}

// =============================================================================
// TOML Format Benchmarks
// =============================================================================

#[cfg(feature = "toml")]
fn bench_toml_parsing(c: &mut Criterion) {
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::TomlParser;

    let mut group = c.benchmark_group("format/toml/parsing");

    let test_cases = [
        ("tiny", generators::tiny_toml()),
        ("small", generators::small_toml()),
        ("medium", generators::medium_toml()),
        ("large", generators::large_toml()),
    ];

    for (name, data) in &test_cases {
        let bytes = data.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // SIMD chunk scanning
        group.bench_with_input(BenchmarkId::new("simd_scan", name), bytes, |b, bytes| {
            let parser = TomlParser::new();
            b.iter(|| {
                let mut pos = 0;
                while pos + 64 <= bytes.len() {
                    let chunk: &[u8; 64] = bytes[pos..pos + 64].try_into().unwrap();
                    let mask = parser.scan_chunk(chunk);
                    hint_black_box(mask);
                    pos += 64;
                }
            });
        });

        // Structural parsing
        group.bench_with_input(BenchmarkId::new("structural", name), bytes, |b, bytes| {
            let parser = TomlParser::new();
            b.iter(|| {
                let positions = parser.parse_structural(black_box(bytes)).unwrap();
                hint_black_box(positions);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "toml")]
fn bench_toml_features(c: &mut Criterion) {
    use fionn_simd::formats::TomlParser;

    let mut group = c.benchmark_group("format/toml/features");

    // Section header detection
    let toml_nested = generators::generate_toml_nested(50, 10);
    let bytes = toml_nested.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    group.bench_function("section_detection", |b| {
        b.iter(|| {
            let mut sections = Vec::new();
            for line in bytes.split(|&b| b == b'\n') {
                if let Some(header) = TomlParser::parse_section_header(line) {
                    sections.push(header);
                }
            }
            hint_black_box(sections);
        });
    });

    // Inline table detection
    let toml_inline = generators::generate_toml_with_inline_tables(50);
    let inline_bytes = toml_inline.as_bytes();

    group.bench_function("inline_table_detection", |b| {
        b.iter(|| {
            let mut count = 0;
            for line in inline_bytes.split(|&b| b == b'\n') {
                if TomlParser::detect_inline_table(line) {
                    count += 1;
                }
            }
            hint_black_box(count);
        });
    });

    // Dotted key parsing
    group.bench_function("dotted_key_parsing", |b| {
        let dotted = b"database.connection.pool.size";
        b.iter(|| {
            let path = TomlParser::parse_dotted_key(black_box(dotted));
            hint_black_box(path);
        });
    });

    group.finish();
}

// =============================================================================
// CSV Format Benchmarks
// =============================================================================

#[cfg(feature = "csv")]
fn bench_csv_parsing(c: &mut Criterion) {
    use fionn_simd::formats::CsvParser;
    use fionn_simd::formats::FormatParser;

    let mut group = c.benchmark_group("format/csv/parsing");

    let test_cases = [
        ("tiny", generators::tiny_csv()),
        ("small", generators::small_csv()),
        ("medium", generators::medium_csv()),
        ("large", generators::large_csv()),
    ];

    for (name, data) in &test_cases {
        let bytes = data.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // SIMD chunk scanning
        group.bench_with_input(BenchmarkId::new("simd_scan", name), bytes, |b, bytes| {
            let parser = CsvParser::new();
            b.iter(|| {
                let mut pos = 0;
                while pos + 64 <= bytes.len() {
                    let chunk: &[u8; 64] = bytes[pos..pos + 64].try_into().unwrap();
                    let mask = parser.scan_chunk(chunk);
                    hint_black_box(mask);
                    pos += 64;
                }
            });
        });

        // Structural parsing
        group.bench_with_input(BenchmarkId::new("structural", name), bytes, |b, bytes| {
            let parser = CsvParser::new();
            b.iter(|| {
                let positions = parser.parse_structural(black_box(bytes)).unwrap();
                hint_black_box(positions);
            });
        });

        // Field counting
        group.bench_with_input(BenchmarkId::new("field_count", name), bytes, |b, bytes| {
            b.iter(|| {
                let mut field_count = 0;
                for line in bytes.split(|&b| b == b'\n') {
                    field_count += CsvParser::count_fields_with_delimiter(line, b',');
                }
                hint_black_box(field_count);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "csv")]
fn bench_csv_features(c: &mut Criterion) {
    use fionn_simd::formats::CsvParser;

    let mut group = c.benchmark_group("format/csv/features");

    // Quote handling benchmark
    let csv_quoted = generators::generate_csv_quoted(100);
    let bytes = csv_quoted.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    group.bench_function("quote_detection", |b| {
        b.iter(|| {
            let mut quoted_count = 0;
            for line in bytes.split(|&b| b == b'\n') {
                for field in CsvParser::split_fields(line, b',') {
                    if CsvParser::is_quoted_field(field) {
                        quoted_count += 1;
                    }
                }
            }
            hint_black_box(quoted_count);
        });
    });

    // Delimiter detection
    group.bench_function("delimiter_detection", |b| {
        let sample_lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').take(10).collect();
        b.iter(|| {
            let delimiter = CsvParser::detect_delimiter(black_box(&sample_lines));
            hint_black_box(delimiter);
        });
    });

    group.finish();
}

// =============================================================================
// ISON Format Benchmarks
// =============================================================================

#[cfg(feature = "ison")]
fn bench_ison_parsing(c: &mut Criterion) {
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::IsonParser;

    let mut group = c.benchmark_group("format/ison/parsing");

    let test_cases = [
        ("tiny", generators::tiny_ison()),
        ("small", generators::small_ison()),
        ("medium", generators::medium_ison()),
        ("large", generators::large_ison()),
    ];

    for (name, data) in &test_cases {
        let bytes = data.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // SIMD chunk scanning
        group.bench_with_input(BenchmarkId::new("simd_scan", name), bytes, |b, bytes| {
            let parser = IsonParser::new();
            b.iter(|| {
                let mut pos = 0;
                while pos + 64 <= bytes.len() {
                    let chunk: &[u8; 64] = bytes[pos..pos + 64].try_into().unwrap();
                    let mask = parser.scan_chunk(chunk);
                    hint_black_box(mask);
                    pos += 64;
                }
            });
        });

        // Structural parsing
        group.bench_with_input(BenchmarkId::new("structural", name), bytes, |b, bytes| {
            let parser = IsonParser::new();
            b.iter(|| {
                let positions = parser.parse_structural(black_box(bytes)).unwrap();
                hint_black_box(positions);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "ison")]
fn bench_ison_features(c: &mut Criterion) {
    use fionn_simd::formats::IsonParser;

    let mut group = c.benchmark_group("format/ison/features");

    // Block header parsing
    let ison_data = generators::large_ison();
    let bytes = ison_data.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    group.bench_function("block_header_parsing", |b| {
        b.iter(|| {
            let mut blocks = Vec::new();
            for line in bytes.split(|&b| b == b'\n') {
                if let Some(header) = IsonParser::parse_block_header(line) {
                    blocks.push(header);
                }
            }
            hint_black_box(blocks);
        });
    });

    // Reference parsing
    group.bench_function("reference_parsing", |b| {
        let refs = [":1", ":user:1", ":BELONGS_TO:42", ":product:abc123"];
        b.iter(|| {
            let mut parsed = Vec::new();
            for r in &refs {
                if let Some(reference) = IsonParser::parse_reference(black_box(r)) {
                    parsed.push(reference);
                }
            }
            hint_black_box(parsed);
        });
    });

    // Field declaration parsing
    group.bench_function("field_declaration", |b| {
        let decl = b"id:int name:string email:string active:bool score:float";
        b.iter(|| {
            let fields = IsonParser::parse_field_declaration(black_box(decl));
            hint_black_box(fields);
        });
    });

    // Data row parsing
    group.bench_function("data_row_parsing", |b| {
        b.iter(|| {
            let mut rows = Vec::new();
            for line in bytes.split(|&b| b == b'\n') {
                if !IsonParser::is_comment(line) && IsonParser::parse_block_header(line).is_none() {
                    let values = IsonParser::parse_data_row(line);
                    if !values.is_empty() {
                        rows.push(values);
                    }
                }
            }
            hint_black_box(rows);
        });
    });

    group.finish();
}

// =============================================================================
// TOON Format Benchmarks
// =============================================================================

#[cfg(feature = "toon")]
fn bench_toon_parsing(c: &mut Criterion) {
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::ToonParser;

    let mut group = c.benchmark_group("format/toon/parsing");

    let test_cases = [
        ("tiny", generators::tiny_toon()),
        ("small", generators::small_toon()),
        ("medium", generators::medium_toon()),
        ("large", generators::large_toon()),
    ];

    for (name, data) in &test_cases {
        let bytes = data.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        // SIMD chunk scanning
        group.bench_with_input(BenchmarkId::new("simd_scan", name), bytes, |b, bytes| {
            let parser = ToonParser::new();
            b.iter(|| {
                let mut pos = 0;
                while pos + 64 <= bytes.len() {
                    let chunk: &[u8; 64] = bytes[pos..pos + 64].try_into().unwrap();
                    let mask = parser.scan_chunk(chunk);
                    hint_black_box(mask);
                    pos += 64;
                }
            });
        });

        // Structural parsing
        group.bench_with_input(BenchmarkId::new("structural", name), bytes, |b, bytes| {
            let parser = ToonParser::new();
            b.iter(|| {
                let positions = parser.parse_structural(black_box(bytes)).unwrap();
                hint_black_box(positions);
            });
        });

        // Indentation
        group.bench_with_input(BenchmarkId::new("indent", name), bytes, |b, bytes| {
            b.iter(|| {
                let mut indent_sum = 0usize;
                for line in bytes.split(|&b| b == b'\n') {
                    indent_sum += ToonParser::count_indent(line);
                }
                hint_black_box(indent_sum);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "toon")]
fn bench_toon_features(c: &mut Criterion) {
    use fionn_simd::formats::ToonParser;

    let mut group = c.benchmark_group("format/toon/features");

    // Tabular array header parsing
    let toon_tabular = generators::generate_toon_tabular(100, 10);
    let bytes = toon_tabular.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    group.bench_function("array_header_parsing", |b| {
        b.iter(|| {
            let mut headers = Vec::new();
            for line in bytes.split(|&b| b == b'\n') {
                if let Some(header) = ToonParser::parse_array_header(line) {
                    headers.push(header);
                }
            }
            hint_black_box(headers);
        });
    });

    // Key folding/expansion
    let toon_folded = generators::generate_toon_folded_keys(5, 50);
    let folded_bytes = toon_folded.as_bytes();

    group.bench_function("key_folding", |b| {
        b.iter(|| {
            let mut paths = Vec::new();
            for line in folded_bytes.split(|&b| b == b'\n') {
                if let Some(path) = ToonParser::parse_folded_key(line) {
                    paths.push(path);
                }
            }
            hint_black_box(paths);
        });
    });

    group.finish();
}

// =============================================================================
// Cross-Format Comparison Benchmarks
// =============================================================================

fn bench_format_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("format/comparison");

    // Medium-sized equivalent data across formats
    let json_data = generators::medium_json();
    let yaml_data = generators::medium_yaml();
    let toml_data = generators::medium_toml();
    let csv_data = generators::medium_csv();
    let ison_data = generators::medium_ison();
    let toon_data = generators::medium_toon();

    // Report bytes for each
    group.throughput(Throughput::Elements(1));

    // JSON baseline
    group.bench_function("json/fionn", |b| {
        b.iter(|| {
            let tape = fionn_tape::DsonTape::parse(black_box(&json_data)).unwrap();
            hint_black_box(tape);
        });
    });

    // YAML
    #[cfg(feature = "yaml")]
    group.bench_function("yaml/simd", |b| {
        use fionn_simd::formats::{FormatParser, YamlParser};
        let parser = YamlParser::new();
        let bytes = yaml_data.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    // TOML
    #[cfg(feature = "toml")]
    group.bench_function("toml/simd", |b| {
        use fionn_simd::formats::{FormatParser, TomlParser};
        let parser = TomlParser::new();
        let bytes = toml_data.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    // CSV
    #[cfg(feature = "csv")]
    group.bench_function("csv/simd", |b| {
        use fionn_simd::formats::{CsvParser, FormatParser};
        let parser = CsvParser::new();
        let bytes = csv_data.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    // ISON
    #[cfg(feature = "ison")]
    group.bench_function("ison/simd", |b| {
        use fionn_simd::formats::{FormatParser, IsonParser};
        let parser = IsonParser::new();
        let bytes = ison_data.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    // TOON
    #[cfg(feature = "toon")]
    group.bench_function("toon/simd", |b| {
        use fionn_simd::formats::{FormatParser, ToonParser};
        let parser = ToonParser::new();
        let bytes = toon_data.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    group.finish();
}

// =============================================================================
// Scaling Benchmarks
// =============================================================================

fn bench_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("format/scaling");

    // Test scaling with increasing data sizes
    let sizes = [100, 500, 1000, 5000, 10000];

    for &size in &sizes {
        let json = generators::generate_json_array(size);
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("json/records", size), &json, |b, json| {
            b.iter(|| {
                let tape = fionn_tape::DsonTape::parse(black_box(json)).unwrap();
                hint_black_box(tape);
            });
        });

        #[cfg(feature = "csv")]
        {
            let csv = generators::generate_csv_records(size, 10);
            group.bench_with_input(BenchmarkId::new("csv/records", size), &csv, |b, csv| {
                use fionn_simd::formats::{CsvParser, FormatParser};
                let parser = CsvParser::new();
                let bytes = csv.as_bytes();
                b.iter(|| {
                    let positions = parser.parse_structural(black_box(bytes)).unwrap();
                    hint_black_box(positions);
                });
            });
        }

        #[cfg(feature = "ison")]
        {
            let ison = generators::generate_ison_table(size);
            group.bench_with_input(BenchmarkId::new("ison/records", size), &ison, |b, ison| {
                use fionn_simd::formats::{FormatParser, IsonParser};
                let parser = IsonParser::new();
                let bytes = ison.as_bytes();
                b.iter(|| {
                    let positions = parser.parse_structural(black_box(bytes)).unwrap();
                    hint_black_box(positions);
                });
            });
        }
    }

    group.finish();
}

// =============================================================================
// Latency Benchmarks (p50, p95, p99 via criterion)
// =============================================================================

fn bench_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("format/latency");
    group.sample_size(1000); // More samples for latency distribution

    // Tiny inputs for latency measurement
    let json = generators::tiny_json();
    let yaml = generators::tiny_yaml();
    let toml = generators::tiny_toml();
    let csv = generators::tiny_csv();
    let ison = generators::tiny_ison();
    let toon = generators::tiny_toon();

    group.bench_function("json/tiny", |b| {
        b.iter(|| {
            let tape = fionn_tape::DsonTape::parse(black_box(&json)).unwrap();
            hint_black_box(tape);
        });
    });

    #[cfg(feature = "yaml")]
    group.bench_function("yaml/tiny", |b| {
        use fionn_simd::formats::{FormatParser, YamlParser};
        let parser = YamlParser::new();
        let bytes = yaml.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    #[cfg(feature = "toml")]
    group.bench_function("toml/tiny", |b| {
        use fionn_simd::formats::{FormatParser, TomlParser};
        let parser = TomlParser::new();
        let bytes = toml.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    #[cfg(feature = "csv")]
    group.bench_function("csv/tiny", |b| {
        use fionn_simd::formats::{CsvParser, FormatParser};
        let parser = CsvParser::new();
        let bytes = csv.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    #[cfg(feature = "ison")]
    group.bench_function("ison/tiny", |b| {
        use fionn_simd::formats::{FormatParser, IsonParser};
        let parser = IsonParser::new();
        let bytes = ison.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    #[cfg(feature = "toon")]
    group.bench_function("toon/tiny", |b| {
        use fionn_simd::formats::{FormatParser, ToonParser};
        let parser = ToonParser::new();
        let bytes = toon.as_bytes();
        b.iter(|| {
            let positions = parser.parse_structural(black_box(bytes)).unwrap();
            hint_black_box(positions);
        });
    });

    group.finish();
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(json_benches, bench_json_baseline,);

#[cfg(feature = "yaml")]
criterion_group!(yaml_benches, bench_yaml_parsing, bench_yaml_features,);

#[cfg(feature = "toml")]
criterion_group!(toml_benches, bench_toml_parsing, bench_toml_features,);

#[cfg(feature = "csv")]
criterion_group!(csv_benches, bench_csv_parsing, bench_csv_features,);

#[cfg(feature = "ison")]
criterion_group!(ison_benches, bench_ison_parsing, bench_ison_features,);

#[cfg(feature = "toon")]
criterion_group!(toon_benches, bench_toon_parsing, bench_toon_features,);

criterion_group!(
    comparison_benches,
    bench_format_comparison,
    bench_scaling,
    bench_latency,
);

// Main entry point with conditional compilation
#[cfg(all(
    feature = "yaml",
    feature = "toml",
    feature = "csv",
    feature = "ison",
    feature = "toon"
))]
criterion_main!(
    json_benches,
    yaml_benches,
    toml_benches,
    csv_benches,
    ison_benches,
    toon_benches,
    comparison_benches
);

#[cfg(not(all(
    feature = "yaml",
    feature = "toml",
    feature = "csv",
    feature = "ison",
    feature = "toon"
)))]
criterion_main!(json_benches, comparison_benches);
