// SPDX-License-Identifier: MIT OR Apache-2.0
//! `x86`/`x86_64` SIMD character classification
//!
//! AVX2-optimized character classification for JSON processing.

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::arch::x86_64::{
    __m256i, _mm256_and_si256, _mm256_andnot_si256, _mm256_cmpeq_epi8, _mm256_cmpgt_epi8,
    _mm256_movemask_epi8, _mm256_or_si256, _mm256_set1_epi8,
};

/// Result of SIMD character classification
#[derive(Debug, Clone)]
pub struct CharacterClasses {
    /// Bitmask of whitespace character positions
    pub whitespace: u64,
    /// Bitmask of structural character positions
    pub structural: u64,
    /// Bitmask of string-related character positions
    pub string_chars: u64,
    /// Bitmask of numeric character positions
    pub numbers: u64,
}

/// SIMD-accelerated character classification for JSON tokens
#[derive(Debug, Clone, Copy, Default)]
pub struct SimdCharClassifier;

impl SimdCharClassifier {
    /// Create a new SIMD character classifier
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Classify characters in a 64-byte chunk using AVX2
    ///
    /// # Safety
    /// Uses SIMD intrinsics that are safe on `x86`/`x86_64` with AVX2 support.
    ///
    /// # Panics
    /// This function will not panic. The `try_into().unwrap()` calls are guaranteed
    /// to succeed because we split a 64-byte array at index 32.
    #[inline]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[must_use]
    #[allow(clippy::too_many_lines)] // SIMD code benefits from inline processing
    pub fn classify_chunk(&self, chunk: &[u8; 64]) -> CharacterClasses {
        // Helper to convert u8 to i8 for SIMD constants
        const fn to_i8(b: u8) -> i8 {
            i8::from_ne_bytes([b])
        }

        // Helper to convert i32 movemask result to u32
        const fn mask_to_u32(mask: i32) -> u32 {
            u32::from_ne_bytes(mask.to_ne_bytes())
        }

        unsafe {
            // Load 64 bytes into two 256-bit AVX2 registers
            let (first_half, second_half) = chunk.split_at(32);
            let first_array: &[u8; 32] = first_half.try_into().unwrap();
            let second_array: &[u8; 32] = second_half.try_into().unwrap();

            let v0: __m256i = std::mem::transmute::<[u8; 32], __m256i>(*first_array);
            let v1: __m256i = std::mem::transmute::<[u8; 32], __m256i>(*second_array);

            // 1. Whitespace: \t (09), \n (0A), \r (0D), space (20)
            let space = _mm256_set1_epi8(to_i8(b' '));
            let tab = _mm256_set1_epi8(to_i8(b'\t'));
            let lf = _mm256_set1_epi8(to_i8(b'\n'));
            let cr = _mm256_set1_epi8(to_i8(b'\r'));

            let ws0 = _mm256_or_si256(
                _mm256_or_si256(_mm256_cmpeq_epi8(v0, space), _mm256_cmpeq_epi8(v0, tab)),
                _mm256_or_si256(_mm256_cmpeq_epi8(v0, lf), _mm256_cmpeq_epi8(v0, cr)),
            );
            let ws1 = _mm256_or_si256(
                _mm256_or_si256(_mm256_cmpeq_epi8(v1, space), _mm256_cmpeq_epi8(v1, tab)),
                _mm256_or_si256(_mm256_cmpeq_epi8(v1, lf), _mm256_cmpeq_epi8(v1, cr)),
            );

            let whitespace_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(ws1))) << 32)
                | u64::from(mask_to_u32(_mm256_movemask_epi8(ws0)));

            // 2. Structural: { } [ ] : ,
            let brace_o = _mm256_set1_epi8(to_i8(b'{'));
            let brace_c = _mm256_set1_epi8(to_i8(b'}'));
            let bracket_o = _mm256_set1_epi8(to_i8(b'['));
            let bracket_c = _mm256_set1_epi8(to_i8(b']'));
            let colon = _mm256_set1_epi8(to_i8(b':'));
            let comma = _mm256_set1_epi8(to_i8(b','));

            let struct0 = _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_cmpeq_epi8(v0, brace_o),
                    _mm256_cmpeq_epi8(v0, brace_c),
                ),
                _mm256_or_si256(
                    _mm256_or_si256(
                        _mm256_cmpeq_epi8(v0, bracket_o),
                        _mm256_cmpeq_epi8(v0, bracket_c),
                    ),
                    _mm256_or_si256(_mm256_cmpeq_epi8(v0, colon), _mm256_cmpeq_epi8(v0, comma)),
                ),
            );
            let struct1 = _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_cmpeq_epi8(v1, brace_o),
                    _mm256_cmpeq_epi8(v1, brace_c),
                ),
                _mm256_or_si256(
                    _mm256_or_si256(
                        _mm256_cmpeq_epi8(v1, bracket_o),
                        _mm256_cmpeq_epi8(v1, bracket_c),
                    ),
                    _mm256_or_si256(_mm256_cmpeq_epi8(v1, colon), _mm256_cmpeq_epi8(v1, comma)),
                ),
            );
            let structural_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(struct1))) << 32)
                | u64::from(mask_to_u32(_mm256_movemask_epi8(struct0)));

            // 3. String: " and \
            let quote = _mm256_set1_epi8(to_i8(b'"'));
            let backslash = _mm256_set1_epi8(to_i8(b'\\'));

            let str0 = _mm256_or_si256(
                _mm256_cmpeq_epi8(v0, quote),
                _mm256_cmpeq_epi8(v0, backslash),
            );
            let str1 = _mm256_or_si256(
                _mm256_cmpeq_epi8(v1, quote),
                _mm256_cmpeq_epi8(v1, backslash),
            );
            let string_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(str1))) << 32)
                | u64::from(mask_to_u32(_mm256_movemask_epi8(str0)));

            // 4. Numbers: 0-9, -, +, .
            let dot = _mm256_set1_epi8(to_i8(b'.'));
            let minus = _mm256_set1_epi8(to_i8(b'-'));
            let plus = _mm256_set1_epi8(to_i8(b'+'));

            let lower_bound = _mm256_set1_epi8(47);
            let fifty_seven = _mm256_set1_epi8(57);
            let all_ones = _mm256_set1_epi8(-1);

            let is_digit0 = _mm256_and_si256(
                _mm256_cmpgt_epi8(v0, lower_bound),
                _mm256_andnot_si256(_mm256_cmpgt_epi8(v0, fifty_seven), all_ones),
            );
            let is_digit1 = _mm256_and_si256(
                _mm256_cmpgt_epi8(v1, lower_bound),
                _mm256_andnot_si256(_mm256_cmpgt_epi8(v1, fifty_seven), all_ones),
            );

            let num_markers0 = _mm256_or_si256(
                _mm256_or_si256(_mm256_cmpeq_epi8(v0, dot), _mm256_cmpeq_epi8(v0, minus)),
                _mm256_cmpeq_epi8(v0, plus),
            );
            let num_markers1 = _mm256_or_si256(
                _mm256_or_si256(_mm256_cmpeq_epi8(v1, dot), _mm256_cmpeq_epi8(v1, minus)),
                _mm256_cmpeq_epi8(v1, plus),
            );

            let num0 = _mm256_or_si256(is_digit0, num_markers0);
            let num1 = _mm256_or_si256(is_digit1, num_markers1);

            let number_mask = (u64::from(mask_to_u32(_mm256_movemask_epi8(num1))) << 32)
                | u64::from(mask_to_u32(_mm256_movemask_epi8(num0)));

            CharacterClasses {
                whitespace: whitespace_mask,
                structural: structural_mask,
                string_chars: string_mask,
                numbers: number_mask,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn test_classify_chunk() {
        let classifier = SimdCharClassifier::new();
        let mut chunk = [0u8; 64];
        chunk[0] = b'{';
        chunk[1] = b'"';
        chunk[2] = b'n';
        chunk[3] = b'a';
        chunk[4] = b'm';
        chunk[5] = b'e';
        chunk[6] = b'"';
        chunk[7] = b':';
        chunk[8] = b' ';
        chunk[9] = b'4';
        chunk[10] = b'2';
        chunk[11] = b'}';

        let classes = classifier.classify_chunk(&chunk);

        // Check structural characters
        assert!(classes.structural & 1 != 0); // {
        assert!(classes.structural & (1 << 7) != 0); // :
        assert!(classes.structural & (1 << 11) != 0); // }

        // Check string characters
        assert!(classes.string_chars & (1 << 1) != 0); // "
        assert!(classes.string_chars & (1 << 6) != 0); // "

        // Check whitespace
        assert!(classes.whitespace & (1 << 8) != 0); // space

        // Check numbers
        assert!(classes.numbers & (1 << 9) != 0); // 4
        assert!(classes.numbers & (1 << 10) != 0); // 2
    }
}
