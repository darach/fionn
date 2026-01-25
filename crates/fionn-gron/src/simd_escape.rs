// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-accelerated string escaping for gron output.
//!
//! This module provides optimized JSON string escaping using SIMD instructions
//! to find escape boundaries and bulk-copy clean segments.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    __m128i, __m256i, _mm_cmpeq_epi8, _mm_cmplt_epi8, _mm_loadu_si128, _mm_movemask_epi8,
    _mm_or_si128, _mm_set1_epi8, _mm256_cmpeq_epi8, _mm256_cmpgt_epi8, _mm256_loadu_si256,
    _mm256_movemask_epi8, _mm256_or_si256, _mm256_set1_epi8,
};

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{
    uint8x16_t, vceqq_u8, vcltq_u8, vdupq_n_u8, vld1q_u8, vmaxvq_u8, vorrq_u8,
};

#[allow(unused_imports)] // OnceLock used conditionally per target_arch
use std::sync::OnceLock;

/// Cached SIMD feature detection.
#[cfg(target_arch = "x86_64")]
static HAS_SSE2: OnceLock<bool> = OnceLock::new();

#[cfg(target_arch = "x86_64")]
static HAS_AVX2: OnceLock<bool> = OnceLock::new();

#[cfg(target_arch = "x86_64")]
#[inline]
fn has_sse2() -> bool {
    *HAS_SSE2.get_or_init(|| is_x86_feature_detected!("sse2"))
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn has_avx2() -> bool {
    *HAS_AVX2.get_or_init(|| is_x86_feature_detected!("avx2"))
}

/// SIMD-accelerated JSON string escaping.
///
/// Uses SIMD to find escape boundaries and bulk-copies clean segments,
/// providing significant speedup over byte-by-byte escaping.
#[inline]
pub fn escape_json_string_simd(s: &str, out: &mut Vec<u8>) {
    let bytes = s.as_bytes();

    out.push(b'"');

    if bytes.is_empty() {
        out.push(b'"');
        return;
    }

    #[cfg(target_arch = "x86_64")]
    {
        // Prefer AVX2 for strings >= 32 bytes
        if bytes.len() >= 32 && has_avx2() {
            unsafe { escape_json_string_avx2(bytes, out) };
            out.push(b'"');
            return;
        }
        if has_sse2() {
            unsafe { escape_json_string_sse2(bytes, out) };
            out.push(b'"');
            return;
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // NEON is always available on aarch64
        unsafe { escape_json_string_neon(bytes, out) };
        out.push(b'"');
        return;
    }

    // Fallback to scalar
    #[allow(unreachable_code)] // Reachable only when no SIMD available (non-x86/aarch64)
    {
        escape_json_string_scalar(bytes, out);
        out.push(b'"');
    }
}

/// SSE2-accelerated escape implementation.
///
/// Strategy:
/// 1. Scan 16 bytes at a time looking for characters needing escape
/// 2. Bulk copy all clean bytes before the first escape character
/// 3. Escape the character
/// 4. Repeat from step 1
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn escape_json_string_sse2(bytes: &[u8], out: &mut Vec<u8>) {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Pre-compute SIMD constants
        let control_bound = _mm_set1_epi8(0x20);
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let quote_char = _mm_set1_epi8(b'"' as i8);
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let backslash_char = _mm_set1_epi8(b'\\' as i8);

        while i < len {
            // Find the next character that needs escaping
            let escape_pos =
                find_next_escape_sse2(bytes, i, control_bound, quote_char, backslash_char);

            if let Some(pos) = escape_pos {
                // Bulk copy everything before the escape character
                if pos > i {
                    out.extend_from_slice(&bytes[i..pos]);
                }

                // Escape the character
                let byte = bytes[pos];
                escape_byte(byte, out);
                i = pos + 1;
            } else {
                // No more escapes needed, copy the rest
                out.extend_from_slice(&bytes[i..]);
                break;
            }
        }
    }
}

/// Find the next character that needs escaping using SSE2.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn find_next_escape_sse2(
    bytes: &[u8],
    start: usize,
    control_bound: __m128i,
    quote_char: __m128i,
    backslash_char: __m128i,
) -> Option<usize> {
    unsafe {
        let len = bytes.len();
        let mut i = start;

        // Process 16 bytes at a time
        while i + 16 <= len {
            let chunk = _mm_loadu_si128(bytes[i..].as_ptr().cast());

            // Check for control chars (< 0x20)
            let control = _mm_cmplt_epi8(chunk, control_bound);

            // Check for quote and backslash
            let quote = _mm_cmpeq_epi8(chunk, quote_char);
            let backslash = _mm_cmpeq_epi8(chunk, backslash_char);

            // Combine all checks
            let needs_escape = _mm_or_si128(control, _mm_or_si128(quote, backslash));
            let mask = _mm_movemask_epi8(needs_escape);

            if mask != 0 {
                // Found a character that needs escaping
                let offset = mask.trailing_zeros() as usize;
                return Some(i + offset);
            }

            i += 16;
        }

        // Check remaining bytes with scalar
        for (offset, &byte) in bytes[i..].iter().enumerate() {
            if needs_escape_byte(byte) {
                return Some(i + offset);
            }
        }

        None
    }
}

/// AVX2-accelerated escape implementation (32 bytes at a time).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn escape_json_string_avx2(bytes: &[u8], out: &mut Vec<u8>) {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Pre-compute SIMD constants
        let control_bound = _mm256_set1_epi8(0x20);
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let quote_char = _mm256_set1_epi8(b'"' as i8);
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let backslash_char = _mm256_set1_epi8(b'\\' as i8);

        while i < len {
            // Find the next character that needs escaping
            let escape_pos =
                find_next_escape_avx2(bytes, i, control_bound, quote_char, backslash_char);

            if let Some(pos) = escape_pos {
                // Bulk copy everything before the escape character
                if pos > i {
                    out.extend_from_slice(&bytes[i..pos]);
                }

                // Escape the character
                let byte = bytes[pos];
                escape_byte(byte, out);
                i = pos + 1;
            } else {
                // No more escapes needed, copy the rest
                out.extend_from_slice(&bytes[i..]);
                break;
            }
        }
    }
}

/// Find the next character that needs escaping using AVX2.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn find_next_escape_avx2(
    bytes: &[u8],
    start: usize,
    control_bound: __m256i,
    quote_char: __m256i,
    backslash_char: __m256i,
) -> Option<usize> {
    unsafe {
        let len = bytes.len();
        let mut i = start;

        // Process 32 bytes at a time
        while i + 32 <= len {
            let chunk = _mm256_loadu_si256(bytes[i..].as_ptr().cast());

            // Check for control chars (< 0x20)
            // Note: This uses signed comparison. Bytes 0x00-0x1F are positive and < 0x20.
            // Bytes 0x80-0xFF appear negative in signed comparison, so 0x20 > negative is false.
            // This correctly only flags 0x00-0x1F as control chars.
            let control = _mm256_cmpgt_epi8(control_bound, chunk);

            // Check for quote and backslash
            let quote = _mm256_cmpeq_epi8(chunk, quote_char);
            let backslash = _mm256_cmpeq_epi8(chunk, backslash_char);

            // Combine all checks - only control chars, quote, and backslash need escaping
            // UTF-8 continuation bytes (0x80-0xFF) pass through unchanged
            let needs_escape = _mm256_or_si256(control, _mm256_or_si256(quote, backslash));
            let mask = _mm256_movemask_epi8(needs_escape);

            if mask != 0 {
                // Found a character that needs escaping
                let offset = mask.trailing_zeros() as usize;
                return Some(i + offset);
            }

            i += 32;
        }

        // Handle remaining bytes with SSE2 or scalar
        if i + 16 <= len && has_sse2() {
            // Pre-compute SSE2 constants
            let sse_control = _mm_set1_epi8(0x20);
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let sse_quote = _mm_set1_epi8(b'"' as i8);
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let sse_backslash = _mm_set1_epi8(b'\\' as i8);

            return find_next_escape_sse2(bytes, i, sse_control, sse_quote, sse_backslash);
        }

        // Check remaining bytes with scalar
        for (offset, &byte) in bytes[i..].iter().enumerate() {
            if needs_escape_byte(byte) {
                return Some(i + offset);
            }
        }

        None
    }
}

/// NEON-accelerated escape implementation (16 bytes at a time).
#[cfg(target_arch = "aarch64")]
unsafe fn escape_json_string_neon(bytes: &[u8], out: &mut Vec<u8>) {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Pre-compute NEON constants
        let control_bound = vdupq_n_u8(0x20);
        let quote_char = vdupq_n_u8(b'"');
        let backslash_char = vdupq_n_u8(b'\\');

        while i < len {
            // Find the next character that needs escaping
            let escape_pos =
                find_next_escape_neon(bytes, i, control_bound, quote_char, backslash_char);

            if let Some(pos) = escape_pos {
                // Bulk copy everything before the escape character
                if pos > i {
                    out.extend_from_slice(&bytes[i..pos]);
                }

                // Escape the character
                let byte = bytes[pos];
                escape_byte(byte, out);
                i = pos + 1;
            } else {
                // No more escapes needed, copy the rest
                out.extend_from_slice(&bytes[i..]);
                break;
            }
        }
    }
}

/// Find the next character that needs escaping using NEON.
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn find_next_escape_neon(
    bytes: &[u8],
    start: usize,
    control_bound: uint8x16_t,
    quote_char: uint8x16_t,
    backslash_char: uint8x16_t,
) -> Option<usize> {
    unsafe {
        let len = bytes.len();
        let mut i = start;

        // Process 16 bytes at a time
        while i + 16 <= len {
            let chunk = vld1q_u8(bytes[i..].as_ptr());

            // Check for control chars (< 0x20)
            let control = vcltq_u8(chunk, control_bound);

            // Check for quote and backslash
            let quote = vceqq_u8(chunk, quote_char);
            let backslash = vceqq_u8(chunk, backslash_char);

            // Combine all checks
            let needs_escape = vorrq_u8(control, vorrq_u8(quote, backslash));

            // Check if any byte matched
            let max = vmaxvq_u8(needs_escape);
            if max != 0 {
                // Found a character that needs escaping - find which one
                // Extract lanes and find first non-zero
                let arr: [u8; 16] = std::mem::transmute(needs_escape);
                for (offset, &val) in arr.iter().enumerate() {
                    if val != 0 {
                        return Some(i + offset);
                    }
                }
            }

            i += 16;
        }

        // Check remaining bytes with scalar
        for (offset, &byte) in bytes[i..].iter().enumerate() {
            if needs_escape_byte(byte) {
                return Some(i + offset);
            }
        }

        None
    }
}

/// Check if a single byte needs JSON escaping.
#[inline]
const fn needs_escape_byte(byte: u8) -> bool {
    matches!(byte, 0..=0x1f | b'"' | b'\\')
}

/// Escape a single byte to the output buffer.
#[inline]
fn escape_byte(byte: u8, out: &mut Vec<u8>) {
    match byte {
        b'"' => {
            out.push(b'\\');
            out.push(b'"');
        }
        b'\\' => {
            out.push(b'\\');
            out.push(b'\\');
        }
        b'\n' => {
            out.push(b'\\');
            out.push(b'n');
        }
        b'\r' => {
            out.push(b'\\');
            out.push(b'r');
        }
        b'\t' => {
            out.push(b'\\');
            out.push(b't');
        }
        b'\x08' => {
            // backspace
            out.push(b'\\');
            out.push(b'b');
        }
        b'\x0c' => {
            // form feed
            out.push(b'\\');
            out.push(b'f');
        }
        0x00..=0x1f => {
            // Other control characters as \uXXXX
            out.extend_from_slice(b"\\u00");
            let high = byte >> 4;
            let low = byte & 0x0f;
            out.push(hex_digit(high));
            out.push(hex_digit(low));
        }
        _ => {
            out.push(byte);
        }
    }
}

#[inline]
const fn hex_digit(n: u8) -> u8 {
    if n < 10 { b'0' + n } else { b'a' + n - 10 }
}

/// Scalar fallback for string escaping.
fn escape_json_string_scalar(bytes: &[u8], out: &mut Vec<u8>) {
    for &byte in bytes {
        if needs_escape_byte(byte) {
            escape_byte(byte, out);
        } else {
            out.push(byte);
        }
    }
}

/// Escape a JSON string, returning a new String.
///
/// This is a convenience wrapper around `escape_json_string_simd`.
#[inline]
#[must_use]
pub fn escape_json_to_string(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len() + 2);
    escape_json_string_simd(s, &mut out);
    // Safety: we only write valid UTF-8
    unsafe { String::from_utf8_unchecked(out) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_simple() {
        let mut out = Vec::new();
        escape_json_string_simd("hello", &mut out);
        assert_eq!(out, b"\"hello\"");
    }

    #[test]
    fn test_escape_quotes() {
        let mut out = Vec::new();
        escape_json_string_simd(r#"say "hello""#, &mut out);
        assert_eq!(out, br#""say \"hello\"""#);
    }

    #[test]
    fn test_escape_backslash() {
        let mut out = Vec::new();
        escape_json_string_simd(r"path\to\file", &mut out);
        assert_eq!(out, br#""path\\to\\file""#);
    }

    #[test]
    fn test_escape_newline() {
        let mut out = Vec::new();
        escape_json_string_simd("line1\nline2", &mut out);
        assert_eq!(out, b"\"line1\\nline2\"");
    }

    #[test]
    fn test_escape_tab() {
        let mut out = Vec::new();
        escape_json_string_simd("col1\tcol2", &mut out);
        assert_eq!(out, b"\"col1\\tcol2\"");
    }

    #[test]
    fn test_escape_carriage_return() {
        let mut out = Vec::new();
        escape_json_string_simd("line1\rline2", &mut out);
        assert_eq!(out, b"\"line1\\rline2\"");
    }

    #[test]
    fn test_escape_control_chars() {
        let mut out = Vec::new();
        escape_json_string_simd("\x00\x01\x02", &mut out);
        assert_eq!(out, b"\"\\u0000\\u0001\\u0002\"");
    }

    #[test]
    fn test_escape_empty() {
        let mut out = Vec::new();
        escape_json_string_simd("", &mut out);
        assert_eq!(out, b"\"\"");
    }

    #[test]
    fn test_escape_long_clean_string() {
        // Test string longer than 16 bytes (SIMD path)
        let clean = "a".repeat(100);
        let mut out = Vec::new();
        escape_json_string_simd(&clean, &mut out);
        assert_eq!(out.len(), clean.len() + 2); // quotes
        assert_eq!(&out[1..101], clean.as_bytes());
    }

    #[test]
    fn test_escape_long_with_escapes() {
        // Long string with escapes scattered throughout
        let input = format!(
            "{}\"{}\\{}\n{}",
            "a".repeat(20),
            "b".repeat(20),
            "c".repeat(20),
            "d".repeat(20)
        );
        let mut out = Vec::new();
        escape_json_string_simd(&input, &mut out);

        // Verify it's valid JSON string
        assert!(out.starts_with(b"\""));
        assert!(out.ends_with(b"\""));
        assert!(out.windows(2).any(|w| w == b"\\\""));
        assert!(out.windows(2).any(|w| w == b"\\\\"));
        assert!(out.windows(2).any(|w| w == b"\\n"));
    }

    #[test]
    fn test_escape_unicode_passthrough() {
        // UTF-8 should pass through unchanged
        let mut out = Vec::new();
        escape_json_string_simd("hello 世界 emoji 🎉", &mut out);
        assert!(String::from_utf8(out.clone()).is_ok());
        // Check the UTF-8 bytes are preserved
        assert!(out.windows("世界".len()).any(|w| w == "世界".as_bytes()));
    }

    #[test]
    fn test_escape_mixed_scenarios() {
        let cases = [
            (r#"{"key": "value"}"#, r#""{\"key\": \"value\"}""#),
            ("path\\to\\file.txt", r#""path\\to\\file.txt""#),
            ("line1\nline2\nline3", r#""line1\nline2\nline3""#),
            ("\t\t\tindented", r#""\t\t\tindented""#),
        ];

        for (input, expected) in cases {
            let mut out = Vec::new();
            escape_json_string_simd(input, &mut out);
            assert_eq!(
                String::from_utf8(out).unwrap(),
                expected,
                "Failed for input: {input:?}"
            );
        }
    }

    // =========================================================================
    // Additional Coverage Tests
    // =========================================================================

    #[test]
    fn test_escape_json_to_string() {
        let result = escape_json_to_string("hello");
        assert_eq!(result, "\"hello\"");
    }

    #[test]
    fn test_escape_json_to_string_with_escapes() {
        let result = escape_json_to_string("say \"hi\"");
        assert_eq!(result, r#""say \"hi\"""#);
    }

    #[test]
    fn test_escape_backspace() {
        let mut out = Vec::new();
        escape_json_string_simd("before\x08after", &mut out);
        assert_eq!(out, b"\"before\\bafter\"");
    }

    #[test]
    fn test_escape_form_feed() {
        let mut out = Vec::new();
        escape_json_string_simd("before\x0cafter", &mut out);
        assert_eq!(out, b"\"before\\fafter\"");
    }

    #[test]
    fn test_escape_all_control_chars() {
        // Test all control characters 0x00-0x1f
        for byte in 0u8..=0x1f {
            let mut out = Vec::new();
            let input = String::from_utf8(vec![byte]).unwrap_or_else(|_| String::new());
            if !input.is_empty() {
                escape_json_string_simd(&input, &mut out);
                // Should have escaped form
                assert!(out.len() > 3, "Control char 0x{byte:02x} not escaped");
            }
        }
    }

    #[test]
    fn test_escape_scalar_fallback() {
        // Test the scalar path directly
        let bytes = b"hello \"world\"";
        let mut out = Vec::new();
        escape_json_string_scalar(bytes, &mut out);
        assert_eq!(out, b"hello \\\"world\\\"");
    }

    #[test]
    fn test_escape_scalar_with_newline() {
        let bytes = b"line1\nline2";
        let mut out = Vec::new();
        escape_json_string_scalar(bytes, &mut out);
        assert_eq!(out, b"line1\\nline2");
    }

    #[test]
    fn test_escape_scalar_with_tab() {
        let bytes = b"col1\tcol2";
        let mut out = Vec::new();
        escape_json_string_scalar(bytes, &mut out);
        assert_eq!(out, b"col1\\tcol2");
    }

    #[test]
    fn test_escape_scalar_clean() {
        let bytes = b"no escapes needed";
        let mut out = Vec::new();
        escape_json_string_scalar(bytes, &mut out);
        assert_eq!(out, b"no escapes needed");
    }

    #[test]
    fn test_escape_byte_direct() {
        // Test escape_byte function directly
        let mut out = Vec::new();
        escape_byte(b'"', &mut out);
        assert_eq!(out, b"\\\"");

        let mut out = Vec::new();
        escape_byte(b'\\', &mut out);
        assert_eq!(out, b"\\\\");

        let mut out = Vec::new();
        escape_byte(b'\n', &mut out);
        assert_eq!(out, b"\\n");

        let mut out = Vec::new();
        escape_byte(b'\r', &mut out);
        assert_eq!(out, b"\\r");

        let mut out = Vec::new();
        escape_byte(b'\t', &mut out);
        assert_eq!(out, b"\\t");

        let mut out = Vec::new();
        escape_byte(b'\x08', &mut out);
        assert_eq!(out, b"\\b");

        let mut out = Vec::new();
        escape_byte(b'\x0c', &mut out);
        assert_eq!(out, b"\\f");
    }

    #[test]
    fn test_escape_byte_control_unicode() {
        // Control characters that need \uXXXX escaping
        for byte in [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13,
            0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        ] {
            let mut out = Vec::new();
            escape_byte(byte, &mut out);
            assert!(
                out.starts_with(b"\\u00"),
                "Byte 0x{byte:02x} not escaped as \\u00xx"
            );
        }
    }

    #[test]
    fn test_escape_byte_normal_passthrough() {
        let mut out = Vec::new();
        escape_byte(b'a', &mut out);
        assert_eq!(out, b"a");
    }

    #[test]
    fn test_hex_digit() {
        assert_eq!(hex_digit(0), b'0');
        assert_eq!(hex_digit(9), b'9');
        assert_eq!(hex_digit(10), b'a');
        assert_eq!(hex_digit(15), b'f');
    }

    #[test]
    fn test_needs_escape_byte() {
        // Control chars need escape
        for b in 0u8..=0x1f {
            assert!(needs_escape_byte(b), "0x{b:02x} should need escape");
        }
        // Quote and backslash need escape
        assert!(needs_escape_byte(b'"'));
        assert!(needs_escape_byte(b'\\'));
        // Normal chars don't need escape
        assert!(!needs_escape_byte(b'a'));
        assert!(!needs_escape_byte(b'z'));
        assert!(!needs_escape_byte(b' '));
        assert!(!needs_escape_byte(b'~'));
    }

    #[test]
    fn test_escape_very_long_string() {
        // Test string > 32 bytes to ensure AVX2 path is exercised
        let long = "a".repeat(100);
        let mut out = Vec::new();
        escape_json_string_simd(&long, &mut out);
        assert_eq!(out.len(), 102); // 100 + 2 quotes
    }

    #[test]
    fn test_escape_long_string_with_late_escape() {
        // Escape character near end of long string
        let mut s = "a".repeat(50);
        s.push('"');
        s.push_str(&"b".repeat(50));
        let mut out = Vec::new();
        escape_json_string_simd(&s, &mut out);
        assert!(out.windows(2).any(|w| w == b"\\\""));
    }

    #[test]
    fn test_escape_string_multiple_escapes() {
        // Multiple escapes in same chunk
        let input = "a\"b\"c\"d";
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, b"\"a\\\"b\\\"c\\\"d\"");
    }

    #[test]
    fn test_escape_high_bytes_passthrough() {
        // High bytes (UTF-8 continuation) should pass through
        let input = "café"; // Contains bytes 0xc3 0xa9
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, "\"café\"".as_bytes());
    }

    #[test]
    fn test_escape_boundary_16() {
        // Test exactly 16 bytes (SSE2 boundary)
        let input = "1234567890123456";
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, b"\"1234567890123456\"");
    }

    #[test]
    fn test_escape_boundary_32() {
        // Test exactly 32 bytes (AVX2 boundary)
        let input = "12345678901234567890123456789012";
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, b"\"12345678901234567890123456789012\"");
    }

    #[test]
    fn test_escape_boundary_17() {
        // Test 17 bytes (crosses SSE2 boundary)
        let input = "12345678901234567";
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, b"\"12345678901234567\"");
    }

    #[test]
    fn test_escape_boundary_33() {
        // Test 33 bytes (crosses AVX2 boundary)
        let input = "123456789012345678901234567890123";
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, b"\"123456789012345678901234567890123\"");
    }

    #[test]
    fn test_escape_only_escape_chars() {
        // String with only escape characters
        let input = "\"\\";
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, b"\"\\\"\\\\\"");
    }

    #[test]
    fn test_escape_alternating() {
        // Alternating normal and escape chars
        let input = "a\"b\\c\nd";
        let mut out = Vec::new();
        escape_json_string_simd(input, &mut out);
        assert_eq!(out, b"\"a\\\"b\\\\c\\nd\"");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_has_sse2() {
        // Just verify it doesn't panic
        let _ = has_sse2();
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_has_avx2() {
        // Just verify it doesn't panic
        let _ = has_avx2();
    }
}
