// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-TOON Batch Processor
//!
//! High-performance TOON processing with schema-aware filtering.
//! TOON batch processor for line-based TOON documents.

use crate::skiptape::error::Result;
use crate::skiptape::schema::CompiledSchema;
use crate::skiptape::simd_ops::SimdLineSeparator;
use fionn_simd::formats::ToonParser;
use std::time::Instant;

use crate::format_dson::{BatchStatistics, FormatBatchProcessor, FormatBatchResult, LineError};

use crate::format_dson::{SegmentBoundary, TapeBatchProcessor, TapeBatchResult};

use crate::skiptape::unified_tape::{ExtendedNodeType, OriginalSyntax, UnifiedNode, UnifiedTape};

// =============================================================================
// TOON Batch Result
// =============================================================================

/// Result of processing a TOON batch
#[derive(Debug)]
pub struct ToonBatchResult {
    /// Successfully processed JSON documents
    pub documents: Vec<String>,
    /// Processing errors
    pub errors: Vec<ToonLineError>,
    /// Batch statistics
    pub statistics: ToonBatchStatistics,
}

/// Error for a specific line during batch processing
#[derive(Debug, Clone)]
pub struct ToonLineError {
    /// Index of the line
    pub line_index: usize,
    /// The error message
    pub error_message: String,
    /// Raw line content
    pub raw_line: String,
}

/// Statistics for TOON batch processing
#[derive(Debug, Clone, Default)]
pub struct ToonBatchStatistics {
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
}

// =============================================================================
// SimdToonBatchProcessor
// =============================================================================

/// SIMD-accelerated TOON batch processor
#[derive(Debug)]
pub struct SimdToonBatchProcessor {
    /// The underlying TOON parser
    parser: ToonParser,
}

impl Default for SimdToonBatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl SimdToonBatchProcessor {
    /// Create a new TOON batch processor
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // ToonParser::new() is not const
    pub fn new() -> Self {
        Self {
            parser: ToonParser::new(),
        }
    }

    /// Process a TOON batch with schema filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_optimized(
        &mut self,
        toon_data: &[u8],
        schema: &CompiledSchema,
    ) -> Result<ToonBatchResult> {
        let start = Instant::now();

        let line_separator = SimdLineSeparator::new();
        let line_boundaries = line_separator.find_line_boundaries(toon_data);

        let mut documents = Vec::with_capacity(line_boundaries.len());
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;
        let mut filtered = 0;

        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line = &toon_data[line_start..line_end];

            // Skip empty lines
            if line.iter().all(|&b| b.is_ascii_whitespace()) {
                line_start = line_end;
                continue;
            }

            // Trim newline
            let line = line
                .strip_suffix(b"\n")
                .unwrap_or(line)
                .strip_suffix(b"\r")
                .unwrap_or(line);

            match self.parse_toon_line_to_json(line) {
                Ok(json) => {
                    if self.matches_schema(&json, schema) {
                        documents.push(json);
                        successful += 1;
                    } else {
                        filtered += 1;
                    }
                }
                Err(e) => {
                    // Skip comments
                    if line.contains(&b'#') {
                        line_start = line_end;
                        continue;
                    }
                    failed += 1;
                    errors.push(ToonLineError {
                        line_index,
                        error_message: e,
                        raw_line: String::from_utf8_lossy(line).to_string(),
                    });
                }
            }

            line_start = line_end;
        }

        let elapsed = start.elapsed();

        Ok(ToonBatchResult {
            documents,
            errors,
            statistics: ToonBatchStatistics {
                total_lines: successful + failed + filtered,
                successful_lines: successful,
                failed_lines: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_lines: filtered,
            },
        })
    }

    /// Process a TOON batch without schema filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_unfiltered(&mut self, toon_data: &[u8]) -> Result<ToonBatchResult> {
        let start = Instant::now();

        let line_separator = SimdLineSeparator::new();
        let line_boundaries = line_separator.find_line_boundaries(toon_data);

        let mut documents = Vec::with_capacity(line_boundaries.len());
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;

        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line = &toon_data[line_start..line_end];

            if line.iter().all(|&b| b.is_ascii_whitespace()) {
                line_start = line_end;
                continue;
            }

            let line = line
                .strip_suffix(b"\n")
                .unwrap_or(line)
                .strip_suffix(b"\r")
                .unwrap_or(line);

            match self.parse_toon_line_to_json(line) {
                Ok(json) => {
                    documents.push(json);
                    successful += 1;
                }
                Err(e) => {
                    if line.contains(&b'#') {
                        line_start = line_end;
                        continue;
                    }
                    failed += 1;
                    errors.push(ToonLineError {
                        line_index,
                        error_message: e,
                        raw_line: String::from_utf8_lossy(line).to_string(),
                    });
                }
            }

            line_start = line_end;
        }

        let elapsed = start.elapsed();

        Ok(ToonBatchResult {
            documents,
            errors,
            statistics: ToonBatchStatistics {
                total_lines: successful + failed,
                successful_lines: successful,
                failed_lines: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_lines: 0,
            },
        })
    }

    /// Parse a TOON line to JSON
    /// TOON format: key: value or key.subkey: value
    #[allow(clippy::unused_self)] // Method signature for API consistency
    #[allow(clippy::option_if_let_else)] // Early returns with if let are clearer here
    fn parse_toon_line_to_json(&self, line: &[u8]) -> std::result::Result<String, String> {
        let line_str = std::str::from_utf8(line).map_err(|e| e.to_string())?;
        let trimmed = line_str.trim();

        // Skip comments and empty
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return Err("Comment or empty line".to_string());
        }

        // Find key: value separator
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim();
            let value = trimmed[colon_pos + 1..].trim();

            let mut json = String::from("{\"");
            json.push_str(&Self::escape_json_string(key));
            json.push_str("\":");

            // Determine value type - booleans, integers, and floats are written verbatim
            if value.is_empty() || value == "null" {
                json.push_str("null");
            } else if value == "true"
                || value == "false"
                || value.parse::<i64>().is_ok()
                || value.parse::<f64>().is_ok()
            {
                json.push_str(value);
            } else {
                // String value
                let unquoted = value
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(value);
                json.push('"');
                json.push_str(&Self::escape_json_string(unquoted));
                json.push('"');
            }

            json.push('}');
            Ok(json)
        } else {
            Err("No key-value separator found".to_string())
        }
    }

    /// Check if JSON matches schema
    #[allow(clippy::unused_self)] // Method signature for API consistency
    fn matches_schema(&self, json: &str, schema: &CompiledSchema) -> bool {
        if schema.include_patterns.is_empty() {
            return true;
        }

        for pattern in &schema.include_patterns {
            if json.contains(&format!("\"{}\"", pattern.path)) {
                return true;
            }
        }

        false
    }

    /// Escape string for JSON
    fn escape_json_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    /// Reset processor state
    pub const fn reset(&mut self) {
        // No state to reset
    }

    /// Get a reference to the parser
    #[must_use]
    pub const fn parser(&self) -> &ToonParser {
        &self.parser
    }
}

// =============================================================================
// FormatBatchProcessor Implementation
// =============================================================================

impl FormatBatchProcessor for SimdToonBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Toon
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
                avg_memory_per_line: 0,
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
                avg_memory_per_line: 0,
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

impl<'arena> TapeBatchProcessor<'arena> for SimdToonBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Toon
    }

    #[allow(clippy::too_many_lines)] // Complex tape construction with schema filtering
    fn process_to_tape(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        arena: &'arena bumpalo::Bump,
    ) -> fionn_core::Result<TapeBatchResult<'arena>> {
        let start = Instant::now();

        let mut tape = UnifiedTape::new(arena, fionn_core::format::FormatKind::Toon);
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

            // Trim newline
            let line = line
                .strip_suffix(b"\n")
                .unwrap_or(line)
                .strip_suffix(b"\r")
                .unwrap_or(line);

            let line_str = if let Ok(s) = std::str::from_utf8(line) {
                s.trim()
            } else {
                failed += 1;
                errors.push(LineError {
                    line_index,
                    error_message: "Invalid UTF-8".to_string(),
                    raw_line: String::from_utf8_lossy(line).to_string(),
                });
                line_start = line_end;
                continue;
            };

            // Handle comments - preserve them in tape
            if let Some(comment_text) = line_str.strip_prefix('#') {
                let comment_text = comment_text.trim_start();
                let _syntax_idx = tape.add_original_syntax(OriginalSyntax::ToonComment {
                    text: comment_text.to_string(),
                });
                line_start = line_end;
                continue;
            }

            // Parse key: value
            if let Some(colon_pos) = line_str.find(':') {
                let key = line_str[..colon_pos].trim();
                let value = line_str[colon_pos + 1..].trim();

                // Check schema match
                let matches = schema.include_patterns.is_empty() || schema.matches_path(key);
                if !matches {
                    filtered += 1;
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::SkipMarker,
                        fionn_core::format::FormatKind::Toon,
                    ));
                    tape.metadata.skipped_count += 1;
                    line_start = line_end;
                    continue;
                }

                let segment_start = tape.nodes().len();

                // Check for folded key (a.b.c: value)
                let is_folded = key.contains('.');
                if is_folded {
                    let path_idx = tape.add_string(key);
                    let syntax_idx = tape.add_original_syntax(OriginalSyntax::ToonFoldedKey {
                        path: key.to_string(),
                    });
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::ToonFoldedKey { path_idx },
                            fionn_core::format::FormatKind::Toon,
                        )
                        .with_original_syntax(syntax_idx),
                    );
                } else {
                    let key_idx = tape.add_string(key);
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Key(key_idx),
                        fionn_core::format::FormatKind::Toon,
                    ));
                }

                // Add value
                if value.is_empty() {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Null,
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if value == "true" {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Bool(true),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if value == "false" {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Bool(false),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if value == "null" {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Null,
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if let Ok(n) = value.parse::<i64>() {
                    #[allow(clippy::cast_precision_loss)] // Acceptable for numeric display
                    let num = n as f64;
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Number(num),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if let Ok(n) = value.parse::<f64>() {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Number(n),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else {
                    // String value (remove quotes if present)
                    let str_val = value
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(value);
                    let str_idx = tape.add_string(str_val);
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::String(str_idx),
                        fionn_core::format::FormatKind::Toon,
                    ));
                }

                segments.push(SegmentBoundary {
                    start_idx: segment_start,
                    end_idx: tape.nodes().len(),
                    source_idx: line_index,
                });

                successful += 1;
            } else {
                failed += 1;
                errors.push(LineError {
                    line_index,
                    error_message: "No key-value separator found".to_string(),
                    raw_line: line_str.to_string(),
                });
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
            #[allow(clippy::cast_precision_loss)] // Acceptable for ratio calculation
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
                overall_schema_match_ratio: if total > 0 {
                    successful as f64 / total as f64
                } else {
                    0.0
                },
            },
        })
    }

    #[allow(clippy::too_many_lines)] // Complex tape construction without filtering
    fn process_to_tape_unfiltered(
        &mut self,
        data: &[u8],
        arena: &'arena bumpalo::Bump,
    ) -> fionn_core::Result<TapeBatchResult<'arena>> {
        let start = Instant::now();

        let mut tape = UnifiedTape::new(arena, fionn_core::format::FormatKind::Toon);
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

            // Trim newline
            let line = line
                .strip_suffix(b"\n")
                .unwrap_or(line)
                .strip_suffix(b"\r")
                .unwrap_or(line);

            let line_str = if let Ok(s) = std::str::from_utf8(line) {
                s.trim()
            } else {
                failed += 1;
                errors.push(LineError {
                    line_index,
                    error_message: "Invalid UTF-8".to_string(),
                    raw_line: String::from_utf8_lossy(line).to_string(),
                });
                line_start = line_end;
                continue;
            };

            // Handle comments - preserve them in tape
            if let Some(comment_text) = line_str.strip_prefix('#') {
                let comment_text = comment_text.trim_start();
                let _syntax_idx = tape.add_original_syntax(OriginalSyntax::ToonComment {
                    text: comment_text.to_string(),
                });
                line_start = line_end;
                continue;
            }

            // Parse key: value
            if let Some(colon_pos) = line_str.find(':') {
                let key = line_str[..colon_pos].trim();
                let value = line_str[colon_pos + 1..].trim();

                let segment_start = tape.nodes().len();

                // Check for folded key
                let is_folded = key.contains('.');
                if is_folded {
                    let path_idx = tape.add_string(key);
                    let syntax_idx = tape.add_original_syntax(OriginalSyntax::ToonFoldedKey {
                        path: key.to_string(),
                    });
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::ToonFoldedKey { path_idx },
                            fionn_core::format::FormatKind::Toon,
                        )
                        .with_original_syntax(syntax_idx),
                    );
                } else {
                    let key_idx = tape.add_string(key);
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Key(key_idx),
                        fionn_core::format::FormatKind::Toon,
                    ));
                }

                // Add value
                if value.is_empty() {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Null,
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if value == "true" {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Bool(true),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if value == "false" {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Bool(false),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if value == "null" {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Null,
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if let Ok(n) = value.parse::<i64>() {
                    #[allow(clippy::cast_precision_loss)] // Acceptable for numeric display
                    let num = n as f64;
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Number(num),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else if let Ok(n) = value.parse::<f64>() {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::Number(n),
                        fionn_core::format::FormatKind::Toon,
                    ));
                } else {
                    // String value (remove quotes if present)
                    let str_val = value
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(value);
                    let str_idx = tape.add_string(str_val);
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::String(str_idx),
                        fionn_core::format::FormatKind::Toon,
                    ));
                }

                segments.push(SegmentBoundary {
                    start_idx: segment_start,
                    end_idx: tape.nodes().len(),
                    source_idx: line_index,
                });

                successful += 1;
            } else {
                failed += 1;
                errors.push(LineError {
                    line_index,
                    error_message: "No key-value separator found".to_string(),
                    raw_line: line_str.to_string(),
                });
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
        let processor = SimdToonBatchProcessor::new();
        let _ = processor.parser();
    }

    #[test]
    fn test_simple_toon() {
        let mut processor = SimdToonBatchProcessor::new();
        let data = b"name: Alice\nage: 30";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 2);
    }

    #[test]
    fn test_toon_types() {
        let mut processor = SimdToonBatchProcessor::new();
        let data = b"count: 42\nactive: true\nscore: 3.14";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 3);
        assert!(result.documents[0].contains(":42"));
        assert!(result.documents[1].contains(":true"));
    }

    #[test]
    fn test_toon_with_comments() {
        let mut processor = SimdToonBatchProcessor::new();
        let data = b"# Comment\nname: Alice";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_empty_schema_matches_all() {
        let mut processor = SimdToonBatchProcessor::new();
        let data = b"name: Alice";
        let schema = CompiledSchema::compile(&[]).unwrap();

        let result = processor.process_batch_optimized(data, &schema).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_reset() {
        let mut processor = SimdToonBatchProcessor::new();
        processor.reset();
    }

    #[test]
    fn test_default() {
        let processor = SimdToonBatchProcessor::default();
        let _ = processor.parser();
    }

    // =========================================================================
    // Tape-based processing tests (requires dson-multi-format + toon-dson features)
    // =========================================================================

    mod tape_tests {
        use super::*;
        use crate::format_dson::TapeBatchProcessor;

        #[test]
        fn test_tape_based_processing() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdToonBatchProcessor::new();
            let data = b"name: Alice\nage: 30";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Check we have nodes
            assert!(!result.tape.nodes().is_empty());
            // Check we have segments
            assert_eq!(result.segments.len(), 2); // 2 lines
            // Check statistics
            assert_eq!(result.statistics.successful_lines, 2);
        }

        #[test]
        fn test_tape_toon_types() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdToonBatchProcessor::new();
            let data = b"count: 42\nactive: true\nscore: 3.14";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find different value types
            let number_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::Number(_)))
                .count();
            let bool_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::Bool(_)))
                .count();

            assert_eq!(number_count, 2); // 42 and 3.14
            assert_eq!(bool_count, 1); // true
        }

        #[test]
        fn test_tape_toon_folded_keys() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdToonBatchProcessor::new();
            let data = b"config.db.host: localhost\nconfig.db.port: 5432";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find ToonFoldedKey nodes
            let folded_key_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::ToonFoldedKey { .. }))
                .count();

            assert_eq!(folded_key_count, 2);
        }

        #[test]
        fn test_tape_toon_schema_filtering() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdToonBatchProcessor::new();
            let data = b"name: Alice\nage: 30\nsecret: password";

            // Filter to only include 'name'
            let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
            let result = processor.process_to_tape(data, &schema, &arena).unwrap();

            // Should have 1 successful (name), 2 skipped
            assert_eq!(result.statistics.successful_lines, 1);
        }

        #[test]
        fn test_tape_toon_keys() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdToonBatchProcessor::new();
            let data = b"name: Alice\nage: 30";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find Key nodes (non-folded)
            let key_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::Key(_)))
                .count();

            assert_eq!(key_count, 2); // name, age
        }

        #[test]
        fn test_tape_toon_comment_preservation() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdToonBatchProcessor::new();
            let data = b"# This is a comment\nname: Alice";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Comments should be preserved as original syntax
            assert!(result.tape.get_original_syntax(0).is_some());
        }

        #[test]
        fn test_tape_toon_null_values() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdToonBatchProcessor::new();
            let data = b"empty:\nnil: null";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find Null nodes
            let null_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::Null))
                .count();

            assert_eq!(null_count, 2);
        }
    }
}
