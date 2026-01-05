// SPDX-License-Identifier: MIT OR Apache-2.0
//! ARM NEON character classification
//!
//! NEON-optimized character classification for JSON processing on aarch64.

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

    /// Classify characters in a 64-byte chunk using NEON
    ///
    /// # Safety
    /// Uses SIMD intrinsics that are safe on aarch64 with NEON support.
    #[inline]
    #[cfg(target_arch = "aarch64")]
    #[must_use]
    #[allow(clippy::too_many_lines)] // SIMD code benefits from inlining
    pub fn classify_chunk(&self, chunk: &[u8; 64]) -> CharacterClasses {
        use std::arch::aarch64::{
            vandq_u8, vceqq_u8, vcgtq_s8, vdupq_n_s8, vdupq_n_u8, vld1q_u8, vmvnq_u8, vorrq_u8,
            vreinterpretq_s8_u8,
        };

        unsafe {
            // Load 64 bytes as 4x 128-bit NEON vectors
            let v0 = vld1q_u8(chunk.as_ptr());
            let v1 = vld1q_u8(chunk.as_ptr().add(16));
            let v2 = vld1q_u8(chunk.as_ptr().add(32));
            let v3 = vld1q_u8(chunk.as_ptr().add(48));

            // 1. Whitespace: \t (09), \n (0A), \r (0D), space (20)
            let space = vdupq_n_u8(b' ');
            let tab = vdupq_n_u8(b'\t');
            let lf = vdupq_n_u8(b'\n');
            let cr = vdupq_n_u8(b'\r');

            let ws0 = vorrq_u8(
                vorrq_u8(vceqq_u8(v0, space), vceqq_u8(v0, tab)),
                vorrq_u8(vceqq_u8(v0, lf), vceqq_u8(v0, cr)),
            );
            let ws1 = vorrq_u8(
                vorrq_u8(vceqq_u8(v1, space), vceqq_u8(v1, tab)),
                vorrq_u8(vceqq_u8(v1, lf), vceqq_u8(v1, cr)),
            );
            let ws2 = vorrq_u8(
                vorrq_u8(vceqq_u8(v2, space), vceqq_u8(v2, tab)),
                vorrq_u8(vceqq_u8(v2, lf), vceqq_u8(v2, cr)),
            );
            let ws3 = vorrq_u8(
                vorrq_u8(vceqq_u8(v3, space), vceqq_u8(v3, tab)),
                vorrq_u8(vceqq_u8(v3, lf), vceqq_u8(v3, cr)),
            );

            let whitespace_mask = neon_to_bitmask_64(ws0, ws1, ws2, ws3);

            // 2. Structural: { } [ ] : ,
            let brace_o = vdupq_n_u8(b'{');
            let brace_c = vdupq_n_u8(b'}');
            let bracket_o = vdupq_n_u8(b'[');
            let bracket_c = vdupq_n_u8(b']');
            let colon = vdupq_n_u8(b':');
            let comma = vdupq_n_u8(b',');

            let struct0 = vorrq_u8(
                vorrq_u8(vceqq_u8(v0, brace_o), vceqq_u8(v0, brace_c)),
                vorrq_u8(
                    vorrq_u8(vceqq_u8(v0, bracket_o), vceqq_u8(v0, bracket_c)),
                    vorrq_u8(vceqq_u8(v0, colon), vceqq_u8(v0, comma)),
                ),
            );
            let struct1 = vorrq_u8(
                vorrq_u8(vceqq_u8(v1, brace_o), vceqq_u8(v1, brace_c)),
                vorrq_u8(
                    vorrq_u8(vceqq_u8(v1, bracket_o), vceqq_u8(v1, bracket_c)),
                    vorrq_u8(vceqq_u8(v1, colon), vceqq_u8(v1, comma)),
                ),
            );
            let struct2 = vorrq_u8(
                vorrq_u8(vceqq_u8(v2, brace_o), vceqq_u8(v2, brace_c)),
                vorrq_u8(
                    vorrq_u8(vceqq_u8(v2, bracket_o), vceqq_u8(v2, bracket_c)),
                    vorrq_u8(vceqq_u8(v2, colon), vceqq_u8(v2, comma)),
                ),
            );
            let struct3 = vorrq_u8(
                vorrq_u8(vceqq_u8(v3, brace_o), vceqq_u8(v3, brace_c)),
                vorrq_u8(
                    vorrq_u8(vceqq_u8(v3, bracket_o), vceqq_u8(v3, bracket_c)),
                    vorrq_u8(vceqq_u8(v3, colon), vceqq_u8(v3, comma)),
                ),
            );

            let structural_mask = neon_to_bitmask_64(struct0, struct1, struct2, struct3);

            // 3. String: " and \
            let quote = vdupq_n_u8(b'"');
            let backslash = vdupq_n_u8(b'\\');

            let str0 = vorrq_u8(vceqq_u8(v0, quote), vceqq_u8(v0, backslash));
            let str1 = vorrq_u8(vceqq_u8(v1, quote), vceqq_u8(v1, backslash));
            let str2 = vorrq_u8(vceqq_u8(v2, quote), vceqq_u8(v2, backslash));
            let str3 = vorrq_u8(vceqq_u8(v3, quote), vceqq_u8(v3, backslash));

            let string_mask = neon_to_bitmask_64(str0, str1, str2, str3);

            // 4. Numbers: 0-9, -, +, .
            let dot = vdupq_n_u8(b'.');
            let minus = vdupq_n_u8(b'-');
            let plus = vdupq_n_u8(b'+');

            let lower_bound = vdupq_n_s8(47);
            let upper_bound = vdupq_n_s8(57);

            let v0_s = vreinterpretq_s8_u8(v0);
            let v1_s = vreinterpretq_s8_u8(v1);
            let v2_s = vreinterpretq_s8_u8(v2);
            let v3_s = vreinterpretq_s8_u8(v3);

            let is_digit0 = vandq_u8(
                vcgtq_s8(v0_s, lower_bound),
                vmvnq_u8(vcgtq_s8(v0_s, upper_bound)),
            );
            let is_digit1 = vandq_u8(
                vcgtq_s8(v1_s, lower_bound),
                vmvnq_u8(vcgtq_s8(v1_s, upper_bound)),
            );
            let is_digit2 = vandq_u8(
                vcgtq_s8(v2_s, lower_bound),
                vmvnq_u8(vcgtq_s8(v2_s, upper_bound)),
            );
            let is_digit3 = vandq_u8(
                vcgtq_s8(v3_s, lower_bound),
                vmvnq_u8(vcgtq_s8(v3_s, upper_bound)),
            );

            let num_markers0 = vorrq_u8(
                vorrq_u8(vceqq_u8(v0, dot), vceqq_u8(v0, minus)),
                vceqq_u8(v0, plus),
            );
            let num_markers1 = vorrq_u8(
                vorrq_u8(vceqq_u8(v1, dot), vceqq_u8(v1, minus)),
                vceqq_u8(v1, plus),
            );
            let num_markers2 = vorrq_u8(
                vorrq_u8(vceqq_u8(v2, dot), vceqq_u8(v2, minus)),
                vceqq_u8(v2, plus),
            );
            let num_markers3 = vorrq_u8(
                vorrq_u8(vceqq_u8(v3, dot), vceqq_u8(v3, minus)),
                vceqq_u8(v3, plus),
            );

            let num0 = vorrq_u8(is_digit0, num_markers0);
            let num1 = vorrq_u8(is_digit1, num_markers1);
            let num2 = vorrq_u8(is_digit2, num_markers2);
            let num3 = vorrq_u8(is_digit3, num_markers3);

            let number_mask = neon_to_bitmask_64(num0, num1, num2, num3);

            CharacterClasses {
                whitespace: whitespace_mask,
                structural: structural_mask,
                string_chars: string_mask,
                numbers: number_mask,
            }
        }
    }
}

/// Convert NEON comparison results to a 16-bit bitmask
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn neon_to_bitmask_16(v: std::arch::aarch64::uint8x16_t) -> u16 {
    // SAFETY: uint8x16_t is a 16-byte SIMD vector, safe to transmute to [u8; 16]
    let arr: [u8; 16] = unsafe { std::mem::transmute(v) };
    let mut result: u16 = 0;
    for (i, &byte) in arr.iter().enumerate() {
        if byte != 0 {
            result |= 1 << i;
        }
    }
    result
}

/// Convert 4 NEON vectors (64 bytes total) to a 64-bit bitmask
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn neon_to_bitmask_64(
    v0: std::arch::aarch64::uint8x16_t,
    v1: std::arch::aarch64::uint8x16_t,
    v2: std::arch::aarch64::uint8x16_t,
    v3: std::arch::aarch64::uint8x16_t,
) -> u64 {
    // SAFETY: Called from within unsafe context, vectors are valid
    unsafe {
        let m0 = u64::from(neon_to_bitmask_16(v0));
        let m1 = u64::from(neon_to_bitmask_16(v1));
        let m2 = u64::from(neon_to_bitmask_16(v2));
        let m3 = u64::from(neon_to_bitmask_16(v3));

        m0 | (m1 << 16) | (m2 << 32) | (m3 << 48)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_classify_chunk() {
        use super::*;

        let classifier = SimdCharClassifier::new();
        let mut chunk = [0u8; 64];
        chunk[0] = b'{';
        chunk[1] = b'"';
        chunk[7] = b':';
        chunk[8] = b' ';
        chunk[9] = b'4';
        chunk[11] = b'}';

        let classes = classifier.classify_chunk(&chunk);

        assert!(classes.structural & 1 != 0); // {
        assert!(classes.structural & (1 << 7) != 0); // :
        assert!(classes.structural & (1 << 11) != 0); // }
        assert!(classes.string_chars & (1 << 1) != 0); // "
        assert!(classes.whitespace & (1 << 8) != 0); // space
        assert!(classes.numbers & (1 << 9) != 0); // 4
    }
}
