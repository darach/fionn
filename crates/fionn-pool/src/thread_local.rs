// SPDX-License-Identifier: MIT OR Apache-2.0
//! Thread-local tape pool implementation.

use super::TapePool;
use super::buffer::PooledBuffer;
use super::strategy::{PoolStats, PoolStrategy};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Thread-local buffer pool.
///
/// Each thread gets its own pool with no synchronization overhead.
/// Best for single-threaded workloads or when buffers don't need
/// to be shared across threads.
///
/// # Example
///
/// ```rust,ignore
/// use fionn::pool::{ThreadLocalPool, PoolStrategy, TapePool};
///
/// let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 8 });
///
/// let buffer = pool.acquire(1024);
/// // ... use buffer ...
/// pool.release(buffer);
/// ```
pub struct ThreadLocalPool {
    strategy: PoolStrategy,
    timestamp: AtomicU64,
    state: RefCell<PoolState>,
}

struct PoolState {
    buffers: Vec<PooledBuffer>,
    stats: PoolStats,
}

impl ThreadLocalPool {
    /// Create a new thread-local pool with the given strategy.
    #[must_use]
    pub fn new(strategy: PoolStrategy) -> Self {
        Self {
            strategy,
            timestamp: AtomicU64::new(0),
            state: RefCell::new(PoolState {
                buffers: Vec::new(),
                stats: PoolStats::default(),
            }),
        }
    }

    /// Create with default strategy (size-limited to 16 buffers).
    #[must_use]
    pub fn with_default() -> Self {
        Self::new(PoolStrategy::default())
    }

    fn next_timestamp(&self) -> u64 {
        self.timestamp.fetch_add(1, Ordering::Relaxed)
    }

    fn evict_if_needed(&self, state: &mut PoolState) {
        match &self.strategy {
            PoolStrategy::Unbounded => {}

            PoolStrategy::SizeLimited { max_tapes } => {
                // Evict until at or under the limit (buffer already added)
                while state.buffers.len() > *max_tapes {
                    if let Some(buf) = state.buffers.pop() {
                        state.stats.total_bytes_in_pool -= buf.capacity();
                        state.stats.evictions += 1;
                    }
                }
            }

            PoolStrategy::MemoryLimited { max_bytes } => {
                // Evict largest buffers until under the limit
                while state.stats.total_bytes_in_pool > *max_bytes && !state.buffers.is_empty() {
                    let max_idx = state
                        .buffers
                        .iter()
                        .enumerate()
                        .max_by_key(|(_, b)| b.capacity())
                        .map(|(i, _)| i);

                    if let Some(idx) = max_idx {
                        let buf = state.buffers.remove(idx);
                        state.stats.total_bytes_in_pool -= buf.capacity();
                        state.stats.evictions += 1;
                    }
                }
            }

            PoolStrategy::Lru { max_tapes } => {
                // Evict LRU until at or under the limit (buffer already added)
                while state.buffers.len() > *max_tapes {
                    let lru_idx = state
                        .buffers
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, b)| b.last_used)
                        .map(|(i, _)| i);

                    if let Some(idx) = lru_idx {
                        let buf = state.buffers.remove(idx);
                        state.stats.total_bytes_in_pool -= buf.capacity();
                        state.stats.evictions += 1;
                    }
                }
            }
        }
    }
}

impl TapePool for ThreadLocalPool {
    fn acquire(&self, min_capacity: usize) -> PooledBuffer {
        let mut state = self.state.borrow_mut();
        state.stats.acquires += 1;

        let suitable_idx = state
            .buffers
            .iter()
            .enumerate()
            .find(|(_, b)| b.capacity() >= min_capacity)
            .map(|(i, _)| i);

        if let Some(idx) = suitable_idx {
            let mut buffer = state.buffers.remove(idx);
            state.stats.total_bytes_in_pool -= buffer.capacity();
            state.stats.buffers_in_pool = state.buffers.len();
            state.stats.reuses += 1;
            buffer.clear();
            buffer.touch(self.next_timestamp());
            buffer
        } else {
            state.stats.allocations += 1;
            let mut buffer = PooledBuffer::with_capacity(min_capacity.max(256));
            buffer.touch(self.next_timestamp());
            buffer
        }
    }

    fn release(&self, mut buffer: PooledBuffer) {
        let mut state = self.state.borrow_mut();
        state.stats.releases += 1;

        buffer.clear();
        buffer.touch(self.next_timestamp());

        // Add buffer first, then evict if needed
        let capacity = buffer.capacity();
        state.buffers.push(buffer);
        state.stats.buffers_in_pool = state.buffers.len();
        state.stats.total_bytes_in_pool += capacity;

        // Evict after adding to ensure limits are enforced
        self.evict_if_needed(&mut state);

        // Update stats after eviction
        state.stats.buffers_in_pool = state.buffers.len();

        if state.stats.buffers_in_pool > state.stats.peak_buffers {
            state.stats.peak_buffers = state.stats.buffers_in_pool;
        }
        if state.stats.total_bytes_in_pool > state.stats.peak_bytes {
            state.stats.peak_bytes = state.stats.total_bytes_in_pool;
        }
    }

    fn clear(&self) {
        let mut state = self.state.borrow_mut();
        state.buffers.clear();
        state.stats.buffers_in_pool = 0;
        state.stats.total_bytes_in_pool = 0;
    }

    fn stats(&self) -> PoolStats {
        self.state.borrow().stats.clone()
    }

    fn strategy(&self) -> &PoolStrategy {
        &self.strategy
    }
}

// ThreadLocalPool is !Sync due to RefCell - intentional for single-threaded use
unsafe impl Send for ThreadLocalPool {}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Constructor Tests
    // =========================================================================

    #[test]
    fn test_acquire_release() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf = pool.acquire(100);
        assert!(buf.capacity() >= 100);

        pool.release(buf);
        assert_eq!(pool.stats().buffers_in_pool, 1);
    }

    #[test]
    fn test_with_default() {
        let pool = ThreadLocalPool::with_default();
        assert!(matches!(
            pool.strategy(),
            PoolStrategy::SizeLimited { max_tapes: 16 }
        ));
    }

    #[test]
    fn test_strategy_accessor() {
        let pool = ThreadLocalPool::new(PoolStrategy::Lru { max_tapes: 8 });
        assert!(matches!(
            pool.strategy(),
            PoolStrategy::Lru { max_tapes: 8 }
        ));
    }

    // =========================================================================
    // Reuse Tests
    // =========================================================================

    #[test]
    fn test_reuse() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let cap1 = buf1.capacity();
        pool.release(buf1);

        let buf2 = pool.acquire(50);
        assert_eq!(buf2.capacity(), cap1);
        assert_eq!(pool.stats().reuses, 1);
    }

    #[test]
    fn test_no_reuse_when_too_small() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        pool.release(buf1);

        // Request larger buffer - won't reuse
        let buf2 = pool.acquire(10000);
        assert!(buf2.capacity() >= 10000);
        assert_eq!(pool.stats().allocations, 2);
        assert_eq!(pool.stats().reuses, 0);
    }

    // =========================================================================
    // Size Limited Strategy Tests
    // =========================================================================

    #[test]
    fn test_size_limited() {
        let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 2 });

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        assert_eq!(pool.stats().buffers_in_pool, 2);
        assert_eq!(pool.stats().evictions, 1);
    }

    #[test]
    fn test_size_limited_many_evictions() {
        let pool = ThreadLocalPool::new(PoolStrategy::SizeLimited { max_tapes: 2 });

        let bufs: Vec<_> = (0..10).map(|_| pool.acquire(100)).collect();

        for buf in bufs {
            pool.release(buf);
        }

        assert_eq!(pool.stats().buffers_in_pool, 2);
        assert_eq!(pool.stats().evictions, 8);
    }

    // =========================================================================
    // Memory Limited Strategy Tests
    // =========================================================================

    #[test]
    fn test_memory_limited_eviction() {
        // Each buffer is at least 256 bytes
        // With max_bytes=300, we should only be able to hold 1 buffer
        let pool = ThreadLocalPool::new(PoolStrategy::MemoryLimited { max_bytes: 300 });

        let buf1 = pool.acquire(256);
        let buf2 = pool.acquire(256);

        pool.release(buf1);
        pool.release(buf2);

        // Should have evicted to get under 300 bytes
        assert!(pool.stats().total_bytes_in_pool <= 300);
        assert!(pool.stats().evictions >= 1);
    }

    #[test]
    fn test_memory_limited_evicts_largest() {
        // Eviction should remove largest buffers first
        let pool = ThreadLocalPool::new(PoolStrategy::MemoryLimited { max_bytes: 1500 });

        let small = pool.acquire(256);
        let large = pool.acquire(1024);

        pool.release(small);
        pool.release(large);

        // At this point we might need to evict to stay under limit
        let stats = pool.stats();
        assert!(stats.total_bytes_in_pool <= 1500);
    }

    #[test]
    fn test_memory_limited_multiple_evictions() {
        let pool = ThreadLocalPool::new(PoolStrategy::MemoryLimited { max_bytes: 600 });

        let bufs: Vec<_> = (0..5).map(|_| pool.acquire(256)).collect();

        for buf in bufs {
            pool.release(buf);
        }

        assert!(pool.stats().total_bytes_in_pool <= 600);
        assert!(pool.stats().evictions >= 3);
    }

    // =========================================================================
    // LRU Strategy Tests
    // =========================================================================

    #[test]
    fn test_lru_eviction() {
        let pool = ThreadLocalPool::new(PoolStrategy::Lru { max_tapes: 2 });

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);

        pool.release(buf1);
        pool.release(buf2);

        let buf3 = pool.acquire(100);
        pool.release(buf3);

        let buf4 = pool.acquire(100);
        pool.release(buf4);

        assert_eq!(pool.stats().buffers_in_pool, 2);
    }

    #[test]
    fn test_lru_multiple_evictions() {
        let pool = ThreadLocalPool::new(PoolStrategy::Lru { max_tapes: 3 });

        let bufs: Vec<_> = (0..10).map(|_| pool.acquire(100)).collect();

        for buf in bufs {
            pool.release(buf);
        }

        assert_eq!(pool.stats().buffers_in_pool, 3);
        assert_eq!(pool.stats().evictions, 7);
    }

    // =========================================================================
    // Clear Tests
    // =========================================================================

    #[test]
    fn test_clear() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        pool.release(buf1);
        pool.release(buf2);

        assert_eq!(pool.stats().buffers_in_pool, 2);

        pool.clear();
        assert_eq!(pool.stats().buffers_in_pool, 0);
        assert_eq!(pool.stats().total_bytes_in_pool, 0);
    }

    #[test]
    fn test_clear_then_use() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        pool.release(buf1);
        pool.clear();

        // Should allocate new buffer after clear
        let buf2 = pool.acquire(100);
        assert!(buf2.capacity() >= 100);
        assert_eq!(pool.stats().allocations, 2);
        assert_eq!(pool.stats().reuses, 0);
    }

    // =========================================================================
    // Stats Tests
    // =========================================================================

    #[test]
    fn test_stats_acquires() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let _ = pool.acquire(100);
        let _ = pool.acquire(200);
        let _ = pool.acquire(300);

        assert_eq!(pool.stats().acquires, 3);
    }

    #[test]
    fn test_stats_releases() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        pool.release(buf1);
        pool.release(buf2);

        assert_eq!(pool.stats().releases, 2);
    }

    #[test]
    fn test_stats_allocations() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(200);
        pool.release(buf1);

        // Next acquire should reuse
        let buf3 = pool.acquire(50);
        pool.release(buf3);
        pool.release(buf2);

        assert_eq!(pool.stats().allocations, 2);
        assert_eq!(pool.stats().reuses, 1);
    }

    #[test]
    fn test_stats_peak_buffers() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        assert_eq!(pool.stats().peak_buffers, 3);

        pool.clear();
        // Peak should remain at 3
        assert_eq!(pool.stats().peak_buffers, 3);
    }

    #[test]
    fn test_stats_peak_bytes() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        pool.release(buf1);
        pool.release(buf2);

        let peak = pool.stats().peak_bytes;
        assert!(peak > 0);

        pool.clear();
        // Peak bytes should remain after clear
        assert_eq!(pool.stats().peak_bytes, peak);
    }

    #[test]
    fn test_stats_total_bytes_in_pool() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf = pool.acquire(256);
        let cap = buf.capacity();
        pool.release(buf);

        assert_eq!(pool.stats().total_bytes_in_pool, cap);
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_minimum_capacity() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        // Very small request should get at least 256 bytes
        let buf = pool.acquire(1);
        assert!(buf.capacity() >= 256);
    }

    #[test]
    fn test_zero_capacity_request() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf = pool.acquire(0);
        assert!(buf.capacity() >= 256);
    }

    #[test]
    fn test_large_capacity_request() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let buf = pool.acquire(1_000_000);
        assert!(buf.capacity() >= 1_000_000);
    }

    #[test]
    fn test_unbounded_no_evictions() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let bufs: Vec<_> = (0..100).map(|_| pool.acquire(100)).collect();

        for buf in bufs {
            pool.release(buf);
        }

        assert_eq!(pool.stats().evictions, 0);
        assert_eq!(pool.stats().buffers_in_pool, 100);
    }

    #[test]
    fn test_buffer_cleared_on_release() {
        let pool = ThreadLocalPool::new(PoolStrategy::Unbounded);

        let mut buf = pool.acquire(100);
        buf.extend_from_slice(b"test data");
        pool.release(buf);

        let buf2 = pool.acquire(50);
        // Buffer should be empty (cleared on release)
        assert_eq!(buf2.len(), 0);
    }
}
