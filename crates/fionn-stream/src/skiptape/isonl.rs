// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-ISONL Batch Processor
//!
//! High-performance ISONL (ISON Lines) processing with schema-aware filtering.
//! ISONL is the streaming variant of ISON where each line is self-contained:
//!
//! ```text
//! table.events|id:int|type:string|1|click
//! table.events|id:int|type:string|2|view
//! ```

use crate::skiptape::error::Result;
use crate::skiptape::schema::CompiledSchema;
use crate::skiptape::simd_ops::SimdLineSeparator;
use fionn_simd::formats::{IsonParser, IsonlParsedLine};
use std::time::Instant;

use crate::format_dson::{BatchStatistics, FormatBatchProcessor, FormatBatchResult, LineError};

use crate::format_dson::{SegmentBoundary, TapeBatchProcessor, TapeBatchResult};

use crate::skiptape::unified_tape::{
    ExtendedNodeType, IsonFieldType, OriginalSyntax, UnifiedNode, UnifiedTape,
};

use fionn_simd::formats::ison::IsonType;

// =============================================================================
// ISONL Batch Result (local version for non-feature-gated usage)
// =============================================================================

/// Result of processing an ISONL batch
#[derive(Debug)]
pub struct IsonlBatchResult {
    /// Successfully processed JSON documents (normalized output)
    pub documents: Vec<String>,
    /// Processing errors
    pub errors: Vec<IsonlLineError>,
    /// Batch statistics
    pub statistics: IsonlBatchStatistics,
}

/// Error for a specific line during batch processing
#[derive(Debug, Clone)]
pub struct IsonlLineError {
    /// Index of the line in the original data
    pub line_index: usize,
    /// The error message
    pub error_message: String,
    /// Raw line content
    pub raw_line: String,
}

/// Statistics for ISONL batch processing
#[derive(Debug, Clone, Default)]
pub struct IsonlBatchStatistics {
    /// Total lines processed
    pub total_lines: usize,
    /// Successfully parsed lines
    pub successful_lines: usize,
    /// Failed lines
    pub failed_lines: usize,
    /// Total processing time in milliseconds
    pub processing_time_ms: f64,
    /// Lines skipped by schema filter
    pub schema_filtered_lines: usize,
    /// Average bytes per line
    pub avg_bytes_per_line: usize,
}

// =============================================================================
// SimdIsonlBatchProcessor
// =============================================================================

/// SIMD-accelerated ISONL batch processor
///
/// Processes ISONL data with schema-aware filtering, producing JSON-normalized
/// output suitable for DSON operations.
#[derive(Debug)]
pub struct SimdIsonlBatchProcessor {
    /// The underlying ISON parser in streaming mode
    parser: IsonParser,
    /// Reusable buffer for line data
    line_buffer: Vec<u8>,
}

impl Default for SimdIsonlBatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl SimdIsonlBatchProcessor {
    /// Create a new ISONL batch processor
    #[must_use]
    pub fn new() -> Self {
        Self {
            parser: IsonParser::streaming(),
            line_buffer: Vec::with_capacity(4096),
        }
    }

    /// Process an ISONL batch with schema filtering
    ///
    /// # Arguments
    /// * `isonl_data` - Raw ISONL bytes
    /// * `schema` - Compiled schema for filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_optimized(
        &mut self,
        isonl_data: &[u8],
        schema: &CompiledSchema,
    ) -> Result<IsonlBatchResult> {
        let start = Instant::now();

        // Use SIMD line boundary detection
        let line_separator = SimdLineSeparator::new();
        let line_boundaries = line_separator.find_line_boundaries(isonl_data);

        let estimated_lines = line_boundaries.len().max(1);
        let mut documents = Vec::with_capacity(estimated_lines);
        let mut errors = Vec::new();
        let mut successful_lines = 0;
        let mut failed_lines = 0;
        let mut schema_filtered = 0;

        // Process each line
        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line = &isonl_data[line_start..line_end];

            // Skip empty lines
            if line.iter().all(|&b| b.is_ascii_whitespace()) {
                line_start = line_end;
                continue;
            }

            // Parse the ISONL line
            if let Some(parsed) = IsonParser::parse_isonl_line(line) {
                // Check if any fields match the schema
                if self.matches_schema(&parsed, schema) {
                    // Convert to JSON
                    let json = IsonParser::isonl_to_json(&parsed);
                    documents.push(json);
                    successful_lines += 1;
                } else {
                    schema_filtered += 1;
                }
            } else {
                // Check if it's a comment or empty line (not an error)
                let trimmed = line.iter().filter(|&&b| !b.is_ascii_whitespace()).count();
                if trimmed > 0 && !line.contains(&b'#') {
                    // Actually failed to parse
                    failed_lines += 1;
                    errors.push(IsonlLineError {
                        line_index,
                        error_message: "Failed to parse ISONL line".to_string(),
                        raw_line: String::from_utf8_lossy(line).to_string(),
                    });
                }
            }

            line_start = line_end;
        }

        // Handle last line if no trailing newline
        if line_start < isonl_data.len() {
            let line = &isonl_data[line_start..];
            if let Some(parsed) = IsonParser::parse_isonl_line(line) {
                if self.matches_schema(&parsed, schema) {
                    let json = IsonParser::isonl_to_json(&parsed);
                    documents.push(json);
                    successful_lines += 1;
                } else {
                    schema_filtered += 1;
                }
            }
        }

        let total_lines = successful_lines + failed_lines + schema_filtered;
        let elapsed = start.elapsed();

        Ok(IsonlBatchResult {
            documents,
            errors,
            statistics: IsonlBatchStatistics {
                total_lines,
                successful_lines,
                failed_lines,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_lines: schema_filtered,
                avg_bytes_per_line: if total_lines > 0 {
                    isonl_data.len() / total_lines
                } else {
                    0
                },
            },
        })
    }

    /// Process an ISONL batch without schema filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_unfiltered(&mut self, isonl_data: &[u8]) -> Result<IsonlBatchResult> {
        let start = Instant::now();

        let line_separator = SimdLineSeparator::new();
        let line_boundaries = line_separator.find_line_boundaries(isonl_data);

        let estimated_lines = line_boundaries.len().max(1);
        let mut documents = Vec::with_capacity(estimated_lines);
        let mut errors = Vec::new();
        let mut successful_lines = 0;
        let mut failed_lines = 0;

        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line = &isonl_data[line_start..line_end];

            if line.iter().all(|&b| b.is_ascii_whitespace()) {
                line_start = line_end;
                continue;
            }

            if let Some(parsed) = IsonParser::parse_isonl_line(line) {
                let json = IsonParser::isonl_to_json(&parsed);
                documents.push(json);
                successful_lines += 1;
            } else {
                let trimmed = line.iter().filter(|&&b| !b.is_ascii_whitespace()).count();
                if trimmed > 0 && !line.contains(&b'#') {
                    failed_lines += 1;
                    errors.push(IsonlLineError {
                        line_index,
                        error_message: "Failed to parse ISONL line".to_string(),
                        raw_line: String::from_utf8_lossy(line).to_string(),
                    });
                }
            }

            line_start = line_end;
        }

        // Handle last line
        if line_start < isonl_data.len() {
            let line = &isonl_data[line_start..];
            if let Some(parsed) = IsonParser::parse_isonl_line(line) {
                let json = IsonParser::isonl_to_json(&parsed);
                documents.push(json);
                successful_lines += 1;
            }
        }

        let total_lines = successful_lines + failed_lines;
        let elapsed = start.elapsed();

        Ok(IsonlBatchResult {
            documents,
            errors,
            statistics: IsonlBatchStatistics {
                total_lines,
                successful_lines,
                failed_lines,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_lines: 0,
                avg_bytes_per_line: if total_lines > 0 {
                    isonl_data.len() / total_lines
                } else {
                    0
                },
            },
        })
    }

    /// Check if a parsed ISONL line matches the schema
    #[allow(clippy::unused_self)] // Method signature for API consistency
    fn matches_schema(&self, parsed: &IsonlParsedLine, schema: &CompiledSchema) -> bool {
        // If schema has no patterns, match everything
        if schema.include_patterns.is_empty() {
            return true;
        }

        // Check if any field name matches the schema
        for field in &parsed.fields {
            if schema.matches_path(&field.name) {
                return true;
            }
            // Also check with table prefix: table.field
            let full_path = format!("{}.{}", parsed.name, field.name);
            if schema.matches_path(&full_path) {
                return true;
            }
        }

        // Also check if table name itself matches
        schema.matches_path(&parsed.name)
    }

    /// Reset processor state for a new batch
    pub fn reset(&mut self) {
        self.parser.reset();
        self.line_buffer.clear();
    }

    /// Get a reference to the underlying parser
    #[must_use]
    pub const fn parser(&self) -> &IsonParser {
        &self.parser
    }
}

// =============================================================================
// FormatBatchProcessor Implementation
// =============================================================================

impl FormatBatchProcessor for SimdIsonlBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Ison
    }

    fn process_batch(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
    ) -> fionn_core::Result<FormatBatchResult> {
        let result = self
            .process_batch_optimized(data, schema)
            .map_err(|e| fionn_core::DsonError::ParseError(e.to_string()))?;

        Ok(FormatBatchResult {
            documents: result.documents,
            errors: result
                .errors
                .into_iter()
                .map(|e| LineError {
                    line_index: e.line_index,
                    error_message: e.error_message,
                    raw_line: e.raw_line,
                })
                .collect(),
            statistics: BatchStatistics {
                total_lines: result.statistics.total_lines,
                successful_lines: result.statistics.successful_lines,
                failed_lines: result.statistics.failed_lines,
                processing_time_ms: result.statistics.processing_time_ms,
                avg_memory_per_line: result.statistics.avg_bytes_per_line,
                #[allow(clippy::cast_precision_loss)] // Acceptable for ratio calculation
                overall_schema_match_ratio: if result.statistics.total_lines > 0 {
                    result.statistics.successful_lines as f64 / result.statistics.total_lines as f64
                } else {
                    0.0
                },
            },
        })
    }

    fn process_batch_unfiltered(&mut self, data: &[u8]) -> fionn_core::Result<FormatBatchResult> {
        let result = Self::process_batch_unfiltered(self, data)
            .map_err(|e| fionn_core::DsonError::ParseError(e.to_string()))?;

        Ok(FormatBatchResult {
            documents: result.documents,
            errors: result
                .errors
                .into_iter()
                .map(|e| LineError {
                    line_index: e.line_index,
                    error_message: e.error_message,
                    raw_line: e.raw_line,
                })
                .collect(),
            statistics: BatchStatistics {
                total_lines: result.statistics.total_lines,
                successful_lines: result.statistics.successful_lines,
                failed_lines: result.statistics.failed_lines,
                processing_time_ms: result.statistics.processing_time_ms,
                avg_memory_per_line: result.statistics.avg_bytes_per_line,
                overall_schema_match_ratio: 1.0,
            },
        })
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}

// =============================================================================
// TapeBatchProcessor Implementation
// =============================================================================

impl<'arena> TapeBatchProcessor<'arena> for SimdIsonlBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Ison
    }

    #[allow(clippy::too_many_lines)] // Complex tape construction with schema filtering
    fn process_to_tape(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        arena: &'arena bumpalo::Bump,
    ) -> fionn_core::Result<TapeBatchResult<'arena>> {
        let start = Instant::now();

        let mut tape = UnifiedTape::new(arena, fionn_core::format::FormatKind::Ison);
        tape.metadata.original_size = data.len();

        let line_separator = SimdLineSeparator::new();
        let line_boundaries = line_separator.find_line_boundaries(data);

        let mut segments: Vec<SegmentBoundary> = Vec::with_capacity(line_boundaries.len());
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;
        let mut filtered = 0;

        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line = &data[line_start..line_end];

            // Skip empty lines
            if line.iter().all(|&b| b.is_ascii_whitespace()) {
                line_start = line_end;
                continue;
            }

            // Parse ISONL line
            if let Some(parsed) = IsonParser::parse_isonl_line(line) {
                // Check schema match
                if !self.matches_schema(&parsed, schema) {
                    filtered += 1;
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::SkipMarker,
                        fionn_core::format::FormatKind::Ison,
                    ));
                    tape.metadata.skipped_count += 1;
                    line_start = line_end;
                    continue;
                }

                let segment_start = tape.nodes().len();

                // Add ISON schema header - convert IsonType to IsonFieldType
                let field_types: Vec<(String, IsonFieldType)> = parsed
                    .fields
                    .iter()
                    .map(|f| {
                        let ftype = match f.field_type {
                            Some(IsonType::Int) => IsonFieldType::Int,
                            Some(IsonType::Float) => IsonFieldType::Float,
                            Some(IsonType::Bool) => IsonFieldType::Bool,
                            Some(IsonType::String | IsonType::Computed | IsonType::Reference)
                            | None => IsonFieldType::String,
                        };
                        (f.name.clone(), ftype)
                    })
                    .collect();

                let syntax_idx = tape.add_original_syntax(OriginalSyntax::IsonSchemaHeader {
                    fields: field_types,
                });

                // Add table block with name
                let name_idx = tape.add_string(&parsed.name);
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::IsonTableBlock { name_idx },
                        fionn_core::format::FormatKind::Ison,
                    )
                    .with_original_syntax(syntax_idx),
                );

                // Add each field with its value
                for (i, field) in parsed.fields.iter().enumerate() {
                    // Check if this field matches schema (for column-level filtering)
                    let field_matches = schema.include_patterns.is_empty()
                        || schema.matches_path(&field.name)
                        || schema.matches_path(&format!("{}.{}", parsed.name, field.name));

                    if !field_matches {
                        tape.add_node(UnifiedNode::new(
                            ExtendedNodeType::SkipMarker,
                            fionn_core::format::FormatKind::Ison,
                        ));
                        tape.metadata.skipped_count += 1;
                        continue;
                    }

                    let field_type = match field.field_type {
                        Some(IsonType::Int) => IsonFieldType::Int,
                        Some(IsonType::Float) => IsonFieldType::Float,
                        Some(IsonType::Bool) => IsonFieldType::Bool,
                        Some(IsonType::String | IsonType::Computed | IsonType::Reference)
                        | None => IsonFieldType::String,
                    };

                    // Get the value from the values vector
                    let value = parsed.values.get(i).map_or("", String::as_str);

                    let name_idx = tape.add_string(&field.name);

                    // Add key node
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Key(name_idx),
                        fionn_core::format::FormatKind::Ison,
                    ));

                    // Add typed field marker
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::IsonTypedField { field_type },
                        fionn_core::format::FormatKind::Ison,
                    ));

                    // Add field value based on type
                    match field_type {
                        IsonFieldType::Int => {
                            if let Ok(n) = value.parse::<i64>() {
                                tape.add_node(UnifiedNode::new(
                                    #[allow(clippy::cast_precision_loss)]
                                    // Acceptable for JSON number representation
                                    ExtendedNodeType::Number(n as f64),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            } else {
                                // Fallback to string
                                let value_idx = tape.add_string(value);
                                tape.add_node(UnifiedNode::new(
                                    ExtendedNodeType::String(value_idx),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            }
                        }
                        IsonFieldType::Float => {
                            if let Ok(n) = value.parse::<f64>() {
                                tape.add_node(UnifiedNode::new(
                                    ExtendedNodeType::Number(n),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            } else {
                                // Fallback to string
                                let value_idx = tape.add_string(value);
                                tape.add_node(UnifiedNode::new(
                                    ExtendedNodeType::String(value_idx),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            }
                        }
                        IsonFieldType::Bool => {
                            let b = value == "true" || value == "1";
                            tape.add_node(UnifiedNode::new(
                                ExtendedNodeType::Bool(b),
                                fionn_core::format::FormatKind::Ison,
                            ));
                        }
                        _ => {
                            let value_idx = tape.add_string(value);
                            tape.add_node(UnifiedNode::new(
                                ExtendedNodeType::String(value_idx),
                                fionn_core::format::FormatKind::Ison,
                            ));
                        }
                    }
                }

                segments.push(SegmentBoundary {
                    start_idx: segment_start,
                    end_idx: tape.nodes().len(),
                    source_idx: line_index,
                });

                successful += 1;
            } else {
                // Check if it's a comment
                let trimmed = line.iter().filter(|&&b| !b.is_ascii_whitespace()).count();
                if trimmed > 0 && !line.contains(&b'#') {
                    failed += 1;
                    errors.push(LineError {
                        line_index,
                        error_message: "Failed to parse ISONL line".to_string(),
                        raw_line: String::from_utf8_lossy(line).to_string(),
                    });
                }
            }

            line_start = line_end;
        }

        let elapsed = start.elapsed();
        let total = successful + failed + filtered;
        #[allow(clippy::cast_precision_loss)] // Acceptable for ratio calculation
        {
            tape.metadata.schema_match_ratio = if total > 0 {
                successful as f64 / total as f64
            } else {
                0.0
            };
        }

        Ok(TapeBatchResult {
            tape,
            segments,
            errors,
            statistics: BatchStatistics {
                total_lines: total,
                successful_lines: successful,
                failed_lines: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                avg_memory_per_line: if successful > 0 {
                    data.len() / successful
                } else {
                    0
                },
                #[allow(clippy::cast_precision_loss)] // Acceptable for ratio calculation
                overall_schema_match_ratio: if total > 0 {
                    successful as f64 / total as f64
                } else {
                    0.0
                },
            },
        })
    }

    #[allow(clippy::too_many_lines)] // Complex tape construction for all fields
    fn process_to_tape_unfiltered(
        &mut self,
        data: &[u8],
        arena: &'arena bumpalo::Bump,
    ) -> fionn_core::Result<TapeBatchResult<'arena>> {
        let start = Instant::now();

        let mut tape = UnifiedTape::new(arena, fionn_core::format::FormatKind::Ison);
        tape.metadata.original_size = data.len();

        let line_separator = SimdLineSeparator::new();
        let line_boundaries = line_separator.find_line_boundaries(data);

        let mut segments: Vec<SegmentBoundary> = Vec::with_capacity(line_boundaries.len());
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;

        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line = &data[line_start..line_end];

            // Skip empty lines
            if line.iter().all(|&b| b.is_ascii_whitespace()) {
                line_start = line_end;
                continue;
            }

            // Parse ISONL line
            if let Some(parsed) = IsonParser::parse_isonl_line(line) {
                let segment_start = tape.nodes().len();

                // Add ISON schema header - convert IsonType to IsonFieldType
                let field_types: Vec<(String, IsonFieldType)> = parsed
                    .fields
                    .iter()
                    .map(|f| {
                        let ftype = match f.field_type {
                            Some(IsonType::Int) => IsonFieldType::Int,
                            Some(IsonType::Float) => IsonFieldType::Float,
                            Some(IsonType::Bool) => IsonFieldType::Bool,
                            Some(IsonType::String | IsonType::Computed | IsonType::Reference)
                            | None => IsonFieldType::String,
                        };
                        (f.name.clone(), ftype)
                    })
                    .collect();

                let syntax_idx = tape.add_original_syntax(OriginalSyntax::IsonSchemaHeader {
                    fields: field_types,
                });

                // Add table block with name
                let name_idx = tape.add_string(&parsed.name);
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::IsonTableBlock { name_idx },
                        fionn_core::format::FormatKind::Ison,
                    )
                    .with_original_syntax(syntax_idx),
                );

                // Add each field with its value
                for (i, field) in parsed.fields.iter().enumerate() {
                    let field_type = match field.field_type {
                        Some(IsonType::Int) => IsonFieldType::Int,
                        Some(IsonType::Float) => IsonFieldType::Float,
                        Some(IsonType::Bool) => IsonFieldType::Bool,
                        Some(IsonType::String | IsonType::Computed | IsonType::Reference)
                        | None => IsonFieldType::String,
                    };

                    // Get the value from the values vector
                    let value = parsed.values.get(i).map_or("", String::as_str);

                    let name_idx = tape.add_string(&field.name);

                    // Add key node
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Key(name_idx),
                        fionn_core::format::FormatKind::Ison,
                    ));

                    // Add typed field marker
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::IsonTypedField { field_type },
                        fionn_core::format::FormatKind::Ison,
                    ));

                    // Add field value based on type
                    match field_type {
                        IsonFieldType::Int => {
                            if let Ok(n) = value.parse::<i64>() {
                                tape.add_node(UnifiedNode::new(
                                    #[allow(clippy::cast_precision_loss)]
                                    // Acceptable for JSON number representation
                                    ExtendedNodeType::Number(n as f64),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            } else {
                                // Fallback to string
                                let value_idx = tape.add_string(value);
                                tape.add_node(UnifiedNode::new(
                                    ExtendedNodeType::String(value_idx),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            }
                        }
                        IsonFieldType::Float => {
                            if let Ok(n) = value.parse::<f64>() {
                                tape.add_node(UnifiedNode::new(
                                    ExtendedNodeType::Number(n),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            } else {
                                // Fallback to string
                                let value_idx = tape.add_string(value);
                                tape.add_node(UnifiedNode::new(
                                    ExtendedNodeType::String(value_idx),
                                    fionn_core::format::FormatKind::Ison,
                                ));
                            }
                        }
                        IsonFieldType::Bool => {
                            let b = value == "true" || value == "1";
                            tape.add_node(UnifiedNode::new(
                                ExtendedNodeType::Bool(b),
                                fionn_core::format::FormatKind::Ison,
                            ));
                        }
                        _ => {
                            let value_idx = tape.add_string(value);
                            tape.add_node(UnifiedNode::new(
                                ExtendedNodeType::String(value_idx),
                                fionn_core::format::FormatKind::Ison,
                            ));
                        }
                    }
                }

                segments.push(SegmentBoundary {
                    start_idx: segment_start,
                    end_idx: tape.nodes().len(),
                    source_idx: line_index,
                });

                successful += 1;
            } else {
                // Check if it's a comment
                let trimmed = line.iter().filter(|&&b| !b.is_ascii_whitespace()).count();
                if trimmed > 0 && !line.contains(&b'#') {
                    failed += 1;
                    errors.push(LineError {
                        line_index,
                        error_message: "Failed to parse ISONL line".to_string(),
                        raw_line: String::from_utf8_lossy(line).to_string(),
                    });
                }
            }

            line_start = line_end;
        }

        let elapsed = start.elapsed();
        tape.metadata.schema_match_ratio = 1.0;

        Ok(TapeBatchResult {
            tape,
            segments,
            errors,
            statistics: BatchStatistics {
                total_lines: successful + failed,
                successful_lines: successful,
                failed_lines: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                avg_memory_per_line: if successful > 0 {
                    data.len() / successful
                } else {
                    0
                },
                overall_schema_match_ratio: 1.0,
            },
        })
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processor_creation() {
        let processor = SimdIsonlBatchProcessor::new();
        assert!(processor.parser().is_streaming());
    }

    #[test]
    fn test_process_single_line() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("\"id\":1"));
        assert!(result.documents[0].contains("\"name\":\"Alice\""));
    }

    #[test]
    fn test_process_multiple_lines() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice\ntable.users|id:int|name:string|2|Bob";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 2);
    }

    #[test]
    fn test_process_with_comments() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"# This is a comment\ntable.users|id:int|name:string|1|Alice";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_process_with_empty_lines() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data =
            b"table.users|id:int|name:string|1|Alice\n\ntable.users|id:int|name:string|2|Bob";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 2);
    }

    #[test]
    fn test_schema_filtering() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data =
            b"table.users|id:int|name:string|1|Alice\ntable.events|id:int|type:string|1|click";

        // Schema that only matches "users"
        let schema = CompiledSchema::compile(&["users".to_string()]).unwrap();

        let result = processor.process_batch_optimized(data, &schema).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("Alice"));
        assert_eq!(result.statistics.schema_filtered_lines, 1);
    }

    #[test]
    fn test_schema_field_matching() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";

        // Schema that matches "name" field
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();

        let result = processor.process_batch_optimized(data, &schema).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_empty_schema_matches_all() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";

        // Empty schema - should match everything
        let schema = CompiledSchema::compile(&[]).unwrap();

        let result = processor.process_batch_optimized(data, &schema).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_invalid_line_error() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"invalid line without pipe delimiters";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 0);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_statistics() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice\ntable.users|id:int|name:string|2|Bob";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.statistics.total_lines, 2);
        assert_eq!(result.statistics.successful_lines, 2);
        assert_eq!(result.statistics.failed_lines, 0);
        assert!(result.statistics.processing_time_ms >= 0.0);
    }

    #[test]
    fn test_reset() {
        let mut processor = SimdIsonlBatchProcessor::new();
        processor.reset();
        // Should not panic
    }

    #[test]
    fn test_default() {
        let processor = SimdIsonlBatchProcessor::default();
        assert!(processor.parser().is_streaming());
    }
}

// =============================================================================
// TapeBatchProcessor Tests
// =============================================================================

#[cfg(test)]
mod tape_tests {
    use super::*;
    use crate::format_dson::TapeBatchProcessor;

    #[test]
    fn test_tape_based_processing() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice\ntable.users|id:int|name:string|2|Bob";
        let schema = CompiledSchema::compile(&[]).unwrap();
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape(data, &schema, &arena).unwrap();

        // Should have 2 segments (one per line)
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.statistics.successful_lines, 2);

        // Tape should contain ISON nodes
        assert!(!result.tape.nodes().is_empty());
    }

    #[test]
    fn test_tape_unfiltered_processing() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.events|type:string|count:int|click|42";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.statistics.successful_lines, 1);
        assert!(!result.tape.nodes().is_empty());
    }

    #[test]
    fn test_tape_schema_filtering() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data =
            b"table.users|id:int|name:string|1|Alice\ntable.events|id:int|type:string|1|click";
        let schema = CompiledSchema::compile(&["users".to_string()]).unwrap();
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape(data, &schema, &arena).unwrap();

        // Only users line should be fully processed
        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.statistics.successful_lines, 1);

        // Tape should have SkipMarker for filtered line
        assert!(result.tape.metadata.skipped_count > 0);
    }

    #[test]
    fn test_tape_isonl_schema_preservation() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.users|id:int|name:string|1|Alice";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        // Should have original syntax preserved (IsonSchemaHeader)
        assert!(!result.tape.original_syntax.is_empty());

        // First syntax entry should be IsonSchemaHeader
        if let OriginalSyntax::IsonSchemaHeader { fields } = &result.tape.original_syntax[0] {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].0, "id");
            assert!(matches!(fields[0].1, IsonFieldType::Int));
            assert_eq!(fields[1].0, "name");
            assert!(matches!(fields[1].1, IsonFieldType::String));
        } else {
            panic!("Expected IsonSchemaHeader original syntax");
        }
    }

    #[test]
    fn test_tape_ison_field_types() {
        let mut processor = SimdIsonlBatchProcessor::new();
        // Test all field types: int, float, bool, string
        let data = b"table.test|a:int|b:float|c:bool|d:string|42|3.14|true|hello";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.statistics.successful_lines, 1);

        // Check that we have nodes for each field type
        let nodes = result.tape.nodes();
        let mut has_number = false;
        let mut has_bool = false;
        let mut has_ison_typed_field = false;

        for node in nodes {
            match &node.node_type {
                ExtendedNodeType::Number(_) => has_number = true,
                ExtendedNodeType::Bool(_) => has_bool = true,
                ExtendedNodeType::IsonTypedField { .. } => has_ison_typed_field = true,
                _ => {}
            }
        }

        assert!(has_number, "Should have Number nodes");
        assert!(has_bool, "Should have Bool nodes");
        assert!(has_ison_typed_field, "Should have IsonTypedField nodes");
    }

    #[test]
    fn test_tape_segment_boundaries() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.a|x:int|1\ntable.b|y:int|2\ntable.c|z:int|3";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        assert_eq!(result.segments.len(), 3);

        // Verify segment boundaries are ordered correctly
        for (i, segment) in result.segments.iter().enumerate() {
            assert_eq!(segment.source_idx, i);
            assert!(segment.start_idx < segment.end_idx);

            // Each segment should not overlap with the next
            if i + 1 < result.segments.len() {
                assert!(segment.end_idx <= result.segments[i + 1].start_idx);
            }
        }
    }

    #[test]
    fn test_tape_empty_input() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        assert_eq!(result.segments.len(), 0);
        assert_eq!(result.statistics.total_lines, 0);
    }

    #[test]
    fn test_tape_format_kind() {
        let processor = SimdIsonlBatchProcessor::new();
        assert!(matches!(
            TapeBatchProcessor::format_kind(&processor),
            fionn_core::format::FormatKind::Ison
        ));
    }

    #[test]
    fn test_tape_table_block_node() {
        let mut processor = SimdIsonlBatchProcessor::new();
        let data = b"table.events|id:int|1";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        // Find the IsonTableBlock node
        // Note: The ISON parser strips the prefix, so "table.events" becomes "events"
        let mut found_table_block = false;
        for node in result.tape.nodes() {
            if let ExtendedNodeType::IsonTableBlock { name_idx } = &node.node_type {
                let name = result.tape.get_string(*name_idx);
                assert_eq!(name, Some("events"));
                found_table_block = true;
                break;
            }
        }

        assert!(found_table_block, "Should have IsonTableBlock node");
    }
}
