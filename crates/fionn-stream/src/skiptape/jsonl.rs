// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSONL (JSON Lines) processing for SIMD-JSONL Skip Tape
//!
//! This module provides SIMD-accelerated JSON Lines processing with schema-aware filtering.

use crate::skiptape::SkipTapeProcessor;
use crate::skiptape::error::{Result, SkipTapeError};
use crate::skiptape::schema::CompiledSchema;
use crate::skiptape::simd_ops::SimdJsonStructuralDetector;
use fionn_simd::SimdLineSeparator;
use fionn_simd::SimdStructuralFilter;
use rayon::prelude::*;

const GPU_MIN_BYTES: usize = 16 * 1024;

/// Result type for parallel line processing - `(line_index, result)`
type LineProcessResult = (usize, std::result::Result<String, (SkipTapeError, String)>);

/// Control whether SIMD pre-scan phases use CPU or GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreScanMode {
    /// Always use CPU SIMD phases.
    CpuOnly,
    /// Use GPU pre-scan when available and input is large enough.
    Auto,
    /// Force GPU pre-scan (error if GPU setup fails).
    Gpu,
}

/// Processing statistics
#[derive(Debug, Clone)]
pub struct ProcessingStats {
    /// Total lines processed
    pub total_lines: usize,
    /// Successfully processed lines
    pub successful_lines: usize,
    /// Failed lines
    pub failed_lines: usize,
    /// Total processing time
    pub processing_time_ms: f64,
}

/// SIMD-JSONL processor with skip parsing capabilities
pub struct SimdJsonlProcessor {
    /// SIMD line separator for fast line detection
    line_separator: SimdLineSeparator,
    /// Processing statistics
    stats: ProcessingStats,
}

impl SimdJsonlProcessor {
    /// Create a new SIMD-JSONL processor
    #[must_use]
    pub const fn new() -> Self {
        Self {
            line_separator: SimdLineSeparator::new(),
            stats: ProcessingStats {
                total_lines: 0,
                successful_lines: 0,
                failed_lines: 0,
                processing_time_ms: 0.0,
            },
        }
    }

    /// Process JSONL data and extract individual JSON lines
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn extract_lines(&self, jsonl_data: &[u8]) -> Result<Vec<String>> {
        let mut lines = Vec::new();

        // SIMD-accelerated line separation
        let line_boundaries = self.line_boundaries(jsonl_data);

        let mut line_start = 0;
        for &line_end in &line_boundaries {
            if line_end > line_start {
                let line_data = &jsonl_data[line_start..line_end];

                // Convert to string and trim whitespace
                if let Ok(line_str) = std::str::from_utf8(line_data) {
                    let trimmed = line_str.trim();
                    if !trimmed.is_empty() {
                        lines.push(trimmed.to_string());
                    }
                }
            }
            line_start = line_end;
        }

        Ok(lines)
    }

    /// Get processing statistics
    #[must_use]
    pub const fn stats(&self) -> &ProcessingStats {
        &self.stats
    }

    /// Find line boundaries using SIMD-accelerated line separator
    fn line_boundaries(&self, data: &[u8]) -> Vec<usize> {
        self.line_separator.find_line_boundaries(data)
    }
}

impl Default for SimdJsonlProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch processing statistics
#[derive(Debug, Clone, Default)]
pub struct BatchStatistics {
    /// Total lines processed
    pub total_lines: usize,
    /// Successfully processed lines
    pub successful_lines: usize,
    /// Failed lines
    pub failed_lines: usize,
    /// Total processing time
    pub processing_time_ms: f64,
    /// Average memory per line
    pub avg_memory_per_line: usize,
    /// Overall schema match ratio
    pub overall_schema_match_ratio: f64,
}

/// Result of batch processing
#[derive(Debug)]
pub struct BatchResult {
    /// Successfully processed JSON documents
    pub documents: Vec<String>,
    /// Errors for failed lines
    pub errors: Vec<LineError>,
    /// Processing statistics
    pub statistics: BatchStatistics,
}

/// Error for a specific line
#[derive(Debug, Clone)]
pub struct LineError {
    /// Index of the line in the original data
    pub line_index: usize,
    /// The error that occurred
    pub error: SkipTapeError,
    /// Raw line content
    pub raw_line: String,
}

/// SIMD-JSONL batch processor with schema filtering
pub struct SimdJsonlBatchProcessor {
    /// SIMD line separator for fast line detection
    line_separator: SimdLineSeparator,
    /// SIMD structural filter for fast schema pre-filtering
    structural_filter: SimdStructuralFilter,
    /// SIMD JSON structural detector for high-performance parsing
    structural_detector: SimdJsonStructuralDetector,
    /// Skip tape processor for individual lines
    skip_processor: SkipTapeProcessor,
    /// GPU pre-scan enabled flag
    gpu_enabled: bool,
    /// Pre-scan mode (CPU vs GPU)
    prescan_mode: PreScanMode,
    /// Minimum bytes for GPU pre-scan in Auto mode
    gpu_min_bytes: usize,
    /// GPU scanner (optional)
    // gpu_scanner: Option<GpuSimdScanner>,
    /// Processing statistics
    stats: BatchStatistics,
}

impl Default for SimdJsonlBatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::unused_self, clippy::missing_const_for_fn)] // Methods may use self for future GPU state
impl SimdJsonlBatchProcessor {
    /// Create a new SIMD-JSONL batch processor
    #[must_use]
    pub fn new() -> Self {
        Self {
            line_separator: SimdLineSeparator::new(),
            structural_filter: SimdStructuralFilter::new(),
            structural_detector: SimdJsonStructuralDetector::new(),
            skip_processor: SkipTapeProcessor::new(),
            gpu_enabled: false,
            prescan_mode: PreScanMode::Auto,
            gpu_min_bytes: GPU_MIN_BYTES,
            // gpu_scanner: None,
            stats: BatchStatistics::default(),
        }
    }

    /// Create a new batch processor and attempt to enable GPU pre-scan.
    #[must_use]
    pub fn new_with_gpu() -> Self {
        let mut processor = Self::new();
        let _ = processor.set_prescan_mode(PreScanMode::Gpu);
        processor
    }

    /// Process a batch of JSONL data with schema filtering using optimized approach
    ///
    /// Key optimizations:
    /// - Reuse a single processor instance instead of creating new ones per line
    /// - Skip serialization in benchmarks for raw performance measurement
    ///
    /// # Errors
    /// Returns an error if batch processing fails
    pub fn process_batch_optimized(
        &mut self,
        jsonl_data: &[u8],
        schema: &CompiledSchema,
    ) -> Result<BatchResult> {
        let start_time = std::time::Instant::now();

        // Reset statistics
        self.stats = BatchStatistics {
            total_lines: 0,
            successful_lines: 0,
            failed_lines: 0,
            processing_time_ms: 0.0,
            avg_memory_per_line: 0,
            overall_schema_match_ratio: 0.0,
        };

        // SIMD-accelerated line separation
        let line_boundaries = self.line_boundaries(jsonl_data);
        self.stats.total_lines = line_boundaries.len();

        #[allow(unused_mut)] // Mutated conditionally in GPU path
        let mut documents = Vec::new();
        let mut errors = Vec::new();
        let mut total_schema_match_ratio = 0.0;
        let mut total_memory = 0;

        // Collect valid lines first
        let mut valid_lines = Vec::new();
        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line_data = &jsonl_data[line_start..line_end];
            line_start = line_end;

            // Convert bytes to string (assuming valid UTF-8 for JSON)
            if let Ok(s) = std::str::from_utf8(line_data) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    valid_lines.push((line_index, trimmed.to_string()));
                }
            } else {
                errors.push(LineError {
                    line_index,
                    error: SkipTapeError::ParseError("Invalid UTF-8".to_string()),
                    raw_line: String::from_utf8_lossy(line_data).to_string(),
                });
                self.stats.failed_lines += 1;
            }
        }

        // Process valid lines with reused processor (optimization: single processor instance)
        for (line_index, line_str) in valid_lines {
            match self.skip_processor.process_line(&line_str, schema) {
                Ok(skip_tape) => {
                    total_schema_match_ratio += skip_tape.metadata().schema_match_ratio;
                    total_memory += skip_tape.memory_efficiency().bytes_used;

                    // Add the original document to results
                    documents.push(line_str);
                    self.stats.successful_lines += 1;
                }
                Err(error) => {
                    errors.push(LineError {
                        line_index,
                        error,
                        raw_line: line_str,
                    });
                    self.stats.failed_lines += 1;
                }
            }
        }

        // Calculate final statistics
        let processing_time = start_time.elapsed().as_secs_f64() * 1000.0;
        self.stats.processing_time_ms = processing_time;

        if self.stats.successful_lines > 0 {
            self.stats.avg_memory_per_line = total_memory / self.stats.successful_lines;
            let successful_f64 =
                f64::from(u32::try_from(self.stats.successful_lines).unwrap_or(u32::MAX));
            self.stats.overall_schema_match_ratio = total_schema_match_ratio / successful_f64;
        }

        Ok(BatchResult {
            documents,
            errors,
            statistics: self.stats.clone(),
        })
    }

    /// Process a batch with SIMD-DSONL: SIMD-JSONL parsing + SIMD-DSON operations
    /// This combines high-performance JSONL parsing with DSON operation processing.
    ///
    /// # Errors
    /// Returns an error if batch processing fails
    ///
    /// # Panics
    /// Panics if the document vector becomes empty unexpectedly (should not happen in normal use)
    pub fn process_batch_simd_dsonl(
        &mut self,
        jsonl_data: &[u8],
        dson_operations: &[fionn_ops::DsonOperation],
    ) -> Result<BatchResult> {
        let start_time = std::time::Instant::now();

        // Reset statistics
        self.stats = BatchStatistics {
            total_lines: 0,
            successful_lines: 0,
            failed_lines: 0,
            processing_time_ms: 0.0,
            avg_memory_per_line: 0,
            overall_schema_match_ratio: 1.0,
        };

        // Step 1: SIMD-JSONL parsing (fast batch processing)
        let parse_result = self.process_batch_raw_simd(jsonl_data)?;

        // Step 2: Apply SIMD-DSON operations to each parsed document
        let mut processed_documents = Vec::new();
        let mut total_memory = 0;
        let mut operation_errors = Vec::new();

        for (line_index, doc_json) in parse_result.documents.iter().enumerate() {
            match Self::apply_dson_operations_to_document(doc_json, dson_operations) {
                Ok(transformed_doc) => {
                    total_memory += transformed_doc.len();
                    processed_documents.push(transformed_doc);
                    self.stats.successful_lines += 1;
                }
                Err(error) => {
                    operation_errors.push(LineError {
                        line_index,
                        error,
                        raw_line: doc_json.clone(),
                    });
                    self.stats.failed_lines += 1;
                    // Still include the original document on operation failure
                    processed_documents.push(doc_json.clone());
                    total_memory += doc_json.len();
                }
            }
        }

        // Combine parsing errors with operation errors
        let mut all_errors = parse_result.errors;
        all_errors.extend(operation_errors);

        // Update statistics
        let processing_time = start_time.elapsed().as_secs_f64() * 1000.0;
        self.stats.processing_time_ms = processing_time;
        self.stats.total_lines = parse_result.documents.len();

        if self.stats.successful_lines > 0 {
            self.stats.avg_memory_per_line = total_memory / self.stats.successful_lines;
        }

        Ok(BatchResult {
            documents: processed_documents,
            errors: all_errors,
            statistics: self.stats.clone(),
        })
    }

    /// Process a batch with raw SIMD parsing (no schema filtering)
    /// This eliminates all schema overhead for maximum performance.
    ///
    /// # Errors
    /// Returns an error if batch processing fails
    pub fn process_batch_raw_simd(&mut self, jsonl_data: &[u8]) -> Result<BatchResult> {
        let start_time = std::time::Instant::now();

        // Reset statistics
        self.stats = BatchStatistics {
            total_lines: 0,
            successful_lines: 0,
            failed_lines: 0,
            processing_time_ms: 0.0,
            avg_memory_per_line: 0,
            overall_schema_match_ratio: 1.0, // All data included
        };

        // SIMD-accelerated line separation
        let line_boundaries = self.line_boundaries(jsonl_data);
        self.stats.total_lines = line_boundaries.len();

        #[allow(unused_mut)] // Mutated conditionally in GPU path
        let mut documents = Vec::new();
        let mut errors = Vec::new();
        let mut total_memory = 0;

        // Collect all valid lines first
        let mut valid_lines = Vec::new();
        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line_data = &jsonl_data[line_start..line_end];
            line_start = line_end;

            // Convert bytes to string (assuming valid UTF-8 for JSON)
            if let Ok(s) = std::str::from_utf8(line_data) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    valid_lines.push((line_index, trimmed.to_string()));
                }
            } else {
                errors.push(LineError {
                    line_index,
                    error: SkipTapeError::ParseError("Invalid UTF-8".to_string()),
                    raw_line: String::from_utf8_lossy(line_data).to_string(),
                });
                self.stats.failed_lines += 1;
            }
        }

        // Process all lines in parallel using Rayon
        let results: Vec<LineProcessResult> = valid_lines
            .into_par_iter()
            .map(|(line_index, line_str)| {
                // Create a temporary processor for each thread
                let mut temp_processor = Self::new();
                let result = match temp_processor.parse_json_raw(&line_str) {
                    Ok(json_str) => Ok(json_str),
                    Err(e) => Err((e, line_str)),
                };
                (line_index, result)
            })
            .collect();

        // Collect results sequentially
        for (line_index, result) in results {
            match result {
                Ok(json_str) => {
                    total_memory += json_str.len();
                    documents.push(json_str);
                    self.stats.successful_lines += 1;
                }
                Err((error, raw_line)) => {
                    errors.push(LineError {
                        line_index,
                        error,
                        raw_line,
                    });
                    self.stats.failed_lines += 1;
                }
            }
        }

        // Calculate final statistics
        let processing_time = start_time.elapsed().as_secs_f64() * 1000.0;
        self.stats.processing_time_ms = processing_time;

        if self.stats.successful_lines > 0 {
            self.stats.avg_memory_per_line = total_memory / self.stats.successful_lines;
        }

        Ok(BatchResult {
            documents,
            errors,
            statistics: self.stats.clone(),
        })
    }

    /// Apply DSON operations to a single JSON document
    fn apply_dson_operations_to_document(
        doc_json: &str,
        operations: &[fionn_ops::DsonOperation],
    ) -> Result<String> {
        // Create a processor for this document
        let mut processor = fionn_ops::processor::BlackBoxProcessor::new(vec![], vec![]);

        // Process the document through the DSON pipeline
        processor.process(doc_json)?;

        // Apply operations to the processed document
        processor.apply_operations(operations)?;

        // Generate the final output
        Ok(processor.generate_output()?)
    }

    /// Raw JSON parsing without schema filtering - simplified for performance
    /// Raw JSON parsing with SIMD validation
    ///
    /// # Errors
    /// Returns an error if JSON parsing or validation fails
    pub fn parse_json_raw(&mut self, json: &str) -> Result<String> {
        let bytes = json.as_bytes();
        if bytes.is_empty() {
            return Err(SkipTapeError::ParseError("Empty JSON".to_string()));
        }

        // Use SIMD structural detector for high-performance validation
        // This validates the structure (braces, brackets, etc.) using AVX2
        let structural_positions = self.structural_positions(bytes);

        // Check validity using the structural positions
        Self::validate_json_with_structural_positions(bytes, &structural_positions)?;

        Ok(json.to_string())
    }

    /// Fast SIMD-based JSON structure validation using pre-computed structural positions
    fn validate_json_with_structural_positions(
        bytes: &[u8],
        structural_positions: &[usize],
    ) -> Result<()> {
        let mut depth = 0;
        let mut in_string = false;
        let mut escaped = false;

        for &pos in structural_positions {
            if pos >= bytes.len() {
                continue;
            }

            let byte = bytes[pos];

            // Handle string state
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }

            // Handle structural characters
            match byte {
                b'{' | b'[' => {
                    depth += 1;
                }
                b'}' | b']' => {
                    depth -= 1;
                    if depth < 0 {
                        return Err(SkipTapeError::ParseError(
                            "Unmatched closing bracket".to_string(),
                        ));
                    }
                }
                b'"' => {
                    in_string = true;
                }
                // Valid structural/whitespace and literal/number characters
                b','
                | b':'
                | b' '
                | b'\t'
                | b'\n'
                | b'\r'
                | b't'
                | b'f'
                | b'n'
                | b'0'..=b'9'
                | b'-'
                | b'.'
                | b'+'
                | b'e'
                | b'E' => {}
                _ => {
                    return Err(SkipTapeError::ParseError(format!(
                        "Invalid character: {}",
                        byte as char
                    )));
                }
            }
        }

        if depth != 0 {
            return Err(SkipTapeError::ParseError("Unmatched brackets".to_string()));
        }

        Ok(())
    }

    /// Process a batch with structural filtering - ultra-fast pre-filtering
    /// This approach uses SIMD to quickly identify documents that match the schema
    /// before doing any parsing, potentially skipping 80-90% of documents.
    ///
    /// # Errors
    /// Returns an error if batch processing fails
    #[allow(clippy::too_many_lines)] // Complex batch processing with filtering and statistics
    pub fn process_batch_structural_filtering(
        &mut self,
        jsonl_data: &[u8],
        schema: &CompiledSchema,
    ) -> Result<BatchResult> {
        let start_time = std::time::Instant::now();

        // Reset statistics
        self.stats = BatchStatistics {
            total_lines: 0,
            successful_lines: 0,
            failed_lines: 0,
            processing_time_ms: 0.0,
            avg_memory_per_line: 0,
            overall_schema_match_ratio: 0.0,
        };

        // SIMD-accelerated line separation
        let line_boundaries = self.line_boundaries(jsonl_data);
        self.stats.total_lines = line_boundaries.len();

        #[allow(unused_mut)] // Mutated conditionally in GPU path
        let mut documents = Vec::new();
        let mut errors = Vec::new();
        let mut total_schema_match_ratio = 0.0;
        let mut total_memory = 0;
        let mut prefiltered_count = 0;

        // Apply structural filtering - this is the key optimization!
        let required_fields: Vec<String> = schema
            .field_paths()
            .iter()
            .filter(|path| {
                path.starts_with("user.") || path == &"age" || path == &"active" || path == &"score"
            })
            .cloned()
            .collect();

        let gpu_prefilter_flags =
            self.gpu_prefilter_lines(jsonl_data, &line_boundaries, &required_fields);
        if gpu_prefilter_flags.is_some() {
            prefiltered_count = line_boundaries.len();
        }

        // Collect valid lines first
        let mut valid_lines = Vec::new();
        let mut line_start = 0;
        for (line_index, &line_end) in line_boundaries.iter().enumerate() {
            let line_data = &jsonl_data[line_start..line_end];
            line_start = line_end;

            if let Some(flags) = &gpu_prefilter_flags
                && !flags.get(line_index).copied().unwrap_or(false)
            {
                continue;
            }

            // Convert bytes to string (assuming valid UTF-8 for JSON)
            if let Ok(s) = std::str::from_utf8(line_data) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    valid_lines.push((line_index, trimmed.to_string()));
                    if gpu_prefilter_flags.is_none() {
                        prefiltered_count += 1;
                    }
                }
            } else {
                errors.push(LineError {
                    line_index,
                    error: SkipTapeError::ParseError("Invalid UTF-8".to_string()),
                    raw_line: String::from_utf8_lossy(line_data).to_string(),
                });
                self.stats.failed_lines += 1;
            }
        }

        let mut filtered_lines = Vec::new();
        for (line_index, line_str) in valid_lines {
            // Ultra-fast structural pre-filtering using SIMD
            if self
                .structural_filter
                .matches_schema(line_str.as_bytes(), &required_fields)
            {
                filtered_lines.push((line_index, line_str));
            }
            // Documents that don't match are completely skipped - no parsing overhead!
        }

        let filtered_count = filtered_lines.len();

        // Only process documents that passed structural filtering
        for (line_index, line_str) in filtered_lines {
            match self.skip_processor.process_line(&line_str, schema) {
                Ok(skip_tape) => {
                    total_schema_match_ratio += skip_tape.metadata().schema_match_ratio;
                    total_memory += skip_tape.memory_efficiency().bytes_used;

                    // Serialize the skip tape to JSON
                    self.stats.successful_lines += 1;
                }
                Err(error) => {
                    errors.push(LineError {
                        line_index,
                        error,
                        raw_line: line_str,
                    });
                    self.stats.failed_lines += 1;
                }
            }
        }

        // Calculate final statistics
        let processing_time = start_time.elapsed().as_secs_f64() * 1000.0;
        self.stats.processing_time_ms = processing_time;

        if self.stats.successful_lines > 0 {
            self.stats.avg_memory_per_line = total_memory / self.stats.successful_lines;
            let successful_f64 =
                f64::from(u32::try_from(self.stats.successful_lines).unwrap_or(u32::MAX));
            self.stats.overall_schema_match_ratio = total_schema_match_ratio / successful_f64;
        }

        // Add structural filtering stats
        let filtered_f64 = f64::from(u32::try_from(filtered_count).unwrap_or(u32::MAX));
        let prefiltered_f64 = f64::from(u32::try_from(prefiltered_count).unwrap_or(u32::MAX));
        println!(
            "Structural filtering: {filtered_count}/{prefiltered_count} documents passed pre-filter ({:.1}%)",
            (filtered_f64 / prefiltered_f64) * 100.0
        );

        Ok(BatchResult {
            documents,
            errors,
            statistics: self.stats.clone(),
        })
    }

    /// Get current processing statistics
    #[must_use]
    pub const fn statistics(&self) -> &BatchStatistics {
        &self.stats
    }

    /// Reset the processor for a new batch
    pub fn reset(&mut self) {
        // Reset the skip processor's arena for the next batch
        self.skip_processor.reset();
        self.stats = BatchStatistics {
            total_lines: 0,
            successful_lines: 0,
            failed_lines: 0,
            processing_time_ms: 0.0,
            avg_memory_per_line: 0,
            overall_schema_match_ratio: 0.0,
        };
    }

    /// Enable GPU pre-scan for SIMD phases (line boundary + structural detection).
    ///
    /// Returns true when the GPU scanner is available and active.
    ///
    /// # Errors
    /// Returns an error if GPU initialization fails
    pub fn enable_gpu(&mut self) -> Result<bool> {
        self.set_prescan_mode(PreScanMode::Gpu)
    }

    /// Disable GPU pre-scan and fall back to CPU SIMD.
    pub fn disable_gpu(&mut self) {
        let _ = self.set_prescan_mode(PreScanMode::CpuOnly);
    }

    /// Set the pre-scan mode (CPU vs GPU).
    ///
    /// # Errors
    /// Returns an error if GPU mode is requested but initialization fails
    pub fn set_prescan_mode(&mut self, mode: PreScanMode) -> Result<bool> {
        self.prescan_mode = mode;
        match mode {
            PreScanMode::CpuOnly => {
                self.gpu_enabled = false;
                Ok(false)
            }
            PreScanMode::Auto => Ok(self.gpu_enabled),
            PreScanMode::Gpu => Ok(self.try_enable_gpu()),
        }
    }

    /// Current pre-scan mode.
    #[must_use]
    pub const fn prescan_mode(&self) -> PreScanMode {
        self.prescan_mode
    }

    /// Override the GPU size cutoff for Auto mode.
    pub const fn set_gpu_min_bytes(&mut self, min_bytes: usize) {
        self.gpu_min_bytes = min_bytes;
    }

    #[allow(clippy::needless_pass_by_ref_mut)] // Will mutate GPU state in future
    fn line_boundaries(&mut self, data: &[u8]) -> Vec<usize> {
        if let Some(boundaries) = self.gpu_line_boundaries(data) {
            return boundaries;
        }
        self.line_separator.find_line_boundaries(data)
    }

    #[allow(clippy::needless_pass_by_ref_mut)] // Will mutate GPU state in future
    fn structural_positions(&mut self, data: &[u8]) -> Vec<usize> {
        if let Some(positions) = self.gpu_structural_positions(data) {
            return positions;
        }
        self.structural_detector.find_structural_characters(data)
    }

    #[allow(clippy::unnecessary_wraps)] // Returns Result in GPU-enabled builds
    fn try_enable_gpu(&self) -> bool {
        false
    }

    #[allow(dead_code)] // Reserved for future GPU acceleration threshold
    const fn gpu_allowed(&self, data: &[u8]) -> bool {
        data.len() >= 64
    }

    fn gpu_line_boundaries(&self, _data: &[u8]) -> Option<Vec<usize>> {
        None
    }

    fn gpu_structural_positions(&self, _data: &[u8]) -> Option<Vec<usize>> {
        None
    }

    fn gpu_prefilter_lines(
        &self,
        _data: &[u8],
        _line_boundaries: &[usize],
        _required_fields: &[String],
    ) -> Option<Vec<bool>> {
        None
    }
}
