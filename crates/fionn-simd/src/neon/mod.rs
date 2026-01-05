// SPDX-License-Identifier: MIT OR Apache-2.0
//! ARM NEON SIMD implementations
//!
//! 128-bit NEON accelerated operations for JSON processing on aarch64.

pub mod classify;
pub mod path;
pub mod skip;

pub use classify::{CharacterClasses, SimdCharClassifier};
pub use skip::{NeonSkip, SkipResult};

#[cfg(target_arch = "aarch64")]
pub use path::{SIMD_NEON_THRESHOLD, find_byte_neon, find_delim_neon, find_two_bytes_neon};
