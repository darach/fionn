// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format-Agnostic DSON Processor
//!
//! This module provides a generic DSON processor that works with any format
//! that implements the `FormatBatchProcessor` trait, enabling high-performance
//! document processing with schema filtering and CRDT semantics across formats.
//!
//! # Unified Tape Architecture
//!
//! The batch processors emit unified tape structures instead of JSON strings,
//! preserving format-specific information for lossless round-trips:
//!
//! - **ExtendedNodeType**: Format-specific markers (YamlAnchor, TomlTableStart, etc.)
//! - **OriginalSyntax**: Preserved syntax for exact reconstruction
//! - **TapeSegment**: Document/line boundaries within unified tape
//!
//! # Legacy String API
//!
//! For backward compatibility, the string-based `FormatBatchResult` is still
//! available. New code should prefer `TapeBatchResult` for full fidelity.

use crate::skiptape::CompiledSchema;
use fionn_core::Result;
use fionn_core::format::FormatKind;
use fionn_ops::DsonOperation;

use crate::skiptape::unified_tape::{TapeSegment, UnifiedTape};

// =============================================================================
// Batch Result Types
// =============================================================================

/// Statistics for batch processing
#[derive(Debug, Clone, Default)]
pub struct BatchStatistics {
    /// Total lines/records processed
    pub total_lines: usize,
    /// Successfully processed lines
    pub successful_lines: usize,
    /// Failed lines
    pub failed_lines: usize,
    /// Total processing time in milliseconds
    pub processing_time_ms: f64,
    /// Average memory per line/record
    pub avg_memory_per_line: usize,
    /// Overall schema match ratio
    pub overall_schema_match_ratio: f64,
}

/// Error for a specific line during batch processing
#[derive(Debug, Clone)]
pub struct LineError {
    /// Index of the line in the original data
    pub line_index: usize,
    /// The error message
    pub error_message: String,
    /// Raw line content
    pub raw_line: String,
}

/// Result of processing a batch of documents
#[derive(Debug)]
pub struct FormatBatchResult {
    /// Successfully processed JSON documents (normalized output)
    pub documents: Vec<String>,
    /// Processing errors
    pub errors: Vec<LineError>,
    /// Batch statistics
    pub statistics: BatchStatistics,
}

impl FormatBatchResult {
    /// Create a new empty batch result
    #[must_use]
    pub fn new() -> Self {
        Self {
            documents: Vec::new(),
            errors: Vec::new(),
            statistics: BatchStatistics::default(),
        }
    }

    /// Create a batch result with pre-allocated capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            documents: Vec::with_capacity(capacity),
            errors: Vec::new(),
            statistics: BatchStatistics::default(),
        }
    }
}

impl Default for FormatBatchResult {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tape-Based Batch Result Types (Unified Tape Architecture)
// =============================================================================

/// Result of processing a batch to unified tape
///
/// This is the preferred result type for new code, preserving format-specific
/// information for lossless round-trips and cross-format CRDT operations.

#[derive(Debug)]
pub struct TapeBatchResult<'arena> {
    /// Unified tape containing all processed documents/lines
    pub tape: UnifiedTape<'arena>,
    /// Segment boundaries for individual documents/lines
    pub segments: Vec<SegmentBoundary>,
    /// Processing errors
    pub errors: Vec<LineError>,
    /// Batch statistics
    pub statistics: BatchStatistics,
}

/// Boundary markers for segments within unified tape

#[derive(Debug, Clone)]
pub struct SegmentBoundary {
    /// Start node index
    pub start_idx: usize,
    /// End node index (exclusive)
    pub end_idx: usize,
    /// Line/document index in source
    pub source_idx: usize,
}

impl<'arena> TapeBatchResult<'arena> {
    /// Create a new tape batch result
    #[must_use]
    pub fn new(tape: UnifiedTape<'arena>) -> Self {
        Self {
            tape,
            segments: Vec::new(),
            errors: Vec::new(),
            statistics: BatchStatistics::default(),
        }
    }

    /// Get a tape segment by index
    #[must_use]
    pub fn segment(&'arena self, idx: usize) -> Option<TapeSegment<'arena>> {
        self.segments
            .get(idx)
            .map(|boundary| TapeSegment::new(&self.tape, boundary.start_idx, boundary.end_idx))
    }

    /// Get the number of segments
    #[must_use]
    pub const fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Iterate over all segments
    pub fn iter_segments(&'arena self) -> impl Iterator<Item = TapeSegment<'arena>> {
        self.segments
            .iter()
            .map(move |boundary| TapeSegment::new(&self.tape, boundary.start_idx, boundary.end_idx))
    }
}

// =============================================================================
// FormatBatchProcessor Trait
// =============================================================================

/// Trait for format-specific batch processors (legacy string-based API)
///
/// Implementations provide SIMD-accelerated batch processing for their format,
/// with schema-aware filtering during the parsing phase.
///
/// **Note**: For new code, prefer implementing `TapeBatchProcessor` which
/// preserves format-specific information for lossless round-trips.
pub trait FormatBatchProcessor {
    /// Get the format kind this processor handles
    fn format_kind(&self) -> FormatKind;

    /// Process a batch of data with schema filtering
    ///
    /// # Arguments
    /// * `data` - Raw bytes in the format's encoding
    /// * `schema` - Compiled schema for filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    fn process_batch(&mut self, data: &[u8], schema: &CompiledSchema) -> Result<FormatBatchResult>;

    /// Process a batch without schema filtering (all fields included)
    ///
    /// # Arguments
    /// * `data` - Raw bytes in the format's encoding
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    fn process_batch_unfiltered(&mut self, data: &[u8]) -> Result<FormatBatchResult>;

    /// Reset processor state for a new batch
    fn reset(&mut self);
}

// =============================================================================
// Tape-Based Batch Processor Trait (Unified Tape Architecture)
// =============================================================================

/// Trait for format-specific batch processors emitting unified tape
///
/// This is the preferred trait for new implementations, preserving format-specific
/// information for lossless round-trips and cross-format CRDT operations.
///
/// # Example
///
/// ```ignore
/// impl<'arena> TapeBatchProcessor<'arena> for MyFormatProcessor {
///     fn process_to_tape(
///         &mut self,
///         data: &[u8],
///         schema: &CompiledSchema,
///         arena: &'arena Bump,
///     ) -> Result<TapeBatchResult<'arena>> {
///         let mut tape = UnifiedTape::new(arena, FormatKind::MyFormat);
///         // Parse and emit to tape...
///         Ok(TapeBatchResult::new(tape))
///     }
/// }
/// ```
pub trait TapeBatchProcessor<'arena> {
    /// Get the format kind this processor handles
    fn format_kind(&self) -> FormatKind;

    /// Process a batch of data to unified tape with schema filtering
    ///
    /// # Arguments
    /// * `data` - Raw bytes in the format's encoding
    /// * `schema` - Compiled schema for filtering
    /// * `arena` - Bump allocator for tape strings
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    fn process_to_tape(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        arena: &'arena bumpalo::Bump,
    ) -> Result<TapeBatchResult<'arena>>;

    /// Process a batch to unified tape without schema filtering
    ///
    /// # Arguments
    /// * `data` - Raw bytes in the format's encoding
    /// * `arena` - Bump allocator for tape strings
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    fn process_to_tape_unfiltered(
        &mut self,
        data: &[u8],
        arena: &'arena bumpalo::Bump,
    ) -> Result<TapeBatchResult<'arena>>;

    /// Reset processor state for a new batch
    fn reset(&mut self);
}

// =============================================================================
// FormatDsonProcessor - Generic DSON Processor
// =============================================================================

/// Generic DSON processor for any format implementing `FormatBatchProcessor`
///
/// This wraps a format-specific batch processor and adds DSON operation support,
/// enabling schema filtering and document transformations across all formats.
pub struct FormatDsonProcessor<P: FormatBatchProcessor> {
    /// The underlying format-specific batch processor
    batch_processor: P,
}

impl<P: FormatBatchProcessor> FormatDsonProcessor<P> {
    /// Create a new format DSON processor
    #[must_use]
    pub const fn new(batch_processor: P) -> Self {
        Self { batch_processor }
    }

    /// Get the format kind
    #[must_use]
    pub fn format_kind(&self) -> FormatKind {
        self.batch_processor.format_kind()
    }

    /// Get a reference to the underlying batch processor
    #[must_use]
    pub const fn batch_processor(&self) -> &P {
        &self.batch_processor
    }

    /// Get a mutable reference to the underlying batch processor
    pub const fn batch_processor_mut(&mut self) -> &mut P {
        &mut self.batch_processor
    }

    /// Process data with schema filtering and DSON operations
    ///
    /// # Arguments
    /// * `data` - Raw bytes in the format's encoding
    /// * `schema` - Compiled schema for filtering
    /// * `operations` - DSON operations to apply to each document
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_with_operations(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        operations: &[DsonOperation],
    ) -> Result<FormatBatchResult> {
        // First, process batch with schema filtering
        let batch_result = self.batch_processor.process_batch(data, schema)?;

        // If no operations, return as-is
        if operations.is_empty() {
            return Ok(batch_result);
        }

        // Apply DSON operations to each filtered document
        let mut processed_documents = Vec::with_capacity(batch_result.documents.len());
        let mut operation_errors = batch_result.errors;

        for (line_index, doc_json) in batch_result.documents.iter().enumerate() {
            match Self::apply_operations_to_document(doc_json, operations) {
                Ok(transformed) => processed_documents.push(transformed),
                Err(e) => {
                    // On error, keep original document and record error
                    operation_errors.push(LineError {
                        line_index,
                        error_message: e.to_string(),
                        raw_line: doc_json.clone(),
                    });
                    processed_documents.push(doc_json.clone());
                }
            }
        }

        Ok(FormatBatchResult {
            documents: processed_documents,
            errors: operation_errors,
            statistics: batch_result.statistics,
        })
    }

    /// Process data with DSON operations only (no schema filtering)
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_unfiltered_with_operations(
        &mut self,
        data: &[u8],
        operations: &[DsonOperation],
    ) -> Result<FormatBatchResult> {
        let batch_result = self.batch_processor.process_batch_unfiltered(data)?;

        if operations.is_empty() {
            return Ok(batch_result);
        }

        let mut processed_documents = Vec::with_capacity(batch_result.documents.len());
        let mut operation_errors = batch_result.errors;

        for (line_index, doc_json) in batch_result.documents.iter().enumerate() {
            match Self::apply_operations_to_document(doc_json, operations) {
                Ok(transformed) => processed_documents.push(transformed),
                Err(e) => {
                    operation_errors.push(LineError {
                        line_index,
                        error_message: e.to_string(),
                        raw_line: doc_json.clone(),
                    });
                    processed_documents.push(doc_json.clone());
                }
            }
        }

        Ok(FormatBatchResult {
            documents: processed_documents,
            errors: operation_errors,
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

    /// Reset the processor for a new batch
    pub fn reset(&mut self) {
        self.batch_processor.reset();
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Mock batch processor for testing
    struct MockBatchProcessor {
        format: FormatKind,
    }

    impl MockBatchProcessor {
        const fn new(format: FormatKind) -> Self {
            Self { format }
        }
    }

    impl FormatBatchProcessor for MockBatchProcessor {
        fn format_kind(&self) -> FormatKind {
            self.format
        }

        fn process_batch(
            &mut self,
            _data: &[u8],
            _schema: &CompiledSchema,
        ) -> Result<FormatBatchResult> {
            Ok(FormatBatchResult {
                documents: vec![r#"{"name":"test","value":42}"#.to_string()],
                errors: vec![],
                statistics: BatchStatistics {
                    total_lines: 1,
                    successful_lines: 1,
                    failed_lines: 0,
                    processing_time_ms: 0.1,
                    avg_memory_per_line: 50,
                    overall_schema_match_ratio: 1.0,
                },
            })
        }

        fn process_batch_unfiltered(&mut self, _data: &[u8]) -> Result<FormatBatchResult> {
            self.process_batch(&[], &CompiledSchema::compile(&[]).unwrap())
        }

        fn reset(&mut self) {}
    }

    #[test]
    fn test_format_dson_processor_creation() {
        let processor = FormatDsonProcessor::new(MockBatchProcessor::new(FormatKind::Json));
        assert_eq!(processor.format_kind(), FormatKind::Json);
    }

    #[test]
    fn test_batch_statistics_default() {
        let stats = BatchStatistics::default();
        assert_eq!(stats.total_lines, 0);
        assert_eq!(stats.successful_lines, 0);
        assert_eq!(stats.failed_lines, 0);
    }

    #[test]
    fn test_format_batch_result_new() {
        let result = FormatBatchResult::new();
        assert!(result.documents.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_format_batch_result_with_capacity() {
        let result = FormatBatchResult::with_capacity(100);
        assert!(result.documents.capacity() >= 100);
    }

    #[test]
    fn test_process_with_no_operations() {
        let mut processor = FormatDsonProcessor::new(MockBatchProcessor::new(FormatKind::Json));
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();

        let result = processor
            .process_with_operations(b"{}", &schema, &[])
            .unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_process_with_operations() {
        use fionn_ops::OperationValue;

        let mut processor = FormatDsonProcessor::new(MockBatchProcessor::new(FormatKind::Json));
        let schema = CompiledSchema::compile(&["*".to_string()]).unwrap();

        let operations = vec![DsonOperation::FieldAdd {
            path: "added".to_string(),
            value: OperationValue::StringRef("new_value".to_string()),
        }];

        let result = processor
            .process_with_operations(b"{}", &schema, &operations)
            .unwrap();
        assert_eq!(result.documents.len(), 1);
        // The document should have the added field
        assert!(result.documents[0].contains("added"));
    }

    #[test]
    fn test_line_error_debug() {
        let error = LineError {
            line_index: 0,
            error_message: "test error".to_string(),
            raw_line: "test".to_string(),
        };
        let debug = format!("{error:?}");
        assert!(debug.contains("LineError"));
    }

    #[test]
    fn test_batch_processor_reset() {
        let mut processor = FormatDsonProcessor::new(MockBatchProcessor::new(FormatKind::Json));
        processor.reset();
        // Reset should not panic
    }

    #[test]
    fn test_batch_processor_mut_access() {
        let mut processor = FormatDsonProcessor::new(MockBatchProcessor::new(FormatKind::Json));
        let _batch_proc = processor.batch_processor_mut();
        // Should be able to get mutable access
    }
}
