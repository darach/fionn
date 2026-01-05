// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming data pipeline processing for large datasets
//!
//! This module provides streaming capabilities for processing large JSON datasets
//! without loading everything into memory at once.

use super::BlackBoxProcessor;
use crate::{DsonOperation, FilterPredicate, OperationValue, StreamGenerator, TransformFunction};
use fionn_core::Result;
use std::collections::VecDeque;

/// Streaming processor for large dataset processing
pub struct StreamingProcessor {
    processor: BlackBoxProcessor,
    buffer_size: usize,
    current_batch: Vec<OperationValue>,
    processed_batches: VecDeque<Vec<OperationValue>>,
}

impl StreamingProcessor {
    /// Create a new streaming processor
    #[must_use]
    pub fn new(buffer_size: usize) -> Self {
        Self {
            processor: BlackBoxProcessor::new(vec![], vec![]),
            buffer_size,
            current_batch: Vec::new(),
            processed_batches: VecDeque::new(),
        }
    }

    /// Process a streaming operation pipeline
    ///
    /// # Errors
    /// Returns an error if processing fails
    pub fn process_stream(&mut self, operations: &[DsonOperation]) -> Result<()> {
        for operation in operations {
            self.apply_streaming_operation(operation)?;
        }
        Ok(())
    }

    /// Apply a streaming operation
    ///
    /// # Errors
    /// Returns an error if the operation fails
    fn apply_streaming_operation(&mut self, operation: &DsonOperation) -> Result<()> {
        match operation {
            DsonOperation::StreamBuild { path, generator } => self.stream_build(path, generator),
            DsonOperation::StreamFilter { path, predicate } => {
                self.stream_filter(path, predicate);
                Ok(())
            }
            DsonOperation::StreamMap { path, transform } => {
                self.stream_map(path, transform);
                Ok(())
            }
            DsonOperation::StreamEmit { path, batch_size } => self.stream_emit(path, *batch_size),
            // Pass through other operations to the underlying processor
            _ => self.processor.apply_operation(operation),
        }
    }

    /// Stream build operation - generate data streams
    fn stream_build(&mut self, path: &str, generator: &StreamGenerator) -> Result<()> {
        match generator {
            StreamGenerator::Range { start, end, step } => {
                self.generate_range(*start, *end, *step);
            }
            StreamGenerator::Repeat(value, count) => {
                self.generate_repeat(value, *count);
            }
            StreamGenerator::Fibonacci(count) => {
                self.generate_fibonacci(*count);
            }
            StreamGenerator::Custom(_) => {
                // Custom generators would be implemented here
                // For now, just track the operation
                self.processor
                    .apply_operation(&DsonOperation::StreamBuild {
                        path: path.to_string(),
                        generator: generator.clone(),
                    })?;
            }
        }
        Ok(())
    }

    /// Generate a numeric range
    fn generate_range(&mut self, start: i64, end: i64, step: i64) {
        let mut current = start;
        while current < end {
            self.current_batch
                .push(OperationValue::NumberRef(current.to_string()));
            current += step;
        }
        // Don't flush - let streaming operations work on current_batch
    }

    /// Generate repeated values
    fn generate_repeat(&mut self, value: &OperationValue, count: usize) {
        for _ in 0..count {
            self.current_batch.push(value.clone());
            if self.current_batch.len() >= self.buffer_size {
                self.flush_batch();
            }
        }
        self.flush_batch();
    }

    /// Generate fibonacci sequence
    fn generate_fibonacci(&mut self, count: usize) {
        let mut a = 0i64;
        let mut b = 1i64;

        for _ in 0..count {
            self.current_batch
                .push(OperationValue::NumberRef(a.to_string()));
            if self.current_batch.len() >= self.buffer_size {
                self.flush_batch();
            }

            let temp = a;
            a = b;
            b += temp;
        }
        self.flush_batch();
    }

    /// Stream filter operation
    fn stream_filter(&mut self, _path: &str, predicate: &FilterPredicate) {
        // Process current batch with filtering
        let mut filtered = Vec::new();

        for (index, value) in self.current_batch.iter().enumerate() {
            if Self::matches_predicate(value, predicate, index) {
                filtered.push(value.clone());
            }
        }

        self.current_batch = filtered;
    }

    /// Stream map operation
    fn stream_map(&mut self, _path: &str, transform: &TransformFunction) {
        // Process current batch with transformation
        let mut transformed = Vec::new();

        for value in &self.current_batch {
            let new_value = Self::apply_transform(value, transform);
            transformed.push(new_value);
        }

        self.current_batch = transformed;
    }

    /// Stream emit operation - output processed batches
    fn stream_emit(&mut self, path: &str, _batch_size: usize) -> Result<()> {
        // Ensure we have data to emit
        if self.current_batch.is_empty() && self.processed_batches.is_empty() {
            return Ok(());
        }

        // Flush current batch if needed
        if !self.current_batch.is_empty() {
            self.flush_batch();
        }

        // Emit batches
        while let Some(batch) = self.processed_batches.pop_front() {
            // In a real implementation, this would send the batch to output
            // For now, we track it in the processor
            let emit_path = format!("{}.batch_{}", path, self.processed_batches.len());
            let batch_value = OperationValue::StringRef(format!("batch_size:{}", batch.len()));
            self.processor.apply_operation(&DsonOperation::FieldAdd {
                path: emit_path,
                value: batch_value,
            })?;
        }

        Ok(())
    }

    /// Check if a value matches a filter predicate
    fn matches_predicate(
        value: &OperationValue,
        predicate: &FilterPredicate,
        index: usize,
    ) -> bool {
        match predicate {
            FilterPredicate::Even => {
                if let OperationValue::NumberRef(num_str) = value {
                    num_str.parse::<i64>().is_ok_and(|num| num % 2 == 0)
                } else {
                    false
                }
            }
            FilterPredicate::Odd => {
                if let OperationValue::NumberRef(num_str) = value {
                    num_str.parse::<i64>().is_ok_and(|num| num % 2 == 1)
                } else {
                    false
                }
            }
            FilterPredicate::EveryNth(n) => index.is_multiple_of(*n),
            FilterPredicate::GreaterThan(threshold) => {
                if let OperationValue::NumberRef(num_str) = value {
                    num_str.parse::<i64>().is_ok_and(|num| num > *threshold)
                } else {
                    false
                }
            }
            FilterPredicate::LessThan(threshold) => {
                if let OperationValue::NumberRef(num_str) = value {
                    num_str.parse::<i64>().is_ok_and(|num| num < *threshold)
                } else {
                    false
                }
            }
            FilterPredicate::Equals(compare_value) => {
                match (value, compare_value) {
                    (OperationValue::NumberRef(num_str), OperationValue::NumberRef(cmp_str)) => {
                        // Compare as numbers
                        if let (Ok(a), Ok(b)) = (num_str.parse::<i64>(), cmp_str.parse::<i64>()) {
                            a == b
                        } else if let (Ok(a), Ok(b)) =
                            (num_str.parse::<f64>(), cmp_str.parse::<f64>())
                        {
                            (a - b).abs() < f64::EPSILON
                        } else {
                            num_str == cmp_str
                        }
                    }
                    (OperationValue::StringRef(s1), OperationValue::StringRef(s2)) => s1 == s2,
                    (OperationValue::BoolRef(b1), OperationValue::BoolRef(b2)) => b1 == b2,
                    (OperationValue::Null, OperationValue::Null) => true,
                    _ => false,
                }
            }
            FilterPredicate::Alternate => {
                // Alternate - select every other element (even indices)
                index.is_multiple_of(2)
            }
            FilterPredicate::Custom(predicate_fn) => {
                // Custom predicate - execute the stored predicate string as a simple expression
                // For now, support basic patterns like "value > N" or "value == N"
                Self::evaluate_custom_predicate(predicate_fn, value)
            }
        }
    }

    /// Evaluate a custom predicate expression
    fn evaluate_custom_predicate(predicate: &str, value: &OperationValue) -> bool {
        let predicate = predicate.trim();

        // Parse simple expressions like "value > 10", "value == 5", "value < 100"
        if let Some(rest) = predicate.strip_prefix("value") {
            let rest = rest.trim();

            // Parse operator and threshold
            if let Some(threshold_str) = rest.strip_prefix(">") {
                if let Ok(threshold) = threshold_str.trim().parse::<i64>()
                    && let OperationValue::NumberRef(num_str) = value
                    && let Ok(num) = num_str.parse::<i64>()
                {
                    return num > threshold;
                }
            } else if let Some(threshold_str) = rest.strip_prefix("<") {
                if let Ok(threshold) = threshold_str.trim().parse::<i64>()
                    && let OperationValue::NumberRef(num_str) = value
                    && let Ok(num) = num_str.parse::<i64>()
                {
                    return num < threshold;
                }
            } else if let Some(threshold_str) = rest.strip_prefix("==") {
                let threshold_str = threshold_str.trim();
                if let OperationValue::NumberRef(num_str) = value {
                    return num_str == threshold_str;
                } else if let OperationValue::StringRef(s) = value {
                    return s == threshold_str.trim_matches('"');
                }
            } else if let Some(threshold_str) = rest.strip_prefix("!=") {
                let threshold_str = threshold_str.trim();
                if let OperationValue::NumberRef(num_str) = value {
                    return num_str != threshold_str;
                } else if let OperationValue::StringRef(s) = value {
                    return s != threshold_str.trim_matches('"');
                }
            }
        }

        // Default to true if predicate couldn't be parsed
        true
    }

    /// Apply a transformation to a value
    fn apply_transform(value: &OperationValue, transform: &TransformFunction) -> OperationValue {
        match (value, transform) {
            (OperationValue::NumberRef(num_str), TransformFunction::Add(delta)) => {
                num_str.parse::<i64>().map_or_else(
                    |_| value.clone(),
                    |num| OperationValue::NumberRef((num + delta).to_string()),
                )
            }
            (OperationValue::NumberRef(num_str), TransformFunction::Multiply(factor)) => {
                num_str.parse::<i64>().map_or_else(
                    |_| value.clone(),
                    |num| OperationValue::NumberRef((num * factor).to_string()),
                )
            }
            (OperationValue::StringRef(text), TransformFunction::ToUppercase) => {
                OperationValue::StringRef(text.to_uppercase())
            }
            (OperationValue::StringRef(text), TransformFunction::ToLowercase) => {
                OperationValue::StringRef(text.to_lowercase())
            }
            (OperationValue::StringRef(text), TransformFunction::Append(suffix)) => {
                OperationValue::StringRef(format!("{text}{suffix}"))
            }
            (OperationValue::StringRef(text), TransformFunction::Prepend(prefix)) => {
                OperationValue::StringRef(format!("{prefix}{text}"))
            }
            _ => value.clone(), // Return unchanged for unsupported combinations
        }
    }

    /// Flush current batch to processed batches
    fn flush_batch(&mut self) {
        if !self.current_batch.is_empty() {
            let batch = std::mem::take(&mut self.current_batch);
            self.processed_batches.push_back(batch);
        }
    }

    /// Get the underlying processor for inspection
    #[must_use]
    pub const fn processor(&self) -> &BlackBoxProcessor {
        &self.processor
    }

    /// Get processed batch count
    #[must_use]
    pub fn batch_count(&self) -> usize {
        self.processed_batches.len()
    }

    /// Get total items processed
    #[must_use]
    pub fn total_items(&self) -> usize {
        self.processed_batches.iter().map(Vec::len).sum::<usize>() + self.current_batch.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilterPredicate;

    #[test]
    fn test_stream_range_generation() {
        let mut processor = StreamingProcessor::new(10);

        let operations = vec![DsonOperation::StreamBuild {
            path: "numbers".to_string(),
            generator: StreamGenerator::Range {
                start: 0,
                end: 25,
                step: 5,
            },
        }];

        processor.process_stream(&operations).unwrap();

        // Should have generated: 0, 5, 10, 15, 20
        assert_eq!(processor.total_items(), 5);
        // Note: No emit operation, so items remain in current batch
    }

    #[test]
    fn test_stream_filter_even() {
        let mut processor = StreamingProcessor::new(10);

        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Even,
            },
        ];

        processor.process_stream(&operations).unwrap();

        // Should have: 0, 2, 4, 6, 8 (5 even numbers)
        assert_eq!(processor.total_items(), 5);
    }

    #[test]
    fn test_stream_map_multiply() {
        let mut processor = StreamingProcessor::new(10);

        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 1,
                    end: 4,
                    step: 1,
                },
            },
            DsonOperation::StreamMap {
                path: "numbers".to_string(),
                transform: TransformFunction::Multiply(2),
            },
        ];

        processor.process_stream(&operations).unwrap();

        // Should have: 2, 4, 6 (1*2, 2*2, 3*2)
        assert_eq!(processor.total_items(), 3);
    }

    #[test]
    fn test_stream_fibonacci() {
        let mut processor = StreamingProcessor::new(10);

        let operations = vec![DsonOperation::StreamBuild {
            path: "fib".to_string(),
            generator: StreamGenerator::Fibonacci(8),
        }];

        processor.process_stream(&operations).unwrap();

        // Should have: 0, 1, 1, 2, 3, 5, 8, 13
        assert_eq!(processor.total_items(), 8);
    }

    #[test]
    fn test_stream_repeat() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![DsonOperation::StreamBuild {
            path: "rep".to_string(),
            generator: StreamGenerator::Repeat(OperationValue::StringRef("x".to_string()), 5),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 5);
    }

    #[test]
    fn test_stream_emit() {
        let mut processor = StreamingProcessor::new(5);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamEmit {
                path: "out".to_string(),
                batch_size: 5,
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_filter_odd() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Odd,
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 5);
    }

    #[test]
    fn test_stream_filter_greater_than() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::GreaterThan(5),
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 4); // 6, 7, 8, 9
    }

    #[test]
    fn test_stream_filter_less_than() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::LessThan(5),
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 5); // 0, 1, 2, 3, 4
    }

    #[test]
    fn test_stream_filter_equals() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Equals(OperationValue::NumberRef("5".to_string())),
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_stream_filter_every_nth() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 12,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::EveryNth(3),
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_filter_alternate() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Alternate,
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_filter_custom() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 20,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Custom("value > 10".to_string()),
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_map_add() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 5,
                    step: 1,
                },
            },
            DsonOperation::StreamMap {
                path: "numbers".to_string(),
                transform: TransformFunction::Add(10),
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_map_to_lowercase() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "strings".to_string(),
                generator: StreamGenerator::Repeat(
                    OperationValue::StringRef("HELLO".to_string()),
                    3,
                ),
            },
            DsonOperation::StreamMap {
                path: "strings".to_string(),
                transform: TransformFunction::ToLowercase,
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_map_to_uppercase() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "strings".to_string(),
                generator: StreamGenerator::Repeat(
                    OperationValue::StringRef("hello".to_string()),
                    3,
                ),
            },
            DsonOperation::StreamMap {
                path: "strings".to_string(),
                transform: TransformFunction::ToUppercase,
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_map_append() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "strings".to_string(),
                generator: StreamGenerator::Repeat(
                    OperationValue::StringRef("hello".to_string()),
                    2,
                ),
            },
            DsonOperation::StreamMap {
                path: "strings".to_string(),
                transform: TransformFunction::Append("!".to_string()),
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_map_prepend() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "strings".to_string(),
                generator: StreamGenerator::Repeat(
                    OperationValue::StringRef("world".to_string()),
                    2,
                ),
            },
            DsonOperation::StreamMap {
                path: "strings".to_string(),
                transform: TransformFunction::Prepend("hello ".to_string()),
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_custom_generator() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![DsonOperation::StreamBuild {
            path: "custom".to_string(),
            generator: StreamGenerator::Custom("test".to_string()),
        }];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_processor_getter() {
        let processor = StreamingProcessor::new(10);
        let _ = processor.processor();
    }

    #[test]
    fn test_batch_count_empty() {
        let processor = StreamingProcessor::new(10);
        assert_eq!(processor.batch_count(), 0);
    }

    // Additional tests for coverage

    #[test]
    fn test_pass_through_operations() {
        let mut processor = StreamingProcessor::new(10);
        // Test operations that pass through to underlying processor
        let operations = vec![DsonOperation::FieldAdd {
            path: "test".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        }];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_generate_repeat_with_flush() {
        // Small buffer to trigger flush during generation
        let mut processor = StreamingProcessor::new(3);
        let operations = vec![DsonOperation::StreamBuild {
            path: "rep".to_string(),
            generator: StreamGenerator::Repeat(OperationValue::NumberRef("1".to_string()), 10),
        }];
        processor.process_stream(&operations).unwrap();
        // Should have flushed multiple times
        assert!(processor.batch_count() > 0);
    }

    #[test]
    fn test_generate_fibonacci_with_flush() {
        // Small buffer to trigger flush during fibonacci generation
        let mut processor = StreamingProcessor::new(3);
        let operations = vec![DsonOperation::StreamBuild {
            path: "fib".to_string(),
            generator: StreamGenerator::Fibonacci(15),
        }];
        processor.process_stream(&operations).unwrap();
        // Should have flushed multiple times
        assert!(processor.batch_count() > 0);
    }

    #[test]
    fn test_stream_emit_empty() {
        let mut processor = StreamingProcessor::new(10);
        // Emit with no data should be ok
        let operations = vec![DsonOperation::StreamEmit {
            path: "out".to_string(),
            batch_size: 5,
        }];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_stream_emit_with_processed_batches() {
        let mut processor = StreamingProcessor::new(3);
        // Generate enough to create processed batches
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Repeat(OperationValue::NumberRef("1".to_string()), 10),
            },
            DsonOperation::StreamEmit {
                path: "out".to_string(),
                batch_size: 3,
            },
        ];
        processor.process_stream(&operations).unwrap();
    }

    #[test]
    fn test_custom_predicate_less_than() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 20,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Custom("value < 5".to_string()),
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 5); // 0, 1, 2, 3, 4
    }

    #[test]
    fn test_custom_predicate_equals() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Custom("value == 5".to_string()),
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_custom_predicate_not_equals() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 10,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Custom("value != 5".to_string()),
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 9); // all except 5
    }

    #[test]
    fn test_custom_predicate_invalid() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "numbers".to_string(),
                generator: StreamGenerator::Range {
                    start: 0,
                    end: 5,
                    step: 1,
                },
            },
            DsonOperation::StreamFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::Custom("invalid_predicate".to_string()),
            },
        ];
        processor.process_stream(&operations).unwrap();
        // Invalid predicate defaults to true, so all items pass
        assert_eq!(processor.total_items(), 5);
    }

    #[test]
    fn test_filter_equals_string() {
        let mut processor = StreamingProcessor::new(10);
        let operations = vec![
            DsonOperation::StreamBuild {
                path: "strings".to_string(),
                generator: StreamGenerator::Repeat(
                    OperationValue::StringRef("hello".to_string()),
                    3,
                ),
            },
            DsonOperation::StreamFilter {
                path: "strings".to_string(),
                predicate: FilterPredicate::Equals(OperationValue::StringRef("hello".to_string())),
            },
        ];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 3);
    }

    #[test]
    fn test_filter_equals_bool() {
        let mut processor = StreamingProcessor::new(10);
        // Add bools to current batch manually
        processor.current_batch.push(OperationValue::BoolRef(true));
        processor.current_batch.push(OperationValue::BoolRef(false));
        processor.current_batch.push(OperationValue::BoolRef(true));

        let operations = vec![DsonOperation::StreamFilter {
            path: "bools".to_string(),
            predicate: FilterPredicate::Equals(OperationValue::BoolRef(true)),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 2); // Two trues
    }

    #[test]
    fn test_filter_equals_null() {
        let mut processor = StreamingProcessor::new(10);
        processor.current_batch.push(OperationValue::Null);
        processor
            .current_batch
            .push(OperationValue::StringRef("not null".to_string()));
        processor.current_batch.push(OperationValue::Null);

        let operations = vec![DsonOperation::StreamFilter {
            path: "nulls".to_string(),
            predicate: FilterPredicate::Equals(OperationValue::Null),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 2); // Two nulls
    }

    #[test]
    fn test_filter_equals_type_mismatch() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::NumberRef("5".to_string()));

        let operations = vec![DsonOperation::StreamFilter {
            path: "test".to_string(),
            predicate: FilterPredicate::Equals(OperationValue::StringRef("5".to_string())),
        }];
        processor.process_stream(&operations).unwrap();
        // Type mismatch returns false
        assert_eq!(processor.total_items(), 0);
    }

    #[test]
    fn test_filter_even_with_non_numbers() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::StringRef("not a number".to_string()));
        processor
            .current_batch
            .push(OperationValue::NumberRef("4".to_string()));
        processor.current_batch.push(OperationValue::BoolRef(true));

        let operations = vec![DsonOperation::StreamFilter {
            path: "test".to_string(),
            predicate: FilterPredicate::Even,
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1); // Only 4 passes
    }

    #[test]
    fn test_filter_odd_with_non_numbers() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::StringRef("not a number".to_string()));
        processor
            .current_batch
            .push(OperationValue::NumberRef("3".to_string()));

        let operations = vec![DsonOperation::StreamFilter {
            path: "test".to_string(),
            predicate: FilterPredicate::Odd,
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1); // Only 3 passes
    }

    #[test]
    fn test_filter_greater_than_with_non_numbers() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::StringRef("text".to_string()));
        processor
            .current_batch
            .push(OperationValue::NumberRef("10".to_string()));

        let operations = vec![DsonOperation::StreamFilter {
            path: "test".to_string(),
            predicate: FilterPredicate::GreaterThan(5),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1); // Only 10 > 5
    }

    #[test]
    fn test_filter_less_than_with_non_numbers() {
        let mut processor = StreamingProcessor::new(10);
        processor.current_batch.push(OperationValue::BoolRef(false));
        processor
            .current_batch
            .push(OperationValue::NumberRef("3".to_string()));

        let operations = vec![DsonOperation::StreamFilter {
            path: "test".to_string(),
            predicate: FilterPredicate::LessThan(5),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1); // Only 3 < 5
    }

    #[test]
    fn test_transform_invalid_number() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::NumberRef("not_a_number".to_string()));

        let operations = vec![DsonOperation::StreamMap {
            path: "test".to_string(),
            transform: TransformFunction::Add(10),
        }];
        processor.process_stream(&operations).unwrap();
        // Invalid number should be returned unchanged
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_transform_multiply_invalid_number() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::NumberRef("invalid".to_string()));

        let operations = vec![DsonOperation::StreamMap {
            path: "test".to_string(),
            transform: TransformFunction::Multiply(2),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_transform_unsupported_combination() {
        let mut processor = StreamingProcessor::new(10);
        // Try to uppercase a number (unsupported)
        processor
            .current_batch
            .push(OperationValue::NumberRef("5".to_string()));

        let operations = vec![DsonOperation::StreamMap {
            path: "test".to_string(),
            transform: TransformFunction::ToUppercase,
        }];
        processor.process_stream(&operations).unwrap();
        // Should return unchanged
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_filter_equals_float_comparison() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::NumberRef("3.14".to_string()));
        processor
            .current_batch
            .push(OperationValue::NumberRef("2.71".to_string()));

        let operations = vec![DsonOperation::StreamFilter {
            path: "floats".to_string(),
            predicate: FilterPredicate::Equals(OperationValue::NumberRef("3.14".to_string())),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_custom_predicate_string_equals() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::StringRef("hello".to_string()));
        processor
            .current_batch
            .push(OperationValue::StringRef("world".to_string()));

        let operations = vec![DsonOperation::StreamFilter {
            path: "strings".to_string(),
            predicate: FilterPredicate::Custom("value == \"hello\"".to_string()),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_custom_predicate_string_not_equals() {
        let mut processor = StreamingProcessor::new(10);
        processor
            .current_batch
            .push(OperationValue::StringRef("hello".to_string()));
        processor
            .current_batch
            .push(OperationValue::StringRef("world".to_string()));

        let operations = vec![DsonOperation::StreamFilter {
            path: "strings".to_string(),
            predicate: FilterPredicate::Custom("value != \"hello\"".to_string()),
        }];
        processor.process_stream(&operations).unwrap();
        assert_eq!(processor.total_items(), 1);
    }

    #[test]
    fn test_batch_count_after_generation() {
        let mut processor = StreamingProcessor::new(3);
        let operations = vec![DsonOperation::StreamBuild {
            path: "nums".to_string(),
            generator: StreamGenerator::Repeat(OperationValue::NumberRef("1".to_string()), 9),
        }];
        processor.process_stream(&operations).unwrap();
        assert!(processor.batch_count() >= 3);
    }

    #[test]
    fn test_total_items_mixed() {
        let mut processor = StreamingProcessor::new(5);
        // Add some to current batch
        processor.current_batch.push(OperationValue::Null);
        processor.current_batch.push(OperationValue::Null);

        // Flush to create processed batch
        processor.flush_batch();

        // Add more to current batch
        processor.current_batch.push(OperationValue::Null);

        assert_eq!(processor.total_items(), 3);
    }
}
