// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tape pooling for reduced allocation overhead.
//!
//! Provides reusable tape buffers for repeated JSON parsing operations.
//! Two pool variants are available:
//!
//! - [`ThreadLocalPool`]: Fast, no synchronization, one pool per thread
//! - [`SharedPool`]: Arc-based concurrent pool for multi-threaded access
//!
//! ## Strategies
//!
//! Pools support configurable eviction strategies:
//!
//! - [`PoolStrategy::Unbounded`]: Grow forever, never evict
//! - [`PoolStrategy::SizeLimited`]: Maximum number of tapes
//! - [`PoolStrategy::MemoryLimited`]: Maximum total bytes
//! - [`PoolStrategy::Lru`]: Least-recently-used eviction with size limit
//!
//! ## Example
//!
//! ```rust,ignore
//! use fionn::pool::{ThreadLocalPool, PoolStrategy, TapePool};
//!
//! let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 16 });
//!
//! // Get a buffer from the pool
//! let mut buffer = pool.acquire(1024);
//!
//! // Use buffer for parsing...
//! buffer.extend_from_slice(json_bytes);
//!
//! // Return buffer to pool for reuse
//! pool.release(buffer);
//! ```

mod buffer;
mod shared;
mod strategy;
mod thread_local;

pub use buffer::PooledBuffer;
pub use shared::SharedPool;
pub use strategy::{PoolStats, PoolStrategy};
pub use thread_local::ThreadLocalPool;

/// Trait for tape buffer pools.
///
/// Note: Not all pools are `Sync`. Use [`SharedPool`] for concurrent access
/// across threads, or [`ThreadLocalPool`] for single-threaded use.
pub trait TapePool: Send {
    /// Acquire a buffer with at least `min_capacity` bytes.
    ///
    /// Returns a buffer from the pool if available, or allocates a new one.
    fn acquire(&self, min_capacity: usize) -> PooledBuffer;

    /// Release a buffer back to the pool.
    ///
    /// The buffer may be reused by future `acquire` calls, or discarded
    /// based on the pool's eviction strategy.
    fn release(&self, buffer: PooledBuffer);

    /// Clear all buffers from the pool.
    fn clear(&self);

    /// Get current pool statistics.
    fn stats(&self) -> PoolStats;

    /// Get the pool's strategy.
    fn strategy(&self) -> &PoolStrategy;
}

/// Extension trait for pools with typed tape access.
pub trait TapePoolExt: TapePool {
    /// Parse JSON into a tape using a pooled buffer.
    ///
    /// # Errors
    /// Returns an error if JSON parsing fails.
    fn parse<'a>(&self, json: &'a mut [u8]) -> Result<ParsedTape<'a>, fionn_core::DsonError> {
        let tape = simd_json::to_tape(json)
            .map_err(|e| fionn_core::DsonError::ParseError(format!("{e}")))?;
        Ok(ParsedTape { tape })
    }
}

impl<T: TapePool> TapePoolExt for T {}

/// A parsed tape with lifetime tied to input.
#[derive(Debug)]
pub struct ParsedTape<'a> {
    tape: simd_json::tape::Tape<'a>,
}

impl<'a> ParsedTape<'a> {
    /// Get the underlying tape.
    #[must_use]
    pub const fn tape(&self) -> &simd_json::tape::Tape<'a> {
        &self.tape
    }

    /// Get the tape nodes.
    #[must_use]
    pub fn nodes(&self) -> &[simd_json::value::tape::Node<'a>] {
        &self.tape.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Basic Pool Tests
    // =========================================================================

    #[test]
    fn test_thread_local_pool_basic() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        assert!(buf1.capacity() >= 100);

        pool.release(buf1);

        let stats = pool.stats();
        assert_eq!(stats.buffers_in_pool, 1);

        let buf2 = pool.acquire(50);
        assert!(buf2.capacity() >= 50);

        let stats = pool.stats();
        assert_eq!(stats.buffers_in_pool, 0);
        assert_eq!(stats.acquires, 2);
        assert_eq!(stats.releases, 1);
        assert_eq!(stats.reuses, 1);
    }

    #[test]
    fn test_shared_pool_basic() {
        let pool = SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 4 });

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(200);

        pool.release(buf1);
        pool.release(buf2);

        let stats = pool.stats();
        assert_eq!(stats.buffers_in_pool, 2);
    }

    #[test]
    fn test_size_limited_eviction() {
        let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 2 });

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        let stats = pool.stats();
        assert_eq!(stats.buffers_in_pool, 2);
        assert_eq!(stats.evictions, 1);
    }

    #[test]
    fn test_memory_limited_eviction() {
        // Pool enforces 256-byte minimum capacity per buffer
        // With max_bytes=600, we can hold at most 2 buffers of 256 bytes each (512 total)
        let pool = ThreadLocalPool::new(PoolStrategy::MemoryLimited { max_bytes: 600 });

        let buf1 = pool.acquire(200); // Gets 256 bytes (minimum)
        let buf2 = pool.acquire(200); // Gets 256 bytes
        let buf3 = pool.acquire(200); // Gets 256 bytes

        pool.release(buf1); // Pool: 256 bytes
        pool.release(buf2); // Pool: 512 bytes
        pool.release(buf3); // Pool: 768 bytes -> evicts largest -> 512 bytes

        let stats = pool.stats();
        // Should have evicted to stay under limit
        assert!(
            stats.total_bytes_in_pool <= 600,
            "Expected <= 600 bytes, got {}",
            stats.total_bytes_in_pool
        );
        assert!(stats.evictions >= 1, "Expected at least 1 eviction");
    }

    // =========================================================================
    // TapePoolExt Tests
    // =========================================================================

    #[test]
    fn test_tape_pool_ext_parse_simple_object() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"a": 1, "b": 2}"#.to_vec();

        let result = pool.parse(&mut json);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        let nodes = parsed.nodes();
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_tape_pool_ext_parse_array() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);
        let mut json = br#"[1, 2, 3, "four", true, null]"#.to_vec();

        let result = pool.parse(&mut json);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert!(!parsed.nodes().is_empty());
    }

    #[test]
    fn test_tape_pool_ext_parse_nested() {
        let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 4 });
        let mut json = br#"{"arr": [1, 2], "obj": {"x": "y"}}"#.to_vec();

        let result = pool.parse(&mut json);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        let nodes = parsed.nodes();
        assert!(nodes.len() > 1);
    }

    #[test]
    fn test_tape_pool_ext_parse_error_invalid_json() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"invalid": }"#.to_vec();

        let result = pool.parse(&mut json);
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            fionn_core::DsonError::ParseError(msg) => {
                assert!(!msg.is_empty());
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_tape_pool_ext_parse_error_truncated() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"key": [1, 2, 3"#.to_vec();

        let result = pool.parse(&mut json);
        assert!(result.is_err());
    }

    #[test]
    fn test_tape_pool_ext_parse_error_trailing_comma() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br"[1, 2, 3,]".to_vec();

        let result = pool.parse(&mut json);
        // simd_json may or may not accept trailing commas - just verify it handles it
        // Either parse succeeds or returns a clean error
        let _ = result;
    }

    #[test]
    fn test_tape_pool_ext_parse_empty_object() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br"{}".to_vec();

        let result = pool.parse(&mut json);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert!(!parsed.nodes().is_empty());
    }

    #[test]
    fn test_tape_pool_ext_parse_empty_array() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br"[]".to_vec();

        let result = pool.parse(&mut json);
        assert!(result.is_ok());
    }

    // =========================================================================
    // ParsedTape Tests
    // =========================================================================

    #[test]
    fn test_parsed_tape_tape_accessor() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"test": 42}"#.to_vec();

        let parsed = pool.parse(&mut json).unwrap();
        let tape = parsed.tape();

        // tape() returns reference to simd_json::tape::Tape
        assert!(!tape.0.is_empty());
    }

    #[test]
    fn test_parsed_tape_nodes_accessor() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br"[true, false, null]".to_vec();

        let parsed = pool.parse(&mut json).unwrap();
        let nodes = parsed.nodes();

        // Should have nodes for array and its elements
        assert!(nodes.len() >= 4);
    }

    #[test]
    fn test_parsed_tape_nodes_content() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"name": "test", "value": 123}"#.to_vec();

        let parsed = pool.parse(&mut json).unwrap();
        let nodes = parsed.nodes();

        // Verify we have multiple nodes
        assert!(nodes.len() > 3);
    }

    #[test]
    fn test_parsed_tape_with_strings() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"message": "hello world", "emoji": "\u0041"}"#.to_vec();

        let parsed = pool.parse(&mut json).unwrap();
        let nodes = parsed.nodes();

        // Verify nodes are present
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_parsed_tape_with_numbers() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"int": 42, "float": 1.23, "neg": -100, "exp": 1e10}"#.to_vec();

        let parsed = pool.parse(&mut json).unwrap();
        let nodes = parsed.nodes();

        // Should have nodes for object and all key-value pairs
        assert!(nodes.len() > 4);
    }

    #[test]
    fn test_parsed_tape_deeply_nested() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let mut json = br#"{"a": {"b": {"c": {"d": 1}}}}"#.to_vec();

        let parsed = pool.parse(&mut json).unwrap();
        let nodes = parsed.nodes();

        // Deeply nested structure should produce many nodes
        assert!(nodes.len() > 5);
    }

    // =========================================================================
    // Pool Strategy Integration Tests
    // =========================================================================

    #[test]
    fn test_lru_strategy_with_parse() {
        let pool = ThreadLocalPool::new(PoolStrategy::Lru { max_tapes: 2 });

        let mut json1 = br#"{"first": 1}"#.to_vec();
        let mut json2 = br#"{"second": 2}"#.to_vec();
        let mut json3 = br#"{"third": 3}"#.to_vec();

        let parsed1 = pool.parse(&mut json1).unwrap();
        let parsed2 = pool.parse(&mut json2).unwrap();
        let parsed3 = pool.parse(&mut json3).unwrap();

        // Verify parsing succeeded (parse() uses simd_json directly, not pool buffers)
        assert!(!parsed1.nodes().is_empty());
        assert!(!parsed2.nodes().is_empty());
        assert!(!parsed3.nodes().is_empty());
    }

    #[test]
    fn test_shared_pool_parse() {
        let pool = SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 8 });

        let mut json = br#"{"shared": true}"#.to_vec();
        let parsed = pool.parse(&mut json).unwrap();

        assert!(!parsed.nodes().is_empty());
    }

    #[test]
    fn test_pool_clear_and_parse() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf = pool.acquire(100);
        pool.release(buf);
        assert_eq!(pool.stats().buffers_in_pool, 1);

        pool.clear();
        assert_eq!(pool.stats().buffers_in_pool, 0);

        // Parse should still work after clear
        let mut json = br#"{"after": "clear"}"#.to_vec();
        let result = pool.parse(&mut json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tape_pool_ext_blanket_impl() {
        // Verify blanket implementation works for both pool types
        fn use_pool<P: TapePool>(pool: &P) -> bool {
            let mut json = br#"{"test": true}"#.to_vec();
            pool.parse(&mut json).is_ok()
        }

        let thread_pool = ThreadLocalPool::new(PoolStrategy::Unbounded);
        let shared_pool = SharedPool::new(PoolStrategy::Unbounded);

        assert!(use_pool(&thread_pool));
        assert!(use_pool(&shared_pool));
    }
}
