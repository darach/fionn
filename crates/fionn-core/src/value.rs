// SPDX-License-Identifier: MIT OR Apache-2.0
//! Operation values for DSON operations
//!
//! This module defines the value types that can be used in DSON operations.
//! Values are designed to be zero-copy references where possible.

/// Values that can be operated on (references to avoid allocation)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationValue {
    /// String value
    StringRef(String),
    /// Number value (stored as string for precision)
    NumberRef(String),
    /// Boolean value
    BoolRef(bool),
    /// Null value
    Null,
    /// Object reference (tape position range)
    ObjectRef {
        /// Start position in tape
        start: usize,
        /// End position in tape
        end: usize,
    },
    /// Array reference (tape position range)
    ArrayRef {
        /// Start position in tape
        start: usize,
        /// End position in tape
        end: usize,
    },
}
