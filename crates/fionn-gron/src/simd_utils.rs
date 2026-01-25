// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-accelerated string utilities for gron.
//!
//! This module provides optimized functions for:
//! - Detecting if a field name needs bracket notation
//! - Detecting if a string needs JSON escaping
//! - Escaping strings for JSON output
//!
//! ## Optimizations
//! - Feature detection is cached at startup (avoids per-call CPUID)
//! - SIMD constants are computed once and reused
//! - Control character escape sequences use lookup tables

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    _mm_cmpeq_epi8, _mm_cmpgt_epi8, _mm_cmplt_epi8, _mm_loadu_si128, _mm_movemask_epi8,
    _mm_or_si128, _mm_set1_epi8, _mm256_cmpeq_epi8, _mm256_cmpgt_epi8, _mm256_loadu_si256,
    _mm256_movemask_epi8, _mm256_or_si256, _mm256_set1_epi8,
};

#[cfg(target_arch = "x86")]
use std::arch::x86::*;

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use std::sync::OnceLock;

/// Cached SIMD feature detection to avoid per-call CPUID overhead.
/// This is critical for performance - CPUID can cost 100-200 cycles per check.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
static HAS_SSE2: OnceLock<bool> = OnceLock::new();

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
static HAS_AVX2: OnceLock<bool> = OnceLock::new();

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
fn has_sse2() -> bool {
    *HAS_SSE2.get_or_init(|| is_x86_feature_detected!("sse2"))
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
fn has_avx2() -> bool {
    *HAS_AVX2.get_or_init(|| is_x86_feature_detected!("avx2"))
}

/// Pre-computed control character escape sequences (0x00-0x1F).
/// Each entry is the 6-byte \uXXXX escape sequence.
static CONTROL_ESCAPE_TABLE: [[u8; 6]; 32] = [
    *b"\\u0000",
    *b"\\u0001",
    *b"\\u0002",
    *b"\\u0003",
    *b"\\u0004",
    *b"\\u0005",
    *b"\\u0006",
    *b"\\u0007",
    *b"\\u0008",
    *b"\\u0009",
    *b"\\u000a",
    *b"\\u000b",
    *b"\\u000c",
    *b"\\u000d",
    *b"\\u000e",
    *b"\\u000f",
    *b"\\u0010",
    *b"\\u0011",
    *b"\\u0012",
    *b"\\u0013",
    *b"\\u0014",
    *b"\\u0015",
    *b"\\u0016",
    *b"\\u0017",
    *b"\\u0018",
    *b"\\u0019",
    *b"\\u001a",
    *b"\\u001b",
    *b"\\u001c",
    *b"\\u001d",
    *b"\\u001e",
    *b"\\u001f",
];

/// Check if a field name needs bracket notation (quoting).
///
/// A field name needs quoting if it:
/// - Is empty
/// - Starts with a digit
/// - Contains `.`, `[`, `]`, `"`, `\`, or whitespace
/// - Contains non-ASCII characters (for safety)
#[inline]
#[must_use]
pub fn needs_quoting(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }

    // Check if starts with digit
    if bytes[0].is_ascii_digit() {
        return true;
    }

    // For short strings, use scalar check
    if bytes.len() < 16 {
        return needs_quoting_scalar(bytes);
    }

    // Use SIMD for longer strings (cached feature detection)
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        // Prefer AVX2 for strings >= 32 bytes (processes 32 bytes at a time)
        if bytes.len() >= 32 && has_avx2() {
            return unsafe { needs_quoting_avx2(bytes) };
        }
        if has_sse2() {
            return unsafe { needs_quoting_sse2(bytes) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { needs_quoting_neon(bytes) };
    }

    #[cfg(not(target_arch = "aarch64"))]
    needs_quoting_scalar(bytes)
}

/// Scalar implementation of quoting check.
#[inline]
fn needs_quoting_scalar(bytes: &[u8]) -> bool {
    for &byte in bytes {
        match byte {
            // Path delimiters, JSON special chars, whitespace, control chars, non-ASCII
            b'.'
            | b'['
            | b']'
            | b'"'
            | b'\\'
            | b' '
            | b'\t'
            | b'\n'
            | b'\r'
            | 0..=0x1f
            | 0x80..=0xff => return true,
            _ => {}
        }
    }
    false
}

/// SSE2 implementation of quoting check.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn needs_quoting_sse2(bytes: &[u8]) -> bool {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Process 16 bytes at a time
        while i + 16 <= len {
            let chunk = _mm_loadu_si128(bytes[i..].as_ptr().cast());

            // Check for special characters
            // We check for bytes that would require quoting:
            // - Control chars (< 0x20)
            // - Special chars: . [ ] " \ space

            // Check for control chars (< 0x20) or high bytes (>= 0x80)
            let low_bound = _mm_set1_epi8(0x20);
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let high_bound = _mm_set1_epi8(0x7f_u8 as i8);

            let below_space = _mm_cmplt_epi8(chunk, low_bound);
            let above_tilde = _mm_cmpgt_epi8(chunk, high_bound);
            let control_or_high = _mm_or_si128(below_space, above_tilde);

            // Check for specific special characters
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let dot = _mm_cmpeq_epi8(chunk, _mm_set1_epi8(b'.' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let open_bracket = _mm_cmpeq_epi8(chunk, _mm_set1_epi8(b'[' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let close_bracket = _mm_cmpeq_epi8(chunk, _mm_set1_epi8(b']' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let quote = _mm_cmpeq_epi8(chunk, _mm_set1_epi8(b'"' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let backslash = _mm_cmpeq_epi8(chunk, _mm_set1_epi8(b'\\' as i8));

            // Combine all checks
            let special1 = _mm_or_si128(dot, open_bracket);
            let special2 = _mm_or_si128(close_bracket, quote);
            let special3 = _mm_or_si128(special1, special2);
            let special4 = _mm_or_si128(special3, backslash);
            let needs_quote = _mm_or_si128(control_or_high, special4);

            let mask = _mm_movemask_epi8(needs_quote);
            if mask != 0 {
                return true;
            }

            i += 16;
        }

        // Check remaining bytes with scalar
        needs_quoting_scalar(&bytes[i..])
    }
}

/// AVX2 implementation of quoting check (32 bytes at a time).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn needs_quoting_avx2(bytes: &[u8]) -> bool {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Process 32 bytes at a time
        while i + 32 <= len {
            let chunk = _mm256_loadu_si256(bytes[i..].as_ptr().cast());

            // Check for control chars (< 0x20) or high bytes (>= 0x80)
            // Note: AVX2 doesn't have _mm256_cmplt_epi8, so we use signed comparison
            let low_bound = _mm256_set1_epi8(0x20);
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let high_bound = _mm256_set1_epi8(0x7f_u8 as i8);

            // For signed comparison: bytes < 0x20 means (signed)byte < 0x20
            // and bytes >= 0x80 means (signed)byte < 0 (since 0x80+ wraps to negative)
            let zero = _mm256_set1_epi8(0);
            let below_space = _mm256_cmpgt_epi8(low_bound, chunk);
            let above_tilde = _mm256_cmpgt_epi8(chunk, high_bound);
            let negative = _mm256_cmpgt_epi8(zero, chunk); // catches 0x80-0xFF
            let control_or_high =
                _mm256_or_si256(below_space, _mm256_or_si256(above_tilde, negative));

            // Check for specific special characters
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let dot = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'.' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let open_bracket = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'[' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let close_bracket = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b']' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let quote = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'"' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let backslash = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\\' as i8));

            // Combine all checks
            let special1 = _mm256_or_si256(dot, open_bracket);
            let special2 = _mm256_or_si256(close_bracket, quote);
            let special3 = _mm256_or_si256(special1, special2);
            let special4 = _mm256_or_si256(special3, backslash);
            let needs_quote = _mm256_or_si256(control_or_high, special4);

            let mask = _mm256_movemask_epi8(needs_quote);
            if mask != 0 {
                return true;
            }

            i += 32;
        }

        // Handle remaining 16-31 bytes with SSE2
        if i + 16 <= len {
            return needs_quoting_sse2(&bytes[i..]);
        }

        // Check remaining bytes with scalar
        needs_quoting_scalar(&bytes[i..])
    }
}

/// NEON implementation of quoting check.
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn needs_quoting_neon(bytes: &[u8]) -> bool {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        while i + 16 <= len {
            let chunk = vld1q_u8(bytes[i..].as_ptr());

            // Check for control chars (< 0x20)
            let low_bound = vdupq_n_u8(0x20);
            let below_space = vcltq_u8(chunk, low_bound);

            // Check for high bytes (>= 0x80)
            let high_bound = vdupq_n_u8(0x80);
            let above_127 = vcgeq_u8(chunk, high_bound);

            // Check for specific characters
            let dot = vceqq_u8(chunk, vdupq_n_u8(b'.'));
            let open_bracket = vceqq_u8(chunk, vdupq_n_u8(b'['));
            let close_bracket = vceqq_u8(chunk, vdupq_n_u8(b']'));
            let quote = vceqq_u8(chunk, vdupq_n_u8(b'"'));
            let backslash = vceqq_u8(chunk, vdupq_n_u8(b'\\'));

            // Combine all checks
            let control_or_high = vorrq_u8(below_space, above_127);
            let special1 = vorrq_u8(dot, open_bracket);
            let special2 = vorrq_u8(close_bracket, quote);
            let special3 = vorrq_u8(special1, special2);
            let special4 = vorrq_u8(special3, backslash);
            let needs_quote = vorrq_u8(control_or_high, special4);

            // Check if any byte matched
            let max = vmaxvq_u8(needs_quote);
            if max != 0 {
                return true;
            }

            i += 16;
        }

        needs_quoting_scalar(&bytes[i..])
    }
}

/// Check if a string needs JSON escaping.
///
/// A string needs escaping if it contains:
/// - Control characters (0x00-0x1F)
/// - Quote (")
/// - Backslash (\)
#[inline]
#[must_use]
pub fn needs_escape(bytes: &[u8]) -> bool {
    if bytes.len() < 16 {
        return needs_escape_scalar(bytes);
    }

    // Use cached feature detection to avoid per-call CPUID overhead
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        // Prefer AVX2 for strings >= 32 bytes (processes 32 bytes at a time)
        if bytes.len() >= 32 && has_avx2() {
            return unsafe { needs_escape_avx2(bytes) };
        }
        if has_sse2() {
            return unsafe { needs_escape_sse2(bytes) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { needs_escape_neon(bytes) };
    }

    #[cfg(not(target_arch = "aarch64"))]
    needs_escape_scalar(bytes)
}

/// Scalar implementation of escape check.
#[inline]
fn needs_escape_scalar(bytes: &[u8]) -> bool {
    for &byte in bytes {
        match byte {
            0..=0x1f | b'"' | b'\\' => return true,
            _ => {}
        }
    }
    false
}

/// SSE2 implementation of escape check.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn needs_escape_sse2(bytes: &[u8]) -> bool {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        while i + 16 <= len {
            let chunk = _mm_loadu_si128(bytes[i..].as_ptr().cast());

            // Check for control chars (< 0x20)
            let low_bound = _mm_set1_epi8(0x20);
            let control = _mm_cmplt_epi8(chunk, low_bound);

            // Check for quote and backslash
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let quote = _mm_cmpeq_epi8(chunk, _mm_set1_epi8(b'"' as i8));
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let backslash = _mm_cmpeq_epi8(chunk, _mm_set1_epi8(b'\\' as i8));

            let needs_escape = _mm_or_si128(control, _mm_or_si128(quote, backslash));
            let mask = _mm_movemask_epi8(needs_escape);

            if mask != 0 {
                return true;
            }

            i += 16;
        }

        needs_escape_scalar(&bytes[i..])
    }
}

/// AVX2 implementation of escape check (32 bytes at a time).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn needs_escape_avx2(bytes: &[u8]) -> bool {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Pre-compute constants
        let low_bound = _mm256_set1_epi8(0x20);
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let quote_char = _mm256_set1_epi8(b'"' as i8);
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let backslash_char = _mm256_set1_epi8(b'\\' as i8);

        // Process 32 bytes at a time
        while i + 32 <= len {
            let chunk = _mm256_loadu_si256(bytes[i..].as_ptr().cast());

            // Check for control chars (< 0x20)
            // Note: Uses signed comparison. Bytes 0x00-0x1F are positive and < 0x20.
            // Bytes 0x80-0xFF appear negative in signed comparison, so 0x20 > negative is false.
            // This correctly only flags 0x00-0x1F as control chars needing escape.
            let control = _mm256_cmpgt_epi8(low_bound, chunk);

            // Check for quote and backslash
            let quote = _mm256_cmpeq_epi8(chunk, quote_char);
            let backslash = _mm256_cmpeq_epi8(chunk, backslash_char);

            // Combine all checks - UTF-8 continuation bytes (0x80-0xFF) don't need escaping
            let needs_escape = _mm256_or_si256(control, _mm256_or_si256(quote, backslash));
            let mask = _mm256_movemask_epi8(needs_escape);

            if mask != 0 {
                return true;
            }

            i += 32;
        }

        // Handle remaining 16-31 bytes with SSE2
        if i + 16 <= len {
            return needs_escape_sse2(&bytes[i..]);
        }

        needs_escape_scalar(&bytes[i..])
    }
}

/// NEON implementation of escape check.
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn needs_escape_neon(bytes: &[u8]) -> bool {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        while i + 16 <= len {
            let chunk = vld1q_u8(bytes[i..].as_ptr());

            // Check for control chars (< 0x20)
            let low_bound = vdupq_n_u8(0x20);
            let control = vcltq_u8(chunk, low_bound);

            // Check for quote and backslash
            let quote = vceqq_u8(chunk, vdupq_n_u8(b'"'));
            let backslash = vceqq_u8(chunk, vdupq_n_u8(b'\\'));

            let needs_escape = vorrq_u8(control, vorrq_u8(quote, backslash));
            let max = vmaxvq_u8(needs_escape);

            if max != 0 {
                return true;
            }

            i += 16;
        }

        needs_escape_scalar(&bytes[i..])
    }
}

/// Escape a string for JSON output.
///
/// This function writes the escaped string (including surrounding quotes)
/// to the output buffer.
#[inline]
pub fn escape_json_string(s: &str, out: &mut Vec<u8>) {
    let bytes = s.as_bytes();

    out.push(b'"');

    // Fast path: no escaping needed
    if !needs_escape(bytes) {
        out.extend_from_slice(bytes);
        out.push(b'"');
        return;
    }

    // Slow path: escape character by character
    escape_json_string_slow(bytes, out);
    out.push(b'"');
}

/// Slow path for JSON string escaping.
/// Uses pre-computed lookup table for control characters.
#[inline(never)]
fn escape_json_string_slow(bytes: &[u8], out: &mut Vec<u8>) {
    for &byte in bytes {
        match byte {
            b'"' => {
                out.extend_from_slice(b"\\\"");
            }
            b'\\' => {
                out.extend_from_slice(b"\\\\");
            }
            b'\n' => {
                out.extend_from_slice(b"\\n");
            }
            b'\r' => {
                out.extend_from_slice(b"\\r");
            }
            b'\t' => {
                out.extend_from_slice(b"\\t");
            }
            b'\x08' => {
                // backspace
                out.extend_from_slice(b"\\b");
            }
            b'\x0c' => {
                // form feed
                out.extend_from_slice(b"\\f");
            }
            0x00..=0x1f => {
                // Use pre-computed lookup table for other control characters
                out.extend_from_slice(&CONTROL_ESCAPE_TABLE[byte as usize]);
            }
            _ => {
                out.push(byte);
            }
        }
    }
}

/// Write a JSON value to output without escaping (for numbers, bools, null).
#[inline]
#[allow(dead_code)] // Public API utility - may be used by downstream crates
pub fn write_raw_value(value: &str, out: &mut Vec<u8>) {
    out.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_quoting_simple() {
        assert!(!needs_quoting(b"name"));
        assert!(!needs_quoting(b"userName"));
        assert!(!needs_quoting(b"user_name"));
    }

    #[test]
    fn test_needs_quoting_special() {
        assert!(needs_quoting(b"field.name"));
        assert!(needs_quoting(b"field[0]"));
        assert!(needs_quoting(b"field]"));
        assert!(needs_quoting(b"field\"quote"));
        assert!(needs_quoting(b"field\\slash"));
    }

    #[test]
    fn test_needs_quoting_whitespace() {
        assert!(needs_quoting(b"field name"));
        assert!(needs_quoting(b"field\tname"));
        assert!(needs_quoting(b"field\nname"));
    }

    #[test]
    fn test_needs_quoting_digit_start() {
        assert!(needs_quoting(b"0field"));
        assert!(needs_quoting(b"123"));
    }

    #[test]
    fn test_needs_quoting_empty() {
        assert!(needs_quoting(b""));
    }

    #[test]
    fn test_needs_escape_simple() {
        assert!(!needs_escape(b"hello world"));
        assert!(!needs_escape(b"no special chars here"));
    }

    #[test]
    fn test_needs_escape_quotes() {
        assert!(needs_escape(b"has \"quotes\""));
        assert!(needs_escape(b"has \\backslash"));
    }

    #[test]
    fn test_needs_escape_control() {
        assert!(needs_escape(b"has\nnewline"));
        assert!(needs_escape(b"has\ttab"));
        assert!(needs_escape(&[0x00])); // null
    }

    #[test]
    fn test_escape_json_string_simple() {
        let mut out = Vec::new();
        escape_json_string("hello", &mut out);
        assert_eq!(out, b"\"hello\"");
    }

    #[test]
    fn test_escape_json_string_quotes() {
        let mut out = Vec::new();
        escape_json_string(r#"say "hello""#, &mut out);
        assert_eq!(out, br#""say \"hello\"""#);
    }

    #[test]
    fn test_escape_json_string_newline() {
        let mut out = Vec::new();
        escape_json_string("line1\nline2", &mut out);
        assert_eq!(out, b"\"line1\\nline2\"");
    }

    #[test]
    fn test_escape_json_string_control() {
        let mut out = Vec::new();
        escape_json_string("\x00\x01\x02", &mut out);
        assert_eq!(out, b"\"\\u0000\\u0001\\u0002\"");
    }

    #[test]
    fn test_long_string_simd_path() {
        // Test string long enough to trigger SIMD path
        let long_clean = "a".repeat(100);
        assert!(!needs_quoting(long_clean.as_bytes()));
        assert!(!needs_escape(long_clean.as_bytes()));

        let long_with_dot = format!("{long_clean}.");
        assert!(needs_quoting(long_with_dot.as_bytes()));

        let long_with_quote = format!("{long_clean}\"");
        assert!(needs_escape(long_with_quote.as_bytes()));
    }

    // =========================================================================
    // Comprehensive needs_quoting Tests
    // =========================================================================

    #[test]
    fn test_needs_quoting_control_chars() {
        // All control characters 0x00-0x1F should require quoting
        for i in 0..=0x1f_u8 {
            assert!(
                needs_quoting(&[i]),
                "Control char 0x{i:02x} should need quoting"
            );
        }
    }

    #[test]
    fn test_needs_quoting_non_ascii() {
        // Non-ASCII bytes should require quoting (0x80-0xFF)
        assert!(needs_quoting(&[0x80]));
        assert!(needs_quoting(&[0xff]));
        assert!(needs_quoting("héllo".as_bytes()));
        assert!(needs_quoting("日本語".as_bytes()));
    }

    #[test]
    fn test_needs_quoting_carriage_return() {
        assert!(needs_quoting(b"field\rname"));
    }

    #[test]
    fn test_needs_quoting_all_special_chars() {
        // Test each special character individually
        assert!(needs_quoting(b"."));
        assert!(needs_quoting(b"["));
        assert!(needs_quoting(b"]"));
        assert!(needs_quoting(b"\""));
        assert!(needs_quoting(b"\\"));
        assert!(needs_quoting(b" "));
        assert!(needs_quoting(b"\t"));
        assert!(needs_quoting(b"\n"));
        assert!(needs_quoting(b"\r"));
    }

    #[test]
    fn test_needs_quoting_16_bytes() {
        // Exactly 16 bytes - triggers SIMD on SSE2-capable systems
        let clean_16 = "abcdefghijklmnop";
        assert_eq!(clean_16.len(), 16);
        assert!(!needs_quoting(clean_16.as_bytes()));

        let dirty_16 = "abcdefghijklmno.";
        assert_eq!(dirty_16.len(), 16);
        assert!(needs_quoting(dirty_16.as_bytes()));
    }

    #[test]
    fn test_needs_quoting_32_bytes() {
        // Exactly 32 bytes - triggers AVX2 on capable systems
        let clean_32 = "abcdefghijklmnopqrstuvwxyzABCDEF";
        assert_eq!(clean_32.len(), 32);
        assert!(!needs_quoting(clean_32.as_bytes()));

        let dirty_32_start = ".bcdefghijklmnopqrstuvwxyzABCDEF";
        assert!(needs_quoting(dirty_32_start.as_bytes()));

        let dirty_32_end = "abcdefghijklmnopqrstuvwxyzABCDE.";
        assert!(needs_quoting(dirty_32_end.as_bytes()));
    }

    #[test]
    fn test_needs_quoting_48_bytes() {
        // 48 bytes - one AVX2 iteration + remainder
        let clean_48 = "a".repeat(48);
        assert!(!needs_quoting(clean_48.as_bytes()));

        let mut dirty_48 = "a".repeat(47);
        dirty_48.push('.');
        assert!(needs_quoting(dirty_48.as_bytes()));
    }

    #[test]
    fn test_needs_quoting_64_bytes() {
        // 64 bytes - two AVX2 iterations
        let clean_64 = "a".repeat(64);
        assert!(!needs_quoting(clean_64.as_bytes()));

        // Special char in second iteration
        let mut dirty_64 = "a".repeat(63);
        dirty_64.push('[');
        assert!(needs_quoting(dirty_64.as_bytes()));
    }

    #[test]
    fn test_needs_quoting_with_remainder() {
        // 35 bytes: 32 (AVX2) + 3 (scalar remainder)
        let clean_35 = "a".repeat(35);
        assert!(!needs_quoting(clean_35.as_bytes()));

        // Dirty in remainder
        let mut dirty_35 = "a".repeat(34);
        dirty_35.push(']');
        assert!(needs_quoting(dirty_35.as_bytes()));
    }

    #[test]
    fn test_needs_quoting_sse2_remainder() {
        // 20 bytes: 16 (SSE2) + 4 (scalar remainder)
        let clean_20 = "a".repeat(20);
        assert!(!needs_quoting(clean_20.as_bytes()));

        // Dirty in SSE2 portion
        let mut dirty_20_middle = "a".repeat(8);
        dirty_20_middle.push('.');
        dirty_20_middle.push_str(&"a".repeat(11));
        assert!(needs_quoting(dirty_20_middle.as_bytes()));
    }

    // =========================================================================
    // Comprehensive needs_escape Tests
    // =========================================================================

    #[test]
    fn test_needs_escape_all_control_chars() {
        // All control characters 0x00-0x1F should require escaping
        for i in 0..=0x1f_u8 {
            assert!(
                needs_escape(&[i]),
                "Control char 0x{i:02x} should need escaping"
            );
        }
    }

    #[test]
    fn test_needs_escape_non_ascii_safe() {
        // Non-ASCII bytes (0x80-0xFF) should NOT need escaping in JSON
        assert!(!needs_escape(&[0x80]));
        assert!(!needs_escape(&[0xff]));
        assert!(!needs_escape("héllo".as_bytes()));
        assert!(!needs_escape("日本語".as_bytes()));
    }

    #[test]
    fn test_needs_escape_16_bytes() {
        let clean_16 = "abcdefghijklmnop";
        assert_eq!(clean_16.len(), 16);
        assert!(!needs_escape(clean_16.as_bytes()));

        let dirty_16 = "abcdefghijklmno\"";
        assert!(needs_escape(dirty_16.as_bytes()));

        let dirty_16_backslash = "abcdefghijklmno\\";
        assert!(needs_escape(dirty_16_backslash.as_bytes()));
    }

    #[test]
    fn test_needs_escape_32_bytes() {
        let clean_32 = "a".repeat(32);
        assert!(!needs_escape(clean_32.as_bytes()));

        let mut dirty_32 = "a".repeat(31);
        dirty_32.push('\\');
        assert!(needs_escape(dirty_32.as_bytes()));
    }

    #[test]
    fn test_needs_escape_48_bytes() {
        let clean_48 = "a".repeat(48);
        assert!(!needs_escape(clean_48.as_bytes()));

        // Control char in second AVX2 chunk
        let mut dirty_48: Vec<u8> = "a".repeat(40).into_bytes();
        dirty_48.push(0x1f);
        dirty_48.extend_from_slice(&"a".repeat(7).into_bytes());
        assert!(needs_escape(&dirty_48));
    }

    #[test]
    fn test_needs_escape_sse2_remainder() {
        // 24 bytes: 16 (SSE2) + 8 (scalar)
        let clean_24 = "a".repeat(24);
        assert!(!needs_escape(clean_24.as_bytes()));

        // Dirty in scalar remainder
        let mut dirty_24 = "a".repeat(23);
        dirty_24.push('"');
        assert!(needs_escape(dirty_24.as_bytes()));
    }

    // =========================================================================
    // escape_json_string Comprehensive Tests
    // =========================================================================

    #[test]
    fn test_escape_json_string_backslash() {
        let mut out = Vec::new();
        escape_json_string("path\\to\\file", &mut out);
        assert_eq!(out, b"\"path\\\\to\\\\file\"");
    }

    #[test]
    fn test_escape_json_string_tab() {
        let mut out = Vec::new();
        escape_json_string("col1\tcol2", &mut out);
        assert_eq!(out, b"\"col1\\tcol2\"");
    }

    #[test]
    fn test_escape_json_string_carriage_return() {
        let mut out = Vec::new();
        escape_json_string("line1\rline2", &mut out);
        assert_eq!(out, b"\"line1\\rline2\"");
    }

    #[test]
    fn test_escape_json_string_backspace() {
        let mut out = Vec::new();
        escape_json_string("back\x08space", &mut out);
        assert_eq!(out, b"\"back\\bspace\"");
    }

    #[test]
    fn test_escape_json_string_form_feed() {
        let mut out = Vec::new();
        escape_json_string("form\x0cfeed", &mut out);
        assert_eq!(out, b"\"form\\ffeed\"");
    }

    #[test]
    fn test_escape_json_string_null_byte() {
        let mut out = Vec::new();
        escape_json_string("\x00", &mut out);
        assert_eq!(out, b"\"\\u0000\"");
    }

    #[test]
    fn test_escape_json_string_all_common_escapes() {
        let mut out = Vec::new();
        escape_json_string("\"\\\n\r\t", &mut out);
        assert_eq!(out, b"\"\\\"\\\\\\n\\r\\t\"");
    }

    #[test]
    fn test_escape_json_string_mixed_content() {
        let mut out = Vec::new();
        escape_json_string("hello \"world\"\nline2", &mut out);
        assert_eq!(out, b"\"hello \\\"world\\\"\\nline2\"");
    }

    #[test]
    fn test_escape_json_string_unicode() {
        let mut out = Vec::new();
        escape_json_string("日本語", &mut out);
        // Unicode characters should pass through unchanged
        assert_eq!(out, "\"日本語\"".as_bytes());
    }

    #[test]
    fn test_escape_json_string_empty() {
        let mut out = Vec::new();
        escape_json_string("", &mut out);
        assert_eq!(out, b"\"\"");
    }

    #[test]
    fn test_escape_json_string_long_clean() {
        let long = "a".repeat(100);
        let mut out = Vec::new();
        escape_json_string(&long, &mut out);
        assert_eq!(out.len(), 102); // 100 chars + 2 quotes
        assert_eq!(out[0], b'"');
        assert_eq!(out[101], b'"');
    }

    #[test]
    fn test_escape_json_string_long_with_escapes() {
        let long = format!("{}\"{}\"", "a".repeat(50), "b".repeat(50));
        let mut out = Vec::new();
        escape_json_string(&long, &mut out);
        // Each quote becomes \", so 102 + 4 = 106
        assert!(out.starts_with(b"\""));
        assert!(out.ends_with(b"\""));
        assert!(out.windows(2).any(|w| w == b"\\\""));
    }

    #[test]
    fn test_escape_json_string_other_control_chars() {
        // Test various control characters that use \uXXXX format
        for i in [
            0x01_u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x0e, 0x0f, 0x10, 0x11, 0x1f,
        ] {
            let input = String::from_utf8(vec![i]).unwrap();
            let mut out = Vec::new();
            escape_json_string(&input, &mut out);
            // Should produce \uXXXX format
            assert!(
                out.len() == 8,
                "Control char 0x{i:02x} should produce \\uXXXX"
            );
            assert_eq!(&out[1..3], b"\\u");
        }
    }

    // =========================================================================
    // write_raw_value Tests
    // =========================================================================

    #[test]
    fn test_write_raw_value_number() {
        let mut out = Vec::new();
        write_raw_value("12345", &mut out);
        assert_eq!(out, b"12345");
    }

    #[test]
    fn test_write_raw_value_float() {
        let mut out = Vec::new();
        write_raw_value("1.5", &mut out);
        assert_eq!(out, b"1.5");
    }

    #[test]
    fn test_write_raw_value_bool_true() {
        let mut out = Vec::new();
        write_raw_value("true", &mut out);
        assert_eq!(out, b"true");
    }

    #[test]
    fn test_write_raw_value_bool_false() {
        let mut out = Vec::new();
        write_raw_value("false", &mut out);
        assert_eq!(out, b"false");
    }

    #[test]
    fn test_write_raw_value_null() {
        let mut out = Vec::new();
        write_raw_value("null", &mut out);
        assert_eq!(out, b"null");
    }

    #[test]
    fn test_write_raw_value_negative() {
        let mut out = Vec::new();
        write_raw_value("-42", &mut out);
        assert_eq!(out, b"-42");
    }

    #[test]
    fn test_write_raw_value_scientific() {
        let mut out = Vec::new();
        write_raw_value("1.23e10", &mut out);
        assert_eq!(out, b"1.23e10");
    }

    // =========================================================================
    // Scalar Path Tests (force scalar by using short strings)
    // =========================================================================

    #[test]
    fn test_needs_quoting_scalar_all_clean() {
        // Short strings use scalar path
        assert!(!needs_quoting(b"abc"));
        assert!(!needs_quoting(b"xyz123"));
        assert!(!needs_quoting(b"_underscore"));
    }

    #[test]
    fn test_needs_quoting_scalar_special_in_middle() {
        assert!(needs_quoting(b"a.b"));
        assert!(needs_quoting(b"a[b"));
        assert!(needs_quoting(b"a]b"));
        assert!(needs_quoting(b"a\"b"));
        assert!(needs_quoting(b"a\\b"));
        assert!(needs_quoting(b"a b"));
    }

    #[test]
    fn test_needs_escape_scalar_all_clean() {
        assert!(!needs_escape(b"abc"));
        assert!(!needs_escape(b"hello world"));
        assert!(!needs_escape(b"!@#$%^&*()"));
    }

    #[test]
    fn test_needs_escape_scalar_with_control() {
        assert!(needs_escape(b"\x00"));
        assert!(needs_escape(b"\x1f"));
        assert!(needs_escape(b"a\nb"));
        assert!(needs_escape(b"a\tb"));
    }

    // =========================================================================
    // CONTROL_ESCAPE_TABLE Tests
    // =========================================================================

    #[test]
    fn test_control_escape_table_contents() {
        // Verify the escape table has correct values
        assert_eq!(&CONTROL_ESCAPE_TABLE[0], b"\\u0000");
        assert_eq!(&CONTROL_ESCAPE_TABLE[1], b"\\u0001");
        assert_eq!(&CONTROL_ESCAPE_TABLE[8], b"\\u0008");
        assert_eq!(&CONTROL_ESCAPE_TABLE[9], b"\\u0009");
        assert_eq!(&CONTROL_ESCAPE_TABLE[10], b"\\u000a");
        assert_eq!(&CONTROL_ESCAPE_TABLE[13], b"\\u000d");
        assert_eq!(&CONTROL_ESCAPE_TABLE[31], b"\\u001f");
    }

    // =========================================================================
    // Feature Detection Tests
    // =========================================================================

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[test]
    fn test_has_sse2_cached() {
        // Call twice to ensure caching works
        let first = has_sse2();
        let second = has_sse2();
        assert_eq!(first, second);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[test]
    fn test_has_avx2_cached() {
        // Call twice to ensure caching works
        let first = has_avx2();
        let second = has_avx2();
        assert_eq!(first, second);
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_needs_quoting_just_under_simd_threshold() {
        // 15 bytes - just under SIMD threshold
        let clean_15 = "a".repeat(15);
        assert!(!needs_quoting(clean_15.as_bytes()));

        let mut dirty_15 = "a".repeat(14);
        dirty_15.push('.');
        assert!(needs_quoting(dirty_15.as_bytes()));
    }

    #[test]
    fn test_needs_escape_just_under_simd_threshold() {
        // 15 bytes - just under SIMD threshold
        let clean_15 = "a".repeat(15);
        assert!(!needs_escape(clean_15.as_bytes()));

        let mut dirty_15 = "a".repeat(14);
        dirty_15.push('"');
        assert!(needs_escape(dirty_15.as_bytes()));
    }

    #[test]
    fn test_escape_all_ascii_printable() {
        // All ASCII printable chars except " and \ shouldn't be escaped
        let printable: String = (0x20..=0x7e_u8)
            .filter(|&b| b != b'"' && b != b'\\')
            .map(|b| b as char)
            .collect();
        let mut out = Vec::new();
        escape_json_string(&printable, &mut out);
        // Output should be input + 2 quotes
        assert_eq!(out.len(), printable.len() + 2);
    }

    #[test]
    fn test_escape_boundary_between_chunks() {
        // Create a string where escape char falls exactly on chunk boundary
        let mut input = "a".repeat(31);
        input.push('"');
        input.push_str(&"b".repeat(32));

        let mut out = Vec::new();
        escape_json_string(&input, &mut out);
        assert!(out.windows(2).any(|w| w == b"\\\""));
    }
}
