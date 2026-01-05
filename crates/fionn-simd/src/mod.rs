// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-accelerated components
//!
//! Architecture-specific SIMD implementations for performance-critical operations.
//! Provides runtime feature detection and fallback to scalar implementations.

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

#[cfg(target_arch = "aarch64")]
pub mod neon;

pub mod dispatch;
pub mod string_ops;

// Re-exports
pub use dispatch::SimdDispatch;
pub use string_ops::SimdStringOps;
