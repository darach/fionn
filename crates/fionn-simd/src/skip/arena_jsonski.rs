// SPDX-License-Identifier: MIT OR Apache-2.0
//! Arena-based `JSONSki` skip implementation
//!
//! Uses bumpalo arena allocator for temporary buffers, reducing heap allocations
//! during parsing. This is the key optimization from sonic-rs.

use bumpalo::Bump;
use std::num::NonZeroU8;

use super::{Skip, SkipResult};

/// Arena-based `JSONSki` skip using bumpalo
///
/// The arena is used for:
/// - Temporary 64-byte aligned chunk buffers
/// - Remainder padding buffers
/// - Any intermediate state that would otherwise heap-allocate
pub struct ArenaJsonSkiSkip<'a> {
    arena: &'a Bump,
}

impl<'a> ArenaJsonSkiSkip<'a> {
    /// Create a new arena-based `JSONSki` skipper
    #[must_use]
    pub const fn new(arena: &'a Bump) -> Self {
        Self { arena }
    }

    /// Get the arena for external use
    #[must_use]
    pub const fn arena(&self) -> &'a Bump {
        self.arena
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
    *prev_instring = 0u64.wrapping_sub(in_string >> 63);

    in_string
}

/// Core `JSONSki` skip loop - processes one 64-byte chunk
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

    lbrace &= !instring;
    rbrace &= !instring;

    let last_lbrace_num = *lbrace_num;

    while rbrace != 0 {
        *rbrace_num += 1;
        *lbrace_num = last_lbrace_num + (lbrace & (rbrace - 1)).count_ones() as usize;

        if *lbrace_num < *rbrace_num {
            debug_assert_eq!(*rbrace_num, *lbrace_num + 1);
            let cnt = rbrace.trailing_zeros() + 1;
            #[allow(clippy::cast_possible_truncation)] // cnt always <= 64 (chunk size)
            return NonZeroU8::new(cnt as u8);
        }

        rbrace &= rbrace - 1;
    }

    *lbrace_num = last_lbrace_num + lbrace.count_ones() as usize;

    None
}

impl ArenaJsonSkiSkip<'_> {
    fn skip_container(&self, input: &[u8], left: u8, right: u8) -> Option<SkipResult> {
        let mut prev_instring: u64 = 0;
        let mut prev_escaped: u64 = 0;
        let mut lbrace_num: usize = 0;
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

        // Handle remainder using arena-allocated buffer
        if offset < input.len() {
            // Allocate remainder buffer from arena instead of stack
            let remain = self.arena.alloc_slice_fill_copy(64, 0u8);
            let remain: &mut [u8; 64] = remain.try_into().unwrap();
            let n = input.len() - offset;
            remain[..n].copy_from_slice(&input[offset..]);

            if let Some(count) = skip_container_loop(
                remain,
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

    fn skip_string_impl(&self, input: &[u8]) -> Option<SkipResult> {
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

        // Handle remainder using arena
        if offset < input.len() {
            let remain = self.arena.alloc_slice_fill_copy(64, 0u8);
            let remain: &mut [u8; 64] = remain.try_into().unwrap();
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

impl Skip for ArenaJsonSkiSkip<'_> {
    fn skip_object(&self, input: &[u8]) -> Option<SkipResult> {
        self.skip_container(input, b'{', b'}')
    }

    fn skip_array(&self, input: &[u8]) -> Option<SkipResult> {
        self.skip_container(input, b'[', b']')
    }

    fn skip_string(&self, input: &[u8]) -> Option<SkipResult> {
        self.skip_string_impl(input)
    }

    fn skip_value(&self, input: &[u8]) -> Option<SkipResult> {
        let start = input
            .iter()
            .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))?;
        let first = input[start];

        let result = match first {
            b'{' => self.skip_container(&input[start + 1..], b'{', b'}'),
            b'[' => self.skip_container(&input[start + 1..], b'[', b']'),
            b'"' => self.skip_string_impl(&input[start + 1..]),
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
    // Basic Construction
    // =========================================================================

    #[test]
    fn test_arena_new() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let _ = skip.arena(); // Just verify it compiles
    }

    #[test]
    fn test_arena_accessor() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let a = skip.arena();
        // Allocate something to prove arena is usable
        let _buf: &mut [u8] = a.alloc_slice_fill_copy(16, 0u8);
    }

    // =========================================================================
    // skip_object tests
    // =========================================================================

    #[test]
    fn test_arena_skip_object() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#""name": "test"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_arena_skip_nested() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#""a": {"b": {"c": 1}}}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_arena_skip_object_empty() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"}";
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_arena_skip_object_unclosed() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#""key": "value""#;
        let result = skip.skip_object(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // skip_array tests
    // =========================================================================

    #[test]
    fn test_arena_skip_array_simple() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"1, 2, 3]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_arena_skip_array_nested() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"[[1, 2], [3, 4]]]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_arena_skip_array_empty() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    // =========================================================================
    // skip_string tests
    // =========================================================================

    #[test]
    fn test_arena_skip_string_simple() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#"hello world""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 12);
        assert!(!result.unwrap().has_escapes);
    }

    #[test]
    fn test_arena_skip_string_with_escapes() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#"hello \"world\"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_arena_skip_string_empty() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_arena_skip_string_unclosed() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"hello world";
        let result = skip.skip_string(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // skip_value tests
    // =========================================================================

    #[test]
    fn test_arena_skip_value_object() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#"{"key": "value"}"#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
        // skip_value doesn't include opening char in consumed for containers
        assert_eq!(result.unwrap().consumed, input.len() - 1);
    }

    #[test]
    fn test_arena_skip_value_array() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"[1, 2, 3]";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        // skip_value doesn't include opening char in consumed for containers
        assert_eq!(result.unwrap().consumed, input.len() - 1);
    }

    #[test]
    fn test_arena_skip_value_string() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#""hello world""#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
        // skip_value doesn't include opening quote in consumed for strings
        assert_eq!(result.unwrap().consumed, input.len() - 1);
    }

    #[test]
    fn test_arena_skip_value_true() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"true";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_arena_skip_value_false() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"false";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_arena_skip_value_null() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"null";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_arena_skip_value_number_int() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"12345";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_arena_skip_value_number_negative() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"-123";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_arena_skip_value_number_float() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"123.456";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 7);
    }

    #[test]
    fn test_arena_skip_value_number_exp() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"1.5e10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
    }

    #[test]
    fn test_arena_skip_value_with_whitespace() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"  \t\n  42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 8); // whitespace + number
    }

    #[test]
    fn test_arena_skip_value_invalid() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"invalid";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_arena_skip_value_empty() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_arena_skip_value_whitespace_only() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"   \t\n\r   ";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_arena_skip_value_true_incomplete() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"tru";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_arena_skip_value_false_incomplete() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"fals";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_arena_skip_value_null_incomplete() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = b"nul";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // Arena reuse and large input tests
    // =========================================================================

    #[test]
    fn test_arena_reuse() {
        let mut arena = Bump::new();

        // First document
        {
            let skip = ArenaJsonSkiSkip::new(&arena);
            let input1 = br#""x": 1}"#;
            let r1 = skip.skip_object(input1);
            assert!(r1.is_some());
        }

        // Reset arena between documents
        arena.reset();

        // Second document - reuses arena memory
        {
            let skip = ArenaJsonSkiSkip::new(&arena);
            let input2 = br#""y": 2}"#;
            let r2 = skip.skip_object(input2);
            assert!(r2.is_some());
        }
    }

    #[test]
    fn test_arena_large_remainder() {
        use std::fmt::Write;
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);

        // Create input that has a remainder (not multiple of 64)
        let mut json = String::from(r#""fields": ["#);
        for i in 0..20 {
            if i > 0 {
                json.push_str(", ");
            }
            let _ = write!(json, "{i}");
        }
        json.push_str("]}");

        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, json.len());
    }

    #[test]
    fn test_arena_multiple_chunks() {
        use std::fmt::Write;
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);

        // Create input >64 bytes
        let mut json = String::from(r#""fields": ["#);
        for i in 0..50 {
            if i > 0 {
                json.push_str(", ");
            }
            let _ = write!(json, "{i}");
        }
        json.push_str("]}");

        assert!(json.len() > 64);
        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, json.len());
    }

    #[test]
    fn test_arena_string_multiple_chunks() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);

        // Create string >64 bytes
        let long_string = "a".repeat(100) + "\"";
        let result = skip.skip_string(long_string.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 101);
    }

    // =========================================================================
    // Escape handling
    // =========================================================================

    #[test]
    fn test_arena_escaped_quote_in_string() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#"hello \" world""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_arena_escaped_backslash_in_string() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        let input = br#"hello \\ world""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_arena_braces_in_string() {
        let arena = Bump::new();
        let skip = ArenaJsonSkiSkip::new(&arena);
        // Object with string containing braces - should not confuse the parser
        let input = br#""key": "{ not a real brace }"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    // =========================================================================
    // Helper function tests
    // =========================================================================

    #[test]
    fn test_prefix_xor() {
        assert_eq!(prefix_xor(0), 0);
        assert_eq!(prefix_xor(1), u64::MAX);
        assert_eq!(prefix_xor(2), u64::MAX - 1);
    }

    #[test]
    fn test_get_escaped_branchless_no_backslash() {
        let mut prev = 0u64;
        let result = get_escaped_branchless(&mut prev, 0);
        assert_eq!(result, 0);
        assert_eq!(prev, 0);
    }

    #[test]
    fn test_get_escaped_branchless_single_backslash() {
        let mut prev = 0u64;
        // Backslash at position 0
        let result = get_escaped_branchless(&mut prev, 1);
        // Position 1 should be escaped
        assert_ne!(result & 2, 0);
    }

    #[test]
    fn test_get_string_bits_no_quotes() {
        let chunk: [u8; 64] = [b'a'; 64];
        let mut prev_in = 0u64;
        let mut prev_esc = 0u64;
        let result = get_string_bits(&chunk, &mut prev_in, &mut prev_esc);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_get_string_bits_single_quote() {
        let mut chunk: [u8; 64] = [b'a'; 64];
        chunk[0] = b'"';
        let mut prev_in = 0u64;
        let mut prev_esc = 0u64;
        let result = get_string_bits(&chunk, &mut prev_in, &mut prev_esc);
        // After quote at 0, all bits 1-63 should be in string
        assert_ne!(result, 0);
    }
}
