// SPDX-License-Identifier: MIT OR Apache-2.0
//! `x86`/`x86_64` SIMD implementations
//!
//! SSE2, AVX2, and AVX-512 accelerated operations for JSON processing.

pub mod classify;
pub mod path;
pub mod skip;

pub use classify::{CharacterClasses, SimdCharClassifier};
pub use skip::{Avx2Skip, SkipResult};
