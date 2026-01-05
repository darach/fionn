// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD runtime dispatch
//!
//! Runtime feature detection and dispatch to appropriate SIMD implementations.

/// SIMD dispatch for runtime feature detection
#[derive(Debug, Clone, Copy)]
pub struct SimdDispatch {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    has_avx2: bool,
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    has_avx512: bool,
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    has_sse2: bool,
    #[cfg(target_arch = "aarch64")]
    has_neon: bool,
}

impl SimdDispatch {
    /// Detect available SIMD features at runtime
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // is_x86_feature_detected! is not const on x86
    pub fn detect() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            Self {
                has_avx2: is_x86_feature_detected!("avx2"),
                has_avx512: is_x86_feature_detected!("avx512f"),
                has_sse2: is_x86_feature_detected!("sse2"),
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // NEON is always available on aarch64
            Self { has_neon: true }
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self {}
        }
    }

    /// Check if AVX2 is available (`x86`/`x86_64` only)
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[must_use]
    pub const fn has_avx2(&self) -> bool {
        self.has_avx2
    }

    /// Check if AVX-512 is available (`x86`/`x86_64` only)
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[must_use]
    pub const fn has_avx512(&self) -> bool {
        self.has_avx512
    }

    /// Check if SSE2 is available (`x86`/`x86_64` only)
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[must_use]
    pub const fn has_sse2(&self) -> bool {
        self.has_sse2
    }

    /// Check if NEON is available (aarch64 only)
    #[cfg(target_arch = "aarch64")]
    #[must_use]
    pub const fn has_neon(&self) -> bool {
        self.has_neon
    }

    /// Get the best available SIMD level
    #[must_use]
    pub const fn best_simd_level(&self) -> SimdLevel {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if self.has_avx512 {
                SimdLevel::Avx512
            } else if self.has_avx2 {
                SimdLevel::Avx2
            } else if self.has_sse2 {
                SimdLevel::Sse2
            } else {
                SimdLevel::Scalar
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if self.has_neon {
                SimdLevel::Neon
            } else {
                SimdLevel::Scalar
            }
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
        {
            SimdLevel::Scalar
        }
    }
}

impl Default for SimdDispatch {
    fn default() -> Self {
        Self::detect()
    }
}

/// Available SIMD instruction levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdLevel {
    /// No SIMD, scalar operations only
    Scalar,
    /// x86 SSE2 (128-bit)
    Sse2,
    /// x86 AVX2 (256-bit)
    Avx2,
    /// x86 AVX-512 (512-bit)
    Avx512,
    /// ARM NEON (128-bit)
    Neon,
}

impl SimdLevel {
    /// Get the vector width in bytes
    #[must_use]
    pub const fn vector_width(&self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Sse2 | Self::Neon => 16,
            Self::Avx2 => 32,
            Self::Avx512 => 64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // SimdDispatch Tests
    // =========================================================================

    #[test]
    fn test_dispatch_detect() {
        let dispatch = SimdDispatch::detect();
        let level = dispatch.best_simd_level();
        // Should at least detect something
        assert!(matches!(
            level,
            SimdLevel::Scalar
                | SimdLevel::Sse2
                | SimdLevel::Avx2
                | SimdLevel::Avx512
                | SimdLevel::Neon
        ));
    }

    #[test]
    fn test_dispatch_default() {
        let dispatch = SimdDispatch::default();
        // Default should be same as detect
        let _ = dispatch.best_simd_level();
    }

    #[test]
    fn test_dispatch_debug() {
        let dispatch = SimdDispatch::detect();
        let debug_str = format!("{dispatch:?}");
        assert!(debug_str.contains("SimdDispatch"));
    }

    #[test]
    fn test_dispatch_clone() {
        let dispatch = SimdDispatch::detect();
        let cloned = dispatch;
        assert_eq!(dispatch.best_simd_level(), cloned.best_simd_level());
    }

    #[test]
    fn test_dispatch_copy() {
        let dispatch = SimdDispatch::detect();
        let copied = dispatch;
        // Both should work since it's Copy
        assert_eq!(dispatch.best_simd_level(), copied.best_simd_level());
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn test_dispatch_has_sse2() {
        let dispatch = SimdDispatch::detect();
        // SSE2 is always available on x86_64
        #[cfg(target_arch = "x86_64")]
        assert!(dispatch.has_sse2());
        // On x86, it might not be available
        #[cfg(target_arch = "x86")]
        let _ = dispatch.has_sse2();
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn test_dispatch_has_avx2() {
        let dispatch = SimdDispatch::detect();
        // AVX2 might or might not be available
        let _ = dispatch.has_avx2();
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn test_dispatch_has_avx512() {
        let dispatch = SimdDispatch::detect();
        // AVX512 might or might not be available
        let _ = dispatch.has_avx512();
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn test_dispatch_has_neon() {
        let dispatch = SimdDispatch::detect();
        // NEON is always available on aarch64
        assert!(dispatch.has_neon());
    }

    // =========================================================================
    // SimdLevel Tests
    // =========================================================================

    #[test]
    fn test_vector_width() {
        assert_eq!(SimdLevel::Scalar.vector_width(), 1);
        assert_eq!(SimdLevel::Sse2.vector_width(), 16);
        assert_eq!(SimdLevel::Avx2.vector_width(), 32);
        assert_eq!(SimdLevel::Avx512.vector_width(), 64);
        assert_eq!(SimdLevel::Neon.vector_width(), 16);
    }

    #[test]
    fn test_simd_level_debug() {
        assert!(format!("{:?}", SimdLevel::Scalar).contains("Scalar"));
        assert!(format!("{:?}", SimdLevel::Sse2).contains("Sse2"));
        assert!(format!("{:?}", SimdLevel::Avx2).contains("Avx2"));
        assert!(format!("{:?}", SimdLevel::Avx512).contains("Avx512"));
        assert!(format!("{:?}", SimdLevel::Neon).contains("Neon"));
    }

    #[test]
    fn test_simd_level_clone() {
        let level = SimdLevel::Avx2;
        let cloned = level;
        assert_eq!(level, cloned);
    }

    #[test]
    fn test_simd_level_copy() {
        let level = SimdLevel::Avx2;
        let copied = level;
        assert_eq!(level, copied);
    }

    #[test]
    fn test_simd_level_equality() {
        assert_eq!(SimdLevel::Scalar, SimdLevel::Scalar);
        assert_eq!(SimdLevel::Sse2, SimdLevel::Sse2);
        assert_eq!(SimdLevel::Avx2, SimdLevel::Avx2);
        assert_eq!(SimdLevel::Avx512, SimdLevel::Avx512);
        assert_eq!(SimdLevel::Neon, SimdLevel::Neon);
        assert_ne!(SimdLevel::Scalar, SimdLevel::Avx2);
        assert_ne!(SimdLevel::Sse2, SimdLevel::Neon);
    }

    // =========================================================================
    // Best SIMD Level Tests
    // =========================================================================

    #[test]
    fn test_best_simd_level_returns_valid() {
        let dispatch = SimdDispatch::detect();
        let level = dispatch.best_simd_level();
        // Vector width should be positive
        assert!(level.vector_width() >= 1);
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn test_best_simd_level_x86() {
        let dispatch = SimdDispatch::detect();
        let level = dispatch.best_simd_level();
        // On x86/x86_64, should be one of the x86 levels or scalar
        assert!(matches!(
            level,
            SimdLevel::Scalar | SimdLevel::Sse2 | SimdLevel::Avx2 | SimdLevel::Avx512
        ));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn test_best_simd_level_aarch64() {
        let dispatch = SimdDispatch::detect();
        let level = dispatch.best_simd_level();
        // On aarch64, should be NEON (always available)
        assert_eq!(level, SimdLevel::Neon);
    }
}
