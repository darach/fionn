// SPDX-License-Identifier: MIT OR Apache-2.0
//! SIMD-YAML Batch Processor
//!
//! High-performance YAML processing with schema-aware filtering.
//!
//! # Unified Tape Architecture
//!
//! This processor emits to unified tape with YAML-specific markers:
//! - `YamlDocumentStart` / `YamlDocumentEnd` for multi-document YAML
//! - `YamlAnchor` / `YamlAlias` for anchor/alias preservation
//! - `YamlTag` for explicit YAML tags
//!
//! Original syntax is preserved via `OriginalSyntax::YamlAnchor`, etc.
//! for lossless round-trips.
//!
//! # Legacy String API
//!
//! The string-based API (`FormatBatchProcessor`) remains for backward
//! compatibility but does not preserve YAML-specific features.

use crate::skiptape::error::Result;
use crate::skiptape::schema::CompiledSchema;
use fionn_simd::formats::YamlParser;
use std::time::Instant;

use crate::format_dson::{
    BatchStatistics, FormatBatchProcessor, FormatBatchResult, LineError, SegmentBoundary,
    TapeBatchProcessor, TapeBatchResult,
};

use crate::skiptape::unified_tape::{ExtendedNodeType, OriginalSyntax, UnifiedNode, UnifiedTape};

// =============================================================================
// YAML Batch Result
// =============================================================================

/// Result of processing a YAML batch
#[derive(Debug)]
pub struct YamlBatchResult {
    /// Successfully processed JSON documents
    pub documents: Vec<String>,
    /// Processing errors
    pub errors: Vec<YamlDocError>,
    /// Batch statistics
    pub statistics: YamlBatchStatistics,
}

/// Error for a specific document during batch processing
#[derive(Debug, Clone)]
pub struct YamlDocError {
    /// Index of the document
    pub doc_index: usize,
    /// The error message
    pub error_message: String,
    /// Raw document content
    pub raw_doc: String,
}

/// Statistics for YAML batch processing
#[derive(Debug, Clone, Default)]
pub struct YamlBatchStatistics {
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
// SimdYamlBatchProcessor
// =============================================================================

/// SIMD-accelerated YAML batch processor
#[derive(Debug)]
pub struct SimdYamlBatchProcessor {
    /// The underlying YAML parser
    parser: YamlParser,
}

impl Default for SimdYamlBatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl SimdYamlBatchProcessor {
    /// Create a new YAML batch processor
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // YamlParser::new() is not const
    pub fn new() -> Self {
        Self {
            parser: YamlParser::new(),
        }
    }

    /// Process a YAML batch with schema filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_optimized(
        &mut self,
        yaml_data: &[u8],
        schema: &CompiledSchema,
    ) -> Result<YamlBatchResult> {
        let start = Instant::now();

        // Split on YAML document separators
        let documents = Self::split_yaml_documents(yaml_data);

        let mut results = Vec::with_capacity(documents.len());
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;
        let mut filtered = 0;

        for (doc_index, doc) in documents.iter().enumerate() {
            match self.parse_yaml_to_json(doc) {
                Ok(json) => {
                    // Check schema match
                    if self.matches_schema(&json, schema) {
                        results.push(json);
                        successful += 1;
                    } else {
                        filtered += 1;
                    }
                }
                Err(e) => {
                    failed += 1;
                    errors.push(YamlDocError {
                        doc_index,
                        error_message: e,
                        raw_doc: String::from_utf8_lossy(doc).to_string(),
                    });
                }
            }
        }

        let elapsed = start.elapsed();

        Ok(YamlBatchResult {
            documents: results,
            errors,
            statistics: YamlBatchStatistics {
                total_documents: successful + failed + filtered,
                successful_documents: successful,
                failed_documents: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_documents: filtered,
            },
        })
    }

    /// Process a YAML batch without schema filtering
    ///
    /// # Errors
    /// Returns an error if batch processing fundamentally fails
    pub fn process_batch_unfiltered(&mut self, yaml_data: &[u8]) -> Result<YamlBatchResult> {
        let start = Instant::now();

        let documents = Self::split_yaml_documents(yaml_data);

        let mut results = Vec::with_capacity(documents.len());
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;

        for (doc_index, doc) in documents.iter().enumerate() {
            match self.parse_yaml_to_json(doc) {
                Ok(json) => {
                    results.push(json);
                    successful += 1;
                }
                Err(e) => {
                    failed += 1;
                    errors.push(YamlDocError {
                        doc_index,
                        error_message: e,
                        raw_doc: String::from_utf8_lossy(doc).to_string(),
                    });
                }
            }
        }

        let elapsed = start.elapsed();

        Ok(YamlBatchResult {
            documents: results,
            errors,
            statistics: YamlBatchStatistics {
                total_documents: successful + failed,
                successful_documents: successful,
                failed_documents: failed,
                processing_time_ms: elapsed.as_secs_f64() * 1000.0,
                schema_filtered_documents: 0,
            },
        })
    }

    /// Split YAML data into individual documents
    fn split_yaml_documents(data: &[u8]) -> Vec<&[u8]> {
        let mut documents = Vec::new();
        let mut start = 0;

        // Find document separators (---)
        let data_str = std::str::from_utf8(data).unwrap_or("");
        for (i, line) in data_str.lines().enumerate() {
            if line.trim() == "---" && i > 0 {
                let end = data_str[..start].len()
                    + data_str[start..]
                        .find("---")
                        .unwrap_or_else(|| data_str[start..].len());
                if end > start {
                    documents.push(&data[start..end]);
                }
                start = end + 3;
                while start < data.len() && (data[start] == b'\n' || data[start] == b'\r') {
                    start += 1;
                }
            }
        }

        // Add remaining content
        if start < data.len() {
            documents.push(&data[start..]);
        }

        // If no separators found, treat entire input as one document
        if documents.is_empty() && !data.is_empty() {
            documents.push(data);
        }

        documents
    }

    /// Parse YAML document to JSON string
    fn parse_yaml_to_json(&self, doc: &[u8]) -> std::result::Result<String, String> {
        let yaml_str = std::str::from_utf8(doc).map_err(|e| e.to_string())?;

        // Use serde_yaml for parsing (if available), otherwise simple parsing
        // For now, use a simple key-value parser for basic YAML
        self.simple_yaml_to_json(yaml_str)
    }

    /// Simple YAML to JSON converter for basic documents
    #[allow(clippy::unused_self)] // Method signature for API consistency
    #[allow(clippy::unnecessary_wraps)] // Error propagation expected in future
    fn simple_yaml_to_json(&self, yaml: &str) -> std::result::Result<String, String> {
        let mut json = String::from("{");
        let mut first = true;

        for line in yaml.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Skip document markers
            if trimmed == "---" || trimmed == "..." {
                continue;
            }

            // Parse key: value
            if let Some(colon_pos) = trimmed.find(':') {
                let key = trimmed[..colon_pos].trim();
                let value = trimmed[colon_pos + 1..].trim();

                if !first {
                    json.push(',');
                }
                first = false;

                json.push('"');
                json.push_str(&Self::escape_json_string(key));
                json.push_str("\":");

                // Determine value type - booleans, integers, and floats are written verbatim
                if value.is_empty() || value == "null" || value == "~" {
                    json.push_str("null");
                } else if value == "true"
                    || value == "false"
                    || value.parse::<i64>().is_ok()
                    || value.parse::<f64>().is_ok()
                {
                    json.push_str(value);
                } else {
                    // String value - remove quotes if present
                    let unquoted = value
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                        .unwrap_or(value);
                    json.push('"');
                    json.push_str(&Self::escape_json_string(unquoted));
                    json.push('"');
                }
            }
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

        // Simple check: see if any pattern key exists in the JSON
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
    pub const fn parser(&self) -> &YamlParser {
        &self.parser
    }
}

// =============================================================================
// FormatBatchProcessor Implementation
// =============================================================================

impl FormatBatchProcessor for SimdYamlBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Yaml
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
// TapeBatchProcessor Implementation (Unified Tape Architecture)
// =============================================================================

impl<'arena> TapeBatchProcessor<'arena> for SimdYamlBatchProcessor {
    fn format_kind(&self) -> fionn_core::format::FormatKind {
        fionn_core::format::FormatKind::Yaml
    }

    fn process_to_tape(
        &mut self,
        data: &[u8],
        schema: &CompiledSchema,
        arena: &'arena bumpalo::Bump,
    ) -> fionn_core::Result<TapeBatchResult<'arena>> {
        let start = Instant::now();
        let format_kind = fionn_core::format::FormatKind::Yaml;

        // Create unified tape
        let mut tape = UnifiedTape::new(arena, format_kind);
        let mut segments = Vec::new();
        let mut errors = Vec::new();
        let mut successful = 0;
        let mut failed = 0;
        let mut filtered = 0;

        // Split on YAML document separators
        let documents = Self::split_yaml_documents(data);

        for (doc_index, doc) in documents.iter().enumerate() {
            let segment_start = tape.nodes().len();

            match self.parse_yaml_to_tape(doc, &mut tape, schema) {
                Ok(matches_schema) => {
                    if matches_schema {
                        let segment_end = tape.nodes().len();
                        segments.push(SegmentBoundary {
                            start_idx: segment_start,
                            end_idx: segment_end,
                            source_idx: doc_index,
                        });
                        successful += 1;
                    } else {
                        filtered += 1;
                    }
                }
                Err(e) => {
                    failed += 1;
                    errors.push(LineError {
                        line_index: doc_index,
                        error_message: e,
                        raw_line: String::from_utf8_lossy(doc).to_string(),
                    });
                }
            }
        }

        let elapsed = start.elapsed();
        let total = successful + failed + filtered;

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
                avg_memory_per_line: 0,
                overall_schema_match_ratio: if total > 0 {
                    successful as f64 / total as f64
                } else {
                    0.0
                },
            },
        })
    }

    fn process_to_tape_unfiltered(
        &mut self,
        data: &[u8],
        arena: &'arena bumpalo::Bump,
    ) -> fionn_core::Result<TapeBatchResult<'arena>> {
        // Use empty schema for unfiltered processing
        let schema = CompiledSchema::compile(&[])
            .map_err(|e| fionn_core::DsonError::ParseError(e.to_string()))?;
        self.process_to_tape(data, &schema, arena)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}

impl SimdYamlBatchProcessor {
    /// Parse a YAML document directly to unified tape
    ///
    /// This preserves YAML-specific features:
    /// - Document markers (`---`, `...`)
    /// - Anchors (`&anchor`)
    /// - Aliases (`*alias`)
    /// - Tags (`!tag`)
    #[allow(clippy::too_many_lines)] // Complex tape construction with YAML features
    #[allow(clippy::unused_self)] // Method signature for API consistency
    #[allow(clippy::unnecessary_wraps)] // Error propagation expected in future
    fn parse_yaml_to_tape(
        &self,
        doc: &[u8],
        tape: &mut UnifiedTape<'_>,
        schema: &CompiledSchema,
    ) -> std::result::Result<bool, String> {
        let yaml_str = std::str::from_utf8(doc).map_err(|e| e.to_string())?;
        let format_kind = fionn_core::format::FormatKind::Yaml;

        // Track anchors for alias resolution
        #[allow(clippy::collection_is_never_read)] // Anchors stored for future alias resolution
        let mut anchors: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut has_schema_match = schema.include_patterns.is_empty();

        // Emit YAML document start marker
        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::YamlDocumentStart,
            format_kind,
        ));

        // Emit object start
        tape.add_node(UnifiedNode::new(ExtendedNodeType::ObjectStart, format_kind).with_depth(1));

        for line in yaml_str.lines() {
            let trimmed = line.trim();

            // Skip empty lines and document markers
            if trimmed.is_empty() || trimmed == "---" || trimmed == "..." {
                continue;
            }

            // Handle comments (preserve as original syntax)
            if trimmed.starts_with('#') {
                // Comments are not emitted to tape but could be preserved
                // in OriginalSyntax if needed for exact round-trips
                continue;
            }

            // Handle anchors (&anchor)
            if let Some(anchor_start) = trimmed.find('&') {
                let anchor_end = trimmed[anchor_start + 1..]
                    .find(|c: char| c.is_whitespace() || c == ':')
                    .map_or(trimmed.len(), |i| anchor_start + 1 + i);
                let anchor_name = &trimmed[anchor_start + 1..anchor_end];

                // Store anchor position
                anchors.insert(anchor_name.to_string(), tape.nodes().len());

                // Add original syntax for lossless round-trip
                let syntax_idx = tape.add_original_syntax(OriginalSyntax::YamlAnchor {
                    name: anchor_name.to_string(),
                });

                // Emit anchor node
                let name_idx = tape.add_string(anchor_name);
                #[allow(clippy::cast_possible_truncation)] // Node index won't exceed u32::MAX
                let target_idx = tape.nodes().len() as u32 + 1;
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::YamlAnchor {
                            name_idx,
                            target_idx,
                        },
                        format_kind,
                    )
                    .with_depth(1)
                    .with_original_syntax(syntax_idx),
                );
            }

            // Handle aliases (*alias)
            if let Some(alias_pos) = trimmed.find('*') {
                let alias_end = trimmed[alias_pos + 1..]
                    .find(|c: char| c.is_whitespace())
                    .map_or(trimmed.len(), |i| alias_pos + 1 + i);
                let alias_target = &trimmed[alias_pos + 1..alias_end];

                // Add original syntax
                let syntax_idx = tape.add_original_syntax(OriginalSyntax::YamlAlias {
                    target: alias_target.to_string(),
                });

                // Emit alias node
                let target_idx = tape.add_string(alias_target);
                tape.add_node(
                    UnifiedNode::new(
                        ExtendedNodeType::YamlAlias {
                            target_name_idx: target_idx,
                        },
                        format_kind,
                    )
                    .with_depth(1)
                    .with_original_syntax(syntax_idx),
                );
                continue;
            }

            // Parse key: value
            if let Some(colon_pos) = trimmed.find(':') {
                let key = trimmed[..colon_pos].trim();
                // Remove any anchor from key
                let key = key.split('&').next().unwrap_or(key).trim();
                let value = trimmed[colon_pos + 1..].trim();

                // Check schema match
                if !has_schema_match && schema.matches_path(key) {
                    has_schema_match = true;
                }

                // Emit key
                let key_idx = tape.add_string(key);
                tape.add_node(
                    UnifiedNode::new(ExtendedNodeType::Key(key_idx), format_kind).with_depth(2),
                );

                // Emit value
                if value.is_empty() {
                    tape.add_node(
                        UnifiedNode::new(ExtendedNodeType::Null, format_kind).with_depth(2),
                    );
                } else if value == "true" {
                    tape.add_node(
                        UnifiedNode::new(ExtendedNodeType::Bool(true), format_kind).with_depth(2),
                    );
                } else if value == "false" {
                    tape.add_node(
                        UnifiedNode::new(ExtendedNodeType::Bool(false), format_kind).with_depth(2),
                    );
                } else if value == "null" || value == "~" {
                    tape.add_node(
                        UnifiedNode::new(ExtendedNodeType::Null, format_kind).with_depth(2),
                    );
                } else if let Ok(n) = value.parse::<i64>() {
                    #[allow(clippy::cast_precision_loss)] // Acceptable for numeric display
                    let num = n as f64;
                    tape.add_node(
                        UnifiedNode::new(ExtendedNodeType::Number(num), format_kind).with_depth(2),
                    );
                } else if let Ok(n) = value.parse::<f64>() {
                    tape.add_node(
                        UnifiedNode::new(ExtendedNodeType::Number(n), format_kind).with_depth(2),
                    );
                } else {
                    // String value - remove quotes if present
                    let unquoted = value
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                        .unwrap_or(value);
                    let str_idx = tape.add_string(unquoted);
                    tape.add_node(
                        UnifiedNode::new(ExtendedNodeType::String(str_idx), format_kind)
                            .with_depth(2),
                    );
                }
            }
        }

        // Emit object end
        tape.add_node(UnifiedNode::new(ExtendedNodeType::ObjectEnd, format_kind).with_depth(1));

        // Emit YAML document end marker
        tape.add_node(UnifiedNode::new(
            ExtendedNodeType::YamlDocumentEnd,
            format_kind,
        ));

        Ok(has_schema_match)
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
        let processor = SimdYamlBatchProcessor::new();
        let _ = processor.parser();
    }

    #[test]
    fn test_simple_yaml() {
        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"name: Alice\nage: 30";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        assert!(result.documents[0].contains("Alice"));
        assert!(result.documents[0].contains("30"));
    }

    #[test]
    fn test_yaml_with_types() {
        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"name: Test\ncount: 42\nactive: true\nscore: 3.14";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        let json = &result.documents[0];
        assert!(json.contains("\"count\":42"));
        assert!(json.contains("\"active\":true"));
        assert!(json.contains("\"score\":3.14"));
    }

    #[test]
    fn test_yaml_with_null() {
        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"name: Test\nempty: ~\nnullval: null";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
        let json = &result.documents[0];
        assert!(json.contains("null"));
    }

    #[test]
    fn test_yaml_with_comments() {
        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"# Comment\nname: Alice\n# Another comment\nage: 30";

        let result = processor.process_batch_unfiltered(data).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_empty_schema_matches_all() {
        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"name: Alice\nage: 30";
        let schema = CompiledSchema::compile(&[]).unwrap();

        let result = processor.process_batch_optimized(data, &schema).unwrap();
        assert_eq!(result.documents.len(), 1);
    }

    #[test]
    fn test_reset() {
        let mut processor = SimdYamlBatchProcessor::new();
        processor.reset();
    }

    #[test]
    fn test_default() {
        let processor = SimdYamlBatchProcessor::default();
        let _ = processor.parser();
    }

    #[test]
    fn test_tape_based_processing() {
        use crate::format_dson::TapeBatchProcessor;

        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"name: Alice\nage: 30\nactive: true";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        assert_eq!(result.segment_count(), 1);
        assert!(!result.tape.nodes().is_empty());

        // Check that we have key-value pairs in the tape
        let has_keys = result
            .tape
            .nodes()
            .iter()
            .any(|n| matches!(n.node_type, ExtendedNodeType::Key(_)));
        assert!(has_keys);
    }

    #[test]
    fn test_tape_yaml_anchor_preservation() {
        use crate::format_dson::TapeBatchProcessor;

        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"base: &base_config\n  timeout: 30\nref: *base_config";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        // Check for anchor node
        let has_anchor = result
            .tape
            .nodes()
            .iter()
            .any(|n| matches!(n.node_type, ExtendedNodeType::YamlAnchor { .. }));
        assert!(has_anchor, "Should have YamlAnchor node");

        // Check for alias node
        let has_alias = result
            .tape
            .nodes()
            .iter()
            .any(|n| matches!(n.node_type, ExtendedNodeType::YamlAlias { .. }));
        assert!(has_alias, "Should have YamlAlias node");

        // Check that original syntax is preserved
        assert!(
            result.tape.original_syntax.len() >= 2,
            "Should have original syntax entries for anchor and alias"
        );
    }

    #[test]
    fn test_tape_yaml_document_markers() {
        use crate::format_dson::TapeBatchProcessor;

        let mut processor = SimdYamlBatchProcessor::new();
        let data = b"key: value";
        let arena = bumpalo::Bump::new();

        let result = processor.process_to_tape_unfiltered(data, &arena).unwrap();

        // Check for document start
        let has_doc_start = result
            .tape
            .nodes()
            .iter()
            .any(|n| matches!(n.node_type, ExtendedNodeType::YamlDocumentStart));
        assert!(has_doc_start, "Should have YamlDocumentStart marker");

        // Check for document end
        let has_doc_end = result
            .tape
            .nodes()
            .iter()
            .any(|n| matches!(n.node_type, ExtendedNodeType::YamlDocumentEnd));
        assert!(has_doc_end, "Should have YamlDocumentEnd marker");
    }
}
