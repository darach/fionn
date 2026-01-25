// SPDX-License-Identifier: MIT OR Apache-2.0
//! YAML-DSON Integration

use crate::format_dson::{FormatBatchResult, FormatDsonProcessor};
use crate::skiptape::CompiledSchema;
use crate::skiptape::yaml_batch::SimdYamlBatchProcessor;
use fionn_core::Result;
use fionn_core::format::FormatKind;
use fionn_ops::DsonOperation;

/// YAML-DSON integration processor
pub struct YamlDsonProcessor {
    inner: FormatDsonProcessor<SimdYamlBatchProcessor>,
}

impl Default for YamlDsonProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl YamlDsonProcessor {
    /// Create a new YAML-DSON processor
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FormatDsonProcessor::new(SimdYamlBatchProcessor::new()),
        }
    }

    /// Process YAML data with schema filtering and DSON operations
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

    /// Process YAML data with schema filtering only
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

    /// Process YAML data without filtering
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_unfiltered(&mut self, data: &[u8]) -> Result<FormatBatchResult> {
        self.inner.process_unfiltered_with_operations(data, &[])
    }

    /// Process YAML data with DSON operations only
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

#[cfg(test)]
mod tests {
    use super::*;
    use fionn_ops::OperationValue;

    #[test]
    fn test_processor_creation() {
        let processor = YamlDsonProcessor::new();
        assert_eq!(processor.format_kind(), FormatKind::Yaml);
    }

    #[test]
    fn test_process_unfiltered() {
        let mut processor = YamlDsonProcessor::new();
        let data = b"name: Alice\nage: 30";

        let result = processor.process_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("Alice"));
    }

    #[test]
    fn test_process_with_operations() {
        let mut processor = YamlDsonProcessor::new();
        let data = b"name: Alice";
        let operations = vec![DsonOperation::FieldAdd {
            path: "source".to_string(),
            value: OperationValue::StringRef("yaml".to_string()),
        }];

        let result = processor
            .process_with_operations(data, &operations)
            .unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("source"));
    }

    #[test]
    fn test_reset() {
        let mut processor = YamlDsonProcessor::new();
        processor.reset();
    }
}
