// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-TOML Batch Processor
//!
//! High-performance TOML processing with schema-aware filtering.
//! Converts TOML documents to JSON for DSON operations.

use crate::skiptape::error::Result;
use crate::skiptape::schema::CompiledSchema;
use fionn_simd::formats::TomlParser;
use std::time::Instant;

use crate::format_dson::{BatchStatistics, FormatBatchProcessor, FormatBatchResult, LineError};

use crate::format_dson::{SegmentBoundary, TapeBatchProcessor, TapeBatchResult};

use crate::skiptape::unified_tape::{ExtendedNodeType, OriginalSyntax, UnifiedNode, UnifiedTape};

// =============================================================================
// TOML Batch Result
// =============================================================================

/// Result of processing a TOML batch
#[derive(Debug)]
pub struct TomlBatchResult {
    /// Successfully processed JSON documents
    pub documents: Vec<String>,
    /// Processing errors
    pub errors: Vec<TomlDocError>,
    /// Batch statistics
    pub statistics: TomlBatchStatistics,
}

/// Error for a specific document during batch processing
#[derive(Debug, Clone)]
pub struct TomlDocError {
    /// Index of the document
    pub doc_index: usize,
    /// The error message
    pub error_message: String,
    /// Raw document content
    pub raw_doc: String,
}

/// Statistics for TOML batch processing
#[derive(Debug, Clone, Default)]
pub struct TomlBatchStatistics {
    /// Total documents processed
    pub total_documents: usize,
    /// Successfully parsed documents
    pub successful_documents: usize,
    /// Failed documents
    pub failed_documents: usize,
    /// Total processing time in milliseconds
    pub processing_time_ms: f64,
    /// Documents skipped by schema filter
    pub schema_filtered_documents: usize,
}

// =============================================================================
// SimdTomlBatchProcessor
// =============================================================================

/// SIMD-accelerated TOML batch processor
#[derive(Debug)]
pub struct SimdTomlBatchProcessor {
    /// The underlying TOML parser
    parser: TomlParser,
}

impl Default for SimdTomlBatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl SimdTomlBatchProcessor {
    /// Create a new TOML batch processor
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // TomlParser::new() is not const
    pub fn new() -> Self {
        Self {
            parser: TomlParser::new(),
        }
    }

    /// Process a TOML batch with schema filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_optimized(
        &mut self,
        toml_data: &[u8],
        schema: &CompiledSchema,
    ) -> Result<TomlBatchResult> {
        let start = Instant::now();

        // TOML is typically a single document
        let mut results = Vec::new();
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;
        let mut filtered = 0;

        match self.parse_toml_to_json(toml_data) {
            Ok(json) => {
                if self.matches_schema(&json, schema) {
                    results.push(json);
                    successful += 1;
                } else {
                    filtered += 1;
                }
            }
            Err(e) => {
                failed += 1;
                errors.push(TomlDocError {
                    doc_index: 0,
                    error_message: e,
                    raw_doc: String::from_utf8_lossy(toml_data).to_string(),
                });
            }
        }

        let elapsed = start.elapsed();

        Ok(TomlBatchResult {
            documents: results,
            errors,
            statistics: TomlBatchStatistics {
                total_documents: successful + failed + filtered,
                successful_documents: successful,
                failed_documents: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_documents: filtered,
            },
        })
    }

    /// Process a TOML batch without schema filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_unfiltered(&mut self, toml_data: &[u8]) -> Result<TomlBatchResult> {
        let start = Instant::now();

        let mut results = Vec::new();
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;

        match self.parse_toml_to_json(toml_data) {
            Ok(json) => {
                results.push(json);
                successful += 1;
            }
            Err(e) => {
                failed += 1;
                errors.push(TomlDocError {
                    doc_index: 0,
                    error_message: e,
                    raw_doc: String::from_utf8_lossy(toml_data).to_string(),
                });
            }
        }

        let elapsed = start.elapsed();

        Ok(TomlBatchResult {
            documents: results,
            errors,
            statistics: TomlBatchStatistics {
                total_documents: successful + failed,
                successful_documents: successful,
                failed_documents: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_documents: 0,
            },
        })
    }

    /// Parse TOML to JSON string
    #[allow(clippy::unused_self)] // Method signature for API consistency
    fn parse_toml_to_json(&self, data: &[u8]) -> std::result::Result<String, String> {
        let toml_str = std::str::from_utf8(data).map_err(|e| e.to_string())?;
        self.simple_toml_to_json(toml_str)
    }

    /// Simple TOML to JSON converter for basic documents
    #[allow(clippy::unused_self)] // Method signature for API consistency
    #[allow(clippy::unnecessary_wraps)] // Error propagation expected in future
    fn simple_toml_to_json(&self, toml: &str) -> std::result::Result<String, String> {
        let mut json = String::from("{");
        let mut first = true;
        let mut current_section: Option<String> = None;
        let mut section_first = true;

        for line in toml.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Check for section header [section]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // Close previous section if any
                if current_section.is_some() {
                    json.push('}');
                }

                if !first {
                    json.push(',');
                }
                first = false;

                let section_name = &trimmed[1..trimmed.len() - 1];
                json.push('"');
                json.push_str(&Self::escape_json_string(section_name));
                json.push_str("\":{");
                current_section = Some(section_name.to_string());
                section_first = true;
                continue;
            }

            // Parse key = value
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let value = trimmed[eq_pos + 1..].trim();

                if current_section.is_some() {
                    if !section_first {
                        json.push(',');
                    }
                    section_first = false;
                } else {
                    if !first {
                        json.push(',');
                    }
                    first = false;
                }

                json.push('"');
                json.push_str(&Self::escape_json_string(key));
                json.push_str("\":");

                // Determine value type - booleans, numbers, arrays, and tables are written verbatim
                if value == "true"
                    || value == "false"
                    || value.parse::<i64>().is_ok()
                    || value.parse::<f64>().is_ok()
                    || (value.starts_with('[') && value.ends_with(']'))
                    || (value.starts_with('{') && value.ends_with('}'))
                {
                    json.push_str(value);
                } else if value.starts_with('"') && value.ends_with('"') {
                    // Already quoted string
                    let unquoted = &value[1..value.len() - 1];
                    json.push('"');
                    json.push_str(&Self::escape_json_string(unquoted));
                    json.push('"');
                } else {
                    // Unquoted string
                    json.push('"');
                    json.push_str(&Self::escape_json_string(value));
                    json.push('"');
                }
            }
        }

        // Close last section if any
        if current_section.is_some() {
            json.push('}');
        }

        json.push('}');
        Ok(json)
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
    pub const fn parser(&self) -> &TomlParser {
        &self.parser
    }
}

// =============================================================================
// FormatBatchProcessor Implementation
// =============================================================================

impl FormatBatchProcessor for SimdTomlBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Toml
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
                    line_index: e.doc_index,
                    error_message: e.error_message,
                    raw_line: e.raw_doc,
                })
                .collect(),
            statistics: BatchStatistics {
                total_lines: result.statistics.total_documents,
                successful_lines: result.statistics.successful_documents,
                failed_lines: result.statistics.failed_documents,
                processing_time_ms: result.statistics.processing_time_ms,
                avg_memory_per_line: 0,
                #[allow(clippy::cast_precision_loss)] // Acceptable for ratio calculation
                overall_schema_match_ratio: if result.statistics.total_documents > 0 {
                    result.statistics.successful_documents as f64
                        / result.statistics.total_documents as f64
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
                    line_index: e.doc_index,
                    error_message: e.error_message,
                    raw_line: e.raw_doc,
                })
                .collect(),
            statistics: BatchStatistics {
                total_lines: result.statistics.total_documents,
                successful_lines: result.statistics.successful_documents,
                failed_lines: result.statistics.failed_documents,
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

impl<'arena> TapeBatchProcessor<'arena> for SimdTomlBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Toml
    }

    fn process_to_tape(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        arena: &'arena bumpalo::Bump,
    ) -> fionn_core::Result<TapeBatchResult<'arena>> {
        let start = Instant::now();

        // Create unified tape for TOML
        let mut tape = UnifiedTape::new(arena, fionn_core::format::FormatKind::Toml);
        tape.metadata.original_size = data.len();

        let mut segments: Vec<SegmentBoundary> = Vec::new();
        let mut errors = Vec::new();
        let mut successful = 0;
        let failed = 0;
        let mut filtered = 0;

        // TOML is a single document format
        let toml_str = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(e) => {
                errors.push(LineError {
                    line_index: 0,
                    error_message: e.to_string(),
                    raw_line: String::from_utf8_lossy(data).to_string(),
                });
                let elapsed = start.elapsed();
                return Ok(TapeBatchResult {
                    tape,
                    segments,
                    errors,
                    statistics: BatchStatistics {
                        total_lines: 1,
                        successful_lines: 0,
                        failed_lines: 1,
                        processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                        avg_memory_per_line: 0,
                        overall_schema_match_ratio: 0.0,
                    },
                });
            }
        };

        // Parse TOML to tape structure
        let segment_start = tape.nodes().len();
        let matched = self.parse_toml_to_tape(toml_str, &mut tape, schema);

        if matched {
            segments.push(SegmentBoundary {
                start_idx: segment_start,
                end_idx: tape.nodes().len(),
                source_idx: 0,
            });
            successful += 1;
        } else {
            // Document didn't match schema - add skip marker
            tape.add_node(UnifiedNode::new(
                ExtendedNodeType::SkipMarker,
                fionn_core::format::FormatKind::Toml,
            ));
            tape.metadata.skipped_count += 1;
            filtered += 1;
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

        // Create unified tape for TOML
        let mut tape = UnifiedTape::new(arena, fionn_core::format::FormatKind::Toml);
        tape.metadata.original_size = data.len();

        let mut segments: Vec<SegmentBoundary> = Vec::new();
        let mut errors = Vec::new();
        let mut successful = 0;
        let failed = 0;

        // TOML is a single document format
        let toml_str = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(e) => {
                errors.push(LineError {
                    line_index: 0,
                    error_message: e.to_string(),
                    raw_line: String::from_utf8_lossy(data).to_string(),
                });
                let elapsed = start.elapsed();
                return Ok(TapeBatchResult {
                    tape,
                    segments,
                    errors,
                    statistics: BatchStatistics {
                        total_lines: 1,
                        successful_lines: 0,
                        failed_lines: 1,
                        processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                        avg_memory_per_line: 0,
                        overall_schema_match_ratio: 0.0,
                    },
                });
            }
        };

        // Parse TOML to tape structure (no schema filtering)
        let segment_start = tape.nodes().len();

        // Start object
        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::ObjectStart,
            fionn_core::format::FormatKind::Toml,
        ));

        let mut current_depth: u8 = 1;
        let mut in_section = false;

        for line in toml_str.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() {
                continue;
            }

            // Preserve comments
            if let Some(comment_content) = trimmed.strip_prefix('#') {
                let comment_text = comment_content.trim_start();
                let _comment_idx = tape.add_string(comment_text);
                let _syntax_idx = tape.add_original_syntax(OriginalSyntax::ToonComment {
                    text: comment_text.to_string(),
                });
                // We store comments as a special marker (reusing TOON comment for now)
                continue;
            }

            // Check for section header [section]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // Close previous section if any
                if in_section {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::ObjectEnd,
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                    current_depth = current_depth.saturating_sub(1);
                }

                let is_array_table = trimmed.starts_with("[[") && trimmed.ends_with("]]");
                let section_name = if is_array_table {
                    &trimmed[2..trimmed.len() - 2]
                } else {
                    &trimmed[1..trimmed.len() - 1]
                };

                let path_idx = tape.add_string(section_name);
                let syntax_idx = tape.add_original_syntax(OriginalSyntax::TomlDottedKey {
                    full_key: section_name.to_string(),
                });

                // Add key for section
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::Key(path_idx),
                        fionn_core::format::FormatKind::Toml,
                    )
                    .with_depth(current_depth),
                );

                if is_array_table {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::TomlArrayTableStart { path_idx },
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_original_syntax(syntax_idx)
                        .with_depth(current_depth),
                    );
                } else {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::TomlTableStart { path_idx },
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_original_syntax(syntax_idx)
                        .with_depth(current_depth),
                    );
                }

                current_depth = current_depth.saturating_add(1);
                in_section = true;
                continue;
            }

            // Parse key = value
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let value = trimmed[eq_pos + 1..].trim();

                let key_idx = tape.add_string(key);
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::Key(key_idx),
                        fionn_core::format::FormatKind::Toml,
                    )
                    .with_depth(current_depth),
                );

                // Determine value type and add appropriate node
                if value == "true" {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Bool(true),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if value == "false" {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Bool(false),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if let Ok(n) = value.parse::<i64>() {
                    #[allow(clippy::cast_precision_loss)] // Acceptable for numeric display
                    let num = n as f64;
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Number(num),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if let Ok(n) = value.parse::<f64>() {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Number(n),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    // Quoted string
                    let unquoted = &value[1..value.len() - 1];
                    let str_idx = tape.add_string(unquoted);
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::String(str_idx),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if value.starts_with('[') && value.ends_with(']') {
                    // Inline array - simplified handling
                    let str_idx = tape.add_string(value);
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::String(str_idx),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if value.starts_with('{') && value.ends_with('}') {
                    // Inline table - add marker
                    let syntax_idx = tape.add_original_syntax(OriginalSyntax::TomlInlineTable);
                    let str_idx = tape.add_string(value);
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::String(str_idx),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_original_syntax(syntax_idx)
                        .with_depth(current_depth),
                    );
                } else {
                    // Unquoted or other value
                    let str_idx = tape.add_string(value);
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::String(str_idx),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                }
            }
        }

        // Close any open section
        if in_section {
            tape.add_node(
                UnifiedNode::new(
                    ExtendedNodeType::ObjectEnd,
                    fionn_core::format::FormatKind::Toml,
                )
                .with_depth(current_depth),
            );
        }

        // Close root object
        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::ObjectEnd,
            fionn_core::format::FormatKind::Toml,
        ));

        segments.push(SegmentBoundary {
            start_idx: segment_start,
            end_idx: tape.nodes().len(),
            source_idx: 0,
        });
        successful += 1;

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

impl SimdTomlBatchProcessor {
    /// Parse TOML to unified tape with schema filtering
    #[allow(clippy::unused_self)] // Method signature for API consistency
    #[allow(clippy::too_many_lines)] // Complex tape construction with schema filtering
    fn parse_toml_to_tape(
        &self,
        toml: &str,
        tape: &mut UnifiedTape<'_>,
        schema: &CompiledSchema,
    ) -> bool {
        // Check schema match first (if schema has patterns)
        if !schema.include_patterns.is_empty() {
            let mut has_match = false;
            for pattern in &schema.include_patterns {
                if toml.contains(&pattern.path) {
                    has_match = true;
                    break;
                }
            }
            if !has_match {
                return false;
            }
        }

        // Start object
        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::ObjectStart,
            fionn_core::format::FormatKind::Toml,
        ));

        let mut current_depth: u8 = 1;
        let mut in_section = false;

        for line in toml.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Check for section header
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // Close previous section if any
                if in_section {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::ObjectEnd,
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                    current_depth = current_depth.saturating_sub(1);
                }

                let is_array_table = trimmed.starts_with("[[") && trimmed.ends_with("]]");
                let section_name = if is_array_table {
                    &trimmed[2..trimmed.len() - 2]
                } else {
                    &trimmed[1..trimmed.len() - 1]
                };

                // Check if section matches schema
                let section_matches =
                    schema.include_patterns.is_empty() || schema.matches_path(section_name);

                if !section_matches {
                    // Skip this section
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::SkipMarker,
                        fionn_core::format::FormatKind::Toml,
                    ));
                    tape.metadata.skipped_count += 1;
                    in_section = false;
                    continue;
                }

                let path_idx = tape.add_string(section_name);
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::Key(path_idx),
                        fionn_core::format::FormatKind::Toml,
                    )
                    .with_depth(current_depth),
                );

                if is_array_table {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::TomlArrayTableStart { path_idx },
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::TomlTableStart { path_idx },
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                }

                current_depth = current_depth.saturating_add(1);
                in_section = true;
                continue;
            }

            // Parse key = value
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let value = trimmed[eq_pos + 1..].trim();

                // Check if key matches schema
                let key_matches = schema.include_patterns.is_empty() || schema.matches_path(key);

                if !key_matches {
                    tape.add_node(UnifiedNode::new(
                        ExtendedNodeType::SkipMarker,
                        fionn_core::format::FormatKind::Toml,
                    ));
                    tape.metadata.skipped_count += 1;
                    continue;
                }

                let key_idx = tape.add_string(key);
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::Key(key_idx),
                        fionn_core::format::FormatKind::Toml,
                    )
                    .with_depth(current_depth),
                );

                // Add value node (simplified - same as unfiltered)
                if value == "true" {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Bool(true),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if value == "false" {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Bool(false),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if let Ok(n) = value.parse::<i64>() {
                    #[allow(clippy::cast_precision_loss)] // Acceptable for numeric display
                    let num = n as f64;
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Number(num),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else if let Ok(n) = value.parse::<f64>() {
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::Number(n),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                } else {
                    // String value (remove quotes if present)
                    let str_val =
                        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                            &value[1..value.len() - 1]
                        } else {
                            value
                        };
                    let str_idx = tape.add_string(str_val);
                    tape.add_node(
                        UnifiedNode::new(
                            ExtendedNodeType::String(str_idx),
                            fionn_core::format::FormatKind::Toml,
                        )
                        .with_depth(current_depth),
                    );
                }
            }
        }

        // Close any open section
        if in_section {
            tape.add_node(
                UnifiedNode::new(
                    ExtendedNodeType::ObjectEnd,
                    fionn_core::format::FormatKind::Toml,
                )
                .with_depth(current_depth),
            );
        }

        // Close root object
        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::ObjectEnd,
            fionn_core::format::FormatKind::Toml,
        ));

        true
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
        let processor = SimdTomlBatchProcessor::new();
        let _ = processor.parser();
    }

    #[test]
    fn test_simple_toml() {
        let mut processor = SimdTomlBatchProcessor::new();
        let data = b"name = \"Alice\"\nage = 30";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("Alice"));
        assert!(result.documents[0].contains("30"));
    }

    #[test]
    fn test_toml_with_section() {
        let mut processor = SimdTomlBatchProcessor::new();
        let data = b"[database]\nhost = \"localhost\"\nport = 5432";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("database"));
        assert!(result.documents[0].contains("localhost"));
    }

    #[test]
    fn test_toml_types() {
        let mut processor = SimdTomlBatchProcessor::new();
        let data = b"name = \"Test\"\ncount = 42\nactive = true\nscore = 3.14";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        let json = &result.documents[0];
        assert!(json.contains("\"count\":42"));
        assert!(json.contains("\"active\":true"));
    }

    #[test]
    fn test_toml_with_comments() {
        let mut processor = SimdTomlBatchProcessor::new();
        let data = b"# Comment\nname = \"Alice\"\n# Another comment\nage = 30";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_empty_schema_matches_all() {
        let mut processor = SimdTomlBatchProcessor::new();
        let data = b"name = \"Alice\"";
        let schema = CompiledSchema::compile(&[]).unwrap();

        let result = processor.process_batch_optimized(data, &schema).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_reset() {
        let mut processor = SimdTomlBatchProcessor::new();
        processor.reset();
    }

    #[test]
    fn test_default() {
        let processor = SimdTomlBatchProcessor::default();
        let _ = processor.parser();
    }

    // =========================================================================
    // Tape-based processing tests (requires dson-multi-format + toml-dson features)
    // =========================================================================

    mod tape_tests {
        use super::*;
        use crate::format_dson::TapeBatchProcessor;

        #[test]
        fn test_tape_based_processing() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdTomlBatchProcessor::new();
            let data = b"name = \"Alice\"\nage = 30";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Check we have nodes
            assert!(!result.tape.nodes().is_empty());
            // Check we have segments
            assert_eq!(result.segments.len(), 1); // 1 document
            // Check statistics
            assert_eq!(result.statistics.successful_lines, 1);
        }

        #[test]
        fn test_tape_toml_structure() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdTomlBatchProcessor::new();
            let data = b"name = \"Alice\"\nage = 30";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find ObjectStart and ObjectEnd
            let object_start_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::ObjectStart))
                .count();
            let object_end_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::ObjectEnd))
                .count();

            assert_eq!(object_start_count, 1);
            assert_eq!(object_end_count, 1);
        }

        #[test]
        fn test_tape_toml_section() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdTomlBatchProcessor::new();
            let data = b"[database]\nhost = \"localhost\"\nport = 5432";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find TomlTableStart nodes
            let table_start_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::TomlTableStart { .. }))
                .count();

            assert_eq!(table_start_count, 1);
        }

        #[test]
        fn test_tape_toml_types() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdTomlBatchProcessor::new();
            let data = b"name = \"Test\"\ncount = 42\nactive = true\nscore = 3.14";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find different value types
            let string_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::String(_)))
                .count();
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

            assert!(string_count > 0);
            assert_eq!(number_count, 2); // 42 and 3.14
            assert_eq!(bool_count, 1); // true
        }

        #[test]
        fn test_tape_toml_schema_filtering() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdTomlBatchProcessor::new();
            let data = b"name = \"Alice\"\nage = 30\nsecret = \"password\"";

            // Filter to only include 'name'
            let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
            let result = processor.process_to_tape(data, &schema, &arena).unwrap();

            // Should have parsed successfully with schema match
            assert_eq!(result.statistics.successful_lines, 1);
        }

        #[test]
        fn test_tape_toml_array_table() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdTomlBatchProcessor::new();
            let data = b"[[servers]]\nname = \"alpha\"\nip = \"10.0.0.1\"";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find TomlArrayTableStart nodes
            let array_table_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::TomlArrayTableStart { .. }))
                .count();

            assert_eq!(array_table_count, 1);
        }

        #[test]
        fn test_tape_toml_keys() {
            let arena = bumpalo::Bump::new();
            let mut processor = SimdTomlBatchProcessor::new();
            let data = b"name = \"Alice\"\nage = 30";

            let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

            // Find Key nodes
            let key_count = result
                .tape
                .nodes()
                .iter()
                .filter(|n| matches!(n.node_type, ExtendedNodeType::Key(_)))
                .count();

            assert_eq!(key_count, 2); // name, age
        }
    }
}
