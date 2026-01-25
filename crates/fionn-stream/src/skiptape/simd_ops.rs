// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD operations for high-performance JSON processing
//!
//! This module provides SIMD-accelerated operations for character classification,
//! string processing, and pattern matching used in JSON parsing and schema filtering.

/// SIMD-accelerated character classification for JSON tokens
pub struct SimdCharClassifier;

/// High-performance SIMD JSON structural detector
/// Processes entire buffers using SIMD operations instead of per-character loops
pub struct SimdJsonStructuralDetector {
    classifier: SimdCharClassifier,
}

impl SimdJsonStructuralDetector {
    /// Create a new SIMD JSON structural detector
    #[must_use]
    pub const fn new() -> Self {
        Self {
            classifier: SimdCharClassifier::new(),
        }
    }

    /// Process an entire JSON buffer using SIMD operations
    /// Returns positions of all structural characters in one pass
    #[must_use]
    #[inline]
    pub fn find_structural_characters(&self, json_bytes: &[u8]) -> Vec<usize> {
        let mut positions = Vec::new();

        // Process in 64-byte chunks for optimal SIMD performance
        for chunk_start in (0..json_bytes.len()).step_by(64) {
            let chunk_end = (chunk_start + 64).min(json_bytes.len());
            // Create a padded chunk if we're at the end
            let mut padding = [0u8; 64];
            let len = chunk_end - chunk_start;
            padding[..len].copy_from_slice(&json_bytes[chunk_start..chunk_end]);

            if let Some(structural_pos) = self.process_chunk_simd(&padding, len, chunk_start) {
                positions.extend(structural_pos);
            }
        }

        positions
    }

    /// Process a 64-byte chunk using SIMD operations
    #[inline]
    fn process_chunk_simd(
        &self,
        chunk: &[u8; 64],
        valid_len: usize,
        offset: usize,
    ) -> Option<Vec<usize>> {
        // Use the classifier to get bitmasks for all character types in parallel
        let classes = self.classifier.classify_chunk(chunk);

        // Combine all structural character positions
        // We're interested in anything that is whitespace, structural, string, or number
        let all_structural =
            classes.whitespace | classes.structural | classes.string_chars | classes.numbers;

        if all_structural == 0 {
            return None;
        }

        // Convert bitmask to positions
        let mut positions = Vec::with_capacity(all_structural.count_ones() as usize);

        // Iterate over set bits
        let mut mask = all_structural;
        while mask != 0 {
            let idx = mask.trailing_zeros() as usize;
            if idx < valid_len {
                positions.push(offset + idx);
            }
            mask &= !(1u64 << idx);
        }

        Some(positions)
    }
}

impl SimdCharClassifier {
    /// Create a new SIMD character classifier
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

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

/// SIMD-accelerated string operations
pub struct SimdStringOps;

impl SimdStringOps {
    /// SIMD-accelerated string equality check
    #[inline]
    #[must_use]
    pub fn equals(a: &[u8], b: &[u8]) -> bool {
        // Rust's slice equality uses memcmp which is highly optimized (AVX/SSE)
        a == b
    }

    /// SIMD-accelerated substring search
    #[inline]
    #[must_use]
    pub fn find_substring(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        // Use memchr::memmem for SIMD-accelerated substring search (AVX2/SSE4.2)
        memchr::memmem::find(haystack, needle)
    }

    /// SIMD-accelerated hash computation for field names
    #[inline]
    #[must_use]
    pub fn hash_field_name(field: &[u8]) -> u64 {
        // Use AHash for high-performance hashing (often uses AES-NI or similar)
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        field.hash(&mut hasher);
        hasher.finish()
    }
}

/// SIMD-accelerated line separator detection for JSONL
pub struct SimdLineSeparator {
    // Markers are implicit in memchr
}

impl SimdLineSeparator {
    /// Create a new SIMD line separator detector
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Detect line boundaries in a data chunk using SIMD
    #[must_use]
    pub fn find_line_boundaries(&self, data: &[u8]) -> Vec<usize> {
        // Use memchr iterator which exploits SIMD for finding byte occurrences
        let mut boundaries: Vec<usize> = memchr::memchr_iter(b'\n', data)
            .map(|pos| pos + 1) // Position after the \n
            .collect();

        // If data doesn't end with \n, add the end position
        if !data.is_empty() && data[data.len() - 1] != b'\n' {
            boundaries.push(data.len());
        }

        boundaries
    }
}

/// SIMD-accelerated structural filtering for JSONL documents
pub struct SimdStructuralFilter {
    // No pre-computed masks needed for memchr implementation
}

impl SimdStructuralFilter {
    /// Create a new SIMD structural filter
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Check if a JSON line contains required schema fields using SIMD
    #[must_use]
    pub fn matches_schema(&self, line: &[u8], required_fields: &[String]) -> bool {
        if line.is_empty() {
            return false;
        }

        // Fast pre-filter using memchr::memmem to check for required fields in the raw bytes
        // This avoids utf-8 validation overhead if we just want to check presence
        for field in required_fields {
            // Need to search for "field" to be accurate, but strict JSON parsing is expensive here.
            // Approximating with finding "field" substring is usually good enough for pre-filter.
            // We construct the search needle: "field"
            let needle = format!("\"{field}\"");
            if memchr::memmem::find(line, needle.as_bytes()).is_none() {
                return false;
            }
        }
        true
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::arch::x86_64::{
    __m256i, _mm256_and_si256, _mm256_andnot_si256, _mm256_cmpeq_epi8, _mm256_cmpgt_epi8,
    _mm256_movemask_epi8, _mm256_or_si256, _mm256_set1_epi8,
};

impl SimdCharClassifier {
    /// Classify characters in a 64-byte chunk using SIMD (AVX2 optimized)
    ///
    /// # Safety
    /// Uses SIMD intrinsics that are safe on `x86`/`x86_64` with AVX2 support.
    /// The `_mm256_loadu_si256` intrinsic handles unaligned loads safely.
    ///
    /// # Panics
    /// This function does not panic. The `try_into().unwrap()` calls are
    /// guaranteed to succeed because we split a 64-byte array at index 32,
    /// producing exactly two 32-byte slices.
    #[inline]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[must_use]
    #[allow(clippy::too_many_lines)] // SIMD code benefits from inline processing
    pub fn classify_chunk(&self, chunk: &[u8; 64]) -> CharacterClasses {
        // Helper to convert u8 to i8 for SIMD constants (wrapping is intentional for byte values)
        const fn to_i8(b: u8) -> i8 {
            // Use from_ne_bytes for const-safe conversion that avoids the wrap warning
            i8::from_ne_bytes([b])
        }

        // Helper to convert i32 movemask result to u32 (reinterpret bits)
        const fn mask_to_u32(mask: i32) -> u32 {
            // Use from_ne_bytes to reinterpret bits without sign-related warnings
            u32::from_ne_bytes(mask.to_ne_bytes())
        }

        unsafe {
            // Load 64 bytes into two 256-bit AVX2 registers
            // We use std::mem::transmute to load the bytes, which is safe because
            // _mm256_loadu_si256 is designed for unaligned loads.
            // We avoid direct pointer cast to satisfy clippy's alignment checks.
            let (first_half, second_half) = chunk.split_at(32);
            let first_array: &[u8; 32] = first_half.try_into().unwrap();
            let second_array: &[u8; 32] = second_half.try_into().unwrap();

            // Use transmute to reinterpret the bytes as __m256i
            // This is safe because we're just reinterpreting bytes
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

            // Range check for digits: x >= '0' && x <= '9'
            // val >= 48 <=> val > 47. val <= 57 <=> !(val > 57)
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

    /// Classify characters in a 64-byte chunk using SIMD (ARM NEON optimized)
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

            // Range check for digits: x >= '0' && x <= '9'
            // Using signed comparison: val > 47 && val <= 57
            let lower_bound = vdupq_n_s8(47);
            let upper_bound = vdupq_n_s8(57);

            let v0_s = vreinterpretq_s8_u8(v0);
            let v1_s = vreinterpretq_s8_u8(v1);
            let v2_s = vreinterpretq_s8_u8(v2);
            let v3_s = vreinterpretq_s8_u8(v3);

            // vcgtq_s8 returns uint8x16_t (0xFF for true, 0x00 for false)
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

    /// Scalar fallback for architectures without SIMD support
    #[inline]
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    #[must_use]
    pub fn classify_chunk(&self, chunk: &[u8; 64]) -> CharacterClasses {
        let mut whitespace: u64 = 0;
        let mut structural: u64 = 0;
        let mut string_chars: u64 = 0;
        let mut numbers: u64 = 0;

        for (i, &byte) in chunk.iter().enumerate() {
            let bit = 1u64 << i;
            match byte {
                b' ' | b'\t' | b'\n' | b'\r' => whitespace |= bit,
                b'{' | b'}' | b'[' | b']' | b':' | b',' => structural |= bit,
                b'"' | b'\\' => string_chars |= bit,
                b'0'..=b'9' | b'-' | b'+' | b'.' => numbers |= bit,
                _ => {}
            }
        }

        CharacterClasses {
            whitespace,
            structural,
            string_chars,
            numbers,
        }
    }
}

/// Convert NEON comparison results to a 16-bit bitmask
///
/// NEON doesn't have a direct movemask equivalent, so we extract
/// the non-zero bytes and pack them into a u16.
/// Each byte is either 0x00 or 0xFF after comparison operations.
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

/// SIMD-accelerated pattern matching for schema operations
pub struct SimdPatternMatcher {
    /// Pre-compiled SIMD patterns
    patterns: Vec<SimdPattern>,
}

impl SimdPatternMatcher {
    /// Create a new SIMD pattern matcher
    #[must_use]
    pub const fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    /// Add a pattern to match against
    pub fn add_pattern(&mut self, pattern: &str) {
        // Compile pattern into SIMD-friendly representation
        let compiled = SimdPattern::compile(pattern);
        self.patterns.push(compiled);
    }

    /// Check if text matches any of the compiled patterns
    #[must_use]
    pub fn matches_any(&self, text: &[u8]) -> bool {
        for pattern in &self.patterns {
            if pattern.matches(text) {
                return true;
            }
        }
        false
    }
}

/// SIMD-compiled pattern for fast matching
struct SimdPattern {
    /// Pattern bytes for SIMD comparison
    pattern_bytes: Vec<u8>,
    /// SIMD-friendly hash for fast pre-filtering
    pattern_hash: u64,
}

impl SimdPattern {
    fn compile(pattern: &str) -> Self {
        let pattern_bytes = pattern.as_bytes().to_vec();
        let pattern_hash = SimdStringOps::hash_field_name(&pattern_bytes);

        Self {
            pattern_bytes,
            pattern_hash,
        }
    }

    fn matches(&self, text: &[u8]) -> bool {
        // SIMD-accelerated pattern matching
        // First check hash for fast rejection
        let text_hash = SimdStringOps::hash_field_name(text);
        if text_hash != self.pattern_hash {
            return false;
        }

        // Then do full SIMD comparison
        SimdStringOps::equals(text, &self.pattern_bytes)
    }
}

impl Default for SimdCharClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimdJsonStructuralDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimdLineSeparator {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimdStructuralFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimdPatternMatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_char_classifier_new() {
        let classifier = SimdCharClassifier::new();
        let chunk = [0u8; 64];
        let classes = classifier.classify_chunk(&chunk);
        assert_eq!(classes.whitespace, 0);
        assert_eq!(classes.structural, 0);
    }

    #[test]
    fn test_simd_char_classifier_whitespace() {
        let classifier = SimdCharClassifier::new();
        let mut chunk = [0u8; 64];
        chunk[0] = b' ';
        chunk[1] = b'\t';
        chunk[2] = b'\n';
        chunk[3] = b'\r';
        let classes = classifier.classify_chunk(&chunk);
        assert!(classes.whitespace != 0);
    }

    #[test]
    fn test_simd_char_classifier_structural() {
        let classifier = SimdCharClassifier::new();
        let mut chunk = [0u8; 64];
        chunk[0] = b'{';
        chunk[1] = b'}';
        chunk[2] = b'[';
        chunk[3] = b']';
        chunk[4] = b':';
        chunk[5] = b',';
        let classes = classifier.classify_chunk(&chunk);
        assert!(classes.structural != 0);
    }

    #[test]
    fn test_simd_char_classifier_string_chars() {
        let classifier = SimdCharClassifier::new();
        let mut chunk = [0u8; 64];
        chunk[0] = b'"';
        chunk[1] = b'\\';
        let classes = classifier.classify_chunk(&chunk);
        assert!(classes.string_chars != 0);
    }

    #[test]
    fn test_simd_char_classifier_numbers() {
        let classifier = SimdCharClassifier::new();
        let mut chunk = [0u8; 64];
        chunk[0] = b'0';
        chunk[1] = b'5';
        chunk[2] = b'9';
        chunk[3] = b'-';
        chunk[4] = b'+';
        chunk[5] = b'.';
        let classes = classifier.classify_chunk(&chunk);
        assert!(classes.numbers != 0);
    }

    #[test]
    fn test_character_classes_clone() {
        let classes = CharacterClasses {
            whitespace: 0xFF,
            structural: 0xAA,
            string_chars: 0x55,
            numbers: 0x11,
        };
        let cloned = classes;
        assert_eq!(cloned.whitespace, 0xFF);
        assert_eq!(cloned.structural, 0xAA);
    }

    #[test]
    fn test_simd_json_structural_detector_new() {
        let detector = SimdJsonStructuralDetector::new();
        let positions = detector.find_structural_characters(b"{}");
        assert!(!positions.is_empty());
    }

    #[test]
    fn test_simd_json_structural_detector_empty() {
        let detector = SimdJsonStructuralDetector::new();
        let positions = detector.find_structural_characters(b"");
        assert!(positions.is_empty());
    }

    #[test]
    fn test_simd_json_structural_detector_json() {
        let detector = SimdJsonStructuralDetector::new();
        let json = b"{\"name\":\"test\",\"value\":123}";
        let positions = detector.find_structural_characters(json);
        assert!(!positions.is_empty());
    }

    #[test]
    fn test_simd_json_structural_detector_large() {
        let detector = SimdJsonStructuralDetector::new();
        // Create a buffer larger than 64 bytes to test chunk processing
        let json =
            b"{\"name\":\"test\",\"value\":123,\"extra\":\"more data here to exceed 64 bytes\"}";
        let positions = detector.find_structural_characters(json);
        assert!(!positions.is_empty());
    }

    #[test]
    fn test_simd_string_ops_equals() {
        assert!(SimdStringOps::equals(b"hello", b"hello"));
        assert!(!SimdStringOps::equals(b"hello", b"world"));
        assert!(!SimdStringOps::equals(b"hello", b"hell"));
    }

    #[test]
    fn test_simd_string_ops_equals_empty() {
        assert!(SimdStringOps::equals(b"", b""));
        assert!(!SimdStringOps::equals(b"", b"a"));
    }

    #[test]
    fn test_simd_string_ops_find_substring() {
        assert_eq!(
            SimdStringOps::find_substring(b"hello world", b"world"),
            Some(6)
        );
        assert_eq!(
            SimdStringOps::find_substring(b"hello world", b"hello"),
            Some(0)
        );
        assert_eq!(SimdStringOps::find_substring(b"hello world", b"xyz"), None);
    }

    #[test]
    fn test_simd_string_ops_find_substring_empty() {
        assert_eq!(SimdStringOps::find_substring(b"hello", b""), Some(0));
        assert_eq!(SimdStringOps::find_substring(b"", b"a"), None);
    }

    #[test]
    fn test_simd_string_ops_hash_field_name() {
        let hash1 = SimdStringOps::hash_field_name(b"name");
        let hash2 = SimdStringOps::hash_field_name(b"name");
        let hash3 = SimdStringOps::hash_field_name(b"value");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_simd_line_separator_new() {
        let separator = SimdLineSeparator::new();
        let boundaries = separator.find_line_boundaries(b"line1\nline2\n");
        assert!(!boundaries.is_empty());
    }

    #[test]
    fn test_simd_line_separator_empty() {
        let separator = SimdLineSeparator::new();
        let boundaries = separator.find_line_boundaries(b"");
        assert!(boundaries.is_empty());
    }

    #[test]
    fn test_simd_line_separator_no_newline() {
        let separator = SimdLineSeparator::new();
        let boundaries = separator.find_line_boundaries(b"single line");
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0], 11); // End of data
    }

    #[test]
    fn test_simd_line_separator_multiple_lines() {
        let separator = SimdLineSeparator::new();
        let boundaries = separator.find_line_boundaries(b"line1\nline2\nline3");
        assert_eq!(boundaries.len(), 3);
    }

    #[test]
    fn test_simd_line_separator_ends_with_newline() {
        let separator = SimdLineSeparator::new();
        let boundaries = separator.find_line_boundaries(b"line1\nline2\n");
        assert_eq!(boundaries.len(), 2);
    }

    #[test]
    fn test_simd_structural_filter_new() {
        let filter = SimdStructuralFilter::new();
        let matches = filter.matches_schema(b"{\"name\":\"test\"}", &["name".to_string()]);
        assert!(matches);
    }

    #[test]
    fn test_simd_structural_filter_empty() {
        let filter = SimdStructuralFilter::new();
        let matches = filter.matches_schema(b"", &["name".to_string()]);
        assert!(!matches);
    }

    #[test]
    fn test_simd_structural_filter_no_match() {
        let filter = SimdStructuralFilter::new();
        let matches = filter.matches_schema(b"{\"value\":123}", &["name".to_string()]);
        assert!(!matches);
    }

    #[test]
    fn test_simd_structural_filter_multiple_fields() {
        let filter = SimdStructuralFilter::new();
        let json = b"{\"name\":\"test\",\"age\":30}";
        let matches = filter.matches_schema(json, &["name".to_string(), "age".to_string()]);
        assert!(matches);
    }

    #[test]
    fn test_simd_structural_filter_partial_match() {
        let filter = SimdStructuralFilter::new();
        let json = b"{\"name\":\"test\"}";
        let matches = filter.matches_schema(json, &["name".to_string(), "age".to_string()]);
        assert!(!matches); // All fields must match
    }

    #[test]
    fn test_simd_pattern_matcher_new() {
        let matcher = SimdPatternMatcher::new();
        assert!(!matcher.matches_any(b"test"));
    }

    #[test]
    fn test_simd_pattern_matcher_add_pattern() {
        let mut matcher = SimdPatternMatcher::new();
        matcher.add_pattern("test");
        assert!(matcher.matches_any(b"test"));
        assert!(!matcher.matches_any(b"other"));
    }

    #[test]
    fn test_simd_pattern_matcher_multiple_patterns() {
        let mut matcher = SimdPatternMatcher::new();
        matcher.add_pattern("hello");
        matcher.add_pattern("world");
        assert!(matcher.matches_any(b"hello"));
        assert!(matcher.matches_any(b"world"));
        assert!(!matcher.matches_any(b"other"));
    }

    #[test]
    fn test_simd_pattern_compile_and_match() {
        let pattern = SimdPattern::compile("test");
        assert!(pattern.matches(b"test"));
        assert!(!pattern.matches(b"other"));
    }

    #[test]
    fn test_simd_pattern_hash_mismatch() {
        let pattern = SimdPattern::compile("test");
        assert!(!pattern.matches(b"different"));
    }

    #[test]
    fn test_character_classes_debug() {
        let classes = CharacterClasses {
            whitespace: 1,
            structural: 2,
            string_chars: 3,
            numbers: 4,
        };
        let debug = format!("{classes:?}");
        assert!(debug.contains("whitespace"));
        assert!(debug.contains("structural"));
    }

    #[test]
    fn test_simd_json_structural_detector_process_chunk() {
        let detector = SimdJsonStructuralDetector::new();
        // Test with exactly 64 bytes
        let mut json = [b' '; 64];
        json[0] = b'{';
        json[63] = b'}';
        let positions = detector.find_structural_characters(&json);
        assert!(!positions.is_empty());
    }
}
