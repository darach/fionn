// SPDX-License-Identifier: MIT OR Apache-2.0
//! CSV-DSON Integration
//!
//! Provides integration between CSV processing and DSON operations,
//! enabling high-performance streaming document processing with schema
//! filtering and transformations.

use crate::format_dson::{FormatBatchResult, FormatDsonProcessor};
use crate::skiptape::CompiledSchema;
use crate::skiptape::csv_batch::SimdCsvBatchProcessor;
use fionn_core::Result;
use fionn_core::format::FormatKind;
use fionn_ops::DsonOperation;

// =============================================================================
// CsvDsonProcessor
// =============================================================================

/// CSV-DSON integration processor
pub struct CsvDsonProcessor {
    /// Underlying format-agnostic DSON processor
    inner: FormatDsonProcessor<SimdCsvBatchProcessor>,
}

impl Default for CsvDsonProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvDsonProcessor {
    /// Create a new CSV-DSON processor
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FormatDsonProcessor::new(SimdCsvBatchProcessor::new()),
        }
    }

    /// Create a processor with custom delimiter
    #[must_use]
    pub fn with_delimiter(delimiter: u8) -> Self {
        Self {
            inner: FormatDsonProcessor::new(SimdCsvBatchProcessor::with_delimiter(delimiter)),
        }
    }

    /// Process CSV data with schema filtering and DSON operations
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

    /// Process CSV data with schema filtering only
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

    /// Process CSV data without filtering
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_unfiltered(&mut self, data: &[u8]) -> Result<FormatBatchResult> {
        self.inner.process_unfiltered_with_operations(data, &[])
    }

    /// Process CSV data with DSON operations only
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

    /// Reset the processor
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Process CSV data with field filter
///
/// # Errors
/// Returns an error if processing fails
pub fn process_csv_filtered(data: &[u8], fields: &[&str]) -> Result<FormatBatchResult> {
    let mut processor = CsvDsonProcessor::new();
    let field_strings: Vec<String> = fields.iter().map(|s| (*s).to_string()).collect();
    let schema = CompiledSchema::compile(&field_strings)
        .map_err(|e| fionn_core::DsonError::ParseError(e.to_string()))?;
    processor.process_filtered(data, &schema)
}

/// Process CSV data and apply DSON operations
///
/// # Errors
/// Returns an error if processing fails
pub fn process_csv_with_ops(
    data: &[u8],
    operations: &[DsonOperation],
) -> Result<FormatBatchResult> {
    let mut processor = CsvDsonProcessor::new();
    processor.process_with_operations(data, operations)
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
        let processor = CsvDsonProcessor::new();
        assert_eq!(processor.format_kind(), FormatKind::Csv);
    }

    #[test]
    fn test_process_unfiltered() {
        let mut processor = CsvDsonProcessor::new();
        let data = b"name,age\nAlice,30";

        let result = processor.process_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("Alice"));
    }

    #[test]
    fn test_process_with_operations() {
        let mut processor = CsvDsonProcessor::new();
        let data = b"name,age\nAlice,30";
        let operations = vec![DsonOperation::FieldAdd {
            path: "source".to_string(),
            value: OperationValue::StringRef("csv".to_string()),
        }];

        let result = processor
            .process_with_operations(data, &operations)
            .unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("source"));
    }

    #[test]
    fn test_custom_delimiter() {
        let mut processor = CsvDsonProcessor::with_delimiter(b'\t');
        let data = b"name\tage\nAlice\t30";

        let result = processor.process_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_reset() {
        let mut processor = CsvDsonProcessor::new();
        processor.reset();
    }
}
