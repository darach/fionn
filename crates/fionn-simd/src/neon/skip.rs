// SPDX-License-Identifier: MIT OR Apache-2.0
//! NEON SIMD skip implementation (`JSONSki` sonic-rs style)
//!
//! Uses actual NEON intrinsics for high-throughput JSON container skipping on aarch64.
//! Based on the `JSONSki` paper: "Streaming semi-structured data with bit-parallel fast-forwarding"

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{uint8x16_t, vceqq_u8, vdupq_n_u8, vld1q_u8};

/// Result of a skip operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkipResult {
    /// Bytes consumed (offset past closing delimiter)
    pub consumed: usize,
    /// Whether escape sequences were encountered
    pub has_escapes: bool,
}

/// NEON SIMD skip using actual SIMD intrinsics
#[derive(Debug, Clone, Copy, Default)]
pub struct NeonSkip;

impl NeonSkip {
    /// Create a new NEON skipper
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Check if NEON is available (always true on aarch64)
    #[must_use]
    pub const fn is_available() -> bool {
        cfg!(target_arch = "aarch64")
    }
}

// Import Skip trait from the skip module for trait implementation
use crate::skip::Skip;

impl Skip for NeonSkip {
    fn skip_object(&self, input: &[u8]) -> Option<crate::skip::SkipResult> {
        Self::skip_container(self, input, b'{', b'}').map(|r| crate::skip::SkipResult {
            consumed: r.consumed,
            has_escapes: r.has_escapes,
        })
    }

    fn skip_array(&self, input: &[u8]) -> Option<crate::skip::SkipResult> {
        Self::skip_container(self, input, b'[', b']').map(|r| crate::skip::SkipResult {
            consumed: r.consumed,
            has_escapes: r.has_escapes,
        })
    }

    fn skip_string(&self, input: &[u8]) -> Option<crate::skip::SkipResult> {
        Self::skip_string(self, input).map(|r| crate::skip::SkipResult {
            consumed: r.consumed,
            has_escapes: r.has_escapes,
        })
    }

    fn skip_value(&self, input: &[u8]) -> Option<crate::skip::SkipResult> {
        Self::skip_value(self, input).map(|r| crate::skip::SkipResult {
            consumed: r.consumed,
            has_escapes: r.has_escapes,
        })
    }
}

/// XOR prefix computation for in-string detection
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

/// Branchless escape detection (Langdale-Lemire)
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

/// Extract bitmask from NEON comparison result (16 bytes -> 16 bits)
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn neon_movemask(cmp: uint8x16_t) -> u16 {
    unsafe {
        // Simple scalar extraction - NEON comparison sets all bits in matching bytes
        let arr: [u8; 16] = std::mem::transmute(cmp);
        let mut result: u16 = 0;
        for (i, &byte) in arr.iter().enumerate() {
            if byte != 0 {
                result |= 1 << i;
            }
        }
        result
    }
}

/// Classify 64 bytes using NEON, returning (quote_bits, backslash_bits, open_bits, close_bits)
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn classify_chunk_neon(
    chunk: &[u8; 64],
    open: u8,
    close: u8,
) -> (u64, u64, u64, u64) {
    unsafe {
        let quote_char = vdupq_n_u8(b'"');
        let bs_char = vdupq_n_u8(b'\\');
        let open_char = vdupq_n_u8(open);
        let close_char = vdupq_n_u8(close);

        let mut quote_mask: u64 = 0;
        let mut bs_mask: u64 = 0;
        let mut open_mask: u64 = 0;
        let mut close_mask: u64 = 0;

        // Process 4 x 16-byte vectors
        for i in 0..4 {
            let offset = i * 16;
            let v = vld1q_u8(chunk.as_ptr().add(offset));

            let quote_cmp = vceqq_u8(v, quote_char);
            let bs_cmp = vceqq_u8(v, bs_char);
            let open_cmp = vceqq_u8(v, open_char);
            let close_cmp = vceqq_u8(v, close_char);

            quote_mask |= u64::from(neon_movemask(quote_cmp)) << (i * 16);
            bs_mask |= u64::from(neon_movemask(bs_cmp)) << (i * 16);
            open_mask |= u64::from(neon_movemask(open_cmp)) << (i * 16);
            close_mask |= u64::from(neon_movemask(close_cmp)) << (i * 16);
        }

        (quote_mask, bs_mask, open_mask, close_mask)
    }
}

/// Scalar fallback for `classify_chunk`
#[inline]
fn classify_chunk_scalar(chunk: &[u8; 64], open: u8, close: u8) -> (u64, u64, u64, u64) {
    let mut quote_bits: u64 = 0;
    let mut bs_bits: u64 = 0;
    let mut open_bits: u64 = 0;
    let mut close_bits: u64 = 0;

    for (i, &byte) in chunk.iter().enumerate() {
        if byte == b'"' {
            quote_bits |= 1u64 << i;
        }
        if byte == b'\\' {
            bs_bits |= 1u64 << i;
        }
        if byte == open {
            open_bits |= 1u64 << i;
        }
        if byte == close {
            close_bits |= 1u64 << i;
        }
    }

    (quote_bits, bs_bits, open_bits, close_bits)
}

/// Core skip loop processing one 64-byte chunk
#[inline]
fn skip_container_loop(
    quote_bits: u64,
    bs_bits: u64,
    mut open_bits: u64,
    mut close_bits: u64,
    prev_instring: &mut u64,
    prev_escaped: &mut u64,
    lbrace_num: &mut usize,
    rbrace_num: &mut usize,
) -> Option<u8> {
    // Compute escaped positions
    let escaped = if bs_bits != 0 {
        get_escaped_branchless(prev_escaped, bs_bits)
    } else {
        let e = *prev_escaped;
        *prev_escaped = 0;
        e
    };

    // Compute in-string mask
    let unescaped_quotes = quote_bits & !escaped;
    let in_string = prefix_xor(unescaped_quotes) ^ *prev_instring;
    *prev_instring = 0u64.wrapping_sub(in_string >> 63);

    // Exclude brackets inside strings
    open_bits &= !in_string;
    close_bits &= !in_string;

    let last_lbrace_num = *lbrace_num;

    // Process each closing bracket
    while close_bits != 0 {
        *rbrace_num += 1;
        let close_pos = close_bits.trailing_zeros();
        *lbrace_num = last_lbrace_num + (open_bits & ((1u64 << close_pos) - 1)).count_ones() as usize;

        if *lbrace_num < *rbrace_num {
            #[allow(clippy::cast_possible_truncation)] // Position always < 64 (chunk size)
            return Some((close_pos + 1) as u8);
        }
        close_bits &= close_bits - 1;
    }

    *lbrace_num = last_lbrace_num + open_bits.count_ones() as usize;
    None
}

impl NeonSkip {
    /// Skip a JSON container (object or array)
    ///
    /// # Panics
    /// This function will not panic - the slice-to-array conversion is guaranteed
    /// to succeed due to the loop bounds check.
    #[must_use]
    pub fn skip_container(&self, input: &[u8], open: u8, close: u8) -> Option<SkipResult> {
        let mut prev_instring: u64 = 0;
        let mut prev_escaped: u64 = 0;
        let mut lbrace_num: usize = 0;
        let mut rbrace_num: usize = 0;
        let mut offset: usize = 0;
        let mut has_escapes = false;

        // Process 64-byte chunks
        while offset + 64 <= input.len() {
            let chunk: &[u8; 64] = input[offset..offset + 64].try_into().unwrap();

            #[cfg(target_arch = "aarch64")]
            let (quote_bits, bs_bits, open_bits, close_bits) =
                unsafe { classify_chunk_neon(chunk, open, close) };

            #[cfg(not(target_arch = "aarch64"))]
            let (quote_bits, bs_bits, open_bits, close_bits) =
                classify_chunk_scalar(chunk, open, close);

            has_escapes = has_escapes || bs_bits != 0;

            if let Some(pos) = skip_container_loop(
                quote_bits,
                bs_bits,
                open_bits,
                close_bits,
                &mut prev_instring,
                &mut prev_escaped,
                &mut lbrace_num,
                &mut rbrace_num,
            ) {
                return Some(SkipResult {
                    consumed: offset + pos as usize,
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

            let (quote_bits, bs_bits, open_bits, close_bits) =
                classify_chunk_scalar(&remain, open, close);
            has_escapes = has_escapes || bs_bits != 0;

            if let Some(pos) = skip_container_loop(
                quote_bits,
                bs_bits,
                open_bits,
                close_bits,
                &mut prev_instring,
                &mut prev_escaped,
                &mut lbrace_num,
                &mut rbrace_num,
            ) {
                let pos = pos as usize;
                if pos <= n {
                    return Some(SkipResult {
                        consumed: offset + pos,
                        has_escapes,
                    });
                }
            }
        }

        None
    }

    /// Skip a JSON object starting after the opening `{`
    #[must_use]
    pub fn skip_object(&self, input: &[u8]) -> Option<SkipResult> {
        self.skip_container(input, b'{', b'}')
    }

    /// Skip a JSON array starting after the opening `[`
    #[must_use]
    pub fn skip_array(&self, input: &[u8]) -> Option<SkipResult> {
        self.skip_container(input, b'[', b']')
    }

    /// Skip a JSON string starting after the opening `"`
    ///
    /// # Panics
    /// This function will not panic - the slice-to-array conversion is guaranteed
    /// to succeed due to the loop bounds check.
    #[must_use]
    pub fn skip_string(&self, input: &[u8]) -> Option<SkipResult> {
        let mut prev_escaped: u64 = 0;
        let mut offset: usize = 0;
        let mut has_escapes = false;

        while offset + 64 <= input.len() {
            let chunk: &[u8; 64] = input[offset..offset + 64].try_into().unwrap();

            #[cfg(target_arch = "aarch64")]
            let (quote_bits, bs_bits) = unsafe {
                let (q, b, _, _) = classify_chunk_neon(chunk, b'{', b'}');
                (q, b)
            };

            #[cfg(not(target_arch = "aarch64"))]
            let (quote_bits, bs_bits) = {
                let (q, b, _, _) = classify_chunk_scalar(chunk, b'{', b'}');
                (q, b)
            };

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

            let (quote_bits, bs_bits, _, _) = classify_chunk_scalar(&remain, b'{', b'}');

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

    /// Skip any JSON value (auto-detects type from first non-whitespace byte)
    #[must_use]
    pub fn skip_value(&self, input: &[u8]) -> Option<SkipResult> {
        let start = input
            .iter()
            .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))?;
        let first = input[start];

        let result = match first {
            b'{' => self.skip_object(&input[start + 1..]),
            b'[' => self.skip_array(&input[start + 1..]),
            b'"' => self.skip_string(&input[start + 1..]),
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
    fn test_skip_simple_object() {
        let skip = NeonSkip::new();
        let input = br#""name": "test"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_nested_object() {
        let skip = NeonSkip::new();
        let input = br#""a": {"b": {"c": 1}}}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_large_object() {
        use std::fmt::Write;
        let skip = NeonSkip::new();
        let mut json = String::new();
        for i in 0..100 {
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
    fn test_neon_available() {
        // Just verify the function doesn't panic
        let _ = NeonSkip::is_available();
    }
}
