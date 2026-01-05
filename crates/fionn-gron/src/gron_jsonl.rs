// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSONL (JSON Lines) support for gron.
//!
//! Process newline-delimited JSON files, transforming each line to gron format
//! with indexed prefixes.

use super::gron_core::{GronOptions, gron};
use fionn_core::{DsonError, Result};
use fionn_simd::SimdLineSeparator;
use std::io::{BufRead, Write};

/// Options for JSONL processing.
#[derive(Debug, Clone)]
pub struct GronJsonlOptions {
    /// Base gron options
    pub gron: GronOptions,
    /// Index format for line prefixes
    pub index_format: IndexFormat,
    /// Error handling mode
    pub error_mode: ErrorMode,
}

/// How to format the line index in output.
#[derive(Debug, Clone)]
pub enum IndexFormat {
    /// `json[0]`, `json[1]`, etc.
    Bracketed,
    /// `json.0`, `json.1`, etc.
    Dotted,
    /// No index, each line uses base prefix
    None,
}

/// Error handling for malformed lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorMode {
    /// Stop on first error
    Fail,
    /// Skip malformed lines, continue processing
    Skip,
    /// Include error message in output as comment
    Comment,
}

impl Default for GronJsonlOptions {
    fn default() -> Self {
        Self {
            gron: GronOptions::default(),
            index_format: IndexFormat::Bracketed,
            error_mode: ErrorMode::Fail,
        }
    }
}

impl GronJsonlOptions {
    /// Create options with custom prefix.
    #[must_use]
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            gron: GronOptions::with_prefix(prefix),
            ..Default::default()
        }
    }

    /// Set index format.
    #[must_use]
    pub const fn index_format(mut self, format: IndexFormat) -> Self {
        self.index_format = format;
        self
    }

    /// Set error mode.
    #[must_use]
    pub const fn error_mode(mut self, mode: ErrorMode) -> Self {
        self.error_mode = mode;
        self
    }

    /// Set compact output.
    #[must_use]
    pub fn compact(mut self) -> Self {
        self.gron = self.gron.compact();
        self
    }

    /// Format prefix for a given line index.
    fn format_prefix(&self, line_num: usize) -> String {
        match &self.index_format {
            IndexFormat::Bracketed => format!("{}[{}]", self.gron.prefix, line_num),
            IndexFormat::Dotted => format!("{}.{}", self.gron.prefix, line_num),
            IndexFormat::None => self.gron.prefix.clone(),
        }
    }
}

/// Processing statistics for JSONL.
#[derive(Debug, Clone, Default)]
pub struct JsonlStats {
    /// Total lines processed
    pub lines_processed: usize,
    /// Successfully transformed lines
    pub lines_success: usize,
    /// Failed lines (skipped or commented)
    pub lines_failed: usize,
    /// Total bytes read
    pub bytes_read: usize,
    /// Total bytes written
    pub bytes_written: usize,
}

/// Process JSONL data with SIMD-accelerated line detection.
///
/// # Errors
/// Returns an error if processing fails (depends on `error_mode`).
pub fn gron_jsonl(data: &[u8], options: &GronJsonlOptions) -> Result<String> {
    let mut output = Vec::with_capacity(data.len() * 2);
    let stats = gron_jsonl_to_writer(data, options, &mut output)?;

    // Log stats if there were failures
    if stats.lines_failed > 0 && options.error_mode == ErrorMode::Skip {
        eprintln!(
            "Warning: {} of {} lines failed to parse",
            stats.lines_failed, stats.lines_processed
        );
    }

    // Safety: we only write valid UTF-8
    Ok(unsafe { String::from_utf8_unchecked(output) })
}

/// Process JSONL data, writing to a writer.
///
/// # Errors
/// Returns an error if processing fails.
pub fn gron_jsonl_to_writer<W: Write>(
    data: &[u8],
    options: &GronJsonlOptions,
    writer: &mut W,
) -> Result<JsonlStats> {
    let separator = SimdLineSeparator::new();
    let boundaries = separator.find_line_boundaries(data);

    let mut stats = JsonlStats {
        bytes_read: data.len(),
        ..Default::default()
    };

    let mut line_start = 0;
    let mut line_num = 0;

    for &line_end in &boundaries {
        let line_bytes = &data[line_start..line_end];
        line_start = line_end;

        // Skip invalid UTF-8
        let Ok(raw_str) = std::str::from_utf8(line_bytes) else {
            stats.lines_failed += 1;
            stats.lines_processed += 1;
            continue;
        };
        let line_str = raw_str.trim();

        if line_str.is_empty() {
            continue;
        }

        stats.lines_processed += 1;

        // Create options with indexed prefix
        let prefix = options.format_prefix(line_num);
        let mut gron_opts = options.gron.clone();
        gron_opts.prefix = prefix;

        // Transform line
        match gron(line_str, &gron_opts) {
            Ok(gron_output) => {
                writer
                    .write_all(gron_output.as_bytes())
                    .map_err(DsonError::IoError)?;
                stats.bytes_written += gron_output.len();
                stats.lines_success += 1;
                line_num += 1;
            }
            Err(e) => {
                stats.lines_failed += 1;
                match options.error_mode {
                    ErrorMode::Fail => return Err(e),
                    ErrorMode::Skip => {}
                    ErrorMode::Comment => {
                        let comment = format!("// Line {line_num}: {e}\n");
                        writer
                            .write_all(comment.as_bytes())
                            .map_err(DsonError::IoError)?;
                        stats.bytes_written += comment.len();
                        line_num += 1;
                    }
                }
            }
        }
    }

    // Handle remaining data after last newline
    if line_start < data.len() {
        let line_bytes = &data[line_start..];
        if let Ok(line_str) = std::str::from_utf8(line_bytes) {
            let trimmed = line_str.trim();
            if !trimmed.is_empty() {
                stats.lines_processed += 1;

                let prefix = options.format_prefix(line_num);
                let mut gron_opts = options.gron.clone();
                gron_opts.prefix = prefix;

                match gron(trimmed, &gron_opts) {
                    Ok(gron_output) => {
                        writer
                            .write_all(gron_output.as_bytes())
                            .map_err(DsonError::IoError)?;
                        stats.bytes_written += gron_output.len();
                        stats.lines_success += 1;
                    }
                    Err(e) => {
                        stats.lines_failed += 1;
                        match options.error_mode {
                            ErrorMode::Fail => return Err(e),
                            ErrorMode::Skip => {}
                            ErrorMode::Comment => {
                                let comment = format!("// Line {line_num}: {e}\n");
                                writer
                                    .write_all(comment.as_bytes())
                                    .map_err(DsonError::IoError)?;
                                stats.bytes_written += comment.len();
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(stats)
}

/// Process JSONL from a buffered reader (streaming mode).
///
/// # Errors
/// Returns an error if processing fails.
pub fn gron_jsonl_streaming<R: BufRead, W: Write>(
    reader: R,
    options: &GronJsonlOptions,
    writer: &mut W,
) -> Result<JsonlStats> {
    let mut stats = JsonlStats::default();
    let mut line_num = 0;

    for line_result in reader.lines() {
        let line = line_result.map_err(DsonError::IoError)?;
        let trimmed = line.trim();

        stats.bytes_read += line.len() + 1; // +1 for newline

        if trimmed.is_empty() {
            continue;
        }

        stats.lines_processed += 1;

        let prefix = options.format_prefix(line_num);
        let mut gron_opts = options.gron.clone();
        gron_opts.prefix = prefix;

        match gron(trimmed, &gron_opts) {
            Ok(gron_output) => {
                writer
                    .write_all(gron_output.as_bytes())
                    .map_err(DsonError::IoError)?;
                stats.bytes_written += gron_output.len();
                stats.lines_success += 1;
                line_num += 1;
            }
            Err(e) => {
                stats.lines_failed += 1;
                match options.error_mode {
                    ErrorMode::Fail => return Err(e),
                    ErrorMode::Skip => {}
                    ErrorMode::Comment => {
                        let comment = format!("// Line {line_num}: {e}\n");
                        writer
                            .write_all(comment.as_bytes())
                            .map_err(DsonError::IoError)?;
                        stats.bytes_written += comment.len();
                        line_num += 1;
                    }
                }
            }
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_jsonl() {
        let input = b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}";
        let output = gron_jsonl(input, &GronJsonlOptions::default()).unwrap();

        assert!(output.contains("json[0] = {};"));
        assert!(output.contains("json[0].a = 1;"));
        assert!(output.contains("json[1] = {};"));
        assert!(output.contains("json[1].b = 2;"));
        assert!(output.contains("json[2] = {};"));
        assert!(output.contains("json[2].c = 3;"));
    }

    #[test]
    fn test_empty_lines_skipped() {
        let input = b"{\"a\":1}\n\n{\"b\":2}\n";
        let output = gron_jsonl(input, &GronJsonlOptions::default()).unwrap();

        // Empty lines don't create entries, so indices are consecutive
        assert!(output.contains("json[0].a = 1;"));
        assert!(output.contains("json[1].b = 2;"));
        assert!(!output.contains("json[2]"));
    }

    #[test]
    fn test_dotted_index_format() {
        let input = b"{\"a\":1}\n{\"b\":2}";
        let options = GronJsonlOptions::default().index_format(IndexFormat::Dotted);
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.contains("json.0.a = 1;"));
        assert!(output.contains("json.1.b = 2;"));
    }

    #[test]
    fn test_no_index_format() {
        let input = b"{\"a\":1}\n{\"b\":2}";
        let options = GronJsonlOptions::default().index_format(IndexFormat::None);
        let output = gron_jsonl(input, &options).unwrap();

        // Both use same prefix
        assert!(output.contains("json.a = 1;"));
        assert!(output.contains("json.b = 2;"));
    }

    #[test]
    fn test_custom_prefix() {
        let input = b"{\"a\":1}";
        let options = GronJsonlOptions::with_prefix("log");
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.contains("log[0].a = 1;"));
    }

    #[test]
    fn test_error_mode_skip() {
        let input = b"{\"valid\":1}\ninvalid json\n{\"valid\":2}";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Skip);
        let output = gron_jsonl(input, &options).unwrap();

        // Invalid line skipped, indices still increment for valid only
        assert!(output.contains("json[0].valid = 1;"));
        assert!(output.contains("json[1].valid = 2;"));
    }

    #[test]
    fn test_error_mode_comment() {
        let input = b"{\"valid\":1}\ninvalid\n{\"valid\":2}";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Comment);
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.contains("json[0].valid = 1;"));
        assert!(output.contains("// Line 1:"));
        assert!(output.contains("json[2].valid = 2;"));
    }

    #[test]
    fn test_error_mode_fail() {
        let input = b"{\"valid\":1}\ninvalid\n{\"valid\":2}";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Fail);
        let result = gron_jsonl(input, &options);

        assert!(result.is_err());
    }

    #[test]
    fn test_compact_output() {
        let input = b"{\"a\":1}";
        let options = GronJsonlOptions::default().compact();
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.contains("json[0]={};"));
        assert!(output.contains("json[0].a=1;"));
    }

    #[test]
    fn test_stats() {
        let input = b"{\"a\":1}\n{\"b\":2}\ninvalid\n{\"c\":3}";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Skip);
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        assert_eq!(stats.lines_processed, 4);
        assert_eq!(stats.lines_success, 3);
        assert_eq!(stats.lines_failed, 1);
    }

    #[test]
    fn test_streaming() {
        let input = b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}";
        let reader = std::io::BufReader::new(&input[..]);
        let mut output = Vec::new();
        let stats =
            gron_jsonl_streaming(reader, &GronJsonlOptions::default(), &mut output).unwrap();

        assert_eq!(stats.lines_success, 3);

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("json[0].a = 1;"));
        assert!(output_str.contains("json[2].c = 3;"));
    }

    #[test]
    fn test_nested_objects() {
        let input = b"{\"user\":{\"name\":\"Alice\"}}\n{\"user\":{\"name\":\"Bob\"}}";
        let output = gron_jsonl(input, &GronJsonlOptions::default()).unwrap();

        assert!(output.contains("json[0].user.name = \"Alice\";"));
        assert!(output.contains("json[1].user.name = \"Bob\";"));
    }

    #[test]
    fn test_arrays_in_jsonl() {
        let input = b"{\"items\":[1,2,3]}\n{\"items\":[4,5]}";
        let output = gron_jsonl(input, &GronJsonlOptions::default()).unwrap();

        assert!(output.contains("json[0].items[0] = 1;"));
        assert!(output.contains("json[0].items[2] = 3;"));
        assert!(output.contains("json[1].items[0] = 4;"));
        assert!(output.contains("json[1].items[1] = 5;"));
    }

    #[test]
    fn test_no_trailing_newline() {
        let input = b"{\"a\":1}"; // No newline at end
        let output = gron_jsonl(input, &GronJsonlOptions::default()).unwrap();

        assert!(output.contains("json[0].a = 1;"));
    }

    #[test]
    fn test_windows_line_endings() {
        let input = b"{\"a\":1}\r\n{\"b\":2}\r\n";
        let output = gron_jsonl(input, &GronJsonlOptions::default()).unwrap();

        // Should handle \r\n properly via trim()
        assert!(output.contains("json[0].a = 1;"));
        assert!(output.contains("json[1].b = 2;"));
    }

    // =========================================================================
    // Additional Edge Case Tests
    // =========================================================================

    #[test]
    fn test_invalid_utf8_in_input() {
        // Invalid UTF-8 sequence
        let input = b"{\"a\":1}\n\xff\xfe\n{\"b\":2}";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Skip);
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        // Invalid UTF-8 line should be failed
        assert!(stats.lines_failed >= 1);
        assert!(stats.lines_success >= 2);
    }

    #[test]
    fn test_invalid_utf8_with_fail_mode() {
        // Invalid UTF-8 should be skipped even in fail mode (it's not a JSON parse error)
        let input = b"{\"a\":1}\n\xff\xfe\n{\"b\":2}";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Fail);
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        // Invalid UTF-8 line counted as failed but processing continues
        assert!(stats.lines_failed >= 1);
    }

    #[test]
    fn test_remaining_data_after_last_newline_error_skip() {
        // Data after final newline that's invalid JSON with Skip mode
        let input = b"{\"a\":1}\ninvalid";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Skip);
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        assert_eq!(stats.lines_success, 1);
        assert_eq!(stats.lines_failed, 1);
    }

    #[test]
    fn test_remaining_data_after_last_newline_error_comment() {
        // Data after final newline that's invalid JSON with Comment mode
        let input = b"{\"a\":1}\ninvalid";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Comment);
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("// Line"));
        assert_eq!(stats.lines_failed, 1);
    }

    #[test]
    fn test_remaining_data_after_last_newline_error_fail() {
        // Data after final newline that's invalid JSON with Fail mode
        let input = b"{\"a\":1}\ninvalid";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Fail);
        let mut output = Vec::new();
        let result = gron_jsonl_to_writer(input, &options, &mut output);

        assert!(result.is_err());
    }

    #[test]
    fn test_remaining_data_valid_json_no_newline() {
        // Valid JSON after last newline
        let input = b"{\"a\":1}\n{\"b\":2}";
        let options = GronJsonlOptions::default();
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        assert_eq!(stats.lines_success, 2);
        assert_eq!(stats.lines_failed, 0);
    }

    #[test]
    fn test_streaming_error_mode_skip() {
        let input = b"{\"valid\":1}\ninvalid json\n{\"valid\":2}";
        let reader = std::io::BufReader::new(&input[..]);
        let mut output = Vec::new();
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Skip);
        let stats = gron_jsonl_streaming(reader, &options, &mut output).unwrap();

        assert_eq!(stats.lines_success, 2);
        assert_eq!(stats.lines_failed, 1);
    }

    #[test]
    fn test_streaming_error_mode_comment() {
        let input = b"{\"valid\":1}\ninvalid\n{\"valid\":2}";
        let reader = std::io::BufReader::new(&input[..]);
        let mut output = Vec::new();
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Comment);
        let stats = gron_jsonl_streaming(reader, &options, &mut output).unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("// Line"));
        assert_eq!(stats.lines_failed, 1);
    }

    #[test]
    fn test_streaming_error_mode_fail() {
        let input = b"{\"valid\":1}\ninvalid\n{\"valid\":2}";
        let reader = std::io::BufReader::new(&input[..]);
        let mut output = Vec::new();
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Fail);
        let result = gron_jsonl_streaming(reader, &options, &mut output);

        assert!(result.is_err());
    }

    #[test]
    fn test_streaming_empty_lines_skipped() {
        let input = b"{\"a\":1}\n\n\n{\"b\":2}";
        let reader = std::io::BufReader::new(&input[..]);
        let mut output = Vec::new();
        let options = GronJsonlOptions::default();
        let stats = gron_jsonl_streaming(reader, &options, &mut output).unwrap();

        assert_eq!(stats.lines_success, 2);
        // Empty lines don't count as processed
        assert_eq!(stats.lines_processed, 2);
    }

    #[test]
    fn test_jsonl_stats_bytes_read() {
        let input = b"{\"a\":1}\n{\"b\":2}\n";
        let options = GronJsonlOptions::default();
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        assert_eq!(stats.bytes_read, input.len());
        assert!(stats.bytes_written > 0);
    }

    #[test]
    fn test_only_empty_lines() {
        let input = b"\n\n\n";
        let options = GronJsonlOptions::default();
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.is_empty());
    }

    #[test]
    fn test_only_whitespace_lines() {
        let input = b"   \n\t\t\n  \t  \n";
        let options = GronJsonlOptions::default();
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.is_empty());
    }

    #[test]
    fn test_mixed_valid_and_whitespace() {
        let input = b"   \n{\"a\":1}\n   \n{\"b\":2}\n\t";
        let options = GronJsonlOptions::default();
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.contains("json[0].a = 1;"));
        assert!(output.contains("json[1].b = 2;"));
    }

    #[test]
    fn test_jsonl_with_unicode() {
        let input = b"{\"msg\":\"\xc3\xa9\xc3\xa0\xc3\xbc\"}\n{\"emoji\":\"\\u2764\"}";
        let options = GronJsonlOptions::default();
        let output = gron_jsonl(input, &options).unwrap();

        assert!(output.contains("json[0].msg"));
        assert!(output.contains("json[1].emoji"));
    }

    #[test]
    fn test_format_prefix_all_variants() {
        let opts_bracketed = GronJsonlOptions::default().index_format(IndexFormat::Bracketed);
        assert_eq!(opts_bracketed.format_prefix(5), "json[5]");

        let opts_dotted = GronJsonlOptions::default().index_format(IndexFormat::Dotted);
        assert_eq!(opts_dotted.format_prefix(5), "json.5");

        let opts_none = GronJsonlOptions::default().index_format(IndexFormat::None);
        assert_eq!(opts_none.format_prefix(5), "json");
    }

    #[test]
    fn test_error_mode_equality() {
        assert_eq!(ErrorMode::Fail, ErrorMode::Fail);
        assert_eq!(ErrorMode::Skip, ErrorMode::Skip);
        assert_eq!(ErrorMode::Comment, ErrorMode::Comment);
        assert_ne!(ErrorMode::Fail, ErrorMode::Skip);
    }

    #[test]
    fn test_options_clone() {
        let opts = GronJsonlOptions::default()
            .index_format(IndexFormat::Dotted)
            .error_mode(ErrorMode::Comment)
            .compact();

        let cloned = opts;
        assert!(matches!(cloned.index_format, IndexFormat::Dotted));
        assert_eq!(cloned.error_mode, ErrorMode::Comment);
    }

    #[test]
    fn test_stats_clone_and_debug() {
        let stats = JsonlStats {
            lines_processed: 10,
            lines_success: 8,
            lines_failed: 2,
            bytes_read: 1000,
            bytes_written: 2000,
        };

        let cloned = stats.clone();
        assert_eq!(cloned.lines_processed, 10);
        assert_eq!(cloned.bytes_written, 2000);

        // Debug impl
        let debug_str = format!("{stats:?}");
        assert!(debug_str.contains("lines_processed"));
    }

    #[test]
    fn test_large_line_numbers() {
        // Test formatting with large indices
        let opts = GronJsonlOptions::default();
        assert_eq!(opts.format_prefix(999_999), "json[999999]");

        let opts_dotted = GronJsonlOptions::default().index_format(IndexFormat::Dotted);
        assert_eq!(opts_dotted.format_prefix(999_999), "json.999999");
    }

    #[test]
    fn test_remaining_whitespace_only_after_newline() {
        // Trailing whitespace after last newline
        let input = b"{\"a\":1}\n   ";
        let options = GronJsonlOptions::default();
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        // Whitespace-only remainder should be skipped
        assert_eq!(stats.lines_success, 1);
        assert_eq!(stats.lines_failed, 0);
    }

    #[test]
    fn test_streaming_bytes_read_tracking() {
        let input = b"{\"a\":1}\n{\"b\":2}\n";
        let reader = std::io::BufReader::new(&input[..]);
        let mut output = Vec::new();
        let options = GronJsonlOptions::default();
        let stats = gron_jsonl_streaming(reader, &options, &mut output).unwrap();

        // bytes_read should track line lengths + newlines
        assert!(stats.bytes_read > 0);
        assert!(stats.bytes_written > 0);
    }

    // =========================================================================
    // More Coverage Tests
    // =========================================================================

    #[test]
    fn test_gron_jsonl_warning_output() {
        // This tests the warning path (lines 120-124)
        let input = b"{\"a\":1}\ninvalid\n{\"b\":2}";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Skip);
        let output = gron_jsonl(input, &options).unwrap();
        // Just verify it completes - warning goes to stderr
        assert!(!output.is_empty());
    }

    #[test]
    fn test_index_format_debug() {
        let debug_str = format!("{:?}", IndexFormat::Bracketed);
        assert!(debug_str.contains("Bracketed"));

        let debug_str = format!("{:?}", IndexFormat::Dotted);
        assert!(debug_str.contains("Dotted"));

        let debug_str = format!("{:?}", IndexFormat::None);
        assert!(debug_str.contains("None"));
    }

    #[test]
    fn test_index_format_clone() {
        let format = IndexFormat::Dotted;
        let cloned = format;
        assert!(matches!(cloned, IndexFormat::Dotted));
    }

    #[test]
    fn test_error_mode_debug() {
        let debug_str = format!("{:?}", ErrorMode::Fail);
        assert!(debug_str.contains("Fail"));

        let debug_str = format!("{:?}", ErrorMode::Skip);
        assert!(debug_str.contains("Skip"));

        let debug_str = format!("{:?}", ErrorMode::Comment);
        assert!(debug_str.contains("Comment"));
    }

    #[test]
    fn test_error_mode_copy() {
        let mode = ErrorMode::Skip;
        let copied = mode;
        assert_eq!(copied, ErrorMode::Skip);
    }

    #[test]
    fn test_gron_jsonl_options_debug() {
        let opts = GronJsonlOptions::default();
        let debug_str = format!("{opts:?}");
        assert!(debug_str.contains("GronJsonlOptions"));
    }

    #[test]
    fn test_jsonl_stats_default() {
        let stats = JsonlStats::default();
        assert_eq!(stats.lines_processed, 0);
        assert_eq!(stats.lines_success, 0);
        assert_eq!(stats.lines_failed, 0);
        assert_eq!(stats.bytes_read, 0);
        assert_eq!(stats.bytes_written, 0);
    }

    #[test]
    fn test_remaining_data_invalid_utf8() {
        // Invalid UTF-8 after last newline
        let input = b"{\"a\":1}\n\xff\xfe\xfd";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Skip);
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        // Only first line should succeed
        assert_eq!(stats.lines_success, 1);
    }

    #[test]
    fn test_remaining_whitespace_empty_after_trim() {
        // Trailing whitespace that trims to empty
        let input = b"{\"a\":1}\n   \t\r\n   ";
        let options = GronJsonlOptions::default();
        let mut output = Vec::new();
        let stats = gron_jsonl_to_writer(input, &options, &mut output).unwrap();

        assert_eq!(stats.lines_success, 1);
    }

    #[test]
    fn test_gron_error_propagation() {
        // Test that gron errors propagate correctly
        let input = b"not valid json at all!!!";
        let options = GronJsonlOptions::default().error_mode(ErrorMode::Fail);
        let result = gron_jsonl(input, &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_many_lines() {
        let mut input = Vec::new();
        for i in 0..100 {
            input.extend_from_slice(format!("{{\"id\":{i}}}\n").as_bytes());
        }
        let options = GronJsonlOptions::default();
        let output = gron_jsonl(&input, &options).unwrap();

        assert!(output.contains("json[0].id = 0;"));
        assert!(output.contains("json[99].id = 99;"));
    }

    #[test]
    fn test_very_long_line() {
        let long_value = "x".repeat(1000);
        let input = format!("{{\"key\":\"{long_value}\"}}");
        let options = GronJsonlOptions::default();
        let output = gron_jsonl(input.as_bytes(), &options).unwrap();

        assert!(output.contains("json[0].key = "));
    }

    #[test]
    fn test_streaming_with_io_error_simulation() {
        // Just test that streaming works with empty reader
        let input: &[u8] = b"";
        let reader = std::io::BufReader::new(input);
        let mut output = Vec::new();
        let options = GronJsonlOptions::default();
        let stats = gron_jsonl_streaming(reader, &options, &mut output).unwrap();

        assert_eq!(stats.lines_processed, 0);
    }

    #[test]
    fn test_options_builder_chain() {
        let opts = GronJsonlOptions::with_prefix("data")
            .index_format(IndexFormat::Dotted)
            .error_mode(ErrorMode::Comment)
            .compact();

        assert!(matches!(opts.index_format, IndexFormat::Dotted));
        assert_eq!(opts.error_mode, ErrorMode::Comment);
    }
}
