// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for format parsers
//!
//! These tests verify parser invariants across randomly generated inputs.

#![cfg(any(
    feature = "yaml",
    feature = "toml",
    feature = "csv",
    feature = "ison",
    feature = "toon"
))]

use proptest::prelude::*;

#[cfg(feature = "yaml")]
mod yaml_proptests {
    use super::*;
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::yaml::{YamlParser, YamlStructural};

    proptest! {
        /// Indentation counting should always return <= input length
        #[test]
        fn indent_count_bounded(line in "[ ]*[^\n]*") {
            let indent = YamlParser::count_indent(line.as_bytes());
            prop_assert!(indent <= line.len());
        }

        /// Indentation count should equal number of leading spaces
        #[test]
        fn indent_count_correct(spaces in 0usize..100, rest in "[a-z]*") {
            let line = format!("{}{}", " ".repeat(spaces), rest);
            let indent = YamlParser::count_indent(line.as_bytes());
            prop_assert_eq!(indent, spaces);
        }

        /// Anchor detection should only succeed when input starts with &
        #[test]
        fn anchor_requires_ampersand(input in "[a-zA-Z0-9_-]*") {
            let result = YamlParser::detect_anchor(input.as_bytes(), 0);
            prop_assert!(result.is_none());
        }

        /// Valid anchors should be detected
        #[test]
        fn valid_anchor_detected(name in "[a-zA-Z][a-zA-Z0-9_-]{0,20}") {
            let input = format!("&{name} value");
            let result = YamlParser::detect_anchor(input.as_bytes(), 0);
            prop_assert!(result.is_some());
            prop_assert_eq!(result.unwrap(), name.as_bytes());
        }

        /// Alias detection should only succeed when input starts with *
        #[test]
        fn alias_requires_star(input in "[a-zA-Z0-9_-]*") {
            let result = YamlParser::detect_alias(input.as_bytes(), 0);
            prop_assert!(result.is_none());
        }

        /// Valid aliases should be detected
        #[test]
        fn valid_alias_detected(name in "[a-zA-Z][a-zA-Z0-9_-]{0,20}") {
            let input = format!("*{name} ");
            let result = YamlParser::detect_alias(input.as_bytes(), 0);
            prop_assert!(result.is_some());
            prop_assert_eq!(result.unwrap(), name.as_bytes());
        }

        /// Document markers should be detected correctly (no padding)
        #[test]
        fn document_start_detected(_dummy in Just(())) {
            let input = b"---";
            let result = YamlParser::detect_document_marker(input);
            prop_assert_eq!(result, Some(YamlStructural::DocumentStart));
        }

        /// Merge key detection
        #[test]
        fn merge_key_detected(prefix in "[ ]*", suffix in "[ ]*\\*[a-z]+") {
            let input = format!("{prefix}<<:{suffix}");
            prop_assert!(YamlParser::detect_merge_key(input.as_bytes()));
        }

        /// parse_structural should not panic on arbitrary input
        #[test]
        fn parse_structural_no_panic(input in ".*") {
            let parser = YamlParser::new();
            let _ = parser.parse_structural(input.as_bytes());
        }

        /// scan_chunk should not panic on any 64-byte input
        #[test]
        fn scan_chunk_no_panic(input in prop::collection::vec(any::<u8>(), 64)) {
            let parser = YamlParser::new();
            let chunk: [u8; 64] = input.try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        /// String detection should be consistent
        #[test]
        fn is_in_string_consistent(
            prefix in "[^\"]*",
            in_string in prop::bool::ANY,
            suffix in "[^\"]*"
        ) {
            let parser = YamlParser::new();
            let input = if in_string {
                format!("{prefix}\"{suffix}\"")
            } else {
                format!("{prefix}{suffix}")
            };
            let bytes = input.as_bytes();

            // Test at various positions
            for pos in 0..bytes.len() {
                let _ = parser.is_in_string(bytes, pos);
            }
        }
    }
}

#[cfg(feature = "toml")]
mod toml_proptests {
    use super::*;
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::toml::{TomlParser, TomlStructural};

    proptest! {
        /// Table header parsing should detect valid headers
        #[test]
        fn valid_table_header(name in "[a-z][a-z0-9_]{0,20}") {
            let input = format!("[{name}]");
            let result = TomlParser::parse_table_header(input.as_bytes());
            prop_assert!(result.is_some());
            if let Some(TomlStructural::Table(path)) = result {
                prop_assert_eq!(path.len(), 1);
                prop_assert_eq!(&path[0], &name);
            }
        }

        /// Array table header parsing
        #[test]
        fn valid_array_table_header(name in "[a-z][a-z0-9_]{0,20}") {
            let input = format!("[[{name}]]");
            let result = TomlParser::parse_table_header(input.as_bytes());
            prop_assert!(result.is_some());
            if let Some(TomlStructural::ArrayTable(path)) = result {
                prop_assert_eq!(path.len(), 1);
                prop_assert_eq!(&path[0], &name);
            }
        }

        /// Dotted table paths
        #[test]
        fn dotted_table_path(
            part1 in "[a-z][a-z0-9]{0,10}",
            part2 in "[a-z][a-z0-9]{0,10}"
        ) {
            let input = format!("[{part1}.{part2}]");
            let result = TomlParser::parse_table_header(input.as_bytes());
            prop_assert!(result.is_some());
            if let Some(TomlStructural::Table(path)) = result {
                prop_assert_eq!(path.len(), 2);
            }
        }

        /// Dotted key detection
        #[test]
        fn dotted_key_detected(
            key1 in "[a-z][a-z0-9]{0,10}",
            key2 in "[a-z][a-z0-9]{0,10}",
            value in "[a-z0-9]+"
        ) {
            let input = format!("{key1}.{key2} = \"{value}\"");
            let result = TomlParser::detect_dotted_key(input.as_bytes());
            prop_assert!(result.is_some());
            let parts = result.unwrap();
            prop_assert_eq!(parts.len(), 2);
        }

        /// Simple key should not be detected as dotted
        #[test]
        fn simple_key_not_dotted(key in "[a-z][a-z0-9]{0,20}", value in "[a-z0-9]+") {
            let input = format!("{key} = \"{value}\"");
            let result = TomlParser::detect_dotted_key(input.as_bytes());
            prop_assert!(result.is_none());
        }

        /// Inline table detection
        #[test]
        fn inline_table_detected(key in "[a-z]+", inner_key in "[a-z]+", value in "[0-9]+") {
            let input = format!("{key} = {{{inner_key} = {value}}}");
            prop_assert!(TomlParser::detect_inline_table(input.as_bytes()));
        }

        /// Non-inline table not detected
        #[test]
        fn non_inline_table_not_detected(key in "[a-z]+", value in "[a-z0-9]+") {
            let input = format!("{key} = \"{value}\"");
            prop_assert!(!TomlParser::detect_inline_table(input.as_bytes()));
        }

        /// parse_structural should not panic
        #[test]
        fn parse_structural_no_panic(input in ".*") {
            let parser = TomlParser::new();
            let _ = parser.parse_structural(input.as_bytes());
        }

        /// scan_chunk should not panic on any 64-byte input
        #[test]
        fn scan_chunk_no_panic(input in prop::collection::vec(any::<u8>(), 64)) {
            let parser = TomlParser::new();
            let chunk: [u8; 64] = input.try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }
    }
}

#[cfg(feature = "csv")]
mod csv_proptests {
    use super::*;
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::csv::CsvParser;

    proptest! {
        /// Field counting should be consistent
        #[test]
        fn field_count_at_least_one(input in "[^,\n]+") {
            let parser = CsvParser::new();
            let count = parser.count_fields(input.as_bytes());
            prop_assert!(count >= 1);
        }

        /// Field count increases with delimiters
        #[test]
        fn field_count_increases_with_delimiters(
            field1 in "[a-z]+",
            field2 in "[a-z]+",
            field3 in "[a-z]+"
        ) {
            let parser = CsvParser::new();
            let row = format!("{field1},{field2},{field3}");
            let count = parser.count_fields(row.as_bytes());
            prop_assert_eq!(count, 3);
        }

        /// Quoted fields should not split on internal delimiters
        #[test]
        fn quoted_field_no_split(prefix in "[a-z]*", suffix in "[a-z]*") {
            let parser = CsvParser::new();
            let row = format!("\"{prefix},{suffix}\"");
            let count = parser.count_fields(row.as_bytes());
            prop_assert_eq!(count, 1);
        }

        /// parse_row should return correct number of fields
        #[test]
        fn parse_row_field_count(
            fields in prop::collection::vec("[a-z]+", 1..10)
        ) {
            let parser = CsvParser::new();
            let row = format!("{}\n", fields.join(","));
            let parsed = parser.parse_row(row.as_bytes());
            prop_assert_eq!(parsed.len(), fields.len());
        }

        /// is_quoted should correctly identify quoted fields
        #[test]
        fn is_quoted_correct(content in "[a-z]*") {
            let quoted = format!("\"{content}\"");
            prop_assert!(CsvParser::is_quoted(quoted.as_bytes()));

            let unquoted = content.as_bytes();
            prop_assert!(!CsvParser::is_quoted(unquoted));
        }

        /// unquote should remove surrounding quotes
        #[test]
        fn unquote_removes_quotes(content in "[a-z]*") {
            let quoted = format!("\"{content}\"");
            let unquoted = CsvParser::unquote(quoted.as_bytes());
            prop_assert_eq!(unquoted, content.as_bytes());
        }

        /// Escaped quotes should be unescaped
        #[test]
        fn unquote_handles_escaped_quotes(prefix in "[a-z]*", suffix in "[a-z]*") {
            let quoted = format!("\"{prefix}\"\"{suffix}\"");
            let unquoted = CsvParser::unquote(quoted.as_bytes());
            let expected = format!("{prefix}\"{suffix}");
            prop_assert_eq!(unquoted, expected.as_bytes());
        }

        /// Delimiter detection should find the most common delimiter
        #[test]
        fn delimiter_detection(delimiter in prop::sample::select(vec![b',', b'\t', b'|', b';'])) {
            let delim_char = delimiter as char;
            let row = format!("a{delim_char}b{delim_char}c");
            let detected = CsvParser::detect_delimiter(&[row.as_bytes()]);
            prop_assert_eq!(detected, delimiter);
        }

        /// parse_structural should not panic
        #[test]
        fn parse_structural_no_panic(input in ".*") {
            let parser = CsvParser::new();
            let _ = parser.parse_structural(input.as_bytes());
        }

        /// scan_chunk should not panic on any 64-byte input
        #[test]
        fn scan_chunk_no_panic(input in prop::collection::vec(any::<u8>(), 64)) {
            let parser = CsvParser::new();
            let chunk: [u8; 64] = input.try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        /// TSV parser should use tab delimiter
        #[test]
        fn tsv_uses_tab(field1 in "[a-z]+", field2 in "[a-z]+") {
            let parser = CsvParser::tsv();
            let row = format!("{field1}\t{field2}");
            let count = parser.count_fields(row.as_bytes());
            prop_assert_eq!(count, 2);
        }

        /// PSV parser should use pipe delimiter
        #[test]
        fn psv_uses_pipe(field1 in "[a-z]+", field2 in "[a-z]+") {
            let parser = CsvParser::psv();
            let row = format!("{field1}|{field2}");
            let count = parser.count_fields(row.as_bytes());
            prop_assert_eq!(count, 2);
        }
    }
}

#[cfg(feature = "ison")]
mod ison_proptests {
    use super::*;
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::ison::{IsonBlockKind, IsonParser, IsonReference, IsonType};

    proptest! {
        /// Table block header parsing
        #[test]
        fn table_block_header(name in "[a-z][a-z0-9_]{0,20}") {
            let input = format!("table.{name}");
            let result = IsonParser::parse_block_header(input.as_bytes());
            prop_assert!(result.is_some());
            let (kind, parsed_name) = result.unwrap();
            prop_assert_eq!(kind, IsonBlockKind::Table);
            prop_assert_eq!(&parsed_name, &name);
        }

        /// Object block header parsing
        #[test]
        fn object_block_header(name in "[a-z][a-z0-9_]{0,20}") {
            let input = format!("object.{name}");
            let result = IsonParser::parse_block_header(input.as_bytes());
            prop_assert!(result.is_some());
            let (kind, parsed_name) = result.unwrap();
            prop_assert_eq!(kind, IsonBlockKind::Object);
            prop_assert_eq!(&parsed_name, &name);
        }

        /// Invalid block header should return None
        #[test]
        fn invalid_block_header(name in "[a-z]+") {
            let input = format!("invalid.{name}");
            let result = IsonParser::parse_block_header(input.as_bytes());
            prop_assert!(result.is_none());
        }

        /// Simple reference parsing
        #[test]
        fn simple_reference(id in "[a-z0-9]+") {
            let input = format!(":{id}");
            let result = IsonParser::parse_reference(&input);
            prop_assert!(result.is_some());
            if let Some(IsonReference::Simple(parsed_id)) = result {
                prop_assert_eq!(&parsed_id, &id);
            }
        }

        /// Typed reference parsing
        #[test]
        fn typed_reference(ref_type in "[a-z]+", id in "[a-z0-9]+") {
            let input = format!(":{ref_type}:{id}");
            let result = IsonParser::parse_reference(&input);
            prop_assert!(result.is_some());
            if let Some(IsonReference::Typed { ref_type: rt, id: parsed_id }) = result {
                prop_assert_eq!(&rt, &ref_type);
                prop_assert_eq!(&parsed_id, &id);
            }
        }

        /// Relationship reference parsing (uppercase)
        #[test]
        fn relationship_reference(rel in "[A-Z_]+", id in "[a-z0-9]+") {
            let input = format!(":{rel}:{id}");
            let result = IsonParser::parse_reference(&input);
            prop_assert!(result.is_some());
            if let Some(IsonReference::Relationship { relationship, id: parsed_id }) = result {
                prop_assert_eq!(&relationship, &rel);
                prop_assert_eq!(&parsed_id, &id);
            }
        }

        /// Field declaration parsing
        #[test]
        fn field_declaration(
            fields in prop::collection::vec("[a-z]+", 1..5)
        ) {
            let input = fields.join(" ");
            let parsed = IsonParser::parse_field_declaration(input.as_bytes());
            prop_assert_eq!(parsed.len(), fields.len());
        }

        /// Field with type annotation
        #[test]
        fn field_with_type(name in "[a-z]+", type_name in prop::sample::select(vec!["int", "float", "string", "bool"])) {
            let input = format!("{name}:{type_name}");
            let parsed = IsonParser::parse_field_declaration(input.as_bytes());
            prop_assert_eq!(parsed.len(), 1);
            prop_assert_eq!(&parsed[0].name, &name);
            prop_assert!(parsed[0].field_type.is_some());
        }

        /// Data row parsing
        #[test]
        fn data_row_parsing(values in prop::collection::vec("[a-z0-9]+", 1..5)) {
            let input = values.join(" ");
            let parsed = IsonParser::parse_data_row(input.as_bytes());
            prop_assert_eq!(parsed.len(), values.len());
        }

        /// Quoted data in row
        #[test]
        fn quoted_data_row(value1 in "[a-z]+", value2 in "[a-z ]+", value3 in "[a-z]+") {
            let input = format!("{value1} \"{value2}\" {value3}");
            let parsed = IsonParser::parse_data_row(input.as_bytes());
            prop_assert_eq!(parsed.len(), 3);
        }

        /// Summary marker detection
        #[test]
        fn summary_marker_detected(padding in "[ ]*") {
            let input = format!("{padding}---{padding}");
            prop_assert!(IsonParser::is_summary_marker(input.as_bytes()));
        }

        /// Comment detection
        #[test]
        fn comment_detected(content in "[a-z ]*") {
            let input = format!("# {content}");
            prop_assert!(IsonParser::is_comment(input.as_bytes()));
        }

        /// Non-comment not detected
        #[test]
        fn non_comment_not_detected(content in "[a-z]+") {
            prop_assert!(!IsonParser::is_comment(content.as_bytes()));
        }

        /// parse_structural should not panic
        #[test]
        fn parse_structural_no_panic(input in ".*") {
            let parser = IsonParser::new();
            let _ = parser.parse_structural(input.as_bytes());
        }

        /// scan_chunk should not panic on any 64-byte input
        #[test]
        fn scan_chunk_no_panic(input in prop::collection::vec(any::<u8>(), 64)) {
            let parser = IsonParser::new();
            let chunk: [u8; 64] = input.try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }
    }

    #[test]
    fn type_parsing() {
        assert_eq!(IsonType::parse("int"), Some(IsonType::Int));
        assert_eq!(IsonType::parse("float"), Some(IsonType::Float));
        assert_eq!(IsonType::parse("string"), Some(IsonType::String));
        assert_eq!(IsonType::parse("bool"), Some(IsonType::Bool));
        assert_eq!(IsonType::parse("computed"), Some(IsonType::Computed));
    }
}

#[cfg(feature = "toon")]
mod toon_proptests {
    use super::*;
    use fionn_simd::formats::FormatParser;
    use fionn_simd::formats::toon::{ToonDelimiter, ToonParser};

    proptest! {
        /// Indentation counting should return correct count
        #[test]
        fn indent_count_correct(spaces in 0usize..50, rest in "[a-z]*") {
            let line = format!("{}{}", " ".repeat(spaces), rest);
            let indent = ToonParser::count_indent(line.as_bytes());
            prop_assert_eq!(indent, spaces);
        }

        /// Depth computation in strict mode
        #[test]
        fn depth_computation_strict(depth in 0usize..20) {
            let parser = ToonParser::new(); // indent_size = 2
            let indent = depth * 2;
            let result = parser.compute_depth(indent);
            prop_assert!(result.is_ok());
            prop_assert_eq!(result.unwrap(), depth);
        }

        /// Depth computation rejects invalid indentation in strict mode
        #[test]
        fn depth_rejects_invalid_strict(depth in 0usize..20) {
            let parser = ToonParser::new(); // indent_size = 2
            let indent = depth * 2 + 1; // Invalid: odd indentation
            let result = parser.compute_depth(indent);
            prop_assert!(result.is_err());
        }

        /// Non-strict mode accepts any indentation
        #[test]
        fn depth_accepts_any_nonstrict(indent in 0usize..100) {
            let parser = ToonParser::new().with_strict(false);
            let result = parser.compute_depth(indent);
            prop_assert!(result.is_ok());
        }

        /// Array header parsing
        #[test]
        fn array_header_parsing(key in "[a-z]+", length in 1usize..100) {
            let input = format!("{key}[{length}]:");
            let result = ToonParser::parse_array_header(input.as_bytes());
            prop_assert!(result.is_some());
            let header = result.unwrap();
            prop_assert_eq!(header.length, length);
            prop_assert_eq!(header.delimiter, ToonDelimiter::Comma);
        }

        /// Array header with pipe delimiter
        #[test]
        fn array_header_pipe(key in "[a-z]+", length in 1usize..100) {
            let input = format!("{key}[{length}|]:");
            let result = ToonParser::parse_array_header(input.as_bytes());
            prop_assert!(result.is_some());
            let header = result.unwrap();
            prop_assert_eq!(header.length, length);
            prop_assert_eq!(header.delimiter, ToonDelimiter::Pipe);
        }

        /// Array header with fields
        #[test]
        fn array_header_with_fields(
            key in "[a-z]+",
            length in 1usize..10,
            fields in prop::collection::vec("[a-z]+", 2..5)
        ) {
            let input = format!("{}[{}]{{{}}}:", key, length, fields.join(","));
            let result = ToonParser::parse_array_header(input.as_bytes());
            prop_assert!(result.is_some());
            let header = result.unwrap();
            prop_assert_eq!(header.length, length);
            prop_assert!(header.fields.is_some());
            prop_assert_eq!(header.fields.unwrap().len(), fields.len());
        }

        /// Folded key detection
        #[test]
        fn folded_key_detected(parts in prop::collection::vec("[a-z][a-z0-9_]*", 2..5)) {
            let key = parts.join(".");
            prop_assert!(ToonParser::is_folded_key(&key));
        }

        /// Simple key not detected as folded
        #[test]
        fn simple_key_not_folded(key in "[a-z][a-z0-9_]*") {
            prop_assert!(!ToonParser::is_folded_key(&key));
        }

        /// Quoted key not detected as folded
        #[test]
        fn quoted_key_not_folded(key in "[a-z.]+") {
            let quoted = format!("\"{key}\"");
            prop_assert!(!ToonParser::is_folded_key(&quoted));
        }

        /// Folded key parsing
        #[test]
        fn folded_key_parsing(parts in prop::collection::vec("[a-z]+", 2..5)) {
            let key = parts.join(".");
            let line = format!("{key}: value");
            let parsed = ToonParser::parse_folded_key(line.as_bytes());
            prop_assert!(parsed.is_some());
            let parsed = parsed.unwrap();
            prop_assert_eq!(parsed.len(), parts.len());
            for (expected, actual) in parts.iter().zip(parsed.iter()) {
                prop_assert_eq!(expected, actual);
            }
        }

        /// List item detection
        #[test]
        fn list_item_detected(indent in 0usize..10, content in "[a-z]*") {
            let input = format!("{}- {}", " ".repeat(indent), content);
            prop_assert!(ToonParser::is_list_item(input.as_bytes()));
        }

        /// Non-list item not detected
        #[test]
        fn non_list_item_not_detected(content in "[a-z]+") {
            prop_assert!(!ToonParser::is_list_item(content.as_bytes()));
        }

        /// Tabular row parsing
        #[test]
        fn tabular_row_parsing(values in prop::collection::vec("[a-z0-9]+", 1..5)) {
            let parser = ToonParser::new();
            let row = values.join(",");
            let parsed = parser.parse_tabular_row(row.as_bytes());
            prop_assert_eq!(parsed.len(), values.len());
        }

        /// Tabular row with pipe delimiter
        #[test]
        fn tabular_row_pipe_delimiter(values in prop::collection::vec("[a-z0-9]+", 1..5)) {
            let mut parser = ToonParser::new();
            parser.push_delimiter(ToonDelimiter::Pipe);
            let row = values.join("|");
            let parsed = parser.parse_tabular_row(row.as_bytes());
            prop_assert_eq!(parsed.len(), values.len());
        }

        /// Simple strings don't need quoting
        #[test]
        fn simple_string_no_quoting(s in "[a-zA-Z][a-zA-Z0-9_]*") {
            // Skip reserved words
            if s == "true" || s == "false" || s == "null" {
                return Ok(());
            }
            prop_assert!(!ToonParser::needs_quoting(&s, ToonDelimiter::Comma));
        }

        /// Delimiter stack operations
        #[test]
        fn delimiter_stack_push_pop(delimiters in prop::collection::vec(
            prop::sample::select(vec![ToonDelimiter::Comma, ToonDelimiter::Tab, ToonDelimiter::Pipe]),
            1..10
        )) {
            let mut parser = ToonParser::new();

            for &d in &delimiters {
                parser.push_delimiter(d);
                prop_assert_eq!(parser.active_delimiter(), d);
            }

            for &d in delimiters.iter().rev() {
                prop_assert_eq!(parser.active_delimiter(), d);
                parser.pop_delimiter();
            }

            prop_assert_eq!(parser.active_delimiter(), ToonDelimiter::Comma);
        }

        /// parse_structural should not panic
        #[test]
        fn parse_structural_no_panic(input in ".*") {
            let parser = ToonParser::new();
            let _ = parser.parse_structural(input.as_bytes());
        }

        /// scan_chunk should not panic on any 64-byte input
        #[test]
        fn scan_chunk_no_panic(input in prop::collection::vec(any::<u8>(), 64)) {
            let parser = ToonParser::new();
            let chunk: [u8; 64] = input.try_into().unwrap();
            let _ = parser.scan_chunk(&chunk);
        }

        /// Custom indent size works correctly
        #[test]
        fn custom_indent_size(indent_size in 1usize..8, depth in 0usize..10) {
            let parser = ToonParser::new().with_indent_size(indent_size).with_strict(true);
            let indent = depth * indent_size;
            let result = parser.compute_depth(indent);
            prop_assert!(result.is_ok());
            prop_assert_eq!(result.unwrap(), depth);
        }
    }

    #[test]
    fn needs_quoting_edge_cases() {
        // Empty needs quoting
        assert!(ToonParser::needs_quoting("", ToonDelimiter::Comma));

        // Reserved words need quoting
        assert!(ToonParser::needs_quoting("true", ToonDelimiter::Comma));
        assert!(ToonParser::needs_quoting("false", ToonDelimiter::Comma));
        assert!(ToonParser::needs_quoting("null", ToonDelimiter::Comma));

        // Numbers need quoting
        assert!(ToonParser::needs_quoting("42", ToonDelimiter::Comma));
        assert!(ToonParser::needs_quoting("3.14", ToonDelimiter::Comma));
    }

    #[test]
    fn delimiter_conversions() {
        for d in [
            ToonDelimiter::Comma,
            ToonDelimiter::Tab,
            ToonDelimiter::Pipe,
        ] {
            assert_eq!(d.as_byte() as char, d.as_char());
        }
    }
}

/// Cross-format invariants
#[cfg(all(
    feature = "yaml",
    feature = "toml",
    feature = "csv",
    feature = "ison",
    feature = "toon"
))]
mod cross_format_proptests {
    use fionn_simd::formats::{ChunkMask, StructuralPositions};

    #[test]
    fn all_parsers_handle_empty() {
        use fionn_simd::formats::FormatParser;
        use fionn_simd::formats::{
            csv::CsvParser, ison::IsonParser, toml::TomlParser, toon::ToonParser, yaml::YamlParser,
        };

        let yaml = YamlParser::new();
        let toml = TomlParser::new();
        let csv = CsvParser::new();
        let ison = IsonParser::new();
        let toon = ToonParser::new();

        assert!(yaml.parse_structural(b"").is_ok());
        assert!(toml.parse_structural(b"").is_ok());
        assert!(csv.parse_structural(b"").is_ok());
        assert!(ison.parse_structural(b"").is_ok());
        assert!(toon.parse_structural(b"").is_ok());
    }

    #[test]
    fn chunk_mask_new_is_empty() {
        let mask = ChunkMask::new();
        assert_eq!(mask.string_mask, 0);
        assert_eq!(mask.comment_mask, 0);
        assert_eq!(mask.escape_mask, 0);
        assert_eq!(mask.structural_mask, 0);
    }

    #[test]
    fn structural_positions_new_is_empty() {
        let pos = StructuralPositions::new();
        assert!(pos.newlines.is_empty());
        assert!(pos.comment_starts.is_empty());
        assert!(pos.string_boundaries.is_empty());
        assert!(pos.delimiters.is_empty());
        assert!(pos.escapes.is_empty());
    }
}
