// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pooled buffer type.

use std::ops::{Deref, DerefMut};

/// A buffer that can be returned to a pool.
///
/// Wraps a `Vec<u8>` with tracking metadata for pool management.
#[derive(Debug)]
pub struct PooledBuffer {
    /// The underlying buffer.
    data: Vec<u8>,
    /// Timestamp for LRU tracking (monotonic counter).
    pub(crate) last_used: u64,
}

impl PooledBuffer {
    /// Create a new buffer with given capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            last_used: 0,
        }
    }

    /// Create from an existing Vec.
    #[must_use]
    pub const fn from_vec(data: Vec<u8>) -> Self {
        Self { data, last_used: 0 }
    }

    /// Get buffer capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.data.capacity()
    }

    /// Get buffer length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if buffer is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Clear the buffer contents (keeps capacity).
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Ensure buffer has at least `min_capacity`.
    pub fn ensure_capacity(&mut self, min_capacity: usize) {
        if self.data.capacity() < min_capacity {
            // reserve() takes additional space beyond current length
            self.data.reserve(min_capacity - self.data.len());
        }
    }

    /// Consume and return the inner Vec.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }

    /// Get a reference to the inner Vec.
    #[must_use]
    pub const fn as_vec(&self) -> &Vec<u8> {
        &self.data
    }

    /// Get a mutable reference to the inner Vec.
    pub const fn as_vec_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }

    /// Resize buffer to given length, filling with zeros.
    pub fn resize(&mut self, new_len: usize) {
        self.data.resize(new_len, 0);
    }

    /// Extend buffer from slice.
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.data.extend_from_slice(slice);
    }

    /// Set the LRU timestamp.
    pub(crate) const fn touch(&mut self, timestamp: u64) {
        self.last_used = timestamp;
    }
}

impl Default for PooledBuffer {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

impl Deref for PooledBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl AsRef<[u8]> for PooledBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl AsMut<[u8]> for PooledBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

impl From<Vec<u8>> for PooledBuffer {
    fn from(data: Vec<u8>) -> Self {
        Self::from_vec(data)
    }
}

impl From<PooledBuffer> for Vec<u8> {
    fn from(buffer: PooledBuffer) -> Self {
        buffer.into_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Basic Operations
    // =========================================================================

    #[test]
    fn test_buffer_basic() {
        let mut buf = PooledBuffer::with_capacity(100);
        assert!(buf.capacity() >= 100);
        assert!(buf.is_empty());

        buf.extend_from_slice(b"hello");
        assert_eq!(buf.len(), 5);
        assert_eq!(&*buf, b"hello");

        buf.clear();
        assert!(buf.is_empty());
        assert!(buf.capacity() >= 100);
    }

    #[test]
    fn test_buffer_from_vec() {
        let v = vec![1, 2, 3, 4, 5];
        let buf = PooledBuffer::from_vec(v);
        assert_eq!(buf.len(), 5);
        assert_eq!(&*buf, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_buffer_ensure_capacity() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.ensure_capacity(100);
        assert!(buf.capacity() >= 100);
    }

    #[test]
    fn test_buffer_ensure_capacity_already_sufficient() {
        let mut buf = PooledBuffer::with_capacity(100);
        let orig_cap = buf.capacity();
        buf.ensure_capacity(50);
        assert_eq!(buf.capacity(), orig_cap); // Should not change
    }

    #[test]
    fn test_buffer_ensure_capacity_with_data() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"hello");
        buf.ensure_capacity(100);
        assert!(buf.capacity() >= 100);
        assert_eq!(buf.len(), 5);
        assert_eq!(&*buf, b"hello"); // Data preserved
    }

    // =========================================================================
    // Default
    // =========================================================================

    #[test]
    fn test_buffer_default() {
        let buf = PooledBuffer::default();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    // =========================================================================
    // into_vec / as_vec / as_vec_mut
    // =========================================================================

    #[test]
    fn test_buffer_into_vec() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"hello");
        let v = buf.into_vec();
        assert_eq!(v, b"hello");
    }

    #[test]
    fn test_buffer_as_vec() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"hello");
        let v = buf.as_vec();
        assert_eq!(v.as_slice(), b"hello");
    }

    #[test]
    fn test_buffer_as_vec_mut() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"hello");
        let v = buf.as_vec_mut();
        v.push(b'!');
        assert_eq!(buf.len(), 6);
        assert_eq!(&*buf, b"hello!");
    }

    // =========================================================================
    // resize
    // =========================================================================

    #[test]
    fn test_buffer_resize_grow() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"hi");
        buf.resize(5);
        assert_eq!(buf.len(), 5);
        assert_eq!(&buf[0..2], b"hi");
        assert_eq!(&buf[2..5], &[0, 0, 0]); // Filled with zeros
    }

    #[test]
    fn test_buffer_resize_shrink() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"hello");
        buf.resize(2);
        assert_eq!(buf.len(), 2);
        assert_eq!(&*buf, b"he");
    }

    #[test]
    fn test_buffer_resize_same() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"hello");
        buf.resize(5);
        assert_eq!(buf.len(), 5);
        assert_eq!(&*buf, b"hello");
    }

    // =========================================================================
    // touch (LRU timestamp)
    // =========================================================================

    #[test]
    fn test_buffer_touch() {
        let mut buf = PooledBuffer::with_capacity(10);
        assert_eq!(buf.last_used, 0);
        buf.touch(42);
        assert_eq!(buf.last_used, 42);
        buf.touch(100);
        assert_eq!(buf.last_used, 100);
    }

    // =========================================================================
    // Deref / DerefMut
    // =========================================================================

    #[test]
    fn test_buffer_deref() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"test");
        let slice: &[u8] = &buf;
        assert_eq!(slice, b"test");
    }

    #[test]
    fn test_buffer_deref_mut() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"test");
        let slice: &mut [u8] = &mut buf;
        slice[0] = b'T';
        assert_eq!(&*buf, b"Test");
    }

    // =========================================================================
    // AsRef / AsMut
    // =========================================================================

    #[test]
    fn test_buffer_as_ref() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"ref");
        let r: &[u8] = buf.as_ref();
        assert_eq!(r, b"ref");
    }

    #[test]
    fn test_buffer_as_mut() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"mut");
        let m: &mut [u8] = buf.as_mut();
        m[0] = b'M';
        assert_eq!(&*buf, b"Mut");
    }

    // =========================================================================
    // From conversions
    // =========================================================================

    #[test]
    fn test_buffer_from_vec_trait() {
        let v = vec![1u8, 2, 3];
        let buf: PooledBuffer = v.into();
        assert_eq!(buf.len(), 3);
        assert_eq!(&*buf, &[1, 2, 3]);
    }

    #[test]
    fn test_buffer_into_vec_trait() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(&[4, 5, 6]);
        let v: Vec<u8> = buf.into();
        assert_eq!(v, vec![4, 5, 6]);
    }

    // =========================================================================
    // Debug
    // =========================================================================

    #[test]
    fn test_buffer_debug() {
        let buf = PooledBuffer::with_capacity(10);
        let debug_str = format!("{buf:?}");
        assert!(debug_str.contains("PooledBuffer"));
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_buffer_zero_capacity() {
        let buf = PooledBuffer::with_capacity(0);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_buffer_large_capacity() {
        let buf = PooledBuffer::with_capacity(1_000_000);
        assert!(buf.capacity() >= 1_000_000);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_buffer_empty_vec() {
        let buf = PooledBuffer::from_vec(Vec::new());
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_buffer_extend_multiple_times() {
        let mut buf = PooledBuffer::with_capacity(10);
        buf.extend_from_slice(b"a");
        buf.extend_from_slice(b"bc");
        buf.extend_from_slice(b"def");
        assert_eq!(buf.len(), 6);
        assert_eq!(&*buf, b"abcdef");
    }
}
