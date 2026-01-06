// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-accelerated comparison utilities for JSON diff operations.
//!
//! Provides fast byte-level comparison for detecting differences between
//! JSON values without full parsing.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm256_cmpeq_epi8, _mm256_loadu_si256,
    _mm256_movemask_epi8,
};

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{vceqq_u8, vld1q_u8, vminvq_u8};

#[cfg(target_arch = "x86_64")]
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

/// SIMD-accelerated byte slice equality check.
///
/// Returns `true` if both slices are equal, `false` otherwise.
/// Uses SIMD to compare 16-32 bytes at a time.
#[inline]
#[must_use]
pub fn simd_bytes_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    if a.is_empty() {
        return true;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if a.len() >= 32 && has_avx2() {
            return unsafe { simd_bytes_equal_avx2(a, b) };
        }
        if a.len() >= 16 && has_sse2() {
            return unsafe { simd_bytes_equal_sse2(a, b) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if a.len() >= 16 {
            return unsafe { simd_bytes_equal_neon(a, b) };
        }
    }

    // Scalar fallback
    a == b
}

/// Find the first position where two byte slices differ.
///
/// Returns `None` if the slices are equal, or `Some(index)` of the first difference.
/// If slices have different lengths, returns the length of the shorter slice
/// if the common prefix is equal.
#[inline]
#[must_use]
pub fn simd_find_first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    let min_len = a.len().min(b.len());

    if min_len == 0 {
        return if a.len() == b.len() { None } else { Some(0) };
    }

    #[cfg(target_arch = "x86_64")]
    {
        if min_len >= 32 && has_avx2() {
            let diff = unsafe { find_first_difference_avx2(a, b, min_len) };
            if let Some(pos) = diff {
                return Some(pos);
            }
            // Check if lengths differ
            return if a.len() == b.len() {
                None
            } else {
                Some(min_len)
            };
        }
        if min_len >= 16 && has_sse2() {
            let diff = unsafe { find_first_difference_sse2(a, b, min_len) };
            if let Some(pos) = diff {
                return Some(pos);
            }
            return if a.len() == b.len() {
                None
            } else {
                Some(min_len)
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if min_len >= 16 {
            let diff = unsafe { find_first_difference_neon(a, b, min_len) };
            if let Some(pos) = diff {
                return Some(pos);
            }
            return if a.len() == b.len() {
                None
            } else {
                Some(min_len)
            };
        }
    }

    // Scalar fallback
    for (i, (&byte_a, &byte_b)) in a.iter().zip(b.iter()).enumerate() {
        if byte_a != byte_b {
            return Some(i);
        }
    }

    if a.len() == b.len() {
        None
    } else {
        Some(min_len)
    }
}

/// SSE2 implementation of byte equality.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn simd_bytes_equal_sse2(a: &[u8], b: &[u8]) -> bool {
    unsafe {
        let len = a.len();
        let mut i = 0;

        // Process 16 bytes at a time
        while i + 16 <= len {
            let chunk_a = _mm_loadu_si128(a[i..].as_ptr().cast());
            let chunk_b = _mm_loadu_si128(b[i..].as_ptr().cast());
            let cmp = _mm_cmpeq_epi8(chunk_a, chunk_b);
            let mask = _mm_movemask_epi8(cmp);

            // All 16 bytes must be equal (mask = 0xFFFF)
            if mask != 0xFFFF {
                return false;
            }

            i += 16;
        }

        // Check remaining bytes
        a[i..] == b[i..]
    }
}

/// AVX2 implementation of byte equality.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_bytes_equal_avx2(a: &[u8], b: &[u8]) -> bool {
    unsafe {
        let len = a.len();
        let mut i = 0;

        // Process 32 bytes at a time
        while i + 32 <= len {
            let chunk_a = _mm256_loadu_si256(a[i..].as_ptr().cast());
            let chunk_b = _mm256_loadu_si256(b[i..].as_ptr().cast());
            let cmp = _mm256_cmpeq_epi8(chunk_a, chunk_b);
            let mask = _mm256_movemask_epi8(cmp);

            // All 32 bytes must be equal (mask = -1 as i32, or 0xFFFFFFFF)
            if mask != -1 {
                return false;
            }

            i += 32;
        }

        // Handle remaining with SSE2 or scalar
        if i + 16 <= len && has_sse2() {
            let chunk_a = _mm_loadu_si128(a[i..].as_ptr().cast());
            let chunk_b = _mm_loadu_si128(b[i..].as_ptr().cast());
            let cmp = _mm_cmpeq_epi8(chunk_a, chunk_b);
            let mask = _mm_movemask_epi8(cmp);

            if mask != 0xFFFF {
                return false;
            }
            i += 16;
        }

        // Check remaining bytes
        a[i..] == b[i..]
    }
}

/// NEON implementation of byte equality.
#[cfg(target_arch = "aarch64")]
unsafe fn simd_bytes_equal_neon(a: &[u8], b: &[u8]) -> bool {
    unsafe {
        let len = a.len();
        let mut i = 0;

        // Process 16 bytes at a time
        while i + 16 <= len {
            let chunk_a = vld1q_u8(a[i..].as_ptr());
            let chunk_b = vld1q_u8(b[i..].as_ptr());
            let cmp = vceqq_u8(chunk_a, chunk_b);

            // Check if all bytes are equal (all comparison results are 0xFF)
            let min_val = vminvq_u8(cmp);
            if min_val != 0xFF {
                return false;
            }

            i += 16;
        }

        // Check remaining bytes
        a[i..] == b[i..]
    }
}

/// SSE2 implementation of finding first difference.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn find_first_difference_sse2(a: &[u8], b: &[u8], min_len: usize) -> Option<usize> {
    unsafe {
        let mut i = 0;

        while i + 16 <= min_len {
            let chunk_a = _mm_loadu_si128(a[i..].as_ptr().cast());
            let chunk_b = _mm_loadu_si128(b[i..].as_ptr().cast());
            let cmp = _mm_cmpeq_epi8(chunk_a, chunk_b);
            let mask = _mm_movemask_epi8(cmp);

            if mask != 0xFFFF {
                // Found a difference - find which byte
                let diff_mask = !mask & 0xFFFF;
                let offset = diff_mask.trailing_zeros() as usize;
                return Some(i + offset);
            }

            i += 16;
        }

        // Check remaining bytes
        for j in i..min_len {
            if a[j] != b[j] {
                return Some(j);
            }
        }

        None
    }
}

/// AVX2 implementation of finding first difference.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_first_difference_avx2(a: &[u8], b: &[u8], min_len: usize) -> Option<usize> {
    unsafe {
        let mut i = 0;

        while i + 32 <= min_len {
            let chunk_a = _mm256_loadu_si256(a[i..].as_ptr().cast());
            let chunk_b = _mm256_loadu_si256(b[i..].as_ptr().cast());
            let cmp = _mm256_cmpeq_epi8(chunk_a, chunk_b);
            let mask = _mm256_movemask_epi8(cmp);

            if mask != -1 {
                // Found a difference - find which byte
                let diff_mask = (!mask).cast_unsigned();
                let offset = diff_mask.trailing_zeros() as usize;
                return Some(i + offset);
            }

            i += 32;
        }

        // Handle remaining with SSE2 or scalar
        if i + 16 <= min_len && has_sse2() {
            let chunk_a = _mm_loadu_si128(a[i..].as_ptr().cast());
            let chunk_b = _mm_loadu_si128(b[i..].as_ptr().cast());
            let cmp = _mm_cmpeq_epi8(chunk_a, chunk_b);
            let mask = _mm_movemask_epi8(cmp);

            if mask != 0xFFFF {
                let diff_mask = !mask & 0xFFFF;
                let offset = diff_mask.trailing_zeros() as usize;
                return Some(i + offset);
            }
            i += 16;
        }

        // Check remaining bytes
        for j in i..min_len {
            if a[j] != b[j] {
                return Some(j);
            }
        }

        None
    }
}

/// NEON implementation of finding first difference.
#[cfg(target_arch = "aarch64")]
unsafe fn find_first_difference_neon(a: &[u8], b: &[u8], min_len: usize) -> Option<usize> {
    unsafe {
        let mut i = 0;

        while i + 16 <= min_len {
            let chunk_a = vld1q_u8(a[i..].as_ptr());
            let chunk_b = vld1q_u8(b[i..].as_ptr());
            let cmp = vceqq_u8(chunk_a, chunk_b);

            // Check if all bytes are equal
            let min_val = vminvq_u8(cmp);
            if min_val != 0xFF {
                // Found a difference - find which byte
                let arr: [u8; 16] = std::mem::transmute(cmp);
                for (offset, &val) in arr.iter().enumerate() {
                    if val != 0xFF {
                        return Some(i + offset);
                    }
                }
            }

            i += 16;
        }

        // Check remaining bytes
        for j in i..min_len {
            if a[j] != b[j] {
                return Some(j);
            }
        }

        None
    }
}

/// Compare two JSON string values for equality.
///
/// This is optimized for JSON strings which are typically short.
/// Uses SIMD for strings >= 16 bytes.
#[inline]
#[must_use]
pub fn json_strings_equal(a: &str, b: &str) -> bool {
    simd_bytes_equal(a.as_bytes(), b.as_bytes())
}

/// Compare two JSON number representations for equality.
///
/// Note: This does byte comparison, so "1.0" != "1" even though
/// they represent the same value. For semantic equality, parse first.
#[inline]
#[must_use]
pub fn json_numbers_equal(a: &str, b: &str) -> bool {
    // Numbers are typically short, use direct comparison
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // simd_bytes_equal Tests
    // =========================================================================

    #[test]
    fn test_simd_bytes_equal_same() {
        let a = b"hello world";
        let b = b"hello world";
        assert!(simd_bytes_equal(a, b));
    }

    #[test]
    fn test_simd_bytes_equal_different() {
        let a = b"hello world";
        let b = b"hello worle";
        assert!(!simd_bytes_equal(a, b));
    }

    #[test]
    fn test_simd_bytes_equal_different_length() {
        let a = b"hello";
        let b = b"hello world";
        assert!(!simd_bytes_equal(a, b));
    }

    #[test]
    fn test_simd_bytes_equal_empty() {
        let a: &[u8] = b"";
        let b: &[u8] = b"";
        assert!(simd_bytes_equal(a, b));
    }

    #[test]
    fn test_simd_bytes_equal_long() {
        // Test with data longer than 32 bytes to trigger AVX2
        let a = "a".repeat(100);
        let b = "a".repeat(100);
        assert!(simd_bytes_equal(a.as_bytes(), b.as_bytes()));

        let mut c = "a".repeat(100);
        c.replace_range(50..51, "b");
        assert!(!simd_bytes_equal(a.as_bytes(), c.as_bytes()));
    }

    #[test]
    fn test_simd_bytes_equal_exactly_16() {
        // Test SSE2 boundary (16 bytes)
        let a = b"1234567890123456";
        let b = b"1234567890123456";
        assert!(simd_bytes_equal(a, b));

        let c = b"1234567890123457";
        assert!(!simd_bytes_equal(a, c));
    }

    #[test]
    fn test_simd_bytes_equal_exactly_32() {
        // Test AVX2 boundary (32 bytes)
        let a = b"12345678901234567890123456789012";
        let b = b"12345678901234567890123456789012";
        assert!(simd_bytes_equal(a, b));

        let c = b"12345678901234567890123456789013";
        assert!(!simd_bytes_equal(a, c));
    }

    #[test]
    fn test_simd_bytes_equal_17_bytes() {
        // Test between SSE2 boundaries (16 < 17 < 32)
        let a = b"12345678901234567";
        let b = b"12345678901234567";
        assert!(simd_bytes_equal(a, b));

        let c = b"12345678901234568";
        assert!(!simd_bytes_equal(a, c));
    }

    #[test]
    fn test_simd_bytes_equal_33_bytes() {
        // Test between AVX2 boundaries (32 < 33 < 48)
        let a = b"123456789012345678901234567890123";
        let b = b"123456789012345678901234567890123";
        assert!(simd_bytes_equal(a, b));

        let c = b"123456789012345678901234567890124";
        assert!(!simd_bytes_equal(a, c));
    }

    #[test]
    fn test_simd_bytes_equal_48_bytes() {
        // Test 48 bytes (32 + 16)
        let a = "x".repeat(48);
        let b = "x".repeat(48);
        assert!(simd_bytes_equal(a.as_bytes(), b.as_bytes()));

        let mut c = "x".repeat(48);
        c.replace_range(47..48, "y");
        assert!(!simd_bytes_equal(a.as_bytes(), c.as_bytes()));
    }

    #[test]
    fn test_simd_bytes_equal_64_bytes() {
        // Test 64 bytes (2 x 32)
        let a = "z".repeat(64);
        let b = "z".repeat(64);
        assert!(simd_bytes_equal(a.as_bytes(), b.as_bytes()));
    }

    #[test]
    fn test_simd_bytes_equal_difference_in_first_chunk() {
        let a = "x".repeat(64);
        let mut b = "x".repeat(64);
        b.replace_range(5..6, "y");
        assert!(!simd_bytes_equal(a.as_bytes(), b.as_bytes()));
    }

    #[test]
    fn test_simd_bytes_equal_difference_in_second_chunk() {
        let a = "x".repeat(64);
        let mut b = "x".repeat(64);
        b.replace_range(35..36, "y");
        assert!(!simd_bytes_equal(a.as_bytes(), b.as_bytes()));
    }

    #[test]
    fn test_simd_bytes_equal_difference_in_remainder() {
        let a = "x".repeat(50);
        let mut b = "x".repeat(50);
        b.replace_range(49..50, "y");
        assert!(!simd_bytes_equal(a.as_bytes(), b.as_bytes()));
    }

    // =========================================================================
    // simd_find_first_difference Tests
    // =========================================================================

    #[test]
    fn test_find_first_difference_none() {
        let a = b"hello world";
        let b = b"hello world";
        assert_eq!(simd_find_first_difference(a, b), None);
    }

    #[test]
    fn test_find_first_difference_at_start() {
        let a = b"hello";
        let b = b"jello";
        assert_eq!(simd_find_first_difference(a, b), Some(0));
    }

    #[test]
    fn test_find_first_difference_at_end() {
        let a = b"hello";
        let b = b"hellp";
        assert_eq!(simd_find_first_difference(a, b), Some(4));
    }

    #[test]
    fn test_find_first_difference_length() {
        let a = b"hello";
        let b = b"hello world";
        assert_eq!(simd_find_first_difference(a, b), Some(5));
    }

    #[test]
    fn test_find_first_difference_empty() {
        let a: &[u8] = b"";
        let b: &[u8] = b"";
        assert_eq!(simd_find_first_difference(a, b), None);

        let c = b"hello";
        assert_eq!(simd_find_first_difference(a, c), Some(0));
    }

    #[test]
    fn test_find_first_difference_long() {
        let a = "a".repeat(100);
        let b = "a".repeat(100);
        assert_eq!(simd_find_first_difference(a.as_bytes(), b.as_bytes()), None);

        let mut c = "a".repeat(100);
        c.replace_range(75..76, "b");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), c.as_bytes()),
            Some(75)
        );
    }

    #[test]
    fn test_find_first_difference_16_bytes() {
        let a = b"1234567890123456";
        let b = b"1234567890123456";
        assert_eq!(simd_find_first_difference(a, b), None);

        let c = b"1234567890123457";
        assert_eq!(simd_find_first_difference(a, c), Some(15));
    }

    #[test]
    fn test_find_first_difference_32_bytes() {
        let a = b"12345678901234567890123456789012";
        let b = b"12345678901234567890123456789012";
        assert_eq!(simd_find_first_difference(a, b), None);

        let c = b"12345678901234567890123456789013";
        assert_eq!(simd_find_first_difference(a, c), Some(31));
    }

    #[test]
    fn test_find_first_difference_in_first_sse_chunk() {
        // Difference in first 16 bytes
        let a = "x".repeat(32);
        let mut b = "x".repeat(32);
        b.replace_range(3..4, "y");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), b.as_bytes()),
            Some(3)
        );
    }

    #[test]
    fn test_find_first_difference_in_second_sse_chunk() {
        // Difference in second 16 bytes (for SSE path)
        let a = "x".repeat(32);
        let mut b = "x".repeat(32);
        b.replace_range(20..21, "y");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), b.as_bytes()),
            Some(20)
        );
    }

    #[test]
    fn test_find_first_difference_in_first_avx_chunk() {
        // Difference in first 32 bytes
        let a = "x".repeat(64);
        let mut b = "x".repeat(64);
        b.replace_range(10..11, "y");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), b.as_bytes()),
            Some(10)
        );
    }

    #[test]
    fn test_find_first_difference_in_second_avx_chunk() {
        // Difference in second 32 bytes
        let a = "x".repeat(64);
        let mut b = "x".repeat(64);
        b.replace_range(40..41, "y");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), b.as_bytes()),
            Some(40)
        );
    }

    #[test]
    fn test_find_first_difference_in_sse_remainder_of_avx() {
        // 48 bytes: 32 (AVX) + 16 (SSE remainder)
        let a = "x".repeat(48);
        let mut b = "x".repeat(48);
        b.replace_range(35..36, "y");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), b.as_bytes()),
            Some(35)
        );
    }

    #[test]
    fn test_find_first_difference_in_scalar_remainder() {
        // 50 bytes: 32 (AVX) + 16 (SSE) + 2 (scalar)
        let a = "x".repeat(50);
        let mut b = "x".repeat(50);
        b.replace_range(49..50, "y");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), b.as_bytes()),
            Some(49)
        );
    }

    #[test]
    fn test_find_first_difference_different_lengths_same_prefix() {
        let a = b"hello";
        let b = b"hello world";
        assert_eq!(simd_find_first_difference(a, b), Some(5));

        // Longer a
        let a = b"hello world";
        let b = b"hello";
        assert_eq!(simd_find_first_difference(a, b), Some(5));
    }

    #[test]
    fn test_find_first_difference_long_equal_slices() {
        // Very long equal slices (tests multiple SIMD iterations)
        let a = "x".repeat(500);
        let b = "x".repeat(500);
        assert_eq!(simd_find_first_difference(a.as_bytes(), b.as_bytes()), None);
    }

    #[test]
    fn test_find_first_difference_long_difference_at_end() {
        let a = "x".repeat(500);
        let mut b = "x".repeat(500);
        b.replace_range(499..500, "y");
        assert_eq!(
            simd_find_first_difference(a.as_bytes(), b.as_bytes()),
            Some(499)
        );
    }

    // =========================================================================
    // json_strings_equal Tests
    // =========================================================================

    #[test]
    fn test_json_strings_equal() {
        assert!(json_strings_equal("hello", "hello"));
        assert!(!json_strings_equal("hello", "world"));
        assert!(json_strings_equal("", ""));
    }

    #[test]
    fn test_json_strings_equal_long() {
        let s = "a".repeat(100);
        assert!(json_strings_equal(&s, &s));
    }

    #[test]
    fn test_json_strings_equal_unicode() {
        assert!(json_strings_equal("日本語", "日本語"));
        assert!(!json_strings_equal("日本語", "中文"));
    }

    // =========================================================================
    // json_numbers_equal Tests
    // =========================================================================

    #[test]
    fn test_json_numbers_equal() {
        assert!(json_numbers_equal("42", "42"));
        assert!(json_numbers_equal("3.14", "3.14"));
        // Note: These are semantically equal but byte-different
        assert!(!json_numbers_equal("1.0", "1"));
    }

    #[test]
    fn test_json_numbers_equal_negative() {
        assert!(json_numbers_equal("-42", "-42"));
        assert!(!json_numbers_equal("-42", "42"));
    }

    #[test]
    fn test_json_numbers_equal_exponent() {
        assert!(json_numbers_equal("1e10", "1e10"));
        assert!(!json_numbers_equal("1e10", "1E10")); // Case sensitive
    }

    // =========================================================================
    // Feature Detection Tests
    // =========================================================================

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_has_sse2() {
        // SSE2 is baseline for x86_64
        assert!(has_sse2());
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_has_avx2() {
        // Just check it doesn't panic
        let _ = has_avx2();
    }

    // =========================================================================
    // Scalar Fallback Tests (small inputs)
    // =========================================================================

    #[test]
    fn test_simd_bytes_equal_scalar_fallback() {
        // Very short inputs use scalar path
        let a = b"hi";
        let b = b"hi";
        assert!(simd_bytes_equal(a, b));

        let c = b"ho";
        assert!(!simd_bytes_equal(a, c));
    }

    #[test]
    fn test_find_first_difference_scalar_fallback() {
        // Very short inputs use scalar path
        let a = b"abc";
        let b = b"abc";
        assert_eq!(simd_find_first_difference(a, b), None);

        let c = b"abd";
        assert_eq!(simd_find_first_difference(a, c), Some(2));
    }

    #[test]
    fn test_simd_bytes_equal_single_byte() {
        assert!(simd_bytes_equal(b"a", b"a"));
        assert!(!simd_bytes_equal(b"a", b"b"));
    }

    #[test]
    fn test_find_first_difference_single_byte() {
        assert_eq!(simd_find_first_difference(b"a", b"a"), None);
        assert_eq!(simd_find_first_difference(b"a", b"b"), Some(0));
    }
}
