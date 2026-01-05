// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended path parser supporting bracket notation
//!
//! This module extends the base path parser to support:
//! - Standard dot notation: `field.name`
//! - Bracket notation: `["field"]`
//! - Array indices: `[0]`
//! - Mixed notation: `field["key"][0].name`
//!
//! ## Grammar
//!
//! ```text
//! path        ::= root segment*
//! root        ::= identifier
//! segment     ::= dot_segment | bracket_segment
//! dot_segment ::= '.' identifier
//! bracket_segment ::= '[' (integer | quoted_string) ']'
//! quoted_string   ::= '"' escaped_char* '"'
//! identifier  ::= [a-zA-Z_][a-zA-Z0-9_]*
//! integer     ::= [0-9]+
//! ```

use memchr::{memchr, memchr2};
use std::ops::Range;

/// Extended path component supporting both field access and array indexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtendedPathComponent {
    /// A field name (from dot notation or bracket notation)
    Field(String),
    /// An array index
    ArrayIndex(usize),
}

/// Borrowed extended path component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtendedPathComponentRef<'a> {
    /// A field name reference
    Field(&'a str),
    /// An array index
    ArrayIndex(usize),
}

/// Range-based extended path component for cached paths.
#[derive(Debug, Clone)]
pub enum ExtendedPathComponentRange {
    /// Field as a range into the original path string
    Field(Range<usize>),
    /// Field that was unescaped (needs owned storage)
    UnescapedField(String),
    /// An array index
    ArrayIndex(usize),
}

/// Parsed extended path with owned storage.
#[derive(Debug, Clone)]
pub struct ParsedExtendedPath {
    /// Original path string
    path: String,
    /// Parsed components
    components: Vec<ExtendedPathComponentRange>,
}

impl ParsedExtendedPath {
    /// Parse an extended path string.
    #[must_use]
    pub fn parse(path: &str) -> Self {
        let mut components = Vec::new();
        parse_extended_ranges(path, &mut components);
        Self {
            path: path.to_string(),
            components,
        }
    }

    /// Get the original path string.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the parsed components.
    #[must_use]
    pub fn components(&self) -> &[ExtendedPathComponentRange] {
        &self.components
    }

    /// Convert to owned components.
    #[must_use]
    pub fn to_owned_components(&self) -> Vec<ExtendedPathComponent> {
        self.components
            .iter()
            .map(|c| match c {
                ExtendedPathComponentRange::Field(range) => {
                    ExtendedPathComponent::Field(self.path[range.clone()].to_string())
                }
                ExtendedPathComponentRange::UnescapedField(s) => {
                    ExtendedPathComponent::Field(s.clone())
                }
                ExtendedPathComponentRange::ArrayIndex(idx) => {
                    ExtendedPathComponent::ArrayIndex(*idx)
                }
            })
            .collect()
    }
}

/// Parse an extended path into owned components.
#[must_use]
pub fn parse_extended_path(path: &str) -> Vec<ExtendedPathComponent> {
    let mut components = Vec::new();
    parse_extended_into(path, &mut components);
    components
}

/// Parse an extended path into borrowed components.
///
/// Note: For paths with escaped strings in brackets, this will return
/// the escaped form. Use `parse_extended_path` for unescaped fields.
#[must_use]
pub fn parse_extended_path_ref(path: &str) -> Vec<ExtendedPathComponentRef<'_>> {
    let mut components = Vec::new();
    parse_extended_ref_into(path, &mut components);
    components
}

/// Parse extended path into a component vector.
fn parse_extended_into(path: &str, components: &mut Vec<ExtendedPathComponent>) {
    components.clear();
    let bytes = path.as_bytes();

    if bytes.is_empty() {
        return;
    }

    // Parse optional root identifier (before any . or [)
    let first_delim = memchr2(b'.', b'[', bytes);
    let mut i = if let Some(pos) = first_delim {
        if pos > 0 {
            // There's a root identifier
            let root = unsafe { std::str::from_utf8_unchecked(&bytes[0..pos]) };
            components.push(ExtendedPathComponent::Field(root.to_string()));
        }
        pos
    } else {
        // No delimiters, entire path is a single field
        components.push(ExtendedPathComponent::Field(path.to_string()));
        return;
    };

    // Parse remaining segments
    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                i += 1;
                // Parse field name after dot
                let field_end = memchr2(b'.', b'[', &bytes[i..]).map_or(bytes.len(), |p| i + p);

                if field_end > i {
                    let field = unsafe { std::str::from_utf8_unchecked(&bytes[i..field_end]) };
                    components.push(ExtendedPathComponent::Field(field.to_string()));
                }
                i = field_end;
            }
            b'[' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }

                if bytes[i] == b'"' {
                    // Quoted string: ["field"]
                    i += 1;
                    let (field, end_pos) = parse_quoted_string(&bytes[i..]);
                    components.push(ExtendedPathComponent::Field(field));
                    i += end_pos;

                    // Skip closing bracket
                    if i < bytes.len() && bytes[i] == b']' {
                        i += 1;
                    }
                } else {
                    // Numeric index: [0]
                    let bracket_end = memchr(b']', &bytes[i..]).map_or(bytes.len(), |p| i + p);

                    let index_bytes = &bytes[i..bracket_end];
                    let index = parse_usize_fast(index_bytes);
                    components.push(ExtendedPathComponent::ArrayIndex(index));
                    i = bracket_end + 1;
                }
            }
            _ => {
                // Skip unexpected characters
                i += 1;
            }
        }
    }
}

/// Parse extended path into borrowed component references.
fn parse_extended_ref_into<'a>(path: &'a str, components: &mut Vec<ExtendedPathComponentRef<'a>>) {
    components.clear();
    let bytes = path.as_bytes();

    if bytes.is_empty() {
        return;
    }

    // Parse optional root identifier
    let first_delim = memchr2(b'.', b'[', bytes);
    let mut i = if let Some(pos) = first_delim {
        if pos > 0 {
            let root = unsafe { std::str::from_utf8_unchecked(&bytes[0..pos]) };
            components.push(ExtendedPathComponentRef::Field(root));
        }
        pos
    } else {
        components.push(ExtendedPathComponentRef::Field(path));
        return;
    };

    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                i += 1;
                let field_end = memchr2(b'.', b'[', &bytes[i..]).map_or(bytes.len(), |p| i + p);

                if field_end > i {
                    let field = unsafe { std::str::from_utf8_unchecked(&bytes[i..field_end]) };
                    components.push(ExtendedPathComponentRef::Field(field));
                }
                i = field_end;
            }
            b'[' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }

                if bytes[i] == b'"' {
                    // For borrowed refs, we return the raw content (may contain escapes)
                    i += 1;
                    let string_end = find_string_end(&bytes[i..]);
                    let field = unsafe { std::str::from_utf8_unchecked(&bytes[i..i + string_end]) };
                    components.push(ExtendedPathComponentRef::Field(field));
                    i += string_end + 1; // Skip closing quote

                    if i < bytes.len() && bytes[i] == b']' {
                        i += 1;
                    }
                } else {
                    let bracket_end = memchr(b']', &bytes[i..]).map_or(bytes.len(), |p| i + p);

                    let index = parse_usize_fast(&bytes[i..bracket_end]);
                    components.push(ExtendedPathComponentRef::ArrayIndex(index));
                    i = bracket_end + 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Parse extended path into range-based components.
fn parse_extended_ranges(path: &str, components: &mut Vec<ExtendedPathComponentRange>) {
    components.clear();
    let bytes = path.as_bytes();

    if bytes.is_empty() {
        return;
    }

    // Parse optional root identifier
    let first_delim = memchr2(b'.', b'[', bytes);
    let mut i = if let Some(pos) = first_delim {
        if pos > 0 {
            components.push(ExtendedPathComponentRange::Field(0..pos));
        }
        pos
    } else {
        components.push(ExtendedPathComponentRange::Field(0..bytes.len()));
        return;
    };

    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                i += 1;
                let field_end = memchr2(b'.', b'[', &bytes[i..]).map_or(bytes.len(), |p| i + p);

                if field_end > i {
                    components.push(ExtendedPathComponentRange::Field(i..field_end));
                }
                i = field_end;
            }
            b'[' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }

                if bytes[i] == b'"' {
                    i += 1;
                    let (field, end_pos) = parse_quoted_string(&bytes[i..]);
                    // If the field contains escapes, we need to store it unescaped
                    let raw_end = i + find_string_end(&bytes[i..]);
                    let raw = unsafe { std::str::from_utf8_unchecked(&bytes[i..raw_end]) };

                    if raw == field {
                        // No escapes, use range
                        components.push(ExtendedPathComponentRange::Field(i..raw_end));
                    } else {
                        // Has escapes, store unescaped
                        components.push(ExtendedPathComponentRange::UnescapedField(field));
                    }

                    i += end_pos;
                    if i < bytes.len() && bytes[i] == b']' {
                        i += 1;
                    }
                } else {
                    let bracket_end = memchr(b']', &bytes[i..]).map_or(bytes.len(), |p| i + p);

                    let index = parse_usize_fast(&bytes[i..bracket_end]);
                    components.push(ExtendedPathComponentRange::ArrayIndex(index));
                    i = bracket_end + 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Parse a quoted string, handling escape sequences.
/// Returns (`unescaped_string`, `bytes_consumed_including_closing_quote`)
fn parse_quoted_string(bytes: &[u8]) -> (String, usize) {
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                return (result, i + 1);
            }
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                match bytes[i] {
                    b'"' => result.push('"'),
                    b'\\' => result.push('\\'),
                    b'n' => result.push('\n'),
                    b'r' => result.push('\r'),
                    b't' => result.push('\t'),
                    b'/' => result.push('/'),
                    b'b' => result.push('\x08'),
                    b'f' => result.push('\x0c'),
                    b'u' if i + 4 < bytes.len() => {
                        // Unicode escape: \uXXXX
                        if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 5])
                            && let Ok(code) = u32::from_str_radix(hex, 16)
                            && let Some(c) = char::from_u32(code)
                        {
                            result.push(c);
                            i += 4;
                        }
                    }
                    other => {
                        result.push('\\');
                        result.push(other as char);
                    }
                }
                i += 1;
            }
            _ => {
                // Safe because we're processing valid UTF-8 input
                result.push(bytes[i] as char);
                i += 1;
            }
        }
    }

    (result, i)
}

/// Find the end of a quoted string (position of closing quote).
fn find_string_end(bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return i,
            b'\\' if i + 1 < bytes.len() => i += 2,
            _ => i += 1,
        }
    }
    i
}

/// Fast usize parsing without error handling.
#[inline]
fn parse_usize_fast(bytes: &[u8]) -> usize {
    let mut result = 0usize;
    for &byte in bytes {
        if byte.is_ascii_digit() {
            result = result.wrapping_mul(10).wrapping_add((byte - b'0') as usize);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // ExtendedPathComponent Tests
    // =========================================================================

    #[test]
    fn test_extended_path_component_field() {
        let comp = ExtendedPathComponent::Field("test".to_string());
        assert_eq!(comp, ExtendedPathComponent::Field("test".to_string()));
    }

    #[test]
    fn test_extended_path_component_array_index() {
        let comp = ExtendedPathComponent::ArrayIndex(42);
        assert_eq!(comp, ExtendedPathComponent::ArrayIndex(42));
    }

    #[test]
    fn test_extended_path_component_debug() {
        let comp = ExtendedPathComponent::Field("test".to_string());
        let debug_str = format!("{comp:?}");
        assert!(debug_str.contains("Field"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_extended_path_component_clone() {
        let comp = ExtendedPathComponent::ArrayIndex(5);
        let cloned = comp.clone();
        assert_eq!(comp, cloned);
    }

    #[test]
    fn test_extended_path_component_eq() {
        let a = ExtendedPathComponent::Field("a".to_string());
        let b = ExtendedPathComponent::Field("a".to_string());
        let c = ExtendedPathComponent::Field("b".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // =========================================================================
    // ExtendedPathComponentRef Tests
    // =========================================================================

    #[test]
    fn test_extended_path_component_ref_field() {
        let comp = ExtendedPathComponentRef::Field("test");
        assert_eq!(comp, ExtendedPathComponentRef::Field("test"));
    }

    #[test]
    fn test_extended_path_component_ref_array_index() {
        let comp = ExtendedPathComponentRef::ArrayIndex(10);
        assert_eq!(comp, ExtendedPathComponentRef::ArrayIndex(10));
    }

    #[test]
    fn test_extended_path_component_ref_debug() {
        let comp = ExtendedPathComponentRef::Field("test");
        let debug_str = format!("{comp:?}");
        assert!(debug_str.contains("Field"));
    }

    #[test]
    fn test_extended_path_component_ref_clone() {
        let comp = ExtendedPathComponentRef::ArrayIndex(3);
        let cloned = comp;
        assert_eq!(comp, cloned);
    }

    #[test]
    fn test_extended_path_component_ref_copy() {
        let comp = ExtendedPathComponentRef::Field("x");
        let copied = comp;
        assert_eq!(comp, copied);
    }

    // =========================================================================
    // ExtendedPathComponentRange Tests
    // =========================================================================

    #[test]
    fn test_extended_path_component_range_field() {
        let comp = ExtendedPathComponentRange::Field(0..5);
        let debug_str = format!("{comp:?}");
        assert!(debug_str.contains("Field"));
    }

    #[test]
    fn test_extended_path_component_range_unescaped() {
        let comp = ExtendedPathComponentRange::UnescapedField("test".to_string());
        let debug_str = format!("{comp:?}");
        assert!(debug_str.contains("UnescapedField"));
    }

    #[test]
    fn test_extended_path_component_range_clone() {
        let comp = ExtendedPathComponentRange::ArrayIndex(7);
        let cloned = comp;
        let debug_str = format!("{cloned:?}");
        assert!(debug_str.contains("ArrayIndex"));
    }

    // =========================================================================
    // ParsedExtendedPath Tests
    // =========================================================================

    #[test]
    fn test_parsed_extended_path() {
        let parsed = ParsedExtendedPath::parse(r#"json["field"].items[0]"#);
        let owned = parsed.to_owned_components();
        assert_eq!(owned.len(), 4);
    }

    #[test]
    fn test_parsed_extended_path_path() {
        let parsed = ParsedExtendedPath::parse("test.path");
        assert_eq!(parsed.path(), "test.path");
    }

    #[test]
    fn test_parsed_extended_path_components() {
        let parsed = ParsedExtendedPath::parse("a.b.c");
        assert_eq!(parsed.components().len(), 3);
    }

    #[test]
    fn test_parsed_extended_path_with_unescaped() {
        // Path with escape sequences that need unescaping
        let parsed = ParsedExtendedPath::parse(r#"json["field\"quote"]"#);
        let owned = parsed.to_owned_components();
        assert_eq!(owned.len(), 2);
        assert_eq!(
            owned[1],
            ExtendedPathComponent::Field(r#"field"quote"#.to_string())
        );
    }

    #[test]
    fn test_parsed_extended_path_debug() {
        let parsed = ParsedExtendedPath::parse("test");
        let debug_str = format!("{parsed:?}");
        assert!(debug_str.contains("ParsedExtendedPath"));
    }

    #[test]
    fn test_parsed_extended_path_clone() {
        let parsed = ParsedExtendedPath::parse("a.b");
        let cloned = parsed;
        assert_eq!(cloned.path(), "a.b");
    }

    // =========================================================================
    // parse_extended_path Tests
    // =========================================================================

    #[test]
    fn test_simple_dot_path() {
        let components = parse_extended_path("json.name");
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[0],
            ExtendedPathComponent::Field("json".to_string())
        );
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field("name".to_string())
        );
    }

    #[test]
    fn test_array_index() {
        let components = parse_extended_path("json.items[0]");
        assert_eq!(components.len(), 3);
        assert_eq!(
            components[0],
            ExtendedPathComponent::Field("json".to_string())
        );
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field("items".to_string())
        );
        assert_eq!(components[2], ExtendedPathComponent::ArrayIndex(0));
    }

    #[test]
    fn test_bracket_notation() {
        let components = parse_extended_path(r#"json["field"]"#);
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[0],
            ExtendedPathComponent::Field("json".to_string())
        );
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field("field".to_string())
        );
    }

    #[test]
    fn test_bracket_with_dot() {
        let components = parse_extended_path(r#"json["field.with.dots"]"#);
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[0],
            ExtendedPathComponent::Field("json".to_string())
        );
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field("field.with.dots".to_string())
        );
    }

    #[test]
    fn test_mixed_notation() {
        let components = parse_extended_path(r#"json["key"].items[0].name"#);
        assert_eq!(components.len(), 5);
        assert_eq!(
            components[0],
            ExtendedPathComponent::Field("json".to_string())
        );
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field("key".to_string())
        );
        assert_eq!(
            components[2],
            ExtendedPathComponent::Field("items".to_string())
        );
        assert_eq!(components[3], ExtendedPathComponent::ArrayIndex(0));
        assert_eq!(
            components[4],
            ExtendedPathComponent::Field("name".to_string())
        );
    }

    #[test]
    fn test_escaped_quote() {
        let components = parse_extended_path(r#"json["field\"quote"]"#);
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field(r#"field"quote"#.to_string())
        );
    }

    #[test]
    fn test_unicode_escape() {
        let components = parse_extended_path(r#"json["\u0041\u0042"]"#);
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field("AB".to_string())
        );
    }

    #[test]
    fn test_single_field() {
        let components = parse_extended_path("name");
        assert_eq!(components.len(), 1);
        assert_eq!(
            components[0],
            ExtendedPathComponent::Field("name".to_string())
        );
    }

    #[test]
    fn test_empty_path() {
        let components = parse_extended_path("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_deep_nesting() {
        let components = parse_extended_path("a.b.c.d.e[0][1][2].f");
        assert_eq!(components.len(), 9);
        assert_eq!(components[5], ExtendedPathComponent::ArrayIndex(0));
        assert_eq!(components[6], ExtendedPathComponent::ArrayIndex(1));
        assert_eq!(components[7], ExtendedPathComponent::ArrayIndex(2));
    }

    #[test]
    fn test_leading_bracket() {
        let components = parse_extended_path(r#"["field"]"#);
        assert_eq!(components.len(), 1);
        assert_eq!(
            components[0],
            ExtendedPathComponent::Field("field".to_string())
        );
    }

    #[test]
    fn test_leading_bracket_index() {
        let components = parse_extended_path("[0].field");
        assert_eq!(components.len(), 2);
        assert_eq!(components[0], ExtendedPathComponent::ArrayIndex(0));
        assert_eq!(
            components[1],
            ExtendedPathComponent::Field("field".to_string())
        );
    }

    #[test]
    fn test_empty_quoted_field() {
        let components = parse_extended_path(r#"json[""]"#);
        assert_eq!(components.len(), 2);
        assert_eq!(components[1], ExtendedPathComponent::Field(String::new()));
    }

    #[test]
    fn test_consecutive_brackets() {
        let components = parse_extended_path(r#"["a"]["b"][0]"#);
        assert_eq!(components.len(), 3);
        assert_eq!(components[0], ExtendedPathComponent::Field("a".to_string()));
        assert_eq!(components[1], ExtendedPathComponent::Field("b".to_string()));
        assert_eq!(components[2], ExtendedPathComponent::ArrayIndex(0));
    }

    // =========================================================================
    // parse_extended_path_ref Tests
    // =========================================================================

    #[test]
    fn test_parse_ref_simple() {
        let components = parse_extended_path_ref("json.name");
        assert_eq!(components.len(), 2);
        assert_eq!(components[0], ExtendedPathComponentRef::Field("json"));
        assert_eq!(components[1], ExtendedPathComponentRef::Field("name"));
    }

    #[test]
    fn test_parse_ref_array_index() {
        let components = parse_extended_path_ref("items[5]");
        assert_eq!(components.len(), 2);
        assert_eq!(components[0], ExtendedPathComponentRef::Field("items"));
        assert_eq!(components[1], ExtendedPathComponentRef::ArrayIndex(5));
    }

    #[test]
    fn test_parse_ref_empty() {
        let components = parse_extended_path_ref("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_parse_ref_single_field() {
        let components = parse_extended_path_ref("field");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], ExtendedPathComponentRef::Field("field"));
    }

    #[test]
    fn test_parse_ref_bracket_notation() {
        let components = parse_extended_path_ref(r#"json["field"]"#);
        assert_eq!(components.len(), 2);
        assert_eq!(components[1], ExtendedPathComponentRef::Field("field"));
    }

    #[test]
    fn test_parse_ref_leading_bracket() {
        let components = parse_extended_path_ref("[0].name");
        assert_eq!(components.len(), 2);
        assert_eq!(components[0], ExtendedPathComponentRef::ArrayIndex(0));
    }

    // =========================================================================
    // parse_quoted_string Tests
    // =========================================================================

    #[test]
    fn test_parse_quoted_string_simple() {
        let (result, _) = parse_quoted_string(br#"hello""#);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_parse_quoted_string_escaped_backslash() {
        let (result, _) = parse_quoted_string(br#"back\\slash""#);
        assert_eq!(result, "back\\slash");
    }

    #[test]
    fn test_parse_quoted_string_newline() {
        let (result, _) = parse_quoted_string(br#"line\nbreak""#);
        assert_eq!(result, "line\nbreak");
    }

    #[test]
    fn test_parse_quoted_string_carriage_return() {
        let (result, _) = parse_quoted_string(br#"line\rbreak""#);
        assert_eq!(result, "line\rbreak");
    }

    #[test]
    fn test_parse_quoted_string_tab() {
        let (result, _) = parse_quoted_string(br#"with\ttab""#);
        assert_eq!(result, "with\ttab");
    }

    #[test]
    fn test_parse_quoted_string_slash() {
        let (result, _) = parse_quoted_string(br#"with\/slash""#);
        assert_eq!(result, "with/slash");
    }

    #[test]
    fn test_parse_quoted_string_backspace() {
        let (result, _) = parse_quoted_string(br#"with\bback""#);
        assert_eq!(result, "with\x08back");
    }

    #[test]
    fn test_parse_quoted_string_formfeed() {
        let (result, _) = parse_quoted_string(br#"with\ffeed""#);
        assert_eq!(result, "with\x0cfeed");
    }

    #[test]
    fn test_parse_quoted_string_invalid_escape() {
        let (result, _) = parse_quoted_string(br#"bad\xescape""#);
        assert_eq!(result, "bad\\xescape");
    }

    #[test]
    fn test_parse_quoted_string_no_closing_quote() {
        let (result, consumed) = parse_quoted_string(b"unterminated");
        assert_eq!(result, "unterminated");
        assert_eq!(consumed, 12);
    }

    // =========================================================================
    // find_string_end Tests
    // =========================================================================

    #[test]
    fn test_find_string_end_simple() {
        let end = find_string_end(br#"hello""#);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_find_string_end_with_escape() {
        let end = find_string_end(br#"hel\"lo""#);
        assert_eq!(end, 7);
    }

    #[test]
    fn test_find_string_end_no_quote() {
        let end = find_string_end(b"no quote");
        assert_eq!(end, 8);
    }

    #[test]
    fn test_find_string_end_empty() {
        let end = find_string_end(br#"""#);
        assert_eq!(end, 0);
    }

    // =========================================================================
    // parse_usize_fast Tests
    // =========================================================================

    #[test]
    fn test_parse_usize_fast_simple() {
        assert_eq!(parse_usize_fast(b"42"), 42);
    }

    #[test]
    fn test_parse_usize_fast_zero() {
        assert_eq!(parse_usize_fast(b"0"), 0);
    }

    #[test]
    fn test_parse_usize_fast_large() {
        assert_eq!(parse_usize_fast(b"12345"), 12345);
    }

    #[test]
    fn test_parse_usize_fast_empty() {
        assert_eq!(parse_usize_fast(b""), 0);
    }

    #[test]
    fn test_parse_usize_fast_non_digit() {
        assert_eq!(parse_usize_fast(b"12abc34"), 1234);
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_trailing_dot() {
        let components = parse_extended_path("json.");
        assert_eq!(components.len(), 1); // Just "json"
    }

    #[test]
    fn test_trailing_bracket() {
        let components = parse_extended_path("json[");
        assert_eq!(components.len(), 1); // Just "json"
    }

    #[test]
    fn test_empty_bracket() {
        let components = parse_extended_path("json[]");
        assert_eq!(components.len(), 2); // "json" and ArrayIndex(0)
    }

    #[test]
    fn test_unexpected_char() {
        // Unexpected characters are skipped
        let components = parse_extended_path("json!.name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_just_dot() {
        let components = parse_extended_path(".");
        assert!(components.is_empty());
    }

    #[test]
    fn test_just_bracket() {
        let components = parse_extended_path("[");
        assert!(components.is_empty());
    }

    #[test]
    fn test_multiple_dots() {
        let components = parse_extended_path("a..b");
        assert_eq!(components.len(), 2); // Empty fields are skipped
    }
}
