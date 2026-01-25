// SPDX-License-Identifier: MIT OR Apache-2.0
//! AVX2 SIMD skip implementation (`JSONSki` sonic-rs style)
//!
//! Uses actual AVX2 intrinsics for high-throughput JSON container skipping.
//! Based on the `JSONSki` paper: "Streaming semi-structured data with bit-parallel fast-forwarding"
//!
//! # Runtime Feature Detection
//! This module uses `#[target_feature(enable = "avx2")]` to compile AVX2 code
//! even without `-C target-feature=+avx2`. The caller must check `is_available()`
//! before calling AVX2-accelerated functions.

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::arch::x86_64::{
    __m128i, __m256i, _MM_HINT_T0, _mm_clmulepi64_si128, _mm_cvtsi64_si128, _mm_cvtsi128_si64,
    _mm_prefetch, _mm_set1_epi8, _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8,
    _mm256_set1_epi8,
};

// AVX-512 intrinsics for 64-byte single-load classification
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::arch::x86_64::{__m512i, _mm512_cmpeq_epi8_mask, _mm512_loadu_si512, _mm512_set1_epi8};

// BMI2 intrinsics for faster bracket counting
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::arch::x86_64::_bzhi_u64;

/// Result of a skip operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkipResult {
    /// Bytes consumed (offset past closing delimiter)
    pub consumed: usize,
    /// Whether escape sequences were encountered
    pub has_escapes: bool,
}

/// AVX2 SIMD skip using actual SIMD intrinsics
#[derive(Debug, Clone, Copy, Default)]
pub struct Avx2Skip;

impl Avx2Skip {
    /// Create a new AVX2 skipper
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Check if AVX2 is available at runtime
    #[must_use]
    pub fn is_available() -> bool {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            std::arch::is_x86_feature_detected!("avx2")
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        {
            false
        }
    }
}

// Import Skip trait from the skip module for trait implementation
use crate::skip::Skip;

impl Skip for Avx2Skip {
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

/// Helper to convert u8 to i8 for SIMD constants
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const fn to_i8(b: u8) -> i8 {
    i8::from_ne_bytes([b])
}

/// Convert i32 movemask result to u32
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const fn mask_to_u32(mask: i32) -> u32 {
    u32::from_ne_bytes(mask.to_ne_bytes())
}

/// XOR prefix computation for in-string detection (scalar fallback)
#[inline]
const fn prefix_xor_scalar(bitmask: u64) -> u64 {
    let mut m = bitmask;
    m ^= m << 1;
    m ^= m << 2;
    m ^= m << 4;
    m ^= m << 8;
    m ^= m << 16;
    m ^= m << 32;
    m
}

/// XOR prefix computation using PCLMUL instruction
/// clmul(x, -1) computes the prefix XOR in a single operation
///
/// # Safety
/// Caller must ensure PCLMULQDQ is available
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "pclmulqdq")]
#[inline]
unsafe fn prefix_xor_clmul(bitmask: u64) -> u64 {
    // Load bitmask into XMM register
    let input: __m128i = _mm_cvtsi64_si128(bitmask.cast_signed());
    // Multiply by all-ones (-1) to compute prefix XOR
    let all_ones: __m128i = _mm_set1_epi8(-1);
    let result: __m128i = _mm_clmulepi64_si128(input, all_ones, 0);
    // Extract result
    _mm_cvtsi128_si64(result).cast_unsigned()
}

/// XOR prefix computation for in-string detection
/// Uses PCLMUL when available, falls back to scalar otherwise
#[inline]
fn prefix_xor(bitmask: u64) -> u64 {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        // is_x86_feature_detected! uses internal caching (AtomicU32)
        // so this check is very fast after first call
        if std::arch::is_x86_feature_detected!("pclmulqdq") {
            // SAFETY: We just verified PCLMULQDQ is available
            return unsafe { prefix_xor_clmul(bitmask) };
        }
    }
    prefix_xor_scalar(bitmask)
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

/// Classify 64 bytes using AVX2, returning (`quote_bits`, `backslash_bits`, `open_bits`, `close_bits`)
///
/// # Safety
/// Caller must ensure AVX2 is available (check with `is_x86_feature_detected!("avx2")`)
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
#[inline]
#[allow(clippy::cast_ptr_alignment)] // Intentional unaligned load via _mm256_loadu_si256
unsafe fn classify_chunk_avx2(chunk: &[u8; 64], open: u8, close: u8) -> (u64, u64, u64, u64) {
    unsafe {
        // Direct unaligned loads - more efficient than split+transmute
        let ptr = chunk.as_ptr().cast::<__m256i>();
        let v0: __m256i = _mm256_loadu_si256(ptr);
        let v1: __m256i = _mm256_loadu_si256(ptr.add(1));

        // Broadcast comparison targets (these are typically hoisted by the compiler)
        let quote = _mm256_set1_epi8(to_i8(b'"'));
        let backslash = _mm256_set1_epi8(to_i8(b'\\'));
        let open_char = _mm256_set1_epi8(to_i8(open));
        let close_char = _mm256_set1_epi8(to_i8(close));

        // Quote detection
        let quote0 = _mm256_cmpeq_epi8(v0, quote);
        let quote1 = _mm256_cmpeq_epi8(v1, quote);
        let quote_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(quote1))) << 32)
            | u64::from(mask_to_u32(_mm256_movemask_epi8(quote0)));

        // Backslash detection
        let bs0 = _mm256_cmpeq_epi8(v0, backslash);
        let bs1 = _mm256_cmpeq_epi8(v1, backslash);
        let bs_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(bs1))) << 32)
            | u64::from(mask_to_u32(_mm256_movemask_epi8(bs0)));

        // Open bracket detection
        let open0 = _mm256_cmpeq_epi8(v0, open_char);
        let open1 = _mm256_cmpeq_epi8(v1, open_char);
        let open_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(open1))) << 32)
            | u64::from(mask_to_u32(_mm256_movemask_epi8(open0)));

        // Close bracket detection
        let close0 = _mm256_cmpeq_epi8(v0, close_char);
        let close1 = _mm256_cmpeq_epi8(v1, close_char);
        let close_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(close1))) << 32)
            | u64::from(mask_to_u32(_mm256_movemask_epi8(close0)));

        (quote_mask, bs_mask, open_mask, close_mask)
    }
}

/// Classify 64 bytes using AVX-512, returning (`quote_bits`, `backslash_bits`, `open_bits`, `close_bits`)
///
/// AVX-512 processes the entire 64-byte chunk in a single load, providing ~2x throughput
/// over AVX2 which requires two 32-byte loads.
///
/// # Safety
/// Caller must ensure AVX-512BW is available (check with `is_x86_feature_detected!("avx512bw")`)
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx512bw")]
#[inline]
#[allow(clippy::cast_ptr_alignment)] // Intentional unaligned load via _mm512_loadu_si512
unsafe fn classify_chunk_avx512(chunk: &[u8; 64], open: u8, close: u8) -> (u64, u64, u64, u64) {
    unsafe {
        // Single 64-byte load - the key advantage of AVX-512
        let v: __m512i = _mm512_loadu_si512(chunk.as_ptr().cast::<__m512i>());

        // Broadcast comparison targets
        let quote = _mm512_set1_epi8(to_i8(b'"'));
        let backslash = _mm512_set1_epi8(to_i8(b'\\'));
        let open_char = _mm512_set1_epi8(to_i8(open));
        let close_char = _mm512_set1_epi8(to_i8(close));

        // AVX-512 compare returns a 64-bit mask directly - no movemask needed!
        let quote_mask = _mm512_cmpeq_epi8_mask(v, quote);
        let bs_mask = _mm512_cmpeq_epi8_mask(v, backslash);
        let open_mask = _mm512_cmpeq_epi8_mask(v, open_char);
        let close_mask = _mm512_cmpeq_epi8_mask(v, close_char);

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

/// Classify a partial chunk (remainder < 64 bytes) without stack allocation
/// This avoids the memset + copy overhead of the padded approach
#[inline]
fn classify_partial_scalar(data: &[u8], open: u8, close: u8) -> (u64, u64, u64, u64) {
    let mut quote_bits: u64 = 0;
    let mut bs_bits: u64 = 0;
    let mut open_bits: u64 = 0;
    let mut close_bits: u64 = 0;

    for (i, &byte) in data.iter().enumerate() {
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
///
/// Uses branchless escape detection and prefix-xor for string mask computation.
/// Early exits as soon as closing delimiter is found.
/// Uses BMI2 bzhi instruction when available for faster mask operations.
#[inline]
#[allow(clippy::too_many_arguments)] // SIMD state machine requires all parameters
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
    // Compute escaped positions - branchless for common case (no escapes)
    let escaped = if bs_bits != 0 {
        get_escaped_branchless(prev_escaped, bs_bits)
    } else {
        let e = *prev_escaped;
        *prev_escaped = 0;
        e
    };

    // Compute in-string mask using prefix-xor
    let unescaped_quotes = quote_bits & !escaped;
    let in_string = prefix_xor(unescaped_quotes) ^ *prev_instring;
    *prev_instring = 0u64.wrapping_sub(in_string >> 63);

    // Exclude brackets inside strings
    open_bits &= !in_string;
    close_bits &= !in_string;

    let last_lbrace_num = *lbrace_num;

    // Check BMI2 availability for optimized mask operations
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let use_bmi2 = std::arch::is_x86_feature_detected!("bmi2");
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    let use_bmi2 = false;

    // Process each closing bracket - early exit when match found
    while close_bits != 0 {
        *rbrace_num += 1;
        let close_pos = close_bits.trailing_zeros();

        // Count opens before this close position
        // BMI2 bzhi: single instruction vs shift+subtract+and
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let opens_before = if use_bmi2 {
            // SAFETY: We checked BMI2 is available
            unsafe { _bzhi_u64(open_bits, close_pos) }.count_ones() as usize
        } else {
            (open_bits & ((1u64 << close_pos) - 1)).count_ones() as usize
        };

        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let opens_before = (open_bits & ((1u64 << close_pos) - 1)).count_ones() as usize;

        *lbrace_num = last_lbrace_num + opens_before;

        if *lbrace_num < *rbrace_num {
            #[allow(clippy::cast_possible_truncation)] // Position always < 64 (chunk size)
            return Some((close_pos + 1) as u8);
        }
        close_bits &= close_bits - 1;
    }

    *lbrace_num = last_lbrace_num + open_bits.count_ones() as usize;
    None
}

impl Avx2Skip {
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

        // Check SIMD availability once at the start for runtime dispatch
        // Prefer AVX-512 > AVX2 > scalar
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let use_avx512 = std::arch::is_x86_feature_detected!("avx512bw");
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let use_avx2 = !use_avx512 && std::arch::is_x86_feature_detected!("avx2");
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let use_avx512 = false;
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let use_avx2 = false;

        // Process 64-byte chunks
        while offset + 64 <= input.len() {
            // Prefetch next chunk (2 cache lines ahead = 128 bytes)
            // This helps hide memory latency for large inputs
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            if (use_avx512 || use_avx2) && offset + 192 <= input.len() {
                unsafe {
                    _mm_prefetch(input.as_ptr().add(offset + 128).cast::<i8>(), _MM_HINT_T0);
                }
            }

            let chunk: &[u8; 64] = input[offset..offset + 64].try_into().unwrap();

            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            let (quote_bits, bs_bits, open_bits, close_bits) = if use_avx512 {
                // SAFETY: We checked AVX-512BW is available via is_x86_feature_detected
                unsafe { classify_chunk_avx512(chunk, open, close) }
            } else if use_avx2 {
                // SAFETY: We checked AVX2 is available via is_x86_feature_detected
                unsafe { classify_chunk_avx2(chunk, open, close) }
            } else {
                classify_chunk_scalar(chunk, open, close)
            };

            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
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

        // Handle remainder - use partial classifier to avoid stack allocation + memcpy
        if offset < input.len() {
            let remainder = &input[offset..];
            let n = remainder.len();

            let (quote_bits, bs_bits, open_bits, close_bits) =
                classify_partial_scalar(remainder, open, close);
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

        // Check SIMD availability once at the start for runtime dispatch
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let use_avx512 = std::arch::is_x86_feature_detected!("avx512bw");
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let use_avx2 = !use_avx512 && std::arch::is_x86_feature_detected!("avx2");
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let use_avx512 = false;
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let use_avx2 = false;

        while offset + 64 <= input.len() {
            // Prefetch next chunk (2 cache lines ahead = 128 bytes)
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            if (use_avx512 || use_avx2) && offset + 192 <= input.len() {
                unsafe {
                    _mm_prefetch(input.as_ptr().add(offset + 128).cast::<i8>(), _MM_HINT_T0);
                }
            }

            let chunk: &[u8; 64] = input[offset..offset + 64].try_into().unwrap();

            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            let (quote_bits, bs_bits) = if use_avx512 {
                // SAFETY: We checked AVX-512BW is available via is_x86_feature_detected
                unsafe {
                    let (q, b, _, _) = classify_chunk_avx512(chunk, b'{', b'}');
                    (q, b)
                }
            } else if use_avx2 {
                // SAFETY: We checked AVX2 is available via is_x86_feature_detected
                unsafe {
                    let (q, b, _, _) = classify_chunk_avx2(chunk, b'{', b'}');
                    (q, b)
                }
            } else {
                let (q, b, _, _) = classify_chunk_scalar(chunk, b'{', b'}');
                (q, b)
            };

            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
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

        // Handle remainder - use partial classifier to avoid stack allocation + memcpy
        if offset < input.len() {
            let remainder = &input[offset..];
            let n = remainder.len();

            let (quote_bits, bs_bits, _, _) = classify_partial_scalar(remainder, b'{', b'}');

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

    // =========================================================================
    // Basic Construction
    // =========================================================================

    #[test]
    fn test_avx2_skip_new() {
        let skip = Avx2Skip::new();
        let _ = skip; // Just verify construction
    }

    #[test]
    fn test_avx2_skip_default() {
        let skip = Avx2Skip;
        let _ = skip;
    }

    #[test]
    fn test_avx2_skip_debug() {
        let skip = Avx2Skip::new();
        let debug_str = format!("{skip:?}");
        assert!(debug_str.contains("Avx2Skip"));
    }

    #[test]
    fn test_avx2_skip_clone() {
        let skip = Avx2Skip::new();
        let cloned = skip;
        let _ = cloned;
    }

    #[test]
    fn test_avx2_available() {
        // Just verify the function doesn't panic
        let _ = Avx2Skip::is_available();
    }

    // =========================================================================
    // SkipResult Tests
    // =========================================================================

    #[test]
    fn test_skip_result_debug() {
        let result = SkipResult {
            consumed: 10,
            has_escapes: true,
        };
        let debug_str = format!("{result:?}");
        assert!(debug_str.contains("10"));
        assert!(debug_str.contains("true"));
    }

    #[test]
    fn test_skip_result_clone() {
        let result = SkipResult {
            consumed: 5,
            has_escapes: false,
        };
        let cloned = result;
        assert_eq!(result.consumed, cloned.consumed);
        assert_eq!(result.has_escapes, cloned.has_escapes);
    }

    #[test]
    fn test_skip_result_equality() {
        let a = SkipResult {
            consumed: 5,
            has_escapes: false,
        };
        let b = SkipResult {
            consumed: 5,
            has_escapes: false,
        };
        let c = SkipResult {
            consumed: 5,
            has_escapes: true,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // =========================================================================
    // skip_object Tests
    // =========================================================================

    #[test]
    fn test_skip_simple_object() {
        let skip = Avx2Skip::new();
        let input = br#""name": "test"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_nested_object() {
        let skip = Avx2Skip::new();
        let input = br#""a": {"b": {"c": 1}}}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_object_empty() {
        let skip = Avx2Skip::new();
        let input = b"}";
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_object_unclosed() {
        let skip = Avx2Skip::new();
        let input = br#""key": "value""#;
        let result = skip.skip_object(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_large_object() {
        use std::fmt::Write;
        let skip = Avx2Skip::new();
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

    // =========================================================================
    // skip_array Tests
    // =========================================================================

    #[test]
    fn test_skip_array_simple() {
        let skip = Avx2Skip::new();
        let input = b"1, 2, 3]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_array_nested() {
        let skip = Avx2Skip::new();
        let input = b"[[1, 2], [3, 4]]]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_skip_array_empty() {
        let skip = Avx2Skip::new();
        let input = b"]";
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_array_with_strings() {
        let skip = Avx2Skip::new();
        let input = br#""a", "b", "c"]"#;
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    // =========================================================================
    // skip_string Tests
    // =========================================================================

    #[test]
    fn test_skip_string_simple() {
        let skip = Avx2Skip::new();
        let input = br#"hello world""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 12);
        assert!(!result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_with_escapes() {
        let skip = Avx2Skip::new();
        let input = br#"hello \"world\"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_string_empty() {
        let skip = Avx2Skip::new();
        let input = br#"""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_string_unclosed() {
        let skip = Avx2Skip::new();
        let input = b"hello world";
        let result = skip.skip_string(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_string_long() {
        let skip = Avx2Skip::new();
        // >64 bytes to test chunked processing
        let long_string = "a".repeat(100) + "\"";
        let result = skip.skip_string(long_string.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 101);
    }

    // =========================================================================
    // skip_value Tests
    // =========================================================================

    #[test]
    fn test_skip_value_object() {
        let skip = Avx2Skip::new();
        let input = br#"{"key": "value"}"#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_array() {
        let skip = Avx2Skip::new();
        let input = b"[1, 2, 3]";
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_string() {
        let skip = Avx2Skip::new();
        let input = br#""hello world""#;
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_true() {
        let skip = Avx2Skip::new();
        let input = b"true";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_false() {
        let skip = Avx2Skip::new();
        let input = b"false";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_null() {
        let skip = Avx2Skip::new();
        let input = b"null";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_number_int() {
        let skip = Avx2Skip::new();
        let input = b"12345";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_number_negative() {
        let skip = Avx2Skip::new();
        let input = b"-123";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_number_float() {
        let skip = Avx2Skip::new();
        let input = b"123.456";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 7);
    }

    #[test]
    fn test_skip_value_number_exp() {
        let skip = Avx2Skip::new();
        let input = b"1.5e10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 6);
    }

    #[test]
    fn test_skip_value_with_whitespace() {
        let skip = Avx2Skip::new();
        let input = b"  \t\n  42";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 8);
    }

    #[test]
    fn test_skip_value_invalid() {
        let skip = Avx2Skip::new();
        let input = b"invalid";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_empty() {
        let skip = Avx2Skip::new();
        let input = b"";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_whitespace_only() {
        let skip = Avx2Skip::new();
        let input = b"   \t\n\r   ";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_true_incomplete() {
        let skip = Avx2Skip::new();
        let input = b"tru";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_false_incomplete() {
        let skip = Avx2Skip::new();
        let input = b"fals";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_null_incomplete() {
        let skip = Avx2Skip::new();
        let input = b"nul";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    // =========================================================================
    // Skip Trait Tests
    // =========================================================================

    #[test]
    fn test_skip_trait_object() {
        let skip = Avx2Skip::new();
        let input = br#""key": 1}"#;
        let result = <Avx2Skip as Skip>::skip_object(&skip, input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_trait_array() {
        let skip = Avx2Skip::new();
        let input = b"1, 2]";
        let result = <Avx2Skip as Skip>::skip_array(&skip, input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_trait_string() {
        let skip = Avx2Skip::new();
        let input = br#"test""#;
        let result = <Avx2Skip as Skip>::skip_string(&skip, input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_trait_value() {
        let skip = Avx2Skip::new();
        let input = b"42";
        let result = <Avx2Skip as Skip>::skip_value(&skip, input);
        assert!(result.is_some());
    }

    // =========================================================================
    // Helper Function Tests
    // =========================================================================

    #[test]
    fn test_prefix_xor_scalar_zero() {
        assert_eq!(prefix_xor_scalar(0), 0);
    }

    #[test]
    fn test_prefix_xor_scalar_one() {
        assert_eq!(prefix_xor_scalar(1), u64::MAX);
    }

    #[test]
    fn test_prefix_xor_scalar_two() {
        // XOR prefix of 0b10 is 0b11...1110
        assert_eq!(prefix_xor_scalar(2), u64::MAX - 1);
    }

    #[test]
    fn test_prefix_xor_uses_clmul_or_scalar() {
        // Just verify it works (may use CLMUL if available)
        assert_eq!(prefix_xor(0), 0);
        assert_eq!(prefix_xor(1), u64::MAX);
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
    fn test_classify_chunk_scalar_empty() {
        let chunk: [u8; 64] = [b' '; 64];
        let (q, b, o, c) = classify_chunk_scalar(&chunk, b'{', b'}');
        assert_eq!(q, 0);
        assert_eq!(b, 0);
        assert_eq!(o, 0);
        assert_eq!(c, 0);
    }

    #[test]
    fn test_classify_chunk_scalar_quotes() {
        let mut chunk: [u8; 64] = [b' '; 64];
        chunk[0] = b'"';
        chunk[10] = b'"';
        let (q, _, _, _) = classify_chunk_scalar(&chunk, b'{', b'}');
        assert_ne!(q & 1, 0);
        assert_ne!(q & (1 << 10), 0);
    }

    #[test]
    fn test_classify_chunk_scalar_braces() {
        let mut chunk: [u8; 64] = [b' '; 64];
        chunk[0] = b'{';
        chunk[63] = b'}';
        let (_, _, o, c) = classify_chunk_scalar(&chunk, b'{', b'}');
        assert_ne!(o & 1, 0);
        assert_ne!(c & (1 << 63), 0);
    }

    #[test]
    fn test_classify_partial_scalar() {
        let data = b"{}[]\"\\";
        let (q, b, o, c) = classify_partial_scalar(data, b'{', b'}');
        assert_ne!(o & 1, 0); // { at 0
        assert_ne!(c & 2, 0); // } at 1
        assert_ne!(q & (1 << 4), 0); // " at 4
        assert_ne!(b & (1 << 5), 0); // \ at 5
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn test_to_i8() {
        assert_eq!(to_i8(0), 0i8);
        assert_eq!(to_i8(127), 127i8);
        assert_eq!(to_i8(128), -128i8);
        assert_eq!(to_i8(255), -1i8);
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn test_mask_to_u32() {
        assert_eq!(mask_to_u32(0), 0u32);
        assert_eq!(mask_to_u32(-1), u32::MAX);
    }

    // =========================================================================
    // Large Input / Multiple Chunk Tests
    // =========================================================================

    #[test]
    fn test_skip_object_multiple_chunks() {
        use std::fmt::Write;
        let skip = Avx2Skip::new();
        // Create object >128 bytes (2+ chunks)
        let mut json = String::new();
        for i in 0..50 {
            if i > 0 {
                json.push_str(", ");
            }
            let _ = write!(json, "\"field{i}\": {i}");
        }
        json.push('}');

        assert!(json.len() > 128);
        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, json.len());
    }

    #[test]
    fn test_skip_string_multiple_chunks() {
        let skip = Avx2Skip::new();
        // Create string >128 bytes
        let long_string = "a".repeat(200) + "\"";
        assert!(long_string.len() > 128);
        let result = skip.skip_string(long_string.as_bytes());
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 201);
    }

    // =========================================================================
    // Escape Handling Tests
    // =========================================================================

    #[test]
    fn test_escaped_quote_in_string() {
        let skip = Avx2Skip::new();
        let input = br#"hello \" world""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_escaped_backslash_in_string() {
        let skip = Avx2Skip::new();
        let input = br#"hello \\ world""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_braces_in_string() {
        let skip = Avx2Skip::new();
        // Object with string containing braces
        let input = br#""key": "{ not a real brace }"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    // =========================================================================
    // Remainder Handling Tests
    // =========================================================================

    #[test]
    fn test_skip_object_remainder() {
        let skip = Avx2Skip::new();
        // Create input with remainder (not multiple of 64)
        let input = "\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\": 1}";
        let result = skip.skip_object(input.as_bytes());
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_string_remainder() {
        let skip = Avx2Skip::new();
        // 70 bytes (64 + 6 remainder)
        let long_string = "a".repeat(68) + "\"";
        assert!(long_string.len() > 64 && long_string.len() < 128);
        let result = skip.skip_string(long_string.as_bytes());
        assert!(result.is_some());
    }

    // =========================================================================
    // Additional Coverage Tests
    // =========================================================================

    #[test]
    fn test_skip_container_loop_basic() {
        // Test the skip_container_loop function directly through skip_object
        let skip = Avx2Skip::new();
        // Input with multiple opening and closing braces
        let input = br#""a": {"b": 1}, "c": {"d": 2}}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_container_loop_bracket_counting() {
        let skip = Avx2Skip::new();
        // Many nested objects to test bracket counting
        let input = b"{{{{}}}}";
        let result = skip.skip_object(&input[1..]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 7);
    }

    #[test]
    fn test_skip_container_early_exit() {
        let skip = Avx2Skip::new();
        // Input where closing brace comes early
        let input = b"}extra data after";
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_large_input_with_prefetch() {
        let skip = Avx2Skip::new();
        // Create input > 192 bytes to trigger prefetch (if SIMD available)
        let content = "x".repeat(250);
        let input = format!("{content}\"");
        let result = skip.skip_string(input.as_bytes());
        assert!(result.is_some());
        // 250 x's + 1 quote = consumed 251
        assert_eq!(result.unwrap().consumed, 251);
    }

    #[test]
    fn test_very_large_object_with_prefetch() {
        use std::fmt::Write;
        let skip = Avx2Skip::new();
        // Create object > 192 bytes to trigger prefetch
        let mut json = String::new();
        for i in 0..30 {
            if i > 0 {
                json.push_str(", ");
            }
            let _ = write!(json, "\"field{i:04}\": {i}");
        }
        json.push('}');
        assert!(json.len() > 192);
        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_number_with_plus() {
        let skip = Avx2Skip::new();
        let input = b"1e+10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_number_capital_e() {
        let skip = Avx2Skip::new();
        let input = b"1E10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_zero() {
        let skip = Avx2Skip::new();
        let input = b"0";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_skip_value_true_with_extra() {
        let skip = Avx2Skip::new();
        let input = b"true,";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_false_with_extra() {
        let skip = Avx2Skip::new();
        let input = b"false,";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 5);
    }

    #[test]
    fn test_skip_value_null_with_extra() {
        let skip = Avx2Skip::new();
        let input = b"null,";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 4);
    }

    #[test]
    fn test_skip_value_wrong_literal_prefix() {
        let skip = Avx2Skip::new();
        // Starts with 't' but not "true"
        let input = b"test";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_wrong_false_prefix() {
        let skip = Avx2Skip::new();
        // Starts with 'f' but not "false"
        let input = b"foo";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_value_wrong_null_prefix() {
        let skip = Avx2Skip::new();
        // Starts with 'n' but not "null"
        let input = b"no";
        let result = skip.skip_value(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_prev_escaped_carryover() {
        let skip = Avx2Skip::new();
        // String with escape at chunk boundary (character at position 63 is backslash)
        let mut s = vec![b'a'; 63];
        s.push(b'\\');
        s.push(b'"'); // This quote should be escaped
        s.push(b'"'); // This quote ends the string
        let result = skip.skip_string(&s);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
        assert_eq!(result.unwrap().consumed, 66);
    }

    #[test]
    fn test_multiple_escapes_in_string() {
        let skip = Avx2Skip::new();
        let input = br#"\\\\\\\\""#;
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_escaped_in_string_chunk_boundary() {
        let skip = Avx2Skip::new();
        // Create a string where escape detection crosses chunk boundary
        let mut s = vec![b'a'; 62];
        s.push(b'\\');
        s.push(b'\\'); // Escaped backslash at chunk boundary
        s.extend_from_slice(b"more text\"");
        let result = skip.skip_string(&s);
        assert!(result.is_some());
        assert!(result.unwrap().has_escapes);
    }

    #[test]
    fn test_skip_object_quotes_in_value() {
        let skip = Avx2Skip::new();
        // Object with multiple strings with brackets
        let input = br#""key": "value [with] brackets", "key2": "{more}}"}"#;
        let result = skip.skip_object(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_array_mixed_types() {
        let skip = Avx2Skip::new();
        let input = br#"1, "two", {"three": 3}, [4], true, null]"#;
        let result = skip.skip_array(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, input.len());
    }

    #[test]
    fn test_classify_partial_scalar_empty() {
        let data = b"";
        let (q, b, o, c) = classify_partial_scalar(data, b'{', b'}');
        assert_eq!(q, 0);
        assert_eq!(b, 0);
        assert_eq!(o, 0);
        assert_eq!(c, 0);
    }

    #[test]
    fn test_classify_partial_scalar_single_char() {
        let data = b"{";
        let (_, _, o, _) = classify_partial_scalar(data, b'{', b'}');
        assert_eq!(o, 1);
    }

    #[test]
    fn test_classify_chunk_scalar_backslash() {
        let mut chunk: [u8; 64] = [b' '; 64];
        chunk[31] = b'\\';
        let (_, b, _, _) = classify_chunk_scalar(&chunk, b'{', b'}');
        assert_ne!(b & (1 << 31), 0);
    }

    #[test]
    fn test_skip_object_deeply_nested() {
        let skip = Avx2Skip::new();
        // 10 levels of nesting
        let input = "{{{{{{{{{{}}}}}}}}}}";
        let result = skip.skip_object(&input.as_bytes()[1..]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 19);
    }

    #[test]
    fn test_skip_array_deeply_nested() {
        let skip = Avx2Skip::new();
        let input = "[[[[[[[[[[]]]]]]]]]]";
        let result = skip.skip_array(&input.as_bytes()[1..]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 19);
    }

    #[test]
    fn test_skip_string_no_close_quote() {
        let skip = Avx2Skip::new();
        // String that doesn't close within 64 byte chunk
        let s = "a".repeat(100);
        let result = skip.skip_string(s.as_bytes());
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_object_no_close_brace() {
        let skip = Avx2Skip::new();
        // Object that doesn't close within 64 byte chunk
        let mut json = String::new();
        json.push_str("\"key\": \"value\"");
        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_none());
    }

    #[test]
    fn test_in_string_mask_across_chunks() {
        let skip = Avx2Skip::new();
        // String that spans multiple chunks
        let mut s = vec![b'"'; 1]; // open quote
        s.extend(vec![b'a'; 100]); // content spans chunks
        s.push(b'"'); // close quote
        // Skip from after opening quote
        let result = skip.skip_string(&s[1..]);
        assert!(result.is_some());
    }

    #[test]
    fn test_prefix_xor_patterns() {
        // Test various bit patterns - verify scalar and CLMUL match (if available)
        // The actual values are implementation specific, just verify consistency
        let test_values = [0b1010u64, 0b0100, 0b1111, 0xFFFF, 0x5555_5555_5555_5555];
        for val in test_values {
            let result = prefix_xor(val);
            // Verify it matches scalar implementation
            assert_eq!(result, prefix_xor_scalar(val));
        }
    }

    #[test]
    fn test_get_escaped_prev_set() {
        let mut prev = 1u64; // Previous chunk ended with escape
        let result = get_escaped_branchless(&mut prev, 0);
        // Position 0 should be escaped due to carry-over
        assert_eq!(result, 1);
        assert_eq!(prev, 0);
    }

    #[test]
    fn test_get_escaped_double_backslash() {
        let mut prev = 0u64;
        // Two consecutive backslashes at positions 0 and 1
        let result = get_escaped_branchless(&mut prev, 0b11);
        // Position 1 should be escaped (by position 0)
        // Position 2 should NOT be escaped (double backslash cancels)
        assert_ne!(result & 0b10, 0); // position 1 escaped
    }

    #[test]
    fn test_skip_value_object_with_leading_whitespace() {
        let skip = Avx2Skip::new();
        let input = b"   {\"key\": 1}";
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_array_with_leading_whitespace() {
        let skip = Avx2Skip::new();
        let input = b"\t\n[1, 2]";
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_skip_value_string_with_leading_whitespace() {
        let skip = Avx2Skip::new();
        let input = b"\r\n\"hello\"";
        let result = skip.skip_value(input);
        assert!(result.is_some());
    }

    #[test]
    fn test_remainder_pos_exceeds_length() {
        let skip = Avx2Skip::new();
        // Test string that has quote at position 0 - should return consumed=1
        let input = b"\"rest"; // Quote at position 0
        let result = skip.skip_string(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_unclosed_string_in_remainder() {
        let skip = Avx2Skip::new();
        // String that goes past chunk and remains unclosed in remainder
        let s = "a".repeat(70); // 70 bytes, extends into remainder after 64-byte chunk
        // No closing quote, so should return None
        let result = skip.skip_string(s.as_bytes());
        assert!(result.is_none());
    }

    #[test]
    fn test_skip_container_remainder_no_close() {
        let skip = Avx2Skip::new();
        // Object that doesn't close in remainder section
        let mut json = "\"".to_string();
        json.push_str(&"x".repeat(70)); // Push past first chunk
        json.push('"'); // Close string
        json.push_str(": 1"); // But no closing brace
        let result = skip.skip_object(json.as_bytes());
        assert!(result.is_none());
    }

    #[test]
    fn test_classify_chunk_scalar_brackets() {
        let mut chunk: [u8; 64] = [b' '; 64];
        chunk[0] = b'[';
        chunk[63] = b']';
        let (_, _, o, c) = classify_chunk_scalar(&chunk, b'[', b']');
        assert_eq!(o, 1);
        assert_ne!(c & (1u64 << 63), 0);
    }

    #[test]
    fn test_skip_result_copy() {
        let result = SkipResult {
            consumed: 10,
            has_escapes: false,
        };
        let copy = result;
        assert_eq!(copy.consumed, 10);
        assert!(!copy.has_escapes);
    }

    #[test]
    fn test_skip_value_negative_float() {
        let skip = Avx2Skip::new();
        let input = b"-123.456e-10";
        let result = skip.skip_value(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 12);
    }

    #[test]
    fn test_close_brace_before_open() {
        let skip = Avx2Skip::new();
        // Close brace comes before any open in the same chunk
        let input = b"}";
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_multiple_close_braces() {
        let skip = Avx2Skip::new();
        // Multiple close braces - should stop at first matching one
        let input = b"}}}}";
        let result = skip.skip_object(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 1);
    }

    #[test]
    fn test_opens_before_close_counting() {
        let skip = Avx2Skip::new();
        // Test counting opens before each close
        let input = b"{{}}{";
        let result = skip.skip_object(&input[1..]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().consumed, 3);
    }
}
