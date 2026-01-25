// SPDX-License-Identifier: MIT OR Apache-2.0
//! `JSONSki` skip implementation
//!
//! Bracket counting with string mask for on-demand container skipping.
//! Optimized algorithm from sonic-rs based on `JSONSki` paper.

use std::num::NonZeroU8;

use super::{Skip, SkipResult};

/// `JSONSki` skip using bracket counting with string mask
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonSkiSkip;

impl JsonSkiSkip {
    /// Create a new `JSONSki` skipper
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// XOR prefix for in-string detection
#[inline]
const fn prefix_xor(bitmask: u64) -> u64 {
    let mut m = bitmask;
    m ^= m << 1;
    m ^= m << 2;
    m ^= m << 4;
    m ^= m << 8;
    m ^= m << 16;
    m ^= m << 32;
    m
}

/// Branchless escape detection
#[inline]
fn get_escaped_branchless(prev_escaped: &mut u64, backslash: u64) -> u64 {
    const EVEN_BITS: u64 = 0x5555_5555_5555_5555;

    let backslash = backslash & !*prev_escaped;
    let follows_escape = (backslash << 1) | *prev_escaped;
    let odd_sequence_starts = backslash & !EVEN_BITS & !follows_escape;
    let (sequences_starting_on_even_bits, overflow) =
        odd_sequence_starts.overflowing_add(backslash);
    *prev_escaped = u64::from(overflow);
    let invert_mask = sequences_starting_on_even_bits << 1;
    (EVEN_BITS ^ invert_mask) & follows_escape
}

/// Get in-string bitmask for a 64-byte chunk
#[inline]
fn get_string_bits(chunk: &[u8; 64], prev_instring: &mut u64, prev_escaped: &mut u64) -> u64 {
    let mut quote_bits: u64 = 0;
    let mut bs_bits: u64 = 0;

    for (i, &byte) in chunk.iter().enumerate() {
        if byte == b'"' {
            quote_bits |= 1u64 << i;
        }
        if byte == b'\\' {
            bs_bits |= 1u64 << i;
        }
    }

    let escaped = if bs_bits != 0 {
        get_escaped_branchless(prev_escaped, bs_bits)
    } else {
        let e = *prev_escaped;
        *prev_escaped = 0;
        e
    };

    let unescaped_quotes = quote_bits & !escaped;
    let in_string = prefix_xor(unescaped_quotes) ^ *prev_instring;
    // Propagate MSB to all bits: if bit 63 is set, result is all 1s, else 0
    *prev_instring = 0u64.wrapping_sub(in_string >> 63);

    in_string
}

/// Core `JSONSki` skip loop - processes one 64-byte chunk
///
/// Returns position (1-indexed) if container closes in this chunk.
#[inline]
fn skip_container_loop(
    chunk: &[u8; 64],
    prev_instring: &mut u64,
    prev_escaped: &mut u64,
    lbrace_num: &mut usize,
    rbrace_num: &mut usize,
    left: u8,
    right: u8,
) -> Option<NonZeroU8> {
    let instring = get_string_bits(chunk, prev_instring, prev_escaped);

    // Build bitmasks for brackets
    let mut lbrace: u64 = 0;
    let mut rbrace: u64 = 0;

    for (i, &byte) in chunk.iter().enumerate() {
        if byte == left {
            lbrace |= 1u64 << i;
        }
        if byte == right {
            rbrace |= 1u64 << i;
        }
    }

    // Exclude brackets inside strings
    lbrace &= !instring;
    rbrace &= !instring;

    let last_lbrace_num = *lbrace_num;

    // Process each closing bracket
    while rbrace != 0 {
        *rbrace_num += 1;
        // Count left brackets before this right bracket
        *lbrace_num = last_lbrace_num + (lbrace & (rbrace - 1)).count_ones() as usize;

        // Container closed when right count exceeds left
        if *lbrace_num < *rbrace_num {
            debug_assert_eq!(*rbrace_num, *lbrace_num + 1);
            let cnt = rbrace.trailing_zeros() + 1;
            #[allow(clippy::cast_possible_truncation)] // cnt always <= 64 (chunk size)
            return NonZeroU8::new(cnt as u8);
        }

        // Clear lowest set bit
        rbrace &= rbrace - 1;
    }

    // Update left bracket count
    *lbrace_num = last_lbrace_num + lbrace.count_ones() as usize;

    None
}

impl JsonSkiSkip {
    fn skip_container(input: &[u8], left: u8, right: u8) -> Option<SkipResult> {
        let mut prev_instring: u64 = 0;
        let mut prev_escaped: u64 = 0;
        let mut lbrace_num: usize = 0; // Start at 0, already past opening brace
        let mut rbrace_num: usize = 0;
        let mut offset: usize = 0;
        let mut has_escapes = false;

        // Process 64-byte chunks
        while offset + 64 <= input.len() {
            let chunk: &[u8; 64] = input[offset..offset + 64].try_into().unwrap();

            if let Some(count) = skip_container_loop(
                chunk,
                &mut prev_instring,
                &mut prev_escaped,
                &mut lbrace_num,
                &mut rbrace_num,
                left,
                right,
            ) {
                has_escapes = has_escapes || prev_escaped != 0;
                return Some(SkipResult {
                    consumed: offset + count.get() as usize,
                    has_escapes,
                });
            }

            has_escapes = has_escapes || prev_escaped != 0;
            offset += 64;
        }

        // Handle remainder
        if offset < input.len() {
            let mut remain = [0u8; 64];
            let n = input.len() - offset;
            remain[..n].copy_from_slice(&input[offset..]);

            if let Some(count) = skip_container_loop(
                &remain,
                &mut prev_instring,
                &mut prev_escaped,
                &mut lbrace_num,
                &mut rbrace_num,
                left,
                right,
            ) {
                let pos = count.get() as usize;
                if pos <= n {
                    has_escapes = has_escapes || prev_escaped != 0;
                    return Some(SkipResult {
                        consumed: offset + pos,
                        has_escapes,
                    });
                }
            }
        }

        None
    }

    fn skip_string_impl(input: &[u8]) -> Option<SkipResult> {
        let mut prev_escaped: u64 = 0;
        let mut offset: usize = 0;
        let mut has_escapes = false;

        while offset + 64 <= input.len() {
            let chunk: &[u8; 64] = input[offset..offset + 64].try_into().unwrap();

            let mut quote_bits: u64 = 0;
            let mut bs_bits: u64 = 0;

            for (i, &byte) in chunk.iter().enumerate() {
                if byte == b'"' {
                    quote_bits |= 1u64 << i;
                }
                if byte == b'\\' {
                    bs_bits |= 1u64 << i;
                }
            }

            let escaped = if bs_bits != 0 {
                has_escapes = true;
                get_escaped_branchless(&mut prev_escaped, bs_bits)
            } else {
                let e = prev_escaped;
                prev_escaped = 0;
                e
            };

            let unescaped_quotes = quote_bits & !escaped;

            if unescaped_quotes != 0 {
                let pos = unescaped_quotes.trailing_zeros() as usize;
                return Some(SkipResult {
                    consumed: offset + pos + 1,
                    has_escapes,
                });
            }

            offset += 64;
        }

        // Handle remainder
        if offset < input.len() {
            let mut remain = [0u8; 64];
            let n = input.len() - offset;
            remain[..n].copy_from_slice(&input[offset..]);

            let mut quote_bits: u64 = 0;
            let mut bs_bits: u64 = 0;

            for (i, &byte) in remain[..n].iter().enumerate() {
                if byte == b'"' {
                    quote_bits |= 1u64 << i;
                }
                if byte == b'\\' {
                    bs_bits |= 1u64 << i;
                }
            }

            let escaped = if bs_bits != 0 {
                has_escapes = true;
                get_escaped_branchless(&mut prev_escaped, bs_bits)
            } else {
                prev_escaped
            };

            let unescaped_quotes = quote_bits & !escaped;

            if unescaped_quotes != 0 {
                let pos = unescaped_quotes.trailing_zeros() as usize;
                if pos < n {
                    return Some(SkipResult {
                        consumed: offset + pos + 1,
                        has_escapes,
                    });
                }
            }
        }

        None
    }
}

impl Skip for JsonSkiSkip {
    fn skip_object(&self, input: &[u8]) -> Option<SkipResult> {
        Self::skip_container(input, b'{', b'}')
    }

    fn skip_array(&self, input: &[u8]) -> Option<SkipResult> {
        Self::skip_container(input, b'[', b']')
    }

    fn skip_string(&self, input: &[u8]) -> Option<SkipResult> {
        Self::skip_string_impl(input)
    }

    fn skip_value(&self, input: &[u8]) -> Option<SkipResult> {
        let start = input
            .iter()
            .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))?;
        let first = input[start];

        let result = match first {
            b'{' => Self::skip_container(&input[start + 1..], b'{', b'}'),
            b'[' => Self::skip_container(&input[start + 1..], b'[', b']'),
            b'"' => Self::skip_string_impl(&input[start + 1..]),
            b't' => {
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
                let mut end = start;
                while end < input.len() {
                    match input[end] {
                        b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E' => end += 1,
                        _ => break,
                    }
                }
                Some(SkipResult {
                    consumed: end - start,
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
    fn test_json_ski_skip_new() {
        let skip = JsonSkiSkip::new();
        assert!(std::mem::size_of_val(&skip) == 0);
    }

    #[test]
    fn test_json_ski_skip_default() {
        let skip = JsonSkiSkip;
        let _ = skip.skip_object(b"}");
    }

    #[test]
    fn test_json_ski_skip_clone_copy() {
        let skip = JsonSkiSkip::new();
        let cloned = skip;
        let copied = skip;
        let _ = cloned.skip_object(b"}");
        let _ = copied.skip_object(b"}");
    }

    #[test]
    fn test_json_ski_skip_debug() {
        let skip = JsonSkiSkip::new();
        let debug = format!("{skip:?}");
        assert!(debug.contains("JsonSkiSkip"));
    }

    // =========================================================================
    // Object Skip Tests
    // =========================================================================

    #[test]
    fn test_skip_simple_object() {
        let skip = JsonSkiSkip::new();
        let input = br#""name": "test"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_nested_object() {
        let skip = JsonSkiSkip::new();
        let input = br#""a": {"b": {"c": 1}}}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_object_with_string_braces() {
        let skip = JsonSkiSkip::new();
        let input = br#""text": "{ not a brace }"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_array() {
        let skip = JsonSkiSkip::new();
        let input = b"1, 2, [3, 4], 5]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_string_with_escapes() {
        let skip = JsonSkiSkip::new();
        let input = br#"hello \"world\"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.consumed, input.len());
        assert!(r.has_escapes);
    }

    #[test]
    fn test_skip_large_object() {
        use std::fmt::Write;
        let skip = JsonSkiSkip::new();
        let mut json = String::new();
        for i in 0..50 {
            if i > 0 {
                json.push_str(", ");
            }
            let _ = write!(json, "\"field{i}\": {i}");
        }
        json.push('}');

        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, json.len());
    }

    #[test]
    fn test_deeply_nested() {
        let skip = JsonSkiSkip::new();
        // 10 levels deep - 61 bytes with correct brace matching
        let input = br#"{"a":{"b":{"c":{"d":{"e":{"f":{"g":{"h":{"i":{"j":1}}}}}}}}}}"#;
        // Verify structure
        assert_eq!(input.len(), 61, "Input length should be 61");
        assert_eq!(input[0], b'{', "Should start with open brace");
        assert_eq!(input[60], b'}', "Should end with close brace");

        // Skip after first {
        let sliced = &input[1..];
        assert_eq!(sliced.len(), 60, "Sliced length should be 60");

        let result = skip.skip_object(sliced);
        assert!(result.is_some(), "Failed on input len={}", sliced.len());
        assert_eq!(result.unwrap().consumed, 60);
    }

    #[test]
    fn test_prefix_xor_simple() {
        // Quotes at positions 0 and 2
        let result = super::prefix_xor(0b101);
        // Bits 0, 1 should be set (inside string between quotes)
        assert_eq!(result & 0b11, 0b11, "bits 0,1 should be set");
        // Bit 2 should NOT be set (closing quote ends string)
        assert_eq!(result & 0b100, 0, "bit 2 should not be set");
    }

    #[test]
    fn test_three_level_nested() {
        let skip = JsonSkiSkip::new();
        // Simpler 3-level case
        let input = br#"{"a":{"b":{"c":1}}}"#;
        let result = skip.skip_object(&input[1..]);
        assert!(
            result.is_some(),
            "3-level nested failed, input[1..] len={}",
            input.len() - 1
        );
        assert_eq!(result.unwrap().consumed, input.len() - 1);
    }

    #[test]
    fn test_array_with_nested_objects() {
        let skip = JsonSkiSkip::new();
        let input = br#"{"id":1},{"id":2},{"id":3}]"#;
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_unclosed_container() {
        let skip = JsonSkiSkip::new();
        let input = br#""a": {"b": 1"#;
        let result = skip.skip_object(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_object() {
        let skip = JsonSkiSkip::new();
        let input = b"}";
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_empty_array() {
        let skip = JsonSkiSkip::new();
        let input = b"]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    // =========================================================================
    // skip_value Tests
    // =========================================================================

    #[test]
    fn test_skip_value_object() {
        let skip = JsonSkiSkip::new();
        // skip_value for containers: start + skip_container result (which skips past opening brace)
        // For {"a":1}, start=0, skip_container("a":1}") returns 6, total = 6
        let input = br#"{"a":1}"#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.consumed, 6);
    }

    #[test]
    fn test_skip_value_array() {
        let skip = JsonSkiSkip::new();
        // For [1,2,3], start=0, skip_container("1,2,3]") returns 6, total = 6
        let input = b"[1,2,3]";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
    }

    #[test]
    fn test_skip_value_string() {
        let skip = JsonSkiSkip::new();
        // For "hello", start=0, skip_string_impl("hello\"") returns 6, total = 6
        let input = br#""hello""#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
    }

    #[test]
    fn test_skip_value_true() {
        let skip = JsonSkiSkip::new();
        let input = b"true";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_false() {
        let skip = JsonSkiSkip::new();
        let input = b"false";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_null() {
        let skip = JsonSkiSkip::new();
        let input = b"null";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_number_integer() {
        let skip = JsonSkiSkip::new();
        let input = b"12345";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_number_negative() {
        let skip = JsonSkiSkip::new();
        let input = b"-42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 3);
    }

    #[test]
    fn test_skip_value_number_float() {
        let skip = JsonSkiSkip::new();
        let input = b"3.14159";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 7);
    }

    #[test]
    fn test_skip_value_number_exponent() {
        let skip = JsonSkiSkip::new();
        let input = b"1.5e10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
    }

    #[test]
    fn test_skip_value_number_exponent_negative() {
        let skip = JsonSkiSkip::new();
        let input = b"1E-5";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_number_exponent_plus() {
        let skip = JsonSkiSkip::new();
        let input = b"1e+10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_with_leading_whitespace() {
        let skip = JsonSkiSkip::new();
        let input = b"  \t\n  42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 8);
    }

    #[test]
    fn test_skip_value_invalid_true() {
        let skip = JsonSkiSkip::new();
        let input = b"tru";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_invalid_false() {
        let skip = JsonSkiSkip::new();
        let input = b"fals";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_invalid_null() {
        let skip = JsonSkiSkip::new();
        let input = b"nul";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_unknown_literal() {
        let skip = JsonSkiSkip::new();
        let input = b"undefined";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_empty_input() {
        let skip = JsonSkiSkip::new();
        let input = b"";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_only_whitespace() {
        let skip = JsonSkiSkip::new();
        let input = b"   \t\n\r  ";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // String Skip Tests
    // =========================================================================

    #[test]
    fn test_skip_string_simple() {
        let skip = JsonSkiSkip::new();
        let input = br#"hello""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
        assert!(!result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_empty() {
        let skip = JsonSkiSkip::new();
        let input = br#"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_string_escaped_backslash() {
        let skip = JsonSkiSkip::new();
        let input = br#"path\\to\\file""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_unclosed() {
        let skip = JsonSkiSkip::new();
        let input = b"hello world";
        let result = skip.skip_string(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_string_long() {
        let skip = JsonSkiSkip::new();
        // 100 character string plus closing quote
        let mut input = "a".repeat(100);
        input.push('"');
        let result = skip.skip_string(input.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 101);
    }

    #[test]
    fn test_skip_string_multiple_chunks() {
        let skip = JsonSkiSkip::new();
        // String that spans multiple 64-byte chunks
        let mut input = "x".repeat(200);
        input.push('"');
        let result = skip.skip_string(input.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 201);
    }

    #[test]
    fn test_skip_string_escape_at_chunk_boundary() {
        let skip = JsonSkiSkip::new();
        // Put escape exactly at byte 63 (last byte of first chunk)
        let mut input = "a".repeat(63);
        input.push('\\');
        input.push('"');
        input.push('"');
        let result = skip.skip_string(input.as_bytes());
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    // =========================================================================
    // Container Skip Tests - Multi-Chunk
    // =========================================================================

    #[test]
    fn test_skip_object_multi_chunk() {
        let skip = JsonSkiSkip::new();
        // Object that spans multiple 64-byte chunks
        let mut json = r#""key":"#.to_string();
        json.push('"');
        json.push_str(&"x".repeat(100));
        json.push('"');
        json.push('}');
        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, json.len());
    }

    #[test]
    fn test_skip_array_multi_chunk() {
        use std::fmt::Write;
        let skip = JsonSkiSkip::new();
        // Array that spans multiple 64-byte chunks
        let mut json = String::new();
        for i in 0..50 {
            if i > 0 {
                json.push(',');
            }
            let _ = write!(json, "{i}");
        }
        json.push(']');
        let result = skip.skip_array(json.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, json.len());
    }

    #[test]
    fn test_skip_object_with_escaped_quotes_in_value() {
        let skip = JsonSkiSkip::new();
        let input = br#""text": "say \"hello\" here"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    // =========================================================================
    // Prefix XOR and Escape Detection Tests
    // =========================================================================

    #[test]
    fn test_prefix_xor_empty() {
        let result = super::prefix_xor(0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_prefix_xor_single_bit() {
        let result = super::prefix_xor(1);
        assert_eq!(result, u64::MAX); // All bits set after position 0
    }

    #[test]
    fn test_prefix_xor_alternating() {
        let result = super::prefix_xor(0b10_1010);
        // Complex pattern from alternating bits
        assert_ne!(result, 0);
    }

    #[test]
    fn test_get_escaped_branchless() {
        let mut prev_escaped = 0u64;

        // Single backslash at position 0
        let result = get_escaped_branchless(&mut prev_escaped, 1);
        // Position 1 should be escaped
        assert_eq!(result & 0b10, 0b10);
    }

    #[test]
    fn test_get_escaped_branchless_double_backslash() {
        let mut prev_escaped = 0u64;

        // Two backslashes at positions 0 and 1
        let result = get_escaped_branchless(&mut prev_escaped, 0b11);
        // Second backslash escapes itself, no further escape
        assert_eq!(result & 0b10, 0b10);
    }

    #[test]
    fn test_get_string_bits() {
        let mut prev_instring = 0u64;
        let mut prev_escaped = 0u64;

        // Simple: quote at position 0 and 5
        let mut chunk = [0u8; 64];
        chunk[0] = b'"';
        chunk[5] = b'"';

        let result = get_string_bits(&chunk, &mut prev_instring, &mut prev_escaped);
        // Bits 0-4 should indicate "in string"
        assert_ne!(result & 0b11110, 0);
    }

    #[test]
    fn test_get_string_bits_with_escape() {
        let mut prev_instring = 0u64;
        let mut prev_escaped = 0u64;

        // Quote, then backslash+quote (escaped), then quote
        let mut chunk = [0u8; 64];
        chunk[0] = b'"';
        chunk[1] = b'\\';
        chunk[2] = b'"';
        chunk[3] = b'"';

        let result = get_string_bits(&chunk, &mut prev_instring, &mut prev_escaped);
        // String should span from 0 to 3 (escaped quote at 2 doesn't end it)
        assert_ne!(result, 0);
    }

    #[test]
    fn test_get_string_bits_no_quotes() {
        let mut prev_instring = 0u64;
        let mut prev_escaped = 0u64;

        let chunk = [b'a'; 64];
        let result = get_string_bits(&chunk, &mut prev_instring, &mut prev_escaped);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_get_string_bits_no_backslashes() {
        let mut prev_instring = 0u64;
        let mut prev_escaped = 0u64;

        let mut chunk = [b'a'; 64];
        chunk[0] = b'"';
        chunk[10] = b'"';

        let result = get_string_bits(&chunk, &mut prev_instring, &mut prev_escaped);
        // In-string bits should be set between quotes
        assert_ne!(result, 0);
    }

    // =========================================================================
    // Container Loop Tests
    // =========================================================================

    #[test]
    fn test_skip_container_loop_simple_close() {
        let mut prev_instring = 0u64;
        let mut prev_escaped = 0u64;
        let mut lbrace_num = 0;
        let mut rbrace_num = 0;

        let mut chunk = [0u8; 64];
        chunk[0] = b'}';

        let result = skip_container_loop(
            &chunk,
            &mut prev_instring,
            &mut prev_escaped,
            &mut lbrace_num,
            &mut rbrace_num,
            b'{',
            b'}',
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().get(), 1);
    }

    #[test]
    fn test_skip_container_loop_nested() {
        let mut prev_instring = 0u64;
        let mut prev_escaped = 0u64;
        let mut lbrace_num = 0;
        let mut rbrace_num = 0;

        let mut chunk = [0u8; 64];
        // Nested: {} should close at position 2
        chunk[0] = b'{';
        chunk[1] = b'}';
        chunk[2] = b'}';

        let result = skip_container_loop(
            &chunk,
            &mut prev_instring,
            &mut prev_escaped,
            &mut lbrace_num,
            &mut rbrace_num,
            b'{',
            b'}',
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().get(), 3);
    }

    #[test]
    fn test_skip_container_loop_no_close() {
        let mut prev_instring = 0u64;
        let mut prev_escaped = 0u64;
        let mut lbrace_num = 0;
        let mut rbrace_num = 0;

        let mut chunk = [0u8; 64];
        chunk[0] = b'{';
        chunk[1] = b'{';

        let result = skip_container_loop(
            &chunk,
            &mut prev_instring,
            &mut prev_escaped,
            &mut lbrace_num,
            &mut rbrace_num,
            b'{',
            b'}',
        );

        assert!(result.is_none());
        assert_eq!(lbrace_num, 2);
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_skip_object_in_remainder() {
        let skip = JsonSkiSkip::new();
        // Object that ends in the remainder (< 64 bytes)
        let input = br#""a":1}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
    }

    #[test]
    fn test_skip_string_in_remainder() {
        let skip = JsonSkiSkip::new();
        // Short string that's handled in remainder
        let input = br#"test""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_object_spans_chunk() {
        let skip = JsonSkiSkip::new();
        // Object content that spans multiple chunks (> 64 bytes)
        // skip_object expects input AFTER opening {
        let mut input = "\"key\":\"".to_string(); // 7 bytes
        input.push_str(&"x".repeat(80)); // 80 bytes
        input.push('"'); // 1 byte
        input.push('}'); // 1 byte
        // Total: 89 bytes

        let result = skip.skip_object(input.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_object_two_chunks() {
        let skip = JsonSkiSkip::new();
        // Object content that spans two full 64-byte chunks plus remainder
        let mut input = "\"k\":\"".to_string(); // 5 bytes
        input.push_str(&"a".repeat(150)); // 150 bytes
        input.push('"'); // 1 byte
        input.push('}'); // 1 byte
        // Total: 157 bytes

        let result = skip.skip_object(input.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_value_number_with_trailing() {
        let skip = JsonSkiSkip::new();
        let input = b"123,";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 3); // Only the number
    }

    #[test]
    fn test_skip_value_zero() {
        let skip = JsonSkiSkip::new();
        let input = b"0";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }
}
