// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSONL-DSON Integration
//!
//! This module provides integration between SIMD-JSONL processing and DSON operations,
//! enabling high-performance processing of JSON Lines with schema filtering and CRDT semantics.

use crate::skiptape::CompiledSchema;
use crate::skiptape::jsonl::SimdJsonlBatchProcessor;
use fionn_core::Result;
use fionn_ops::DsonOperation;
use std::collections::HashSet;

/// JSONL-DSON processor for high-performance document processing
pub struct JsonlDsonProcessor {
    /// SIMD-JSONL batch processor
    jsonl_processor: SimdJsonlBatchProcessor,
}

impl JsonlDsonProcessor {
    /// Create a new JSONL-DSON processor
    #[must_use]
    pub fn new(_input_schema: HashSet<String>, _output_schema: HashSet<String>) -> Self {
        Self {
            jsonl_processor: SimdJsonlBatchProcessor::new(),
        }
    }

    /// Process JSONL data with schema filtering and DSON operations
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_jsonl_with_operations(
        &mut self,
        jsonl_data: &[u8],
        schema: &CompiledSchema,
        operations: &[DsonOperation],
    ) -> Result<ProcessedBatch> {
        // First, process JSONL with SIMD filtering
        let batch_result = self
            .jsonl_processor
            .process_batch_optimized(jsonl_data, schema)?;

        // If no operations, return as-is
        if operations.is_empty() {
            return Ok(ProcessedBatch {
                documents: batch_result.documents,
                errors: batch_result.errors,
                statistics: batch_result.statistics,
            });
        }

        // Apply DSON operations to each filtered document
        let mut processed_documents = Vec::with_capacity(batch_result.documents.len());

        for doc_json in &batch_result.documents {
            match Self::apply_operations_to_document(doc_json, operations) {
                Ok(transformed) => processed_documents.push(transformed),
                Err(_) => {
                    // On error, keep original document
                    processed_documents.push(doc_json.clone());
                }
            }
        }

        Ok(ProcessedBatch {
            documents: processed_documents,
            errors: batch_result.errors,
            statistics: batch_result.statistics,
        })
    }

    /// Apply DSON operations to a single document
    fn apply_operations_to_document(
        doc_json: &str,
        operations: &[DsonOperation],
    ) -> Result<String> {
        use fionn_ops::processor::BlackBoxProcessor;

        // Create a processor for this document
        let mut processor = BlackBoxProcessor::new_unfiltered();

        // Process the document
        processor.process(doc_json)?;

        // Apply operations
        processor.apply_operations(operations)?;

        // Generate output
        processor.generate_output()
    }
}

/// Result of processing a batch of JSONL documents with DSON operations
#[derive(Debug)]
pub struct ProcessedBatch {
    /// Processed JSON documents
    pub documents: Vec<String>,
    /// Processing errors
    pub errors: Vec<crate::skiptape::jsonl::LineError>,
    /// Batch statistics
    pub statistics: crate::skiptape::jsonl::BatchStatistics,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skiptape::CompiledSchema;
    use fionn_ops::OperationValue;

    #[test]
    fn test_jsonl_dson_processor_creation() {
        let input_schema = HashSet::from(["name".to_string(), "value".to_string()]);
        let output_schema = HashSet::from(["name".to_string()]);

        let _processor = JsonlDsonProcessor::new(input_schema, output_schema);
        // Processor successfully created - test passes if no panic
    }

    #[test]
    fn test_process_empty_jsonl() {
        let mut processor = JsonlDsonProcessor::new(
            HashSet::from(["*".to_string()]),
            HashSet::from(["*".to_string()]),
        );

        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        let result = processor.process_jsonl_with_operations(&[], &schema, &[]);

        // Should succeed with empty results
        assert!(result.is_ok());
        let batch = result.unwrap();
        assert!(batch.documents.is_empty());
        assert!(batch.errors.is_empty());
    }

    #[test]
    fn test_process_single_document() {
        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());

        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let jsonl = b"{\"name\":\"test\"}";
        let result = processor.process_jsonl_with_operations(jsonl, &schema, &[]);

        assert!(result.is_ok());
    }

    #[test]
    fn test_process_with_operations() {
        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());

        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let jsonl = b"{\"name\":\"test\"}";
        let operations = vec![DsonOperation::FieldAdd {
            path: "added".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        }];
        let result = processor.process_jsonl_with_operations(jsonl, &schema, &operations);

        assert!(result.is_ok());
    }

    #[test]
    fn test_processed_batch_debug() {
        let batch = ProcessedBatch {
            documents: vec!["{}".to_string()],
            errors: vec![],
            statistics: crate::skiptape::jsonl::BatchStatistics {
                total_lines: 1,
                successful_lines: 1,
                failed_lines: 0,
                processing_time_ms: 0.1,
                avg_memory_per_line: 10,
                overall_schema_match_ratio: 1.0,
            },
        };
        let debug = format!("{batch:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_process_multiple_documents() {
        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());

        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let jsonl = b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}";
        let result = processor.process_jsonl_with_operations(jsonl, &schema, &[]);

        assert!(result.is_ok());
    }

    #[test]
    fn test_process_with_field_modify_operation() {
        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());

        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let jsonl = b"{\"name\":\"original\"}";
        let operations = vec![DsonOperation::FieldModify {
            path: "name".to_string(),
            value: OperationValue::StringRef("modified".to_string()),
        }];
        let result = processor.process_jsonl_with_operations(jsonl, &schema, &operations);

        assert!(result.is_ok());
        let batch = result.unwrap();
        assert!(!batch.documents.is_empty());
    }

    #[test]
    fn test_process_with_field_add_and_delete() {
        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());

        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let jsonl = b"{\"name\":\"test\",\"toDelete\":\"value\"}";
        let operations = vec![
            DsonOperation::FieldAdd {
                path: "new_field".to_string(),
                value: OperationValue::StringRef("new_value".to_string()),
            },
            DsonOperation::FieldDelete {
                path: "toDelete".to_string(),
            },
        ];
        let result = processor.process_jsonl_with_operations(jsonl, &schema, &operations);

        assert!(result.is_ok());
    }

    #[test]
    fn test_process_multiple_documents_with_operations() {
        let mut processor = JsonlDsonProcessor::new(HashSet::new(), HashSet::new());

        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();
        let jsonl = b"{\"id\":1}\n{\"id\":2}\n{\"id\":3}";
        let operations = vec![DsonOperation::FieldAdd {
            path: "processed".to_string(),
            value: OperationValue::BoolRef(true),
        }];
        let result = processor.process_jsonl_with_operations(jsonl, &schema, &operations);

        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.documents.len(), 3);
    }
}
