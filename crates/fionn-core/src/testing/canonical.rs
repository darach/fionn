// SPDX-License-Identifier: MIT OR Apache-2.0
//! Canonical operation examples and test datasets for DSON benchmarking
//!
//! This module provides comprehensive examples of DSON operations across different
//! scales (small, medium, large) and complexity levels, enabling thorough performance
//! analysis and optimization validation.

use crate::operations::{
    DsonOperation, FilterPredicate, OperationValue, ReduceFunction, StreamGenerator,
    TransformFunction,
};

/// Canonical test dataset with operations
pub struct CanonicalDataset {
    /// Name identifier for the dataset
    pub name: String,
    /// Human-readable description of what this dataset tests
    pub description: String,
    /// The scale/size category of this dataset
    pub scale: DatasetScale,
    /// The JSON input data for testing
    pub json_data: String,
    /// Operations to apply to the JSON data
    pub operations: Vec<DsonOperation>,
    /// Expected result after operations (if deterministic)
    pub expected_result: Option<String>,
}

/// Dataset scale categories
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetScale {
    /// Small scale: 10-100 elements
    Small,
    /// Medium scale: 100-1000 elements
    Medium,
    /// Large scale: 1000-10000 elements
    Large,
    /// Huge scale: 10000+ elements
    Huge,
}

impl DatasetScale {
    /// Get the minimum and maximum element count for this scale.
    #[must_use]
    pub const fn size_range(&self) -> (usize, usize) {
        match self {
            Self::Small => (10, 100),
            Self::Medium => (100, 1000),
            Self::Large => (1000, 10000),
            Self::Huge => (10_000, 100_000),
        }
    }
}

/// Generate canonical datasets for comprehensive benchmarking
#[must_use]
pub fn generate_canonical_datasets() -> Vec<CanonicalDataset> {
    let mut datasets = Vec::new();
    datasets.extend(generate_small_scale_datasets());
    datasets.extend(generate_medium_scale_datasets());
    datasets.extend(generate_large_scale_datasets());
    datasets.extend(generate_batch_datasets());
    datasets.extend(generate_streaming_datasets());
    datasets
}

fn generate_small_scale_datasets() -> Vec<CanonicalDataset> {
    vec![
        CanonicalDataset {
            name: "small_array_build_10".to_string(),
            description: "Build array of 10 elements".to_string(),
            scale: DatasetScale::Small,
            json_data: r#"{"base": "data"}"#.to_string(),
            operations: generate_array_build_operations(10),
            expected_result: None,
        },
        CanonicalDataset {
            name: "small_array_filter_alternate".to_string(),
            description: "Filter every other element from small array".to_string(),
            scale: DatasetScale::Small,
            json_data: r#"{"items": [1,2,3,4,5,6,7,8,9,10]}"#.to_string(),
            operations: vec![DsonOperation::ArrayFilter {
                path: "items".to_string(),
                predicate: FilterPredicate::Alternate,
            }],
            expected_result: Some(r#"{"items": [1,3,5,7,9]}"#.to_string()),
        },
        CanonicalDataset {
            name: "small_array_map_add".to_string(),
            description: "Add 10 to each element in small array".to_string(),
            scale: DatasetScale::Small,
            json_data: r#"{"values": [1,2,3,4,5]}"#.to_string(),
            operations: vec![DsonOperation::ArrayMap {
                path: "values".to_string(),
                transform: TransformFunction::Add(10),
            }],
            expected_result: Some(r#"{"values": [11,12,13,14,15]}"#.to_string()),
        },
    ]
}

fn generate_medium_scale_datasets() -> Vec<CanonicalDataset> {
    vec![
        CanonicalDataset {
            name: "medium_array_build_100".to_string(),
            description: "Build array of 100 elements".to_string(),
            scale: DatasetScale::Medium,
            json_data: r#"{"base": "data"}"#.to_string(),
            operations: generate_array_build_operations(100),
            expected_result: None,
        },
        CanonicalDataset {
            name: "medium_array_filter_every_5th".to_string(),
            description: "Filter every 5th element from medium array".to_string(),
            scale: DatasetScale::Medium,
            json_data: r#"{"numbers": [0,1,2,3,4,5,6,7,8,9]}"#.to_string(),
            operations: vec![DsonOperation::ArrayFilter {
                path: "numbers".to_string(),
                predicate: FilterPredicate::EveryNth(5),
            }],
            expected_result: None,
        },
        CanonicalDataset {
            name: "medium_array_reduce_sum".to_string(),
            description: "Sum all elements in medium array".to_string(),
            scale: DatasetScale::Medium,
            json_data: r#"{"numbers": [0,1,2,3,4]}"#.to_string(),
            operations: vec![DsonOperation::ArrayReduce {
                path: "numbers".to_string(),
                initial: OperationValue::NumberRef("0".to_string()),
                reducer: ReduceFunction::Sum,
            }],
            expected_result: Some(r#"{"sum": 4950}"#.to_string()),
        },
    ]
}

fn generate_large_scale_datasets() -> Vec<CanonicalDataset> {
    vec![
        CanonicalDataset {
            name: "large_array_build_1000".to_string(),
            description: "Build array of 1000 elements".to_string(),
            scale: DatasetScale::Large,
            json_data: r#"{"base": "data"}"#.to_string(),
            operations: generate_array_build_operations(1000),
            expected_result: None,
        },
        CanonicalDataset {
            name: "large_stream_build_1000".to_string(),
            description: "Stream build 1000 elements".to_string(),
            scale: DatasetScale::Large,
            json_data: r#"{"base": "data"}"#.to_string(),
            operations: vec![
                DsonOperation::StreamBuild {
                    path: "streamed_data".to_string(),
                    generator: StreamGenerator::Range {
                        start: 0,
                        end: 1000,
                        step: 1,
                    },
                },
                DsonOperation::StreamFilter {
                    path: "streamed_data".to_string(),
                    predicate: FilterPredicate::Even,
                },
                DsonOperation::StreamEmit {
                    path: "streamed_data".to_string(),
                    batch_size: 100,
                },
            ],
            expected_result: None,
        },
        CanonicalDataset {
            name: "large_complex_pipeline".to_string(),
            description: "Complex pipeline: build -> filter -> map -> reduce".to_string(),
            scale: DatasetScale::Large,
            json_data: r#"{"base": "data"}"#.to_string(),
            operations: vec![
                DsonOperation::ArrayBuild {
                    path: "data".to_string(),
                    elements: generate_number_elements(1000),
                },
                DsonOperation::ArrayFilter {
                    path: "data".to_string(),
                    predicate: FilterPredicate::Even,
                },
                DsonOperation::ArrayMap {
                    path: "data".to_string(),
                    transform: TransformFunction::Multiply(2),
                },
                DsonOperation::ArrayReduce {
                    path: "data".to_string(),
                    initial: OperationValue::NumberRef("0".to_string()),
                    reducer: ReduceFunction::Sum,
                },
            ],
            expected_result: None,
        },
    ]
}

fn generate_batch_datasets() -> Vec<CanonicalDataset> {
    vec![CanonicalDataset {
        name: "batch_operations_small".to_string(),
        description: "Execute batch of operations atomically".to_string(),
        scale: DatasetScale::Small,
        json_data: r#"{"user": {"name": "Alice", "age": 25}}"#.to_string(),
        operations: vec![DsonOperation::BatchExecute {
            operations: vec![
                DsonOperation::FieldAdd {
                    path: "user.email".to_string(),
                    value: OperationValue::StringRef("alice@example.com".to_string()),
                },
                DsonOperation::FieldModify {
                    path: "user.age".to_string(),
                    value: OperationValue::NumberRef("26".to_string()),
                },
                DsonOperation::FieldAdd {
                    path: "user.verified".to_string(),
                    value: OperationValue::BoolRef(true),
                },
            ],
        }],
        expected_result: Some(
            r#"{"user": {"name": "Alice", "age": 26, "email": "alice@example.com", "verified": true}}"#.to_string(),
        ),
    }]
}

fn generate_streaming_datasets() -> Vec<CanonicalDataset> {
    vec![CanonicalDataset {
        name: "streaming_fibonacci".to_string(),
        description: "Generate and process fibonacci sequence".to_string(),
        scale: DatasetScale::Medium,
        json_data: r#"{"base": "data"}"#.to_string(),
        operations: vec![
            DsonOperation::StreamBuild {
                path: "fibonacci".to_string(),
                generator: StreamGenerator::Fibonacci(50),
            },
            DsonOperation::StreamFilter {
                path: "fibonacci".to_string(),
                predicate: FilterPredicate::GreaterThan(100),
            },
            DsonOperation::StreamMap {
                path: "fibonacci".to_string(),
                transform: TransformFunction::Multiply(2),
            },
            DsonOperation::StreamEmit {
                path: "fibonacci".to_string(),
                batch_size: 10,
            },
        ],
        expected_result: None,
    }]
}

/// Generate array build operations for N elements
#[must_use]
pub fn generate_array_build_operations(n: usize) -> Vec<DsonOperation> {
    let elements = generate_number_elements(n);
    vec![DsonOperation::ArrayBuild {
        path: "data".to_string(),
        elements,
    }]
}

/// Generate number elements for testing
#[must_use]
pub fn generate_number_elements(n: usize) -> Vec<OperationValue> {
    (0..n)
        .map(|i| OperationValue::NumberRef(i.to_string()))
        .collect()
}

/// Generate JSON with number array for testing
#[must_use]
pub fn generate_number_array_json(n: usize) -> String {
    let numbers: Vec<String> = (0..n).map(|i| i.to_string()).collect();
    format!(r#"{{"numbers": [{}]}}"#, numbers.join(","))
}

/// Generate string elements for testing
#[must_use]
pub fn generate_string_elements(n: usize) -> Vec<OperationValue> {
    (0..n)
        .map(|i| OperationValue::StringRef(format!("item_{i}")))
        .collect()
}

/// Generate mixed-type elements for testing
#[must_use]
pub fn generate_mixed_elements(n: usize) -> Vec<OperationValue> {
    (0..n)
        .map(|i| match i % 4 {
            0 => OperationValue::NumberRef(i.to_string()),
            1 => OperationValue::StringRef(format!("string_{i}")),
            2 => OperationValue::BoolRef(i % 2 == 0),
            _ => OperationValue::Null,
        })
        .collect()
}

/// Performance comparison datasets
pub struct PerformanceComparison {
    /// Name identifier for the comparison
    pub name: String,
    /// Human-readable description of what is being compared
    pub description: String,
    /// Single sequential operations for baseline
    pub single_operations: Vec<DsonOperation>,
    /// Batched operations for atomic execution
    pub batch_operations: Vec<DsonOperation>,
    /// Streaming operations for lazy evaluation
    pub streaming_operations: Vec<DsonOperation>,
}

/// Generate performance comparison datasets
#[must_use]
pub fn generate_performance_comparisons() -> Vec<PerformanceComparison> {
    vec![
        PerformanceComparison {
            name: "array_filter_comparison".to_string(),
            description: "Compare single vs batch vs streaming array filtering".to_string(),
            single_operations: vec![DsonOperation::ArrayFilter {
                path: "data".to_string(),
                predicate: FilterPredicate::EveryNth(2),
            }],
            batch_operations: vec![DsonOperation::BatchExecute {
                operations: vec![DsonOperation::ArrayFilter {
                    path: "data".to_string(),
                    predicate: FilterPredicate::EveryNth(2),
                }],
            }],
            streaming_operations: vec![
                DsonOperation::StreamFilter {
                    path: "data".to_string(),
                    predicate: FilterPredicate::EveryNth(2),
                },
                DsonOperation::StreamEmit {
                    path: "data".to_string(),
                    batch_size: 50,
                },
            ],
        },
        PerformanceComparison {
            name: "complex_transformation_comparison".to_string(),
            description: "Compare approaches for complex data transformations".to_string(),
            single_operations: vec![
                DsonOperation::ArrayBuild {
                    path: "temp".to_string(),
                    elements: generate_number_elements(100),
                },
                DsonOperation::ArrayFilter {
                    path: "temp".to_string(),
                    predicate: FilterPredicate::GreaterThan(50),
                },
                DsonOperation::ArrayMap {
                    path: "temp".to_string(),
                    transform: TransformFunction::Multiply(2),
                },
                DsonOperation::ArrayReduce {
                    path: "temp".to_string(),
                    initial: OperationValue::NumberRef("0".to_string()),
                    reducer: ReduceFunction::Sum,
                },
            ],
            batch_operations: vec![DsonOperation::BatchExecute {
                operations: vec![
                    DsonOperation::ArrayBuild {
                        path: "temp".to_string(),
                        elements: generate_number_elements(100),
                    },
                    DsonOperation::ArrayFilter {
                        path: "temp".to_string(),
                        predicate: FilterPredicate::GreaterThan(50),
                    },
                    DsonOperation::ArrayMap {
                        path: "temp".to_string(),
                        transform: TransformFunction::Multiply(2),
                    },
                    DsonOperation::ArrayReduce {
                        path: "temp".to_string(),
                        initial: OperationValue::NumberRef("0".to_string()),
                        reducer: ReduceFunction::Sum,
                    },
                ],
            }],
            streaming_operations: vec![
                DsonOperation::StreamBuild {
                    path: "temp".to_string(),
                    generator: StreamGenerator::Range {
                        start: 0,
                        end: 100,
                        step: 1,
                    },
                },
                DsonOperation::StreamFilter {
                    path: "temp".to_string(),
                    predicate: FilterPredicate::GreaterThan(50),
                },
                DsonOperation::StreamMap {
                    path: "temp".to_string(),
                    transform: TransformFunction::Multiply(2),
                },
                DsonOperation::StreamEmit {
                    path: "temp".to_string(),
                    batch_size: 25,
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_datasets_generation() {
        let datasets = generate_canonical_datasets();
        assert!(!datasets.is_empty());

        // Check that we have datasets for all scales
        let scales: Vec<_> = datasets.iter().map(|d| d.scale).collect();
        assert!(scales.contains(&DatasetScale::Small));
        assert!(scales.contains(&DatasetScale::Medium));
        assert!(scales.contains(&DatasetScale::Large));
    }

    #[test]
    fn test_array_build_operations() {
        let ops = generate_array_build_operations(5);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            DsonOperation::ArrayBuild { path, elements } => {
                assert_eq!(path, "data");
                assert_eq!(elements.len(), 5);
            }
            _ => panic!("Expected ArrayBuild operation"),
        }
    }

    #[test]
    fn test_performance_comparisons() {
        let comparisons = generate_performance_comparisons();
        assert!(!comparisons.is_empty());

        for comparison in &comparisons {
            assert!(!comparison.single_operations.is_empty());
            assert!(!comparison.batch_operations.is_empty());
            assert!(!comparison.streaming_operations.is_empty());
        }
    }

    #[test]
    fn test_dataset_scale_size_range_small() {
        let (min, max) = DatasetScale::Small.size_range();
        assert_eq!(min, 10);
        assert_eq!(max, 100);
    }

    #[test]
    fn test_dataset_scale_size_range_medium() {
        let (min, max) = DatasetScale::Medium.size_range();
        assert_eq!(min, 100);
        assert_eq!(max, 1000);
    }

    #[test]
    fn test_dataset_scale_size_range_large() {
        let (min, max) = DatasetScale::Large.size_range();
        assert_eq!(min, 1000);
        assert_eq!(max, 10000);
    }

    #[test]
    fn test_dataset_scale_size_range_huge() {
        let (min, max) = DatasetScale::Huge.size_range();
        assert_eq!(min, 10_000);
        assert_eq!(max, 100_000);
    }

    #[test]
    fn test_dataset_scale_debug() {
        assert!(format!("{:?}", DatasetScale::Small).contains("Small"));
        assert!(format!("{:?}", DatasetScale::Medium).contains("Medium"));
        assert!(format!("{:?}", DatasetScale::Large).contains("Large"));
        assert!(format!("{:?}", DatasetScale::Huge).contains("Huge"));
    }

    #[test]
    fn test_dataset_scale_clone() {
        let scale = DatasetScale::Medium;
        let cloned = scale;
        assert_eq!(scale, cloned);
    }

    #[test]
    fn test_generate_number_elements() {
        let elements = generate_number_elements(5);
        assert_eq!(elements.len(), 5);
        for (i, elem) in elements.iter().enumerate() {
            match elem {
                OperationValue::NumberRef(s) => assert_eq!(s, &i.to_string()),
                _ => panic!("Expected NumberRef"),
            }
        }
    }

    #[test]
    fn test_generate_number_array_json() {
        let json = generate_number_array_json(3);
        assert!(json.contains("numbers"));
        assert!(json.contains('0'));
        assert!(json.contains('1'));
        assert!(json.contains('2'));
    }

    #[test]
    fn test_generate_string_elements() {
        let elements = generate_string_elements(3);
        assert_eq!(elements.len(), 3);
        match &elements[0] {
            OperationValue::StringRef(s) => assert_eq!(s, "item_0"),
            _ => panic!("Expected StringRef"),
        }
        match &elements[1] {
            OperationValue::StringRef(s) => assert_eq!(s, "item_1"),
            _ => panic!("Expected StringRef"),
        }
    }

    #[test]
    fn test_generate_mixed_elements() {
        let elements = generate_mixed_elements(8);
        assert_eq!(elements.len(), 8);
        // i=0: NumberRef
        assert!(matches!(&elements[0], OperationValue::NumberRef(_)));
        // i=1: StringRef
        assert!(matches!(&elements[1], OperationValue::StringRef(_)));
        // i=2: BoolRef
        assert!(matches!(&elements[2], OperationValue::BoolRef(_)));
        // i=3: Null
        assert!(matches!(&elements[3], OperationValue::Null));
        // i=4: NumberRef
        assert!(matches!(&elements[4], OperationValue::NumberRef(_)));
    }

    #[test]
    fn test_canonical_dataset_fields() {
        let datasets = generate_canonical_datasets();
        let dataset = &datasets[0];

        // Test that dataset has all required fields
        assert!(!dataset.name.is_empty());
        assert!(!dataset.description.is_empty());
        assert!(!dataset.json_data.is_empty());
        assert!(!dataset.operations.is_empty());
    }

    #[test]
    fn test_canonical_dataset_with_expected_result() {
        let datasets = generate_canonical_datasets();
        // Find a dataset with expected_result
        let with_result = datasets.iter().find(|d| d.expected_result.is_some());
        assert!(with_result.is_some());
        let result = with_result.unwrap().expected_result.as_ref().unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_canonical_dataset_without_expected_result() {
        let datasets = generate_canonical_datasets();
        // Find a dataset without expected_result
        let without_result = datasets.iter().find(|d| d.expected_result.is_none());
        assert!(without_result.is_some());
    }

    #[test]
    fn test_performance_comparison_fields() {
        let comparisons = generate_performance_comparisons();
        let comparison = &comparisons[0];

        assert!(!comparison.name.is_empty());
        assert!(!comparison.description.is_empty());
        assert!(!comparison.single_operations.is_empty());
        assert!(!comparison.batch_operations.is_empty());
        assert!(!comparison.streaming_operations.is_empty());
    }

    #[test]
    fn test_dataset_scale_equality() {
        assert_eq!(DatasetScale::Small, DatasetScale::Small);
        assert_ne!(DatasetScale::Small, DatasetScale::Large);
    }
}
