// SPDX-License-Identifier: MIT OR Apache-2.0
//! ARM NEON path operations
//!
//! NEON accelerated path delimiter finding for JSON processing on aarch64.
//! Uses 128-bit NEON vectors to process 16 bytes at a time.

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{uint8x16_t, vceqq_u8, vdupq_n_u8, vld1q_u8, vorrq_u8};

/// NEON threshold for using SIMD path finding (16 bytes = 128-bit vector)
pub const SIMD_NEON_THRESHOLD: usize = 16;

/// Find delimiter (`.` or `[`) using ARM NEON SIMD
///
/// Searches for JSON path delimiters in the byte slice starting from `start`.
/// Uses NEON vector operations to process 16 bytes at a time.
///
/// # Safety
/// This function uses NEON intrinsics which are safe on aarch64.
#[cfg(target_arch = "aarch64")]
#[inline]
#[must_use]
pub fn find_delim_neon(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    let len = bytes.len();

    let dot = unsafe { vdupq_n_u8(b'.') };
    let bracket = unsafe { vdupq_n_u8(b'[') };

    while i + 16 <= len {
        let chunk = unsafe { vld1q_u8(bytes.as_ptr().add(i)) };
        let eq_dot = unsafe { vceqq_u8(chunk, dot) };
        let eq_bracket = unsafe { vceqq_u8(chunk, bracket) };
        let combined = unsafe { vorrq_u8(eq_dot, eq_bracket) };

        if let Some(offset) = unsafe { neon_first_set_byte(combined) } {
            return Some(i + offset);
        }

        i += 16;
    }

    // Scalar fallback for remaining bytes
    for (j, &b) in bytes[i..].iter().enumerate() {
        if b == b'.' || b == b'[' {
            return Some(i + j);
        }
    }
    None
}

/// Find single byte using ARM NEON SIMD
///
/// Searches for a specific byte in the slice starting from `start`.
/// Uses NEON vector operations to process 16 bytes at a time.
///
/// # Safety
/// This function uses NEON intrinsics which are safe on aarch64.
#[cfg(target_arch = "aarch64")]
#[inline]
#[must_use]
pub fn find_byte_neon(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    let mut i = start;
    let len = bytes.len();

    let needle_vec = unsafe { vdupq_n_u8(needle) };

    while i + 16 <= len {
        let chunk = unsafe { vld1q_u8(bytes.as_ptr().add(i)) };
        let eq = unsafe { vceqq_u8(chunk, needle_vec) };

        if let Some(offset) = unsafe { neon_first_set_byte(eq) } {
            return Some(i + offset);
        }

        i += 16;
    }

    // Scalar fallback for remaining bytes
    for (j, &b) in bytes[i..].iter().enumerate() {
        if b == needle {
            return Some(i + j);
        }
    }
    None
}

/// Find the index of the first non-zero byte in a NEON vector
///
/// Returns None if all bytes are zero.
///
/// # Safety
/// Requires a valid NEON `uint8x16_t` vector.
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn neon_first_set_byte(v: uint8x16_t) -> Option<usize> {
    // SAFETY: uint8x16_t is a 16-byte SIMD vector, safe to transmute to [u8; 16]
    let arr: [u8; 16] = unsafe { std::mem::transmute(v) };
    for (i, &byte) in arr.iter().enumerate() {
        if byte != 0 {
            return Some(i);
        }
    }
    None
}

/// Find two delimiters using NEON
///
/// Searches for either `delim1` or `delim2` in the byte slice.
#[cfg(target_arch = "aarch64")]
#[inline]
#[must_use]
pub fn find_two_bytes_neon(bytes: &[u8], start: usize, delim1: u8, delim2: u8) -> Option<usize> {
    let mut i = start;
    let len = bytes.len();

    let d1 = unsafe { vdupq_n_u8(delim1) };
    let d2 = unsafe { vdupq_n_u8(delim2) };

    while i + 16 <= len {
        let chunk = unsafe { vld1q_u8(bytes.as_ptr().add(i)) };
        let eq1 = unsafe { vceqq_u8(chunk, d1) };
        let eq2 = unsafe { vceqq_u8(chunk, d2) };
        let combined = unsafe { vorrq_u8(eq1, eq2) };

        if let Some(offset) = unsafe { neon_first_set_byte(combined) } {
            return Some(i + offset);
        }

        i += 16;
    }

    // Scalar fallback
    for (j, &b) in bytes[i..].iter().enumerate() {
        if b == delim1 || b == delim2 {
            return Some(i + j);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "aarch64")]
    use super::*;

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_delim_neon_dot() {
        let data = b"hello.world";
        assert_eq!(find_delim_neon(data, 0), Some(5));
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_delim_neon_bracket() {
        let data = b"items[0].value";
        assert_eq!(find_delim_neon(data, 0), Some(5));
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_delim_neon_not_found() {
        let data = b"nodots";
        assert_eq!(find_delim_neon(data, 0), None);
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_delim_neon_long_string() {
        // String longer than 16 bytes to test SIMD path
        let data = b"this_is_a_very_long_field_name.value";
        assert_eq!(find_delim_neon(data, 0), Some(30));
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_byte_neon() {
        let data = b"hello]world";
        assert_eq!(find_byte_neon(data, 0, b']'), Some(5));
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_byte_neon_not_found() {
        let data = b"hello world";
        assert_eq!(find_byte_neon(data, 0, b']'), None);
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_byte_neon_long_string() {
        // String longer than 16 bytes to test SIMD path
        let data = b"this_is_a_very_long_field_name]";
        assert_eq!(find_byte_neon(data, 0, b']'), Some(30));
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_two_bytes_neon() {
        let data = b"user.profile[0]";
        // Should find '.' first at position 4
        assert_eq!(find_two_bytes_neon(data, 0, b'.', b'['), Some(4));
        // After the dot, should find '[' at position 12
        assert_eq!(find_two_bytes_neon(data, 5, b'.', b'['), Some(12));
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_find_delim_neon_start_offset() {
        let data = b"user.name.first";
        // Start from position 5, should find second dot
        assert_eq!(find_delim_neon(data, 5), Some(9));
    }
}
