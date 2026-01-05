// SPDX-License-Identifier: MIT OR Apache-2.0
//! Scalar (baseline) skip implementation
//!
//! Byte-by-byte traversal with no SIMD. Used as baseline for benchmarks.

use super::{Skip, SkipResult};

/// Scalar byte-by-byte skip implementation
#[derive(Debug, Clone, Copy, Default)]
pub struct ScalarSkip;

impl ScalarSkip {
    /// Skip a container (object or array)
    fn skip_container(input: &[u8], open: u8, close: u8) -> Option<SkipResult> {
        let mut depth: usize = 1;
        let mut i: usize = 0;
        let mut in_string = false;
        let mut has_escapes = false;

        while i < input.len() {
            let b = input[i];

            if in_string {
                if b == b'\\' {
                    has_escapes = true;
                    i += 2; // Skip escaped char
                    continue;
                }
                if b == b'"' {
                    in_string = false;
                }
            } else {
                match b {
                    b'"' => in_string = true,
                    _ if b == open => depth += 1,
                    _ if b == close => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(SkipResult {
                                consumed: i + 1,
                                has_escapes,
                            });
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }

        None
    }
}

impl Skip for ScalarSkip {
    fn skip_object(&self, input: &[u8]) -> Option<SkipResult> {
        Self::skip_container(input, b'{', b'}')
    }

    fn skip_array(&self, input: &[u8]) -> Option<SkipResult> {
        Self::skip_container(input, b'[', b']')
    }

    fn skip_string(&self, input: &[u8]) -> Option<SkipResult> {
        let mut i: usize = 0;
        let mut has_escapes = false;

        while i < input.len() {
            let b = input[i];

            if b == b'\\' {
                has_escapes = true;
                i += 2; // Skip escaped char
                continue;
            }

            if b == b'"' {
                return Some(SkipResult {
                    consumed: i + 1,
                    has_escapes,
                });
            }

            i += 1;
        }

        None
    }

    fn skip_value(&self, input: &[u8]) -> Option<SkipResult> {
        // Find first non-whitespace
        let start = input
            .iter()
            .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))?;
        let first = input[start];

        let result = match first {
            b'{' => self.skip_object(&input[start + 1..]),
            b'[' => self.skip_array(&input[start + 1..]),
            b'"' => self.skip_string(&input[start + 1..]),
            b't' => {
                // true
                if input.len() >= start + 4 && &input[start..start + 4] == b"true" {
                    Some(SkipResult {
                        consumed: 4,
                        has_escapes: false,
                    })
                } else {
                    None
                }
            }
            b'f' => {
                // false
                if input.len() >= start + 5 && &input[start..start + 5] == b"false" {
                    Some(SkipResult {
                        consumed: 5,
                        has_escapes: false,
                    })
                } else {
                    None
                }
            }
            b'n' => {
                // null
                if input.len() >= start + 4 && &input[start..start + 4] == b"null" {
                    Some(SkipResult {
                        consumed: 4,
                        has_escapes: false,
                    })
                } else {
                    None
                }
            }
            b'-' | b'0'..=b'9' => {
                // Number - find end
                let num_start = start;
                let mut end = start;
                while end < input.len() {
                    match input[end] {
                        b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E' => end += 1,
                        _ => break,
                    }
                }
                Some(SkipResult {
                    consumed: end - num_start,
                    has_escapes: false,
                })
            }
            _ => None,
        };

        result.map(|r| SkipResult {
            consumed: start + r.consumed,
            has_escapes: r.has_escapes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Constructor and Trait Tests
    // =========================================================================

    #[test]
    fn test_scalar_skip_debug() {
        let skip = ScalarSkip;
        let debug = format!("{skip:?}");
        assert!(debug.contains("ScalarSkip"));
    }

    #[test]
    fn test_scalar_skip_default() {
        let skip = ScalarSkip;
        let _ = skip; // Just verify construction
    }

    #[test]
    fn test_scalar_skip_clone() {
        let skip = ScalarSkip;
        let cloned = skip;
        let _ = cloned;
    }

    // =========================================================================
    // Object Skip Tests
    // =========================================================================

    #[test]
    fn test_skip_simple_object() {
        let skip = ScalarSkip;
        let input = br#""name": "test"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_nested_object() {
        let skip = ScalarSkip;
        let input = br#""a": {"b": {"c": 1}}}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_object_with_string_braces() {
        let skip = ScalarSkip;
        let input = br#""text": "{ not a brace }"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_object_empty() {
        let skip = ScalarSkip;
        let input = b"}";
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_object_unclosed() {
        let skip = ScalarSkip;
        let input = br#""key": "value""#;
        let result = skip.skip_object(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_object_with_escape_in_string() {
        let skip = ScalarSkip;
        let input = br#""key": "val\"ue"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    // =========================================================================
    // Array Skip Tests
    // =========================================================================

    #[test]
    fn test_skip_array() {
        let skip = ScalarSkip;
        let input = br"1, 2, [3, 4], 5]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_array_empty() {
        let skip = ScalarSkip;
        let input = b"]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_array_unclosed() {
        let skip = ScalarSkip;
        let input = b"1, 2, 3";
        let result = skip.skip_array(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_array_with_strings() {
        let skip = ScalarSkip;
        let input = br#""a", "b", "c"]"#;
        let result = skip.skip_array(input);
        assert!(result.is_some());
    }

    // =========================================================================
    // String Skip Tests
    // =========================================================================

    #[test]
    fn test_skip_string() {
        let skip = ScalarSkip;
        let input = br#"hello world""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_string_with_escapes() {
        let skip = ScalarSkip;
        let input = br#"hello \"world\"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_empty() {
        let skip = ScalarSkip;
        let input = br#"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_string_unclosed() {
        let skip = ScalarSkip;
        let input = b"hello world";
        let result = skip.skip_string(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_string_escaped_backslash() {
        let skip = ScalarSkip;
        // Two escaped backslashes would be \\\\, but we test a simpler case:
        // Escaped backslash, then quote
        let input = br#"\\""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_escape_at_end() {
        let skip = ScalarSkip;
        // String ending with escape sequence without closing quote
        let input = b"test\\";
        let result = skip.skip_string(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // Value Skip Tests
    // =========================================================================

    #[test]
    fn test_skip_value_number() {
        let skip = ScalarSkip;
        let input = b"  123.45e10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_value_bool() {
        let skip = ScalarSkip;
        assert!(skip.skip_value(b"true").is_some());
        assert!(skip.skip_value(b"false").is_some());
        assert!(skip.skip_value(b"null").is_some());
    }

    #[test]
    fn test_skip_value_object() {
        let skip = ScalarSkip;
        let input = br#"{"key": "value"}"#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_array() {
        let skip = ScalarSkip;
        let input = b"[1, 2, 3]";
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_string() {
        let skip = ScalarSkip;
        let input = br#""hello""#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_empty() {
        let skip = ScalarSkip;
        let input = b"";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_whitespace_only() {
        let skip = ScalarSkip;
        let input = b"   \t\n\r   ";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_invalid() {
        let skip = ScalarSkip;
        let input = b"invalid";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_true_incomplete() {
        let skip = ScalarSkip;
        let input = b"tru";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_false_incomplete() {
        let skip = ScalarSkip;
        let input = b"fals";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_null_incomplete() {
        let skip = ScalarSkip;
        let input = b"nul";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_negative_number() {
        let skip = ScalarSkip;
        let input = b"-42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 3);
    }

    #[test]
    fn test_skip_value_number_with_exp() {
        let skip = ScalarSkip;
        let input = b"1e+10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_number_capital_e() {
        let skip = ScalarSkip;
        let input = b"1E10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_zero() {
        let skip = ScalarSkip;
        let input = b"0";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_value_with_leading_whitespace() {
        let skip = ScalarSkip;
        let input = b"   42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        // 3 spaces + 2 digits = 5
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_wrong_literal_t() {
        let skip = ScalarSkip;
        let input = b"test";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_wrong_literal_f() {
        let skip = ScalarSkip;
        let input = b"foo";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_wrong_literal_n() {
        let skip = ScalarSkip;
        let input = b"no";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_skip_object_deeply_nested() {
        let skip = ScalarSkip;
        let input = "{{{{{{{{{{}}}}}}}}}}";
        let result = skip.skip_object(&input.as_bytes()[1..]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 19);
    }

    #[test]
    fn test_skip_array_deeply_nested() {
        let skip = ScalarSkip;
        let input = "[[[[[[[[[[]]]]]]]]]]";
        let result = skip.skip_array(&input.as_bytes()[1..]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 19);
    }

    #[test]
    fn test_skip_string_long() {
        let skip = ScalarSkip;
        let long_string = "a".repeat(1000) + "\"";
        let result = skip.skip_string(long_string.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1001);
    }

    #[test]
    fn test_skip_container_string_with_brackets() {
        let skip = ScalarSkip;
        // Object with array brackets inside strings
        let input = br#""arr": "[not an array]"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
    }
}
