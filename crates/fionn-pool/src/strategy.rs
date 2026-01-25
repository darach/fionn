// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pool eviction strategies.

/// Pool eviction strategy configuration.
#[derive(Debug, Clone)]
pub enum PoolStrategy {
    /// No eviction - pool grows unbounded.
    ///
    /// Best for applications with predictable, bounded buffer needs.
    Unbounded,

    /// Limit maximum number of buffers in pool.
    ///
    /// When limit is reached, oldest buffers are discarded.
    SizeLimited {
        /// Maximum number of buffers to keep.
        max_tapes: usize,
    },

    /// Limit total memory usage of pooled buffers.
    ///
    /// When limit is reached, largest buffers are discarded first.
    MemoryLimited {
        /// Maximum total bytes across all pooled buffers.
        max_bytes: usize,
    },

    /// Least-recently-used eviction with size limit.
    ///
    /// Combines size limit with LRU tracking for better cache behavior.
    Lru {
        /// Maximum number of buffers to keep.
        max_tapes: usize,
    },
}

impl Default for PoolStrategy {
    fn default() -> Self {
        Self::SizeLimited { max_tapes: 16 }
    }
}

impl PoolStrategy {
    /// Create an unbounded strategy.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self::Unbounded
    }

    /// Create a size-limited strategy.
    #[must_use]
    pub const fn size_limited(max_tapes: usize) -> Self {
        Self::SizeLimited { max_tapes }
    }

    /// Create a memory-limited strategy.
    #[must_use]
    pub const fn memory_limited(max_bytes: usize) -> Self {
        Self::MemoryLimited { max_bytes }
    }

    /// Create an LRU strategy.
    #[must_use]
    pub const fn lru(max_tapes: usize) -> Self {
        Self::Lru { max_tapes }
    }

    /// Check if a buffer should be evicted given current pool state.
    #[must_use]
    pub const fn should_evict(&self, current_count: usize, current_bytes: usize) -> bool {
        match self {
            Self::Unbounded => false,
            Self::SizeLimited { max_tapes } | Self::Lru { max_tapes } => {
                current_count >= *max_tapes
            }
            Self::MemoryLimited { max_bytes } => current_bytes >= *max_bytes,
        }
    }

    /// Get maximum allowed buffers (if applicable).
    #[must_use]
    pub const fn max_tapes(&self) -> Option<usize> {
        match self {
            Self::SizeLimited { max_tapes } | Self::Lru { max_tapes } => Some(*max_tapes),
            Self::Unbounded | Self::MemoryLimited { .. } => None,
        }
    }

    /// Get maximum allowed bytes (if applicable).
    #[must_use]
    pub const fn max_bytes(&self) -> Option<usize> {
        match self {
            Self::MemoryLimited { max_bytes } => Some(*max_bytes),
            Self::Unbounded | Self::SizeLimited { .. } | Self::Lru { .. } => None,
        }
    }
}

/// Pool statistics for monitoring and debugging.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Number of buffers currently in pool.
    pub buffers_in_pool: usize,

    /// Total bytes of capacity in pooled buffers.
    pub total_bytes_in_pool: usize,

    /// Total number of acquire calls.
    pub acquires: u64,

    /// Total number of release calls.
    pub releases: u64,

    /// Number of times a pooled buffer was reused.
    pub reuses: u64,

    /// Number of new allocations (no suitable buffer in pool).
    pub allocations: u64,

    /// Number of buffers evicted due to strategy limits.
    pub evictions: u64,

    /// Peak number of buffers in pool.
    pub peak_buffers: usize,

    /// Peak total bytes in pool.
    pub peak_bytes: usize,
}

impl PoolStats {
    /// Calculate reuse rate (0.0 to 1.0).
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Acceptable for statistical rate calculations
    pub fn reuse_rate(&self) -> f64 {
        if self.acquires == 0 {
            0.0
        } else {
            self.reuses as f64 / self.acquires as f64
        }
    }

    /// Calculate eviction rate (0.0 to 1.0).
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Acceptable for statistical rate calculations
    pub fn eviction_rate(&self) -> f64 {
        if self.releases == 0 {
            0.0
        } else {
            self.evictions as f64 / self.releases as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // PoolStrategy Tests
    // =========================================================================

    #[test]
    fn test_pool_strategy_default() {
        let strategy = PoolStrategy::default();
        assert!(matches!(
            strategy,
            PoolStrategy::SizeLimited { max_tapes: 16 }
        ));
    }

    #[test]
    fn test_pool_strategy_unbounded() {
        let strategy = PoolStrategy::unbounded();
        assert!(matches!(strategy, PoolStrategy::Unbounded));
    }

    #[test]
    fn test_pool_strategy_size_limited() {
        let strategy = PoolStrategy::size_limited(32);
        assert!(matches!(
            strategy,
            PoolStrategy::SizeLimited { max_tapes: 32 }
        ));
    }

    #[test]
    fn test_pool_strategy_memory_limited() {
        let strategy = PoolStrategy::memory_limited(1024);
        assert!(matches!(
            strategy,
            PoolStrategy::MemoryLimited { max_bytes: 1024 }
        ));
    }

    #[test]
    fn test_pool_strategy_lru() {
        let strategy = PoolStrategy::lru(8);
        assert!(matches!(strategy, PoolStrategy::Lru { max_tapes: 8 }));
    }

    #[test]
    fn test_pool_strategy_debug() {
        let strategy = PoolStrategy::SizeLimited { max_tapes: 10 };
        let debug_str = format!("{strategy:?}");
        assert!(debug_str.contains("SizeLimited"));
        assert!(debug_str.contains("10"));
    }

    #[test]
    fn test_pool_strategy_clone() {
        let strategy = PoolStrategy::MemoryLimited { max_bytes: 500 };
        let cloned = strategy;
        assert!(matches!(
            cloned,
            PoolStrategy::MemoryLimited { max_bytes: 500 }
        ));
    }

    #[test]
    fn test_should_evict_unbounded() {
        let strategy = PoolStrategy::Unbounded;
        assert!(!strategy.should_evict(0, 0));
        assert!(!strategy.should_evict(100, 1_000_000));
    }

    #[test]
    fn test_should_evict_size_limited() {
        let strategy = PoolStrategy::SizeLimited { max_tapes: 5 };
        assert!(!strategy.should_evict(0, 0));
        assert!(!strategy.should_evict(4, 1000));
        assert!(strategy.should_evict(5, 1000));
        assert!(strategy.should_evict(10, 1000));
    }

    #[test]
    fn test_should_evict_memory_limited() {
        let strategy = PoolStrategy::MemoryLimited { max_bytes: 1000 };
        assert!(!strategy.should_evict(100, 0));
        assert!(!strategy.should_evict(100, 999));
        assert!(strategy.should_evict(100, 1000));
        assert!(strategy.should_evict(100, 2000));
    }

    #[test]
    fn test_should_evict_lru() {
        let strategy = PoolStrategy::Lru { max_tapes: 3 };
        assert!(!strategy.should_evict(0, 0));
        assert!(!strategy.should_evict(2, 10000));
        assert!(strategy.should_evict(3, 10000));
        assert!(strategy.should_evict(5, 10000));
    }

    #[test]
    fn test_max_tapes_unbounded() {
        let strategy = PoolStrategy::Unbounded;
        assert_eq!(strategy.max_tapes(), None);
    }

    #[test]
    fn test_max_tapes_size_limited() {
        let strategy = PoolStrategy::SizeLimited { max_tapes: 42 };
        assert_eq!(strategy.max_tapes(), Some(42));
    }

    #[test]
    fn test_max_tapes_memory_limited() {
        let strategy = PoolStrategy::MemoryLimited { max_bytes: 1000 };
        assert_eq!(strategy.max_tapes(), None);
    }

    #[test]
    fn test_max_tapes_lru() {
        let strategy = PoolStrategy::Lru { max_tapes: 7 };
        assert_eq!(strategy.max_tapes(), Some(7));
    }

    #[test]
    fn test_max_bytes_unbounded() {
        let strategy = PoolStrategy::Unbounded;
        assert_eq!(strategy.max_bytes(), None);
    }

    #[test]
    fn test_max_bytes_size_limited() {
        let strategy = PoolStrategy::SizeLimited { max_tapes: 10 };
        assert_eq!(strategy.max_bytes(), None);
    }

    #[test]
    fn test_max_bytes_memory_limited() {
        let strategy = PoolStrategy::MemoryLimited { max_bytes: 5000 };
        assert_eq!(strategy.max_bytes(), Some(5000));
    }

    #[test]
    fn test_max_bytes_lru() {
        let strategy = PoolStrategy::Lru { max_tapes: 5 };
        assert_eq!(strategy.max_bytes(), None);
    }

    // =========================================================================
    // PoolStats Tests
    // =========================================================================

    #[test]
    fn test_pool_stats_default() {
        let stats = PoolStats::default();
        assert_eq!(stats.buffers_in_pool, 0);
        assert_eq!(stats.total_bytes_in_pool, 0);
        assert_eq!(stats.acquires, 0);
        assert_eq!(stats.releases, 0);
        assert_eq!(stats.reuses, 0);
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.evictions, 0);
        assert_eq!(stats.peak_buffers, 0);
        assert_eq!(stats.peak_bytes, 0);
    }

    #[test]
    fn test_pool_stats_debug() {
        let stats = PoolStats {
            buffers_in_pool: 5,
            total_bytes_in_pool: 1000,
            acquires: 10,
            releases: 8,
            reuses: 3,
            allocations: 7,
            evictions: 2,
            peak_buffers: 6,
            peak_bytes: 1500,
        };
        let debug_str = format!("{stats:?}");
        assert!(debug_str.contains("buffers_in_pool"));
        assert!(debug_str.contains('5'));
    }

    #[test]
    fn test_pool_stats_clone() {
        let stats = PoolStats {
            buffers_in_pool: 3,
            total_bytes_in_pool: 500,
            acquires: 10,
            releases: 7,
            reuses: 4,
            allocations: 6,
            evictions: 1,
            peak_buffers: 4,
            peak_bytes: 600,
        };
        let cloned = stats;
        assert_eq!(cloned.buffers_in_pool, 3);
        assert_eq!(cloned.acquires, 10);
    }

    #[test]
    fn test_reuse_rate_zero_acquires() {
        let stats = PoolStats::default();
        assert!(stats.reuse_rate().abs() < f64::EPSILON);
    }

    #[test]
    fn test_reuse_rate_no_reuses() {
        let stats = PoolStats {
            acquires: 10,
            reuses: 0,
            ..Default::default()
        };
        assert!(stats.reuse_rate().abs() < f64::EPSILON);
    }

    #[test]
    fn test_reuse_rate_all_reuses() {
        let stats = PoolStats {
            acquires: 10,
            reuses: 10,
            ..Default::default()
        };
        assert!((stats.reuse_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_reuse_rate_partial() {
        let stats = PoolStats {
            acquires: 10,
            reuses: 5,
            ..Default::default()
        };
        assert!((stats.reuse_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_eviction_rate_zero_releases() {
        let stats = PoolStats::default();
        assert!(stats.eviction_rate().abs() < f64::EPSILON);
    }

    #[test]
    fn test_eviction_rate_no_evictions() {
        let stats = PoolStats {
            releases: 10,
            evictions: 0,
            ..Default::default()
        };
        assert!(stats.eviction_rate().abs() < f64::EPSILON);
    }

    #[test]
    fn test_eviction_rate_all_evicted() {
        let stats = PoolStats {
            releases: 10,
            evictions: 10,
            ..Default::default()
        };
        assert!((stats.eviction_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_eviction_rate_partial() {
        let stats = PoolStats {
            releases: 20,
            evictions: 5,
            ..Default::default()
        };
        assert!((stats.eviction_rate() - 0.25).abs() < f64::EPSILON);
    }
}
