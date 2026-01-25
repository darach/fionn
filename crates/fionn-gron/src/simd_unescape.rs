// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-accelerated string unescaping for ungron operations.
//!
//! This module provides optimized JSON string unescaping using SIMD instructions
//! to find backslash positions and bulk-copy clean segments. This is the inverse
//! operation of `simd_escape`.
//!
//! Supported escape sequences:
//! - `\"` → `"`
//! - `\\` → `\`
//! - `\/` → `/`
//! - `\n` → newline
//! - `\r` → carriage return
//! - `\t` → tab
//! - `\b` → backspace
//! - `\f` → form feed
//! - `\uXXXX` → Unicode codepoint

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    __m128i, __m256i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8,
    _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_set1_epi8,
};

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{uint8x16_t, vceqq_u8, vdupq_n_u8, vld1q_u8, vmaxvq_u8};

#[allow(unused_imports)] // OnceLock used conditionally per target_arch
use std::sync::OnceLock;

/// Error type for unescape operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnescapeError {
    /// Invalid escape sequence (e.g., `\x`)
    InvalidEscape(usize),
    /// Invalid unicode escape (e.g., `\uXXXX` with non-hex digits)
    InvalidUnicode(usize),
    /// Truncated escape at end of string
    TruncatedEscape,
    /// Invalid UTF-8 in result
    InvalidUtf8,
}

impl std::fmt::Display for UnescapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEscape(pos) => write!(f, "invalid escape sequence at position {pos}"),
            Self::InvalidUnicode(pos) => write!(f, "invalid unicode escape at position {pos}"),
            Self::TruncatedEscape => write!(f, "truncated escape sequence at end of string"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 in result"),
        }
    }
}

impl std::error::Error for UnescapeError {}

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

/// SIMD-accelerated JSON string unescaping.
///
/// Takes a JSON string value (without surrounding quotes) and returns
/// the unescaped bytes. Uses SIMD to find backslash positions and
/// bulk-copies clean segments.
///
/// # Errors
///
/// Returns an error if the string contains invalid escape sequences.
#[inline]
pub fn unescape_json_string_simd(s: &[u8]) -> Result<Vec<u8>, UnescapeError> {
    // Quick path: no backslashes means no escaping needed
    if !s.contains(&b'\\') {
        return Ok(s.to_vec());
    }

    let mut out = Vec::with_capacity(s.len());

    #[cfg(target_arch = "x86_64")]
    {
        // Prefer AVX2 for strings >= 32 bytes
        if s.len() >= 32 && has_avx2() {
            return unsafe { unescape_json_string_avx2(s, &mut out) }.map(|()| out);
        }
        if has_sse2() {
            return unsafe { unescape_json_string_sse2(s, &mut out) }.map(|()| out);
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // NEON is always available on aarch64
        return unsafe { unescape_json_string_neon(s, &mut out) }.map(|()| out);
    }

    // Fallback to scalar
    #[allow(unreachable_code)] // Reachable only when no SIMD available (non-x86/aarch64)
    {
        unescape_json_string_scalar(s, &mut out)?;
        Ok(out)
    }
}

/// SSE2-accelerated unescape implementation.
///
/// Strategy:
/// 1. Scan 16 bytes at a time looking for backslashes
/// 2. Bulk copy all clean bytes before the first backslash
/// 3. Decode the escape sequence
/// 4. Repeat from step 1
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn unescape_json_string_sse2(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), UnescapeError> {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Pre-compute SIMD constant for backslash
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let backslash_char = _mm_set1_epi8(b'\\' as i8);

        while i < len {
            // Find the next backslash
            let backslash_pos = find_next_backslash_sse2(bytes, i, backslash_char);

            if let Some(pos) = backslash_pos {
                // Bulk copy everything before the backslash
                if pos > i {
                    out.extend_from_slice(&bytes[i..pos]);
                }

                // Decode the escape sequence
                if pos + 1 >= len {
                    return Err(UnescapeError::TruncatedEscape);
                }

                let (consumed, decoded_bytes, decoded_len) = decode_escape(&bytes[pos..])?;
                out.extend_from_slice(&decoded_bytes[..decoded_len]);
                i = pos + consumed;
            } else {
                // No more backslashes, copy the rest
                out.extend_from_slice(&bytes[i..]);
                break;
            }
        }

        Ok(())
    }
}

/// Find the next backslash using SSE2.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn find_next_backslash_sse2(
    bytes: &[u8],
    start: usize,
    backslash_char: __m128i,
) -> Option<usize> {
    unsafe {
        let len = bytes.len();
        let mut i = start;

        // Process 16 bytes at a time
        while i + 16 <= len {
            let chunk = _mm_loadu_si128(bytes[i..].as_ptr().cast());
            let cmp = _mm_cmpeq_epi8(chunk, backslash_char);
            let mask = _mm_movemask_epi8(cmp);

            if mask != 0 {
                let offset = mask.trailing_zeros() as usize;
                return Some(i + offset);
            }

            i += 16;
        }

        // Check remaining bytes with scalar
        for (offset, &byte) in bytes[i..].iter().enumerate() {
            if byte == b'\\' {
                return Some(i + offset);
            }
        }

        None
    }
}

/// AVX2-accelerated unescape implementation (32 bytes at a time).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn unescape_json_string_avx2(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), UnescapeError> {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Pre-compute SIMD constant for backslash
        #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
        let backslash_char = _mm256_set1_epi8(b'\\' as i8);

        while i < len {
            // Find the next backslash
            let backslash_pos = find_next_backslash_avx2(bytes, i, backslash_char);

            if let Some(pos) = backslash_pos {
                // Bulk copy everything before the backslash
                if pos > i {
                    out.extend_from_slice(&bytes[i..pos]);
                }

                // Decode the escape sequence
                if pos + 1 >= len {
                    return Err(UnescapeError::TruncatedEscape);
                }

                let (consumed, decoded_bytes, decoded_len) = decode_escape(&bytes[pos..])?;
                out.extend_from_slice(&decoded_bytes[..decoded_len]);
                i = pos + consumed;
            } else {
                // No more backslashes, copy the rest
                out.extend_from_slice(&bytes[i..]);
                break;
            }
        }

        Ok(())
    }
}

/// Find the next backslash using AVX2.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn find_next_backslash_avx2(
    bytes: &[u8],
    start: usize,
    backslash_char: __m256i,
) -> Option<usize> {
    unsafe {
        let len = bytes.len();
        let mut i = start;

        // Process 32 bytes at a time
        while i + 32 <= len {
            let chunk = _mm256_loadu_si256(bytes[i..].as_ptr().cast());
            let cmp = _mm256_cmpeq_epi8(chunk, backslash_char);
            let mask = _mm256_movemask_epi8(cmp);

            if mask != 0 {
                let offset = mask.trailing_zeros() as usize;
                return Some(i + offset);
            }

            i += 32;
        }

        // Handle remaining bytes with SSE2 or scalar
        if i + 16 <= len && has_sse2() {
            #[allow(clippy::cast_possible_wrap)] // Safe: ASCII byte fits in i8
            let sse_backslash = _mm_set1_epi8(b'\\' as i8);
            return find_next_backslash_sse2(bytes, i, sse_backslash);
        }

        // Check remaining bytes with scalar
        for (offset, &byte) in bytes[i..].iter().enumerate() {
            if byte == b'\\' {
                return Some(i + offset);
            }
        }

        None
    }
}

/// NEON-accelerated unescape implementation (16 bytes at a time).
#[cfg(target_arch = "aarch64")]
unsafe fn unescape_json_string_neon(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), UnescapeError> {
    unsafe {
        let len = bytes.len();
        let mut i = 0;

        // Pre-compute NEON constant for backslash
        let backslash_char = vdupq_n_u8(b'\\');

        while i < len {
            // Find the next backslash
            let backslash_pos = find_next_backslash_neon(bytes, i, backslash_char);

            if let Some(pos) = backslash_pos {
                // Bulk copy everything before the backslash
                if pos > i {
                    out.extend_from_slice(&bytes[i..pos]);
                }

                // Decode the escape sequence
                if pos + 1 >= len {
                    return Err(UnescapeError::TruncatedEscape);
                }

                let (consumed, decoded_bytes, decoded_len) = decode_escape(&bytes[pos..])?;
                out.extend_from_slice(&decoded_bytes[..decoded_len]);
                i = pos + consumed;
            } else {
                // No more backslashes, copy the rest
                out.extend_from_slice(&bytes[i..]);
                break;
            }
        }

        Ok(())
    }
}

/// Find the next backslash using NEON.
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn find_next_backslash_neon(
    bytes: &[u8],
    start: usize,
    backslash_char: uint8x16_t,
) -> Option<usize> {
    unsafe {
        let len = bytes.len();
        let mut i = start;

        // Process 16 bytes at a time
        while i + 16 <= len {
            let chunk = vld1q_u8(bytes[i..].as_ptr());
            let cmp = vceqq_u8(chunk, backslash_char);

            // Check if any byte matched
            let max = vmaxvq_u8(cmp);
            if max != 0 {
                // Found a backslash - find which one
                let arr: [u8; 16] = std::mem::transmute(cmp);
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
            if byte == b'\\' {
                return Some(i + offset);
            }
        }

        None
    }
}

/// Decoded escape result: (`bytes_consumed`, `decoded_bytes`, `decoded_len`)
type DecodeResult = (usize, [u8; 4], usize);

/// Decode a single escape sequence starting with backslash.
///
/// Returns (`bytes_consumed`, `decoded_bytes`, `decoded_byte_count`).
#[inline]
fn decode_escape(bytes: &[u8]) -> Result<DecodeResult, UnescapeError> {
    debug_assert!(bytes[0] == b'\\');

    if bytes.len() < 2 {
        return Err(UnescapeError::TruncatedEscape);
    }

    let pos = 0; // Position in original string for error reporting
    match bytes[1] {
        b'"' => Ok((2, [b'"', 0, 0, 0], 1)),
        b'\\' => Ok((2, [b'\\', 0, 0, 0], 1)),
        b'/' => Ok((2, [b'/', 0, 0, 0], 1)),
        b'n' => Ok((2, [b'\n', 0, 0, 0], 1)),
        b'r' => Ok((2, [b'\r', 0, 0, 0], 1)),
        b't' => Ok((2, [b'\t', 0, 0, 0], 1)),
        b'b' => Ok((2, [b'\x08', 0, 0, 0], 1)), // backspace
        b'f' => Ok((2, [b'\x0c', 0, 0, 0], 1)), // form feed
        b'u' => decode_unicode_escape(bytes, pos),
        _ => Err(UnescapeError::InvalidEscape(pos)),
    }
}

/// Decode a `\uXXXX` unicode escape sequence.
///
/// Returns (`bytes_consumed`, `utf8_bytes`, `utf8_len`).
fn decode_unicode_escape(bytes: &[u8], pos: usize) -> Result<DecodeResult, UnescapeError> {
    if bytes.len() < 6 {
        return Err(UnescapeError::TruncatedEscape);
    }

    // Parse the 4 hex digits
    let hex_str = &bytes[2..6];
    let codepoint = parse_hex4(hex_str).ok_or(UnescapeError::InvalidUnicode(pos))?;

    // Check for surrogate pair (high surrogate: 0xD800-0xDBFF)
    if (0xD800..=0xDBFF).contains(&codepoint) {
        // Need to look for low surrogate
        if bytes.len() >= 12 && bytes[6] == b'\\' && bytes[7] == b'u' {
            let low_hex = &bytes[8..12];
            if let Some(low_codepoint) = parse_hex4(low_hex) {
                // Low surrogate: 0xDC00-0xDFFF
                if (0xDC00..=0xDFFF).contains(&low_codepoint) {
                    // Decode surrogate pair
                    let high = u32::from(codepoint);
                    let low = u32::from(low_codepoint);
                    let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);

                    if let Some(ch) = char::from_u32(combined) {
                        let mut utf8_bytes = [0u8; 4];
                        let encoded = ch.encode_utf8(&mut utf8_bytes);
                        let utf8_len = encoded.len();
                        // Return 12 bytes consumed (2 x \uXXXX)
                        return Ok((12, utf8_bytes, utf8_len));
                    }
                }
            }
        }
        // Invalid or unpaired surrogate - return replacement character
        return Ok((6, [0xEF, 0xBF, 0xBD, 0], 3)); // U+FFFD
    }

    // Low surrogate without high surrogate - replacement character
    if (0xDC00..=0xDFFF).contains(&codepoint) {
        return Ok((6, [0xEF, 0xBF, 0xBD, 0], 3)); // U+FFFD
    }

    // Normal BMP character
    char::from_u32(u32::from(codepoint)).map_or_else(
        // Invalid codepoint - replacement character
        || Ok((6, [0xEF, 0xBF, 0xBD, 0], 3)), // U+FFFD
        |ch| {
            let mut utf8_bytes = [0u8; 4];
            let encoded = ch.encode_utf8(&mut utf8_bytes);
            let utf8_len = encoded.len();
            Ok((6, utf8_bytes, utf8_len))
        },
    )
}

/// Parse 4 hex digits into a u16.
#[inline]
fn parse_hex4(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 4 {
        return None;
    }

    let mut result: u16 = 0;
    for &byte in &bytes[0..4] {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        result = result << 4 | u16::from(digit);
    }
    Some(result)
}

/// Scalar fallback for string unescaping.
fn unescape_json_string_scalar(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), UnescapeError> {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let (consumed, decoded_bytes, decoded_len) = decode_escape(&bytes[i..])?;
            out.extend_from_slice(&decoded_bytes[..decoded_len]);
            i += consumed;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(())
}

/// Unescape a JSON string, returning a new String.
///
/// This is a convenience wrapper around `unescape_json_string_simd`.
///
/// # Errors
///
/// Returns an error if the string contains invalid escape sequences or
/// the result is not valid UTF-8.
#[inline]
pub fn unescape_json_to_string(s: &str) -> Result<String, UnescapeError> {
    let bytes = unescape_json_string_simd(s.as_bytes())?;
    String::from_utf8(bytes).map_err(|_| UnescapeError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_simple() {
        let result = unescape_json_string_simd(b"hello").unwrap();
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_unescape_quotes() {
        let result = unescape_json_string_simd(br#"say \"hello\""#).unwrap();
        assert_eq!(result, br#"say "hello""#);
    }

    #[test]
    fn test_unescape_backslash() {
        let result = unescape_json_string_simd(br"path\\to\\file").unwrap();
        assert_eq!(result, br"path\to\file");
    }

    #[test]
    fn test_unescape_newline() {
        let result = unescape_json_string_simd(b"line1\\nline2").unwrap();
        assert_eq!(result, b"line1\nline2");
    }

    #[test]
    fn test_unescape_tab() {
        let result = unescape_json_string_simd(b"col1\\tcol2").unwrap();
        assert_eq!(result, b"col1\tcol2");
    }

    #[test]
    fn test_unescape_carriage_return() {
        let result = unescape_json_string_simd(b"line1\\rline2").unwrap();
        assert_eq!(result, b"line1\rline2");
    }

    #[test]
    fn test_unescape_backspace() {
        let result = unescape_json_string_simd(b"text\\bmore").unwrap();
        assert_eq!(result, b"text\x08more");
    }

    #[test]
    fn test_unescape_formfeed() {
        let result = unescape_json_string_simd(b"text\\fmore").unwrap();
        assert_eq!(result, b"text\x0cmore");
    }

    #[test]
    fn test_unescape_slash() {
        let result = unescape_json_string_simd(b"path\\/to\\/file").unwrap();
        assert_eq!(result, b"path/to/file");
    }

    #[test]
    fn test_unescape_unicode_basic() {
        // \u0041 = 'A'
        let result = unescape_json_string_simd(b"\\u0041").unwrap();
        assert_eq!(result, b"A");
    }

    #[test]
    fn test_unescape_unicode_multibyte() {
        // \u4e16 = '世' (requires 3 UTF-8 bytes)
        let result = unescape_json_string_simd(b"\\u4e16").unwrap();
        assert_eq!(result, "世".as_bytes());
    }

    #[test]
    fn test_unescape_unicode_surrogate_pair() {
        // \uD83D\uDE00 = '😀' (requires surrogate pair)
        let result = unescape_json_string_simd(b"\\uD83D\\uDE00").unwrap();
        assert_eq!(result, "😀".as_bytes());
    }

    #[test]
    fn test_unescape_empty() {
        let result = unescape_json_string_simd(b"").unwrap();
        assert_eq!(result, b"");
    }

    #[test]
    fn test_unescape_long_clean_string() {
        let clean = b"a".repeat(100);
        let result = unescape_json_string_simd(&clean).unwrap();
        assert_eq!(result, clean);
    }

    #[test]
    fn test_unescape_long_with_escapes() {
        // Long string with escapes scattered throughout
        let input = format!(
            "{}\\\"{}\\\\{}\\n{}",
            "a".repeat(20),
            "b".repeat(20),
            "c".repeat(20),
            "d".repeat(20)
        );
        let result = unescape_json_string_simd(input.as_bytes()).unwrap();

        // Verify unescaped result
        let expected = format!(
            "{}\"{}\\{}\n{}",
            "a".repeat(20),
            "b".repeat(20),
            "c".repeat(20),
            "d".repeat(20)
        );
        assert_eq!(result, expected.as_bytes());
    }

    #[test]
    fn test_unescape_invalid_escape() {
        let result = unescape_json_string_simd(b"test\\xinvalid");
        assert!(matches!(result, Err(UnescapeError::InvalidEscape(_))));
    }

    #[test]
    fn test_unescape_truncated() {
        let result = unescape_json_string_simd(b"test\\");
        assert!(matches!(result, Err(UnescapeError::TruncatedEscape)));
    }

    #[test]
    fn test_unescape_invalid_unicode() {
        let result = unescape_json_string_simd(b"test\\uXXXX");
        assert!(matches!(result, Err(UnescapeError::InvalidUnicode(_))));
    }

    #[test]
    fn test_unescape_truncated_unicode() {
        let result = unescape_json_string_simd(b"test\\u00");
        assert!(matches!(result, Err(UnescapeError::TruncatedEscape)));
    }

    #[test]
    fn test_roundtrip() {
        use crate::simd_escape::escape_json_string_simd;

        let test_cases = [
            "hello world",
            "line1\nline2",
            "tab\there",
            r#"quote "here" and there"#,
            r"backslash\here",
            "mixed\n\t\"escape\\test",
            "unicode: 世界 🎉",
        ];

        for original in test_cases {
            // Escape
            let mut escaped = Vec::new();
            escape_json_string_simd(original, &mut escaped);

            // Remove surrounding quotes for unescape
            let escaped_inner = &escaped[1..escaped.len() - 1];

            // Unescape
            let unescaped = unescape_json_string_simd(escaped_inner).unwrap();

            // Verify roundtrip
            assert_eq!(
                unescaped,
                original.as_bytes(),
                "Roundtrip failed for: {original:?}"
            );
        }
    }

    #[test]
    fn test_parse_hex4() {
        assert_eq!(parse_hex4(b"0000"), Some(0x0000));
        assert_eq!(parse_hex4(b"0041"), Some(0x0041));
        assert_eq!(parse_hex4(b"FFFF"), Some(0xFFFF));
        assert_eq!(parse_hex4(b"abcd"), Some(0xABCD));
        assert_eq!(parse_hex4(b"AbCd"), Some(0xABCD));
        assert_eq!(parse_hex4(b"XXXX"), None);
        assert_eq!(parse_hex4(b"00"), None); // Too short
    }

    // =========================================================================
    // Error Display Tests
    // =========================================================================

    #[test]
    fn test_unescape_error_display_invalid_escape() {
        let err = UnescapeError::InvalidEscape(42);
        let msg = err.to_string();
        assert!(msg.contains("invalid escape"));
        assert!(msg.contains("42"));
    }

    #[test]
    fn test_unescape_error_display_invalid_unicode() {
        let err = UnescapeError::InvalidUnicode(10);
        let msg = err.to_string();
        assert!(msg.contains("invalid unicode"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn test_unescape_error_display_truncated() {
        let err = UnescapeError::TruncatedEscape;
        let msg = err.to_string();
        assert!(msg.contains("truncated"));
    }

    #[test]
    fn test_unescape_error_display_invalid_utf8() {
        let err = UnescapeError::InvalidUtf8;
        let msg = err.to_string();
        assert!(msg.contains("invalid UTF-8"));
    }

    #[test]
    fn test_unescape_error_clone_eq() {
        let err1 = UnescapeError::InvalidEscape(5);
        let err2 = err1.clone();
        assert_eq!(err1, err2);

        let err3 = UnescapeError::InvalidUnicode(5);
        assert_ne!(err1, err3);
    }

    #[test]
    fn test_unescape_error_debug() {
        let err = UnescapeError::TruncatedEscape;
        let debug = format!("{err:?}");
        assert!(debug.contains("TruncatedEscape"));
    }

    #[test]
    fn test_unescape_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(UnescapeError::InvalidUtf8);
        assert!(!err.to_string().is_empty());
    }

    // =========================================================================
    // unescape_json_to_string Tests
    // =========================================================================

    #[test]
    fn test_unescape_to_string_basic() {
        let result = unescape_json_to_string("hello world").unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_unescape_to_string_with_escapes() {
        let result = unescape_json_to_string(r#"say \"hello\""#).unwrap();
        assert_eq!(result, r#"say "hello""#);
    }

    #[test]
    fn test_unescape_to_string_unicode() {
        let result = unescape_json_to_string(r"\u4e16\u754c").unwrap();
        assert_eq!(result, "世界");
    }

    #[test]
    fn test_unescape_to_string_invalid_escape() {
        let result = unescape_json_to_string(r"test\x");
        assert!(result.is_err());
    }

    // =========================================================================
    // Unicode Surrogate Pair Tests
    // =========================================================================

    #[test]
    fn test_unescape_high_surrogate_alone() {
        // High surrogate without low surrogate should produce replacement char
        let result = unescape_json_string_simd(b"\\uD83D").unwrap();
        assert_eq!(result, [0xEF, 0xBF, 0xBD]); // U+FFFD replacement character
    }

    #[test]
    fn test_unescape_low_surrogate_alone() {
        // Low surrogate without high surrogate should produce replacement char
        let result = unescape_json_string_simd(b"\\uDE00").unwrap();
        assert_eq!(result, [0xEF, 0xBF, 0xBD]); // U+FFFD replacement character
    }

    #[test]
    fn test_unescape_invalid_surrogate_sequence() {
        // High surrogate followed by non-low-surrogate
        let result = unescape_json_string_simd(b"\\uD83D\\u0041").unwrap();
        // First produces replacement char, second produces 'A'
        let mut expected = vec![0xEF, 0xBF, 0xBD]; // U+FFFD
        expected.push(b'A');
        assert_eq!(result, expected);
    }

    #[test]
    fn test_unescape_high_surrogate_at_end() {
        // High surrogate at end with no more data for low surrogate
        let result = unescape_json_string_simd(b"text\\uD83D").unwrap();
        let mut expected = b"text".to_vec();
        expected.extend_from_slice(&[0xEF, 0xBF, 0xBD]); // U+FFFD
        assert_eq!(result, expected);
    }

    #[test]
    fn test_unescape_high_surrogate_followed_by_invalid() {
        // High surrogate followed by something that's not \uXXXX
        let result = unescape_json_string_simd(b"\\uD83Dabc").unwrap();
        let mut expected = vec![0xEF, 0xBF, 0xBD]; // U+FFFD
        expected.extend_from_slice(b"abc");
        assert_eq!(result, expected);
    }

    #[test]
    fn test_unescape_multiple_surrogates() {
        // Multiple valid surrogate pairs
        let result = unescape_json_string_simd(b"\\uD83D\\uDE00\\uD83D\\uDE01").unwrap();
        let expected = "😀😁".as_bytes();
        assert_eq!(result, expected);
    }

    // =========================================================================
    // SIMD Path Coverage Tests
    // =========================================================================

    #[test]
    fn test_unescape_16_byte_string_with_escape() {
        // SSE2 path - exactly 16 bytes with escape
        let input = b"abcdefghij\\nklmn"; // 16 chars
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, b"abcdefghij\nklmn");
    }

    #[test]
    fn test_unescape_32_byte_string_with_escape() {
        // AVX2 path - exactly 32 bytes with escape
        let input = b"0123456789abcdef\\n0123456789abcde"; // 32 chars
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, b"0123456789abcdef\n0123456789abcde");
    }

    #[test]
    fn test_unescape_48_byte_string() {
        // AVX2 path - 48 bytes with escape near end
        let input = "0123456789".repeat(4) + "\\nabcdefg";
        let result = unescape_json_string_simd(input.as_bytes()).unwrap();
        let expected = "0123456789".repeat(4) + "\nabcdefg";
        assert_eq!(result, expected.as_bytes());
    }

    #[test]
    fn test_unescape_escape_at_boundary() {
        // Escape at SIMD chunk boundary
        let input = "a".repeat(15) + "\\n" + &"b".repeat(16);
        let result = unescape_json_string_simd(input.as_bytes()).unwrap();
        let expected = "a".repeat(15) + "\n" + &"b".repeat(16);
        assert_eq!(result, expected.as_bytes());
    }

    #[test]
    fn test_unescape_escape_at_32_boundary() {
        // Escape at AVX2 chunk boundary
        let input = "a".repeat(31) + "\\n" + &"b".repeat(32);
        let result = unescape_json_string_simd(input.as_bytes()).unwrap();
        let expected = "a".repeat(31) + "\n" + &"b".repeat(32);
        assert_eq!(result, expected.as_bytes());
    }

    #[test]
    fn test_unescape_multiple_escapes_in_chunk() {
        // Multiple escapes within a single SIMD chunk
        let input = b"a\\nb\\tc\\rd\\\"e";
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, b"a\nb\tc\rd\"e");
    }

    #[test]
    fn test_unescape_backslash_at_very_end() {
        // Backslash at the very end of input
        let input = "a".repeat(33) + "\\";
        let result = unescape_json_string_simd(input.as_bytes());
        assert!(matches!(result, Err(UnescapeError::TruncatedEscape)));
    }

    #[test]
    fn test_unescape_long_string_no_escapes() {
        // Long string with no escapes - quick path
        let input = "x".repeat(1000);
        let result = unescape_json_string_simd(input.as_bytes()).unwrap();
        assert_eq!(result, input.as_bytes());
    }

    #[test]
    fn test_unescape_many_escapes() {
        // Many consecutive escapes
        let input = b"\\n\\n\\n\\n\\n\\n\\n\\n\\n\\n";
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, b"\n\n\n\n\n\n\n\n\n\n");
    }

    // =========================================================================
    // Scalar Fallback Tests
    // =========================================================================

    #[test]
    fn test_scalar_unescape() {
        let mut out = Vec::new();
        unescape_json_string_scalar(b"test\\nvalue", &mut out).unwrap();
        assert_eq!(out, b"test\nvalue");
    }

    #[test]
    fn test_scalar_unescape_all_types() {
        let mut out = Vec::new();
        unescape_json_string_scalar(b"\\\"\\\\\\n\\r\\t\\b\\f\\/", &mut out).unwrap();
        assert_eq!(out, b"\"\\\n\r\t\x08\x0c/");
    }

    #[test]
    fn test_scalar_unescape_unicode() {
        let mut out = Vec::new();
        unescape_json_string_scalar(b"\\u0041\\u0042", &mut out).unwrap();
        assert_eq!(out, b"AB");
    }

    #[test]
    fn test_scalar_unescape_error() {
        let mut out = Vec::new();
        let result = unescape_json_string_scalar(b"test\\z", &mut out);
        assert!(result.is_err());
    }

    // =========================================================================
    // decode_escape Direct Tests
    // =========================================================================

    #[test]
    fn test_decode_escape_all_simple() {
        assert_eq!(decode_escape(b"\\\"").unwrap(), (2, [b'"', 0, 0, 0], 1));
        assert_eq!(decode_escape(b"\\\\").unwrap(), (2, [b'\\', 0, 0, 0], 1));
        assert_eq!(decode_escape(b"\\/").unwrap(), (2, [b'/', 0, 0, 0], 1));
        assert_eq!(decode_escape(b"\\n").unwrap(), (2, [b'\n', 0, 0, 0], 1));
        assert_eq!(decode_escape(b"\\r").unwrap(), (2, [b'\r', 0, 0, 0], 1));
        assert_eq!(decode_escape(b"\\t").unwrap(), (2, [b'\t', 0, 0, 0], 1));
        assert_eq!(decode_escape(b"\\b").unwrap(), (2, [8, 0, 0, 0], 1));
        assert_eq!(decode_escape(b"\\f").unwrap(), (2, [12, 0, 0, 0], 1));
    }

    #[test]
    fn test_decode_escape_truncated_single() {
        let result = decode_escape(b"\\");
        assert!(matches!(result, Err(UnescapeError::TruncatedEscape)));
    }

    #[test]
    fn test_decode_escape_invalid_char() {
        let result = decode_escape(b"\\z");
        assert!(matches!(result, Err(UnescapeError::InvalidEscape(_))));
    }

    // =========================================================================
    // Unicode Edge Cases
    // =========================================================================

    #[test]
    fn test_unescape_unicode_null() {
        // \u0000 - null character
        let result = unescape_json_string_simd(b"\\u0000").unwrap();
        assert_eq!(result, &[0u8]);
    }

    #[test]
    fn test_unescape_unicode_max_bmp() {
        // \uFFFF - max BMP character
        let result = unescape_json_string_simd(b"\\uFFFF").unwrap();
        // This is a valid Unicode codepoint
        assert!(!result.is_empty());
    }

    #[test]
    fn test_unescape_unicode_two_byte() {
        // \u00E9 = é (requires 2 UTF-8 bytes)
        let result = unescape_json_string_simd(b"\\u00E9").unwrap();
        assert_eq!(result, "é".as_bytes());
    }

    #[test]
    fn test_unescape_unicode_lowercase() {
        // Lowercase hex digits
        let result = unescape_json_string_simd(b"\\u00e9").unwrap();
        assert_eq!(result, "é".as_bytes());
    }

    #[test]
    fn test_unescape_unicode_mixed_case() {
        // Mixed case hex digits
        let result = unescape_json_string_simd(b"\\u00E9\\u00eA\\u00Eb").unwrap();
        assert_eq!(result, "éêë".as_bytes());
    }

    #[test]
    fn test_parse_hex4_edge_cases() {
        assert_eq!(parse_hex4(b"D83D"), Some(0xD83D));
        assert_eq!(parse_hex4(b"de00"), Some(0xDE00));
        assert_eq!(parse_hex4(b""), None);
        assert_eq!(parse_hex4(b"G000"), None); // Invalid hex
        assert_eq!(parse_hex4(b"000G"), None); // Invalid at end
    }

    // =========================================================================
    // Mixed Content Tests
    // =========================================================================

    #[test]
    fn test_unescape_mixed_ascii_and_unicode() {
        let input = b"Hello \\u4e16\\u754c World";
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, "Hello 世界 World".as_bytes());
    }

    #[test]
    fn test_unescape_consecutive_unicode() {
        let input = b"\\u0041\\u0042\\u0043\\u0044";
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, b"ABCD");
    }

    #[test]
    fn test_unescape_escape_then_unicode() {
        let input = b"\\n\\u0041";
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, b"\nA");
    }

    #[test]
    fn test_unescape_unicode_then_escape() {
        let input = b"\\u0041\\n";
        let result = unescape_json_string_simd(input).unwrap();
        assert_eq!(result, b"A\n");
    }
}
