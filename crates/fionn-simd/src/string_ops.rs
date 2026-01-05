// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-accelerated string operations
//!
//! High-performance string operations that leverage SIMD where available.

/// SIMD-accelerated string operations
pub struct SimdStringOps;

impl SimdStringOps {
    /// SIMD-accelerated string equality check
    ///
    /// Uses memcmp under the hood which is highly optimized (AVX/SSE).
    #[inline]
    #[must_use]
    pub fn equals(a: &[u8], b: &[u8]) -> bool {
        a == b
    }

    /// SIMD-accelerated substring search
    ///
    /// Uses `memchr::memmem` for SIMD-accelerated substring search (AVX2/SSE4.2).
    #[inline]
    #[must_use]
    pub fn find_substring(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        memchr::memmem::find(haystack, needle)
    }

    /// SIMD-accelerated hash computation for field names
    ///
    /// Uses `AHash` for high-performance hashing (often uses AES-NI or similar).
    #[inline]
    #[must_use]
    pub fn hash_field_name(field: &[u8]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        field.hash(&mut hasher);
        hasher.finish()
    }

    /// Find the first occurrence of a byte in a slice
    ///
    /// Uses memchr for SIMD-accelerated byte search.
    #[inline]
    #[must_use]
    pub fn find_byte(haystack: &[u8], needle: u8) -> Option<usize> {
        memchr::memchr(needle, haystack)
    }

    /// Find all occurrences of a byte in a slice
    ///
    /// Uses memchr iterator for SIMD-accelerated multi-search.
    #[inline]
    pub fn find_all_bytes(haystack: &[u8], needle: u8) -> impl Iterator<Item = usize> + '_ {
        memchr::memchr_iter(needle, haystack)
    }
}

/// SIMD-accelerated line separator detection for JSONL
pub struct SimdLineSeparator;

impl SimdLineSeparator {
    /// Detect line boundaries in a data chunk using SIMD
    #[must_use]
    pub fn find_line_boundaries(data: &[u8]) -> Vec<usize> {
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
pub struct SimdStructuralFilter;

impl SimdStructuralFilter {
    /// Check if a JSON line contains required schema fields using SIMD
    #[must_use]
    pub fn matches_schema(line: &[u8], required_fields: &[String]) -> bool {
        if line.is_empty() {
            return false;
        }

        // Fast pre-filter using memchr::memmem to check for required fields
        for field in required_fields {
            let needle = format!("\"{field}\"");
            if memchr::memmem::find(line, needle.as_bytes()).is_none() {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equals() {
        assert!(SimdStringOps::equals(b"hello", b"hello"));
        assert!(!SimdStringOps::equals(b"hello", b"world"));
    }

    #[test]
    fn test_find_substring() {
        assert_eq!(
            SimdStringOps::find_substring(b"hello world", b"world"),
            Some(6)
        );
        assert_eq!(SimdStringOps::find_substring(b"hello", b"xyz"), None);
        assert_eq!(SimdStringOps::find_substring(b"hello", b""), Some(0));
    }

    #[test]
    fn test_hash_field_name() {
        let hash1 = SimdStringOps::hash_field_name(b"test");
        let hash2 = SimdStringOps::hash_field_name(b"test");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_find_byte() {
        assert_eq!(SimdStringOps::find_byte(b"hello", b'l'), Some(2));
        assert_eq!(SimdStringOps::find_byte(b"hello", b'x'), None);
    }

    #[test]
    fn test_line_boundaries() {
        let data = b"line1\nline2\nline3";
        let boundaries = SimdLineSeparator::find_line_boundaries(data);
        assert_eq!(boundaries, vec![6, 12, 17]);
    }

    #[test]
    fn test_structural_filter() {
        let line = br#"{"name": "test", "value": 42}"#;
        assert!(SimdStructuralFilter::matches_schema(
            line,
            &["name".to_string()]
        ));
        assert!(!SimdStructuralFilter::matches_schema(
            line,
            &["missing".to_string()]
        ));
    }

    // =========================================================================
    // Additional Coverage Tests
    // =========================================================================

    #[test]
    fn test_equals_empty() {
        assert!(SimdStringOps::equals(b"", b""));
        assert!(!SimdStringOps::equals(b"", b"x"));
    }

    #[test]
    fn test_equals_different_lengths() {
        assert!(!SimdStringOps::equals(b"hello", b"helloworld"));
        assert!(!SimdStringOps::equals(b"helloworld", b"hello"));
    }

    #[test]
    fn test_find_substring_at_start() {
        assert_eq!(SimdStringOps::find_substring(b"hello world", b"hello"), Some(0));
    }

    #[test]
    fn test_find_substring_at_end() {
        assert_eq!(SimdStringOps::find_substring(b"hello world", b"ld"), Some(9));
    }

    #[test]
    fn test_find_substring_multiple_occurrences() {
        // Returns first occurrence
        assert_eq!(SimdStringOps::find_substring(b"abcabc", b"abc"), Some(0));
    }

    #[test]
    fn test_find_substring_single_byte() {
        assert_eq!(SimdStringOps::find_substring(b"hello", b"e"), Some(1));
    }

    #[test]
    fn test_hash_field_name_different() {
        let hash1 = SimdStringOps::hash_field_name(b"test1");
        let hash2 = SimdStringOps::hash_field_name(b"test2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_field_name_empty() {
        let hash = SimdStringOps::hash_field_name(b"");
        // Just verify it produces a hash without panicking
        let _ = hash;
    }

    #[test]
    fn test_find_byte_at_start() {
        assert_eq!(SimdStringOps::find_byte(b"hello", b'h'), Some(0));
    }

    #[test]
    fn test_find_byte_at_end() {
        assert_eq!(SimdStringOps::find_byte(b"hello", b'o'), Some(4));
    }

    #[test]
    fn test_find_byte_empty() {
        assert_eq!(SimdStringOps::find_byte(b"", b'x'), None);
    }

    #[test]
    fn test_find_all_bytes() {
        let positions: Vec<_> = SimdStringOps::find_all_bytes(b"hello", b'l').collect();
        assert_eq!(positions, vec![2, 3]);
    }

    #[test]
    fn test_find_all_bytes_empty() {
        let positions_count = SimdStringOps::find_all_bytes(b"", b'x').count();
        assert_eq!(positions_count, 0);
    }

    #[test]
    fn test_find_all_bytes_none() {
        let positions_count = SimdStringOps::find_all_bytes(b"hello", b'x').count();
        assert_eq!(positions_count, 0);
    }

    #[test]
    fn test_find_all_bytes_single() {
        let positions: Vec<_> = SimdStringOps::find_all_bytes(b"hello", b'h').collect();
        assert_eq!(positions, vec![0]);
    }

    #[test]
    fn test_line_boundaries_empty() {
        let boundaries = SimdLineSeparator::find_line_boundaries(b"");
        assert!(boundaries.is_empty());
    }

    #[test]
    fn test_line_boundaries_no_newlines() {
        let boundaries = SimdLineSeparator::find_line_boundaries(b"single line");
        assert_eq!(boundaries, vec![11]); // Just the end position
    }

    #[test]
    fn test_line_boundaries_trailing_newline() {
        let boundaries = SimdLineSeparator::find_line_boundaries(b"line1\nline2\n");
        assert_eq!(boundaries, vec![6, 12]); // After each \n
    }

    #[test]
    fn test_line_boundaries_multiple_empty() {
        let boundaries = SimdLineSeparator::find_line_boundaries(b"\n\n\n");
        assert_eq!(boundaries, vec![1, 2, 3]);
    }

    #[test]
    fn test_structural_filter_empty_line() {
        assert!(!SimdStructuralFilter::matches_schema(b"", &["name".to_string()]));
    }

    #[test]
    fn test_structural_filter_empty_fields() {
        let line = br#"{"name": "test"}"#;
        assert!(SimdStructuralFilter::matches_schema(line, &[]));
    }

    #[test]
    fn test_structural_filter_multiple_fields() {
        let line = br#"{"name": "test", "value": 42, "active": true}"#;
        assert!(SimdStructuralFilter::matches_schema(
            line,
            &["name".to_string(), "value".to_string()]
        ));
        assert!(!SimdStructuralFilter::matches_schema(
            line,
            &["name".to_string(), "missing".to_string()]
        ));
    }

    #[test]
    fn test_structural_filter_field_not_quoted() {
        // Filter looks for quoted field names
        let line = b"name: test"; // Not valid JSON, no quotes
        assert!(!SimdStructuralFilter::matches_schema(line, &["name".to_string()]));
    }

    #[test]
    fn test_structural_filter_partial_match() {
        // Should not match partial field names
        let line = br#"{"username": "test"}"#;
        // Looking for exact "name" should not match "username"
        assert!(!SimdStructuralFilter::matches_schema(line, &["name".to_string()]));
    }

    #[test]
    fn test_structural_filter_nested_field() {
        let line = br#"{"user": {"name": "test"}}"#;
        // Matches because "name" appears in the JSON
        assert!(SimdStructuralFilter::matches_schema(line, &["name".to_string()]));
    }

    #[test]
    fn test_line_boundaries_single_newline() {
        let boundaries = SimdLineSeparator::find_line_boundaries(b"\n");
        assert_eq!(boundaries, vec![1]);
    }
}
