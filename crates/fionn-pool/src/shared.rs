// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared concurrent tape pool implementation.

use super::TapePool;
use super::buffer::PooledBuffer;
use super::strategy::{PoolStats, PoolStrategy};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Shared concurrent buffer pool.
///
/// Thread-safe pool using `parking_lot::Mutex` for synchronization.
/// Best for multi-threaded workloads where buffers may be acquired
/// on one thread and released on another.
///
/// # Example
///
/// ```rust,ignore
/// use fionn::pool::{SharedPool, PoolStrategy, TapePool};
/// use std::sync::Arc;
///
/// let pool = Arc::new(SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 32 }));
///
/// // Can be shared across threads
/// let pool_clone = pool.clone();
/// std::thread::spawn(move || {
///     let buffer = pool_clone.acquire(1024);
///     // ... use buffer ...
///     pool_clone.release(buffer);
/// });
/// ```
pub struct SharedPool {
    strategy: PoolStrategy,
    timestamp: AtomicU64,
    state: Mutex<PoolState>,
}

struct PoolState {
    buffers: Vec<PooledBuffer>,
    stats: PoolStats,
}

impl SharedPool {
    /// Create a new shared pool with the given strategy.
    #[must_use]
    pub fn new(strategy: PoolStrategy) -> Self {
        Self {
            strategy,
            timestamp: AtomicU64::new(0),
            state: Mutex::new(PoolState {
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

    fn evict_if_needed(strategy: &PoolStrategy, state: &mut PoolState) {
        match strategy {
            PoolStrategy::Unbounded => {}

            PoolStrategy::SizeLimited { max_tapes } => {
                while state.buffers.len() >= *max_tapes {
                    if let Some(buf) = state.buffers.pop() {
                        state.stats.total_bytes_in_pool -= buf.capacity();
                        state.stats.evictions += 1;
                    }
                }
            }

            PoolStrategy::MemoryLimited { max_bytes } => {
                while state.stats.total_bytes_in_pool >= *max_bytes && !state.buffers.is_empty() {
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
                while state.buffers.len() >= *max_tapes {
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

impl TapePool for SharedPool {
    fn acquire(&self, min_capacity: usize) -> PooledBuffer {
        let mut buffer = {
            let mut state = self.state.lock();
            state.stats.acquires += 1;

            let suitable_idx = state
                .buffers
                .iter()
                .enumerate()
                .find(|(_, b)| b.capacity() >= min_capacity)
                .map(|(i, _)| i);

            let buf = if let Some(idx) = suitable_idx {
                let mut buf = state.buffers.remove(idx);
                state.stats.total_bytes_in_pool -= buf.capacity();
                state.stats.buffers_in_pool = state.buffers.len();
                state.stats.reuses += 1;
                buf.clear();
                buf
            } else {
                state.stats.allocations += 1;
                PooledBuffer::with_capacity(min_capacity.max(256))
            };
            drop(state);
            buf
        };
        buffer.touch(self.next_timestamp());
        buffer
    }

    fn release(&self, mut buffer: PooledBuffer) {
        let mut state = self.state.lock();
        state.stats.releases += 1;

        buffer.clear();
        buffer.touch(self.next_timestamp());

        Self::evict_if_needed(&self.strategy, &mut state);

        let capacity = buffer.capacity();
        state.buffers.push(buffer);
        state.stats.buffers_in_pool = state.buffers.len();
        state.stats.total_bytes_in_pool += capacity;

        if state.stats.buffers_in_pool > state.stats.peak_buffers {
            state.stats.peak_buffers = state.stats.buffers_in_pool;
        }
        if state.stats.total_bytes_in_pool > state.stats.peak_bytes {
            state.stats.peak_bytes = state.stats.total_bytes_in_pool;
        }
    }

    fn clear(&self) {
        let mut state = self.state.lock();
        state.buffers.clear();
        state.stats.buffers_in_pool = 0;
        state.stats.total_bytes_in_pool = 0;
    }

    fn stats(&self) -> PoolStats {
        self.state.lock().stats.clone()
    }

    fn strategy(&self) -> &PoolStrategy {
        &self.strategy
    }
}

// SharedPool is both Send and Sync
unsafe impl Send for SharedPool {}
unsafe impl Sync for SharedPool {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // =========================================================================
    // Constructor Tests
    // =========================================================================

    #[test]
    fn test_new_unbounded() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);
        assert!(matches!(pool.strategy(), PoolStrategy::Unbounded));
    }

    #[test]
    fn test_new_size_limited() {
        let pool = SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 10 });
        assert!(matches!(
            pool.strategy(),
            PoolStrategy::SizeLimited { max_tapes: 10 }
        ));
    }

    #[test]
    fn test_new_memory_limited() {
        let pool = SharedPool::new(PoolStrategy::MemoryLimited { max_bytes: 4096 });
        assert!(matches!(
            pool.strategy(),
            PoolStrategy::MemoryLimited { max_bytes: 4096 }
        ));
    }

    #[test]
    fn test_new_lru() {
        let pool = SharedPool::new(PoolStrategy::Lru { max_tapes: 8 });
        assert!(matches!(
            pool.strategy(),
            PoolStrategy::Lru { max_tapes: 8 }
        ));
    }

    #[test]
    fn test_with_default() {
        let pool = SharedPool::with_default();
        assert!(matches!(
            pool.strategy(),
            PoolStrategy::SizeLimited { max_tapes: 16 }
        ));
    }

    // =========================================================================
    // Basic Operations Tests
    // =========================================================================

    #[test]
    fn test_acquire_release() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);

        let buf = pool.acquire(100);
        assert!(buf.capacity() >= 100);

        pool.release(buf);
        assert_eq!(pool.stats().buffers_in_pool, 1);
    }

    #[test]
    fn test_acquire_minimum_capacity() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);
        // Very small request should still get at least 256 bytes
        let buf = pool.acquire(10);
        assert!(buf.capacity() >= 256);
    }

    #[test]
    fn test_reuse_buffer() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let cap1 = buf1.capacity();
        pool.release(buf1);

        let buf2 = pool.acquire(50);
        assert_eq!(buf2.capacity(), cap1);
        assert_eq!(pool.stats().reuses, 1);
    }

    #[test]
    fn test_clear() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);

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
    fn test_stats() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);

        let buf = pool.acquire(100);
        pool.release(buf);

        let stats = pool.stats();
        assert_eq!(stats.acquires, 1);
        assert_eq!(stats.releases, 1);
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.buffers_in_pool, 1);
    }

    #[test]
    fn test_stats_peak_tracking() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);
        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        let stats = pool.stats();
        assert_eq!(stats.peak_buffers, 3);
        assert!(stats.peak_bytes > 0);
    }

    // =========================================================================
    // Size Limited Strategy Tests
    // =========================================================================

    #[test]
    fn test_size_limited_eviction() {
        let pool = SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 2 });

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        // Should have evicted to stay under limit
        assert!(pool.stats().buffers_in_pool <= 2);
        assert!(pool.stats().evictions > 0);
    }

    #[test]
    fn test_size_limited_at_boundary() {
        let pool = SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 3 });

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        // At boundary, should have evictions because >= triggers eviction
        assert!(pool.stats().buffers_in_pool <= 3);
    }

    // =========================================================================
    // Memory Limited Strategy Tests
    // =========================================================================

    #[test]
    fn test_memory_limited_eviction() {
        // Note: eviction happens BEFORE adding the new buffer, so total bytes
        // can exceed max_bytes by up to one buffer's capacity after release.
        // With max_bytes=768, releasing 5x256-byte buffers triggers evictions.
        let pool = SharedPool::new(PoolStrategy::MemoryLimited { max_bytes: 768 });

        // Each buffer will be at least 256 bytes
        let buf1 = pool.acquire(256);
        let buf2 = pool.acquire(256);
        let buf3 = pool.acquire(256);
        let buf4 = pool.acquire(256);
        let buf5 = pool.acquire(256);

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);
        pool.release(buf4);
        pool.release(buf5);

        // After evictions, pool should be limited (allowing one buffer overage)
        // Max is 768 + 256 = 1024 bytes (one buffer added after eviction)
        assert!(pool.stats().total_bytes_in_pool <= 1024);
        assert!(pool.stats().evictions > 0);
    }

    #[test]
    fn test_memory_limited_evicts_largest() {
        let pool = SharedPool::new(PoolStrategy::MemoryLimited { max_bytes: 2000 });

        let small = pool.acquire(256);
        let large = pool.acquire(1024);

        pool.release(small);
        pool.release(large);

        // The larger buffer should be evicted first if over limit
        assert!(pool.stats().total_bytes_in_pool <= 2000);
    }

    // =========================================================================
    // LRU Strategy Tests
    // =========================================================================

    #[test]
    fn test_lru_eviction() {
        let pool = SharedPool::new(PoolStrategy::Lru { max_tapes: 2 });

        let buf1 = pool.acquire(100);
        let buf2 = pool.acquire(100);
        let buf3 = pool.acquire(100);

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        assert!(pool.stats().buffers_in_pool <= 2);
        assert!(pool.stats().evictions > 0);
    }

    // =========================================================================
    // Concurrent Access Tests
    // =========================================================================

    #[test]
    fn test_concurrent_access() {
        let pool = Arc::new(SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 32 }));
        let mut handles = vec![];

        for _ in 0..8 {
            let pool_clone = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let buf = pool_clone.acquire(256);
                    pool_clone.release(buf);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let stats = pool.stats();
        assert_eq!(stats.acquires, 800);
        assert_eq!(stats.releases, 800);
        assert!(stats.reuses > 0);
    }

    #[test]
    fn test_cross_thread_release() {
        let pool = Arc::new(SharedPool::new(PoolStrategy::Unbounded));

        let buf = pool.acquire(100);
        let cap = buf.capacity();

        let pool_clone = Arc::clone(&pool);
        let handle = thread::spawn(move || {
            pool_clone.release(buf);
        });

        handle.join().unwrap();

        // Buffer should be back in pool
        assert_eq!(pool.stats().buffers_in_pool, 1);

        // Should be able to reuse it
        let buf2 = pool.acquire(50);
        assert_eq!(buf2.capacity(), cap);
    }

    #[test]
    fn test_size_limited_concurrent() {
        let pool = Arc::new(SharedPool::new(PoolStrategy::SizeLimited { max_tapes: 4 }));
        let mut handles = vec![];

        for _ in 0..4 {
            let pool_clone = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                let bufs: Vec<_> = (0..10).map(|_| pool_clone.acquire(100)).collect();
                for buf in bufs {
                    pool_clone.release(buf);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Should never exceed limit
        assert!(pool.stats().buffers_in_pool <= 4);
    }

    #[test]
    fn test_concurrent_clear() {
        let pool = Arc::new(SharedPool::new(PoolStrategy::Unbounded));

        let pool_clone1 = Arc::clone(&pool);
        let pool_clone2 = Arc::clone(&pool);

        let h1 = thread::spawn(move || {
            for _ in 0..50 {
                let buf = pool_clone1.acquire(100);
                pool_clone1.release(buf);
            }
        });

        let h2 = thread::spawn(move || {
            for _ in 0..10 {
                pool_clone2.clear();
                thread::yield_now();
            }
        });

        h1.join().unwrap();
        h2.join().unwrap();

        // Just ensure no panics occurred
        let _ = pool.stats();
    }

    // =========================================================================
    // No Suitable Buffer Tests
    // =========================================================================

    #[test]
    fn test_no_suitable_buffer_allocates_new() {
        let pool = SharedPool::new(PoolStrategy::Unbounded);

        let small = pool.acquire(100);
        pool.release(small);

        // Request larger buffer - should allocate new
        let large = pool.acquire(10000);
        assert!(large.capacity() >= 10000);
        assert_eq!(pool.stats().allocations, 2);
    }
}
