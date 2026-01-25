#![allow(clippy::all)]
#![allow(clippy::pedantic)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! AFL fuzz target for multi-format SIMD parsers.
//!
//! This target tests YAML, TOML, CSV, ISON, and TOON format parsers
//! with arbitrary input to find edge cases and potential crashes.
//!
//! Run with:
//!   cargo afl build --release --features all-formats --bin fuzz_formats
//!   cargo afl fuzz -i fuzz/corpus/formats -o fuzz/output/formats target/release/fuzz_formats

#[macro_use]
extern crate afl;

use fionn_simd::formats::FormatParser;

#[cfg(feature = "yaml")]
use fionn_simd::formats::yaml::YamlParser;

#[cfg(feature = "toml")]
use fionn_simd::formats::toml::TomlParser;

#[cfg(feature = "csv")]
use fionn_simd::formats::csv::CsvParser;

#[cfg(feature = "ison")]
use fionn_simd::formats::ison::IsonParser;

#[cfg(feature = "toon")]
use fionn_simd::formats::toon::ToonParser;

/// Test YAML parsing with arbitrary input
#[cfg(feature = "yaml")]
fn fuzz_yaml(data: &[u8]) {
    let parser = YamlParser::new();

    // Test structural parsing
    let _ = parser.parse_structural(data);

    // Test indentation detection at various positions
    for pos in (0..data.len()).step_by(8) {
        let _ = parser.detect_indent(data, pos);
    }

    // Test string/comment detection
    for pos in (0..data.len()).step_by(8) {
        let _ = parser.is_in_string(data, pos);
        let _ = parser.is_in_comment(data, pos);
    }

    // Test chunk scanning (64-byte chunks)
    if data.len() >= 64 {
        let chunk: [u8; 64] = data[..64].try_into().unwrap();
        let _ = parser.scan_chunk(&chunk);
    }

    // Test anchor detection at various positions
    for pos in 0..data.len().min(100) {
        let _ = YamlParser::detect_anchor(data, pos);
        let _ = YamlParser::detect_alias(data, pos);
    }

    // Test document marker detection
    let _ = YamlParser::detect_document_marker(data);

    // Test merge key detection
    let _ = YamlParser::detect_merge_key(data);

    // Test indentation counting
    let _ = YamlParser::count_indent(data);
}

/// Test TOML parsing with arbitrary input
#[cfg(feature = "toml")]
fn fuzz_toml(data: &[u8]) {
    let parser = TomlParser::new();

    // Test structural parsing
    let _ = parser.parse_structural(data);

    // Test indentation detection at various positions
    for pos in (0..data.len()).step_by(8) {
        let _ = parser.detect_indent(data, pos);
    }

    // Test string/comment detection
    for pos in (0..data.len()).step_by(8) {
        let _ = parser.is_in_string(data, pos);
        let _ = parser.is_in_comment(data, pos);
    }

    // Test chunk scanning (64-byte chunks)
    if data.len() >= 64 {
        let chunk: [u8; 64] = data[..64].try_into().unwrap();
        let _ = parser.scan_chunk(&chunk);
    }

    // Test table header parsing
    let _ = TomlParser::parse_table_header(data);

    // Test dotted key detection
    let _ = TomlParser::detect_dotted_key(data);

    // Test inline table detection
    let _ = TomlParser::detect_inline_table(data);
}

/// Test CSV parsing with arbitrary input
#[cfg(feature = "csv")]
fn fuzz_csv(data: &[u8]) {
    // Test with different delimiter configurations
    let parsers = [
        CsvParser::new(), // comma
        CsvParser::tsv(), // tab
        CsvParser::psv(), // pipe
    ];

    for parser in &parsers {
        // Test structural parsing
        let _ = parser.parse_structural(data);

        // Test string detection
        for pos in (0..data.len()).step_by(8) {
            let _ = parser.is_in_string(data, pos);
        }

        // Test chunk scanning (64-byte chunks)
        if data.len() >= 64 {
            let chunk: [u8; 64] = data[..64].try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        // Test field counting
        let _ = parser.count_fields(data);

        // Test row parsing
        let _ = parser.parse_row(data);
    }

    // Test quoting detection
    let _ = CsvParser::is_quoted(data);

    // Test unquote operation
    let _ = CsvParser::unquote(data);

    // Test delimiter auto-detection
    let _ = CsvParser::detect_delimiter(&[data]);
}

/// Test ISON parsing with arbitrary input
#[cfg(feature = "ison")]
fn fuzz_ison(data: &[u8]) {
    let parser = IsonParser::new();
    let streaming_parser = IsonParser::streaming();

    // Test both parser modes
    for p in [&parser, &streaming_parser] {
        // Test structural parsing
        let _ = p.parse_structural(data);

        // Test string/comment detection
        for pos in (0..data.len()).step_by(8) {
            let _ = p.is_in_string(data, pos);
            let _ = p.is_in_comment(data, pos);
        }

        // Test chunk scanning (64-byte chunks)
        if data.len() >= 64 {
            let chunk: [u8; 64] = data[..64].try_into().unwrap();
            let _ = p.scan_chunk(&chunk);
        }
    }

    // Test block header parsing
    let _ = IsonParser::parse_block_header(data);

    // Test field declaration parsing
    let _ = IsonParser::parse_field_declaration(data);

    // Test data row parsing
    let _ = IsonParser::parse_data_row(data);

    // Test summary marker detection
    let _ = IsonParser::is_summary_marker(data);

    // Test comment detection
    let _ = IsonParser::is_comment(data);

    // Test reference parsing with string slices
    if let Ok(s) = std::str::from_utf8(data) {
        // Test reference parsing at various offsets
        // Use is_char_boundary to avoid panics on multi-byte UTF-8
        for i in 0..s.len().min(50) {
            if s.is_char_boundary(i) {
                let _ = IsonParser::parse_reference(&s[i..]);
            }
        }
    }
}

/// Test TOON parsing with arbitrary input
#[cfg(feature = "toon")]
fn fuzz_toon(data: &[u8]) {
    // Test with different configurations
    let strict_parser = ToonParser::new();
    let lenient_parser = ToonParser::new().with_strict(false);
    let custom_indent = ToonParser::new().with_indent_size(4);

    for parser in [&strict_parser, &lenient_parser, &custom_indent] {
        // Test structural parsing
        let _ = parser.parse_structural(data);

        // Test indentation detection at various positions
        for pos in (0..data.len()).step_by(8) {
            let _ = parser.detect_indent(data, pos);
        }

        // Test string/comment detection
        for pos in (0..data.len()).step_by(8) {
            let _ = parser.is_in_string(data, pos);
            let _ = parser.is_in_comment(data, pos);
        }

        // Test chunk scanning (64-byte chunks)
        if data.len() >= 64 {
            let chunk: [u8; 64] = data[..64].try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        // Test tabular row parsing
        let _ = parser.parse_tabular_row(data);
    }

    // Test indentation counting
    let _ = ToonParser::count_indent(data);

    // Test depth computation with various indentation values
    for indent in 0..data.len().min(100) {
        let _ = strict_parser.compute_depth(indent);
        let _ = lenient_parser.compute_depth(indent);
    }

    // Test array header parsing
    let _ = ToonParser::parse_array_header(data);

    // Test list item detection
    let _ = ToonParser::is_list_item(data);

    // Test folded key operations with string input
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = ToonParser::is_folded_key(s);
        let _ = ToonParser::parse_folded_key(s.as_bytes());

        // Test needs_quoting
        for delim in [
            fionn_simd::formats::toon::ToonDelimiter::Comma,
            fionn_simd::formats::toon::ToonDelimiter::Tab,
            fionn_simd::formats::toon::ToonDelimiter::Pipe,
        ] {
            let _ = ToonParser::needs_quoting(s, delim);
        }
    }
}

/// Test delimiter stack operations
#[cfg(feature = "toon")]
fn fuzz_toon_delimiter_stack(data: &[u8]) {
    use fionn_simd::formats::toon::ToonDelimiter;

    let mut parser = ToonParser::new();

    // Use bytes to drive delimiter stack operations
    for &byte in data.iter().take(1000) {
        match byte % 4 {
            0 => parser.push_delimiter(ToonDelimiter::Comma),
            1 => parser.push_delimiter(ToonDelimiter::Tab),
            2 => parser.push_delimiter(ToonDelimiter::Pipe),
            3 => parser.pop_delimiter(),
            _ => unreachable!(),
        }
        let _ = parser.active_delimiter();
    }

    parser.reset();
}

/// Cross-format testing: try parsing same input with all formats
fn fuzz_cross_format(data: &[u8]) {
    #[cfg(feature = "yaml")]
    fuzz_yaml(data);

    #[cfg(feature = "toml")]
    fuzz_toml(data);

    #[cfg(feature = "csv")]
    fuzz_csv(data);

    #[cfg(feature = "ison")]
    fuzz_ison(data);

    #[cfg(feature = "toon")]
    fuzz_toon(data);

    #[cfg(feature = "toon")]
    fuzz_toon_delimiter_stack(data);
}

/// Test with input that might be valid in multiple formats
fn fuzz_polyglot(data: &[u8]) {
    // Try wrapping input in various format structures
    if let Ok(s) = std::str::from_utf8(data) {
        // YAML-style
        #[cfg(feature = "yaml")]
        {
            let yaml_wrapped = format!("key: {}\n", s);
            let parser = YamlParser::new();
            let _ = parser.parse_structural(yaml_wrapped.as_bytes());
        }

        // TOML-style
        #[cfg(feature = "toml")]
        {
            let toml_wrapped = format!(
                "key = \"{}\"\n",
                s.replace('\\', "\\\\").replace('"', "\\\"")
            );
            let parser = TomlParser::new();
            let _ = parser.parse_structural(toml_wrapped.as_bytes());
        }

        // CSV-style
        #[cfg(feature = "csv")]
        {
            let csv_wrapped = format!("\"{}\",other\n", s.replace('"', "\"\""));
            let parser = CsvParser::new();
            let _ = parser.parse_structural(csv_wrapped.as_bytes());
        }

        // ISON-style
        #[cfg(feature = "ison")]
        {
            let ison_wrapped = format!("table.test\nfield\n{}\n", s);
            let parser = IsonParser::new();
            let _ = parser.parse_structural(ison_wrapped.as_bytes());
        }

        // TOON-style
        #[cfg(feature = "toon")]
        {
            let toon_wrapped = format!("key: {}\n", s);
            let parser = ToonParser::new();
            let _ = parser.parse_structural(toon_wrapped.as_bytes());
        }
    }
}

fn main() {
    fuzz!(|data: &[u8]| {
        // Skip extremely large inputs
        if data.len() <= 100_000 {
            // Cross-format testing
            fuzz_cross_format(data);

            // Polyglot testing
            fuzz_polyglot(data);
        }
    });
}
