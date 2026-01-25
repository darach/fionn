// SPDX-License-Identifier: MIT OR Apache-2.0
//! ISONL-DSON Integration
//!
//! This module provides seamless integration between ISONL (ISON Lines) processing
//! and DSON (Document Structured Object Notation) operations, enabling high-performance
//! streaming document processing with schema filtering and transformations.
//!
//! # Example
//!
//! ```ignore
//! use fionn_stream::isonl_dson::IsonlDsonProcessor;
//! use fionn_stream::skiptape::CompiledSchema;
//! use fionn_ops::DsonOperation;
//!
//! let mut processor = IsonlDsonProcessor::new();
//! let schema = CompiledSchema::compile(&["users.*".to_string()])?;
//! let operations = vec![
//!     DsonOperation::FieldAdd {
//!         path: "processed".to_string(),
//!         value: true.into(),
//!     },
//! ];
//!
//! let result = processor.process(
//!     b"table.users|id:int|name:string|1|Alice",
//!     &schema,
//!     &operations,
//! )?;
//! ```

use crate::format_dson::{FormatBatchResult, FormatDsonProcessor};
use crate::skiptape::CompiledSchema;
use crate::skiptape::isonl::SimdIsonlBatchProcessor;
use fionn_core::Result;
use fionn_core::format::FormatKind;
use fionn_ops::DsonOperation;

// =============================================================================
// IsonlDsonProcessor
// =============================================================================

/// ISONL-DSON integration processor
///
/// Combines the high-performance SIMD-accelerated ISONL batch processing
/// with DSON operation support for document transformations.
pub struct IsonlDsonProcessor {
    /// Underlying format-agnostic DSON processor
    inner: FormatDsonProcessor<SimdIsonlBatchProcessor>,
}

impl Default for IsonlDsonProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl IsonlDsonProcessor {
    /// Create a new ISONL-DSON processor
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FormatDsonProcessor::new(SimdIsonlBatchProcessor::new()),
        }
    }

    /// Process ISONL data with schema filtering and DSON operations
    ///
    /// # Arguments
    /// * `data` - Raw ISONL bytes
    /// * `schema` - Compiled schema for filtering
    /// * `operations` - DSON operations to apply
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        operations: &[DsonOperation],
    ) -> Result<FormatBatchResult> {
        self.inner.process_with_operations(data, schema, operations)
    }

    /// Process ISONL data with schema filtering only (no DSON operations)
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_filtered(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
    ) -> Result<FormatBatchResult> {
        self.inner.process_with_operations(data, schema, &[])
    }

    /// Process ISONL data without filtering (all records included)
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_unfiltered(&mut self, data: &[u8]) -> Result<FormatBatchResult> {
        self.inner.process_unfiltered_with_operations(data, &[])
    }

    /// Process ISONL data with DSON operations only (no schema filtering)
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_with_operations(
        &mut self,
        data: &[u8],
        operations: &[DsonOperation],
    ) -> Result<FormatBatchResult> {
        self.inner
            .process_unfiltered_with_operations(data, operations)
    }

    /// Get the format kind
    #[must_use]
    pub fn format_kind(&self) -> FormatKind {
        self.inner.format_kind()
    }

    /// Reset the processor for a new batch
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// Get a reference to the underlying batch processor
    #[must_use]
    pub const fn batch_processor(&self) -> &SimdIsonlBatchProcessor {
        self.inner.batch_processor()
    }
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Process ISONL data with a simple field filter
///
/// # Arguments
/// * `data` - Raw ISONL bytes
/// * `fields` - Field paths to include
///
/// # Errors
/// Returns an error if processing fails
///
/// # Example
///
/// ```ignore
/// let result = process_isonl_filtered(
///     b"table.users|id:int|name:string|1|Alice",
///     &["name"],
/// )?;
/// ```
pub fn process_isonl_filtered(data: &[u8], fields: &[&str]) -> Result<FormatBatchResult> {
    let mut processor = IsonlDsonProcessor::new();
    let field_strings: Vec<String> = fields.iter().map(|s| (*s).to_string()).collect();
    let schema = CompiledSchema::compile(&field_strings)
        .map_err(|e| fionn_core::DsonError::ParseError(e.to_string()))?;
    processor.process_filtered(data, &schema)
}

/// Process ISONL data and apply DSON operations
///
/// # Arguments
/// * `data` - Raw ISONL bytes
/// * `operations` - DSON operations to apply
///
/// # Errors
/// Returns an error if processing fails
pub fn process_isonl_with_ops(
    data: &[u8],
    operations: &[DsonOperation],
) -> Result<FormatBatchResult> {
    let mut processor = IsonlDsonProcessor::new();
    processor.process_with_operations(data, operations)
}

/// Process ISONL data with both filtering and operations
///
/// # Arguments
/// * `data` - Raw ISONL bytes
/// * `fields` - Field paths to include
/// * `operations` - DSON operations to apply
///
/// # Errors
/// Returns an error if processing fails
pub fn process_isonl_full(
    data: &[u8],
    fields: &[&str],
    operations: &[DsonOperation],
) -> Result<FormatBatchResult> {
    let mut processor = IsonlDsonProcessor::new();
    let field_strings: Vec<String> = fields.iter().map(|s| (*s).to_string()).collect();
    let schema = CompiledSchema::compile(&field_strings)
        .map_err(|e| fionn_core::DsonError::ParseError(e.to_string()))?;
    processor.process(data, &schema, operations)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use fionn_ops::OperationValue;

    #[test]
    fn test_processor_creation() {
        let processor = IsonlDsonProcessor::new();
        assert_eq!(processor.format_kind(), FormatKind::Ison);
    }

    #[test]
    fn test_processor_default() {
        let processor = IsonlDsonProcessor::default();
        assert_eq!(processor.format_kind(), FormatKind::Ison);
    }

    #[test]
    fn test_process_unfiltered() {
        let mut processor = IsonlDsonProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";

        let result = processor.process_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("Alice"));
    }

    #[test]
    fn test_process_filtered() {
        let mut processor = IsonlDsonProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();

        let result = processor.process_filtered(data, &schema).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_process_with_operations() {
        let mut processor = IsonlDsonProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";
        let operations = vec![DsonOperation::FieldAdd {
            path: "processed".to_string(),
            value: OperationValue::BoolRef(true),
        }];

        let result = processor
            .process_with_operations(data, &operations)
            .unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("processed"));
        assert!(result.documents[0].contains("true"));
    }

    #[test]
    fn test_process_full() {
        let mut processor = IsonlDsonProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        let operations = vec![DsonOperation::FieldAdd {
            path: "verified".to_string(),
            value: OperationValue::BoolRef(true),
        }];

        let result = processor.process(data, &schema, &operations).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("verified"));
    }

    #[test]
    fn test_convenience_filtered() {
        let data = b"table.users|id:int|name:string|1|Alice";
        let result = process_isonl_filtered(data, &["name"]).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_convenience_with_ops() {
        let data = b"table.users|id:int|name:string|1|Alice";
        let operations = vec![DsonOperation::FieldAdd {
            path: "added".to_string(),
            value: OperationValue::NumberRef("42".to_string()),
        }];

        let result = process_isonl_with_ops(data, &operations).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("added"));
    }

    #[test]
    fn test_convenience_full() {
        let data = b"table.users|id:int|name:string|1|Alice";
        let operations = vec![DsonOperation::FieldAdd {
            path: "tag".to_string(),
            value: OperationValue::StringRef("test".to_string()),
        }];

        let result = process_isonl_full(data, &["name"], &operations).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("tag"));
    }

    #[test]
    fn test_reset() {
        let mut processor = IsonlDsonProcessor::new();
        processor.reset();
        // Should not panic
    }

    #[test]
    fn test_batch_processor_access() {
        let processor = IsonlDsonProcessor::new();
        let batch = processor.batch_processor();
        assert!(batch.parser().is_streaming());
    }

    #[test]
    fn test_multiple_lines() {
        let mut processor = IsonlDsonProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice\ntable.users|id:int|name:string|2|Bob";

        let result = processor.process_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 2);
    }

    #[test]
    fn test_statistics_populated() {
        let mut processor = IsonlDsonProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";

        let result = processor.process_unfiltered(data).unwrap();
        assert_eq!(result.statistics.total_lines, 1);
        assert_eq!(result.statistics.successful_lines, 1);
        assert_eq!(result.statistics.failed_lines, 0);
    }
}
