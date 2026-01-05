// SPDX-License-Identifier: MIT OR Apache-2.0
//! Langdale-Lemire skip implementation
//!
//! Uses XOR prefix for string detection and branchless escape handling.
//! Processes 64 bytes at a time using bitmask operations.

use super::{Skip, SkipResult};

/// Langdale-Lemire skip using XOR prefix and branchless escapes
#[derive(Debug, Clone, Copy, Default)]
pub struct LangdaleSkip;

impl LangdaleSkip {
    /// Create a new Langdale-Lemire skipper
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// XOR prefix computation - flips state at each set bit
///
/// For quotes at positions 2 and 7: `0b10000100`
/// Result marks positions 2-6 as inside string: `0b01111100`
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

/// Branchless escape detection from simdjson
///
/// Computes which characters follow an odd number of backslashes.
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

/// Get bitmask of positions inside strings for a 64-byte chunk
#[inline]
fn get_string_bits(chunk: &[u8; 64], prev_instring: &mut u64, prev_escaped: &mut u64) -> u64 {
    // Build bitmasks for quotes and backslashes
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

    // Get escaped positions
    let escaped = if bs_bits != 0 {
        get_escaped_branchless(prev_escaped, bs_bits)
    } else {
        let e = *prev_escaped;
        *prev_escaped = 0;
        e
    };

    // Remove escaped quotes
    let unescaped_quotes = quote_bits & !escaped;

    // Compute in-string mask using XOR prefix
    let in_string = prefix_xor(unescaped_quotes) ^ *prev_instring;
    // Propagate MSB to all bits: if bit 63 is set, result is all 1s, else 0
    *prev_instring = 0u64.wrapping_sub(in_string >> 63);

    in_string
}

impl LangdaleSkip {
    fn skip_container(input: &[u8], open: u8, close: u8) -> Option<SkipResult> {
        let mut prev_instring: u64 = 0;
        let mut prev_escaped: u64 = 0;
        let mut lbrace_num: usize = 0;
        let mut rbrace_num: usize = 0;
        let mut offset: usize = 0;
        let mut has_escapes = false;

        // Process 64-byte chunks
        while offset + 64 <= input.len() {
            let chunk: &[u8; 64] = input[offset..offset + 64].try_into().unwrap();
            let instring = get_string_bits(chunk, &mut prev_instring, &mut prev_escaped);
            has_escapes = has_escapes || prev_escaped != 0;

            // Build bitmasks for open/close, excluding those in strings
            let mut open_bits: u64 = 0;
            let mut close_bits: u64 = 0;

            for (i, &byte) in chunk.iter().enumerate() {
                if byte == open {
                    open_bits |= 1u64 << i;
                }
                if byte == close {
                    close_bits |= 1u64 << i;
                }
            }

            open_bits &= !instring;
            close_bits &= !instring;

            let last_lbrace_num = lbrace_num;

            // Process each close brace
            while close_bits != 0 {
                rbrace_num += 1;
                let close_pos = close_bits.trailing_zeros() as usize;
                // Count opens before this close
                lbrace_num =
                    last_lbrace_num + (open_bits & ((1u64 << close_pos) - 1)).count_ones() as usize;

                // Container closed when closes exceed opens
                if lbrace_num < rbrace_num {
                    return Some(SkipResult {
                        consumed: offset + close_pos + 1,
                        has_escapes,
                    });
                }
                close_bits &= close_bits - 1;
            }

            // Add remaining opens
            lbrace_num = last_lbrace_num + open_bits.count_ones() as usize;
            offset += 64;
        }

        // Handle remainder
        if offset < input.len() {
            let mut remain = [0u8; 64];
            let n = input.len() - offset;
            remain[..n].copy_from_slice(&input[offset..]);

            let instring = get_string_bits(&remain, &mut prev_instring, &mut prev_escaped);
            has_escapes = has_escapes || prev_escaped != 0;

            let mut open_bits: u64 = 0;
            let mut close_bits: u64 = 0;

            for (i, &byte) in remain[..n].iter().enumerate() {
                if byte == open {
                    open_bits |= 1u64 << i;
                }
                if byte == close {
                    close_bits |= 1u64 << i;
                }
            }

            open_bits &= !instring;
            close_bits &= !instring;

            let last_lbrace_num = lbrace_num;

            while close_bits != 0 {
                let close_pos = close_bits.trailing_zeros() as usize;
                if close_pos >= n {
                    break;
                }
                rbrace_num += 1;
                lbrace_num =
                    last_lbrace_num + (open_bits & ((1u64 << close_pos) - 1)).count_ones() as usize;

                if lbrace_num < rbrace_num {
                    return Some(SkipResult {
                        consumed: offset + close_pos + 1,
                        has_escapes,
                    });
                }
                close_bits &= close_bits - 1;
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

impl Skip for LangdaleSkip {
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

    #[test]
    fn test_prefix_xor() {
        // Quote at position 0: all following bits flip
        assert_eq!(prefix_xor(0b1) & 0xFF, 0xFF);
        // Quotes at 0 and 4: bits 0-3 inside string
        assert_eq!(prefix_xor(0b10001) & 0xFF, 0b01111);
    }

    #[test]
    fn test_skip_simple_object() {
        let skip = LangdaleSkip::new();
        let input = br#""name": "test"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_nested_object() {
        let skip = LangdaleSkip::new();
        let input = br#""a": {"b": {"c": 1}}}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_object_with_string_braces() {
        let skip = LangdaleSkip::new();
        let input = br#""text": "{ not a brace }"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_string_with_escapes() {
        let skip = LangdaleSkip::new();
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
        let skip = LangdaleSkip::new();
        // Object spanning multiple 64-byte chunks
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
    fn test_escaped_quote_at_chunk_boundary() {
        let skip = LangdaleSkip::new();
        // Create input where escape is at position 63 (end of first chunk)
        let mut input = vec![b' '; 63];
        input.push(b'\\');
        input.push(b'"'); // This quote is escaped
        input.push(b'"'); // This is the real closing quote

        let result = skip.skip_string(&input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    // =========================================================================
    // LangdaleSkip Constructor Tests
    // =========================================================================

    #[test]
    fn test_new_default() {
        let skip1 = LangdaleSkip::new();
        let skip2 = LangdaleSkip;
        // Both should behave the same
        let input = br#""test"}"#;
        assert_eq!(
            skip1.skip_object(input).map(|r| r.consumed),
            skip2.skip_object(input).map(|r| r.consumed)
        );
    }

    #[test]
    fn test_debug() {
        let skip = LangdaleSkip::new();
        let debug_str = format!("{skip:?}");
        assert!(debug_str.contains("LangdaleSkip"));
    }

    #[test]
    fn test_clone() {
        let skip = LangdaleSkip::new();
        let cloned = skip; // Copy
        let input = b"test\"}";
        assert_eq!(
            skip.skip_object(input).is_some(),
            cloned.skip_object(input).is_some()
        );
    }

    // =========================================================================
    // skip_value Tests
    // =========================================================================

    #[test]
    fn test_skip_value_object() {
        let skip = LangdaleSkip::new();
        // skip_value for object: { is found at start=0, then skip_container processes rest
        // skip_container returns consumed=1 (for `}`), final = start + consumed = 0 + 1 = 1
        let input = br"{}";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_value_array() {
        let skip = LangdaleSkip::new();
        // skip_value for array: [ is found at start=0, then skip_container processes rest
        let input = br"[]";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_value_string() {
        let skip = LangdaleSkip::new();
        // skip_value for string: " is found at start=0, then skip_string_impl processes rest
        // "hello" -> skip_string_impl gets "hello", returns consumed=6 (5 chars + closing quote)
        // final = start(0) + consumed(6) = 6
        let input = br#""hello""#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
    }

    #[test]
    fn test_skip_value_true() {
        let skip = LangdaleSkip::new();
        let input = b"true";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
        assert!(!result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_value_false() {
        let skip = LangdaleSkip::new();
        let input = b"false";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
        assert!(!result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_value_null() {
        let skip = LangdaleSkip::new();
        let input = b"null";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_positive_integer() {
        let skip = LangdaleSkip::new();
        let input = b"12345";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_negative_integer() {
        let skip = LangdaleSkip::new();
        let input = b"-42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 3);
    }

    #[test]
    fn test_skip_value_float() {
        let skip = LangdaleSkip::new();
        let input = b"3.14159";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 7);
    }

    #[test]
    fn test_skip_value_scientific() {
        let skip = LangdaleSkip::new();
        let input = b"1.5e+10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 7);
    }

    #[test]
    fn test_skip_value_with_whitespace() {
        let skip = LangdaleSkip::new();
        let input = b"  \t\n42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6); // whitespace + number
    }

    #[test]
    fn test_skip_value_invalid() {
        let skip = LangdaleSkip::new();
        let input = b"xyz"; // Invalid value
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_empty() {
        let skip = LangdaleSkip::new();
        let input = b"   "; // Only whitespace
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_incomplete_true() {
        let skip = LangdaleSkip::new();
        let input = b"tru"; // Incomplete
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_incomplete_false() {
        let skip = LangdaleSkip::new();
        let input = b"fals"; // Incomplete
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_incomplete_null() {
        let skip = LangdaleSkip::new();
        let input = b"nul"; // Incomplete
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // skip_array Tests
    // =========================================================================

    #[test]
    fn test_skip_array_simple() {
        let skip = LangdaleSkip::new();
        let input = b"1, 2, 3]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_array_nested() {
        let skip = LangdaleSkip::new();
        let input = b"[1, 2], [3, 4]]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_array_with_strings() {
        let skip = LangdaleSkip::new();
        let input = br#""a", "b", "c"]"#;
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_array_with_escaped_brackets() {
        let skip = LangdaleSkip::new();
        let input = br#""text with [brackets]"]"#;
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    // =========================================================================
    // skip_string Tests
    // =========================================================================

    #[test]
    fn test_skip_string_simple() {
        let skip = LangdaleSkip::new();
        let input = br#"hello""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
        assert!(!result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_empty() {
        let skip = LangdaleSkip::new();
        let input = br#"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_string_with_backslash() {
        let skip = LangdaleSkip::new();
        let input = br#"path\\to\\file""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_with_newline_escape() {
        let skip = LangdaleSkip::new();
        let input = br#"line1\nline2""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_unclosed() {
        let skip = LangdaleSkip::new();
        let input = b"no closing quote";
        let result = skip.skip_string(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // Large Input Tests (Multiple Chunks)
    // =========================================================================

    #[test]
    fn test_skip_large_array() {
        let skip = LangdaleSkip::new();
        // Create array with many elements spanning multiple chunks
        let mut json = String::new();
        for i in 0..100 {
            if i > 0 {
                json.push_str(", ");
            }
            json.push_str(&i.to_string());
        }
        json.push(']');

        let result = skip.skip_array(json.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, json.len());
    }

    #[test]
    fn test_skip_long_string() {
        let skip = LangdaleSkip::new();
        // String longer than 64 bytes
        let content = "a".repeat(200);
        let input = format!("{content}\"");

        let result = skip.skip_string(input.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 201);
    }

    #[test]
    fn test_skip_string_long_with_escapes() {
        let skip = LangdaleSkip::new();
        // String with escape in middle, longer than 64 bytes
        let content = format!("{}\\n{}", "a".repeat(50), "b".repeat(50));
        let input = format!("{content}\"");

        let result = skip.skip_string(input.as_bytes());
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    // =========================================================================
    // get_escaped_branchless Edge Cases
    // =========================================================================

    #[test]
    fn test_many_consecutive_escapes() {
        let skip = LangdaleSkip::new();
        // Many consecutive backslashes followed by quote
        let input = br#"\\\\\\\\"end""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_alternating_escapes() {
        let skip = LangdaleSkip::new();
        // Alternating backslash-char patterns
        let input = br#"\n\t\r\"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    // =========================================================================
    // Remainder Handling Tests
    // =========================================================================

    #[test]
    fn test_object_exact_64_bytes() {
        let skip = LangdaleSkip::new();
        // Create object that is exactly 64 bytes
        let padding = "a".repeat(50);
        let json = format!("\"{padding}\"}}",);

        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
    }

    #[test]
    fn test_object_65_bytes() {
        let skip = LangdaleSkip::new();
        // Create object that is 65 bytes (triggers remainder path)
        let padding = "a".repeat(51);
        let json = format!("\"{padding}\"}}");

        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
    }

    #[test]
    fn test_string_in_remainder() {
        let skip = LangdaleSkip::new();
        // String with quote in remainder section
        let mut input = vec![b'a'; 70];
        input.push(b'"');

        let result = skip.skip_string(&input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 71);
    }

    #[test]
    fn test_escape_in_remainder() {
        let skip = LangdaleSkip::new();
        // Backslash in remainder section
        let mut input = vec![b'a'; 70];
        input.extend_from_slice(br#"\\""#);

        let result = skip.skip_string(&input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    // =========================================================================
    // Container Close in Remainder
    // =========================================================================

    #[test]
    fn test_object_close_in_remainder() {
        let skip = LangdaleSkip::new();
        // Object close brace in remainder section
        let padding = "x".repeat(70);
        let json = format!("\"{padding}\":{{}}}}");

        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
    }

    #[test]
    fn test_array_close_in_remainder() {
        let skip = LangdaleSkip::new();
        // Array close bracket in remainder section
        let padding = "x".repeat(70);
        let json = format!("\"{padding}\":[]]");

        let result = skip.skip_array(json.as_bytes());
        assert!(result.is_some());
    }

    // =========================================================================
    // prefix_xor Additional Tests
    // =========================================================================

    #[test]
    fn test_prefix_xor_multiple_quotes() {
        // Multiple quote pairs
        let mask = 0b1_0001_0001; // Quotes at 0, 4, 8
        let result = prefix_xor(mask);
        // Bits 0-3 inside, 4-7 outside, 8+ inside
        assert_eq!(result & 0b1111, 0b1111);
        assert_eq!(result & 0b1111_0000, 0);
    }

    #[test]
    fn test_prefix_xor_no_quotes() {
        assert_eq!(prefix_xor(0), 0);
    }

    #[test]
    fn test_prefix_xor_high_bit() {
        // Quote at position 63
        let result = prefix_xor(1u64 << 63);
        assert_ne!(result, 0);
    }
}
