// SPDX-License-Identifier: MIT OR Apache-2.0
//! libFuzzer target for multi-format SIMD parsers.
//!
//! This is the libFuzzer equivalent of the AFL fuzz_formats target.
//! Run with: cargo +nightly fuzz run fuzz_formats_libfuzzer

#![no_main]

use libfuzzer_sys::fuzz_target;

#[cfg(feature = "yaml")]
use fionn_simd::formats::yaml::YamlParser;

#[cfg(feature = "toml")]
use fionn_simd::formats::toml::TomlParser;

#[cfg(feature = "csv")]
use fionn_simd::formats::csv::CsvParser;

#[cfg(feature = "ison")]
use fionn_simd::formats::ison::IsonParser;

#[cfg(feature = "toon")]
use fionn_simd::formats::toon::{ToonParser, ToonDelimiter};

#[cfg(any(feature = "yaml", feature = "toml", feature = "csv", feature = "ison", feature = "toon"))]
use fionn_simd::formats::FormatParser;

fuzz_target!(|data: &[u8]| {
    // Skip extremely large inputs
    if data.len() > 100_000 {
        return;
    }

    // YAML parser fuzzing
    #[cfg(feature = "yaml")]
    {
        let parser = YamlParser::new();
        let _ = parser.parse_structural(data);

        // Test chunk scanning
        if data.len() >= 64 {
            let chunk: [u8; 64] = data[..64].try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        // Test anchor/alias detection
        for pos in (0..data.len().min(100)).step_by(4) {
            let _ = YamlParser::detect_anchor(data, pos);
            let _ = YamlParser::detect_alias(data, pos);
        }

        let _ = YamlParser::detect_document_marker(data);
        let _ = YamlParser::detect_merge_key(data);
        let _ = YamlParser::count_indent(data);
    }

    // TOML parser fuzzing
    #[cfg(feature = "toml")]
    {
        let parser = TomlParser::new();
        let _ = parser.parse_structural(data);

        if data.len() >= 64 {
            let chunk: [u8; 64] = data[..64].try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        let _ = TomlParser::parse_table_header(data);
        let _ = TomlParser::detect_dotted_key(data);
        let _ = TomlParser::detect_inline_table(data);
    }

    // CSV parser fuzzing
    #[cfg(feature = "csv")]
    {
        for parser in [CsvParser::new(), CsvParser::tsv(), CsvParser::psv()] {
            let _ = parser.parse_structural(data);
            let _ = parser.count_fields(data);
            let _ = parser.parse_row(data);

            if data.len() >= 64 {
                let chunk: [u8; 64] = data[..64].try_into().unwrap();
                let _ = parser.scan_chunk(&chunk);
            }
        }

        let _ = CsvParser::is_quoted(data);
        let _ = CsvParser::unquote(data);

        // detect_delimiter expects &[&[u8]], so split into lines
        let lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
        let _ = CsvParser::detect_delimiter(&lines);
    }

    // ISON parser fuzzing
    #[cfg(feature = "ison")]
    {
        let parser = IsonParser::new();
        let streaming = IsonParser::streaming();

        let _ = parser.parse_structural(data);
        let _ = streaming.parse_structural(data);

        if data.len() >= 64 {
            let chunk: [u8; 64] = data[..64].try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        let _ = IsonParser::parse_block_header(data);
        let _ = IsonParser::parse_field_declaration(data);
        let _ = IsonParser::parse_data_row(data);
        let _ = IsonParser::is_summary_marker(data);
        let _ = IsonParser::is_comment(data);

        if let Ok(s) = std::str::from_utf8(data) {
            let _ = IsonParser::parse_reference(s);
        }
    }

    // TOON parser fuzzing
    #[cfg(feature = "toon")]
    {
        let strict = ToonParser::new();
        let lenient = ToonParser::new().with_strict(false);

        let _ = strict.parse_structural(data);
        let _ = lenient.parse_structural(data);

        if data.len() >= 64 {
            let chunk: [u8; 64] = data[..64].try_into().unwrap();
            let _ = strict.scan_chunk(&chunk);
        }

        let _ = ToonParser::count_indent(data);
        let _ = ToonParser::parse_array_header(data);
        let _ = ToonParser::is_list_item(data);
        let _ = strict.parse_tabular_row(data);

        // Depth computation
        for indent in (0..data.len().min(50)).step_by(2) {
            let _ = strict.compute_depth(indent);
            let _ = lenient.compute_depth(indent);
        }

        if let Ok(s) = std::str::from_utf8(data) {
            let _ = ToonParser::is_folded_key(s);
            let _ = ToonParser::needs_quoting(s, ToonDelimiter::Comma);
        }
        // parse_folded_key takes &[u8]
        let _ = ToonParser::parse_folded_key(data);

        // Delimiter stack stress test
        let mut parser = ToonParser::new();
        for &byte in data.iter().take(100) {
            match byte % 4 {
                0 => parser.push_delimiter(ToonDelimiter::Comma),
                1 => parser.push_delimiter(ToonDelimiter::Tab),
                2 => parser.push_delimiter(ToonDelimiter::Pipe),
                _ => parser.pop_delimiter(),
            }
        }
    }
});
