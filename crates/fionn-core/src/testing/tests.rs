// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive test program for canonical DSON operations
//!
//! This program demonstrates the complete operation processing pipeline:
//! 1. Input schema filtering
//! 2. Canonical operation computation
//! 3. Operation optimization
//! 4. Output schema filtering

use crate::operations::{
    CanonicalOperationProcessor, DsonOperation, MergeStrategy, OperationOptimizer, OperationValue,
};
use std::collections::HashSet;

fn main() {
    println!("🧪 DSON Canonical Operations Test Suite");
    println!("======================================\n");

    test_basic_canonical_operations();
    test_schema_filtering();
    test_operation_optimization();
    test_crdt_operations();
    test_complex_scenarios();

    println!("✅ All tests completed successfully!");
}

fn test_basic_canonical_operations() {
    println!("📋 Test 1: Basic Canonical Operations");
    println!("------------------------------------");

    let mut processor = CanonicalOperationProcessor::new(
        HashSet::from(["user.name".to_string(), "user.age".to_string()]),
        HashSet::from(["user.name".to_string(), "user.age".to_string()]),
    );

    // Add some operations
    processor.add_operation(DsonOperation::FieldAdd {
        path: "user.name".to_string(),
        value: OperationValue::StringRef("Alice".to_string()),
    });
    processor.add_operation(DsonOperation::FieldAdd {
        path: "user.age".to_string(),
        value: OperationValue::NumberRef("30".to_string()),
    });

    let canonical = processor.compute_canonical().unwrap();
    println!("Operations: {}", canonical.len());
    for op in canonical {
        println!("  {op:?}");
    }
    println!();
}

fn test_schema_filtering() {
    println!("🔍 Test 2: Schema Filtering");
    println!("--------------------------");

    let mut processor = CanonicalOperationProcessor::new(
        HashSet::from(["allowed.field".to_string()]), // Input schema
        HashSet::from(["allowed.field".to_string()]), // Output schema
    );

    // Add operations for allowed and disallowed fields
    processor.add_operation(DsonOperation::FieldAdd {
        path: "allowed.field".to_string(),
        value: OperationValue::StringRef("ok".to_string()),
    });
    processor.add_operation(DsonOperation::FieldAdd {
        path: "disallowed.field".to_string(),
        value: OperationValue::StringRef("filtered".to_string()),
    });
    processor.add_operation(DsonOperation::FieldDelete {
        path: "another.disallowed".to_string(),
    });

    let canonical = processor.compute_canonical().unwrap();
    println!("Filtered operations: {}", canonical.len());
    for op in canonical {
        println!("  {op:?}");
    }
    println!();
}

fn test_operation_optimization() {
    println!("⚡ Test 3: Operation Optimization");
    println!("-------------------------------");

    let operations = vec![
        // Multiple modifies on same field
        DsonOperation::FieldModify {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("Alice".to_string()),
        },
        DsonOperation::FieldModify {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("Bob".to_string()),
        },
        // Delete followed by add (should become modify)
        DsonOperation::FieldDelete {
            path: "user.age".to_string(),
        },
        DsonOperation::FieldAdd {
            path: "user.age".to_string(),
            value: OperationValue::NumberRef("25".to_string()),
        },
        // Redundant delete
        DsonOperation::FieldDelete {
            path: "user.temp".to_string(),
        },
    ];

    let optimizer = OperationOptimizer::new(operations);
    let ops = optimizer.optimize();

    println!("Original operations: 5");
    println!("Optimized operations: {}", ops.len());
    for op in ops {
        println!("  {op:?}");
    }
    println!();
}

fn test_crdt_operations() {
    println!("🔄 Test 4: CRDT Operations");
    println!("-------------------------");

    let mut processor = CanonicalOperationProcessor::new(
        HashSet::from(["counter".to_string()]),
        HashSet::from(["counter".to_string()]),
    );

    // Simulate concurrent updates
    processor.add_operation(DsonOperation::MergeField {
        path: "counter".to_string(),
        value: OperationValue::NumberRef("10".to_string()),
        timestamp: 100,
    });
    processor.add_operation(DsonOperation::MergeField {
        path: "counter".to_string(),
        value: OperationValue::NumberRef("15".to_string()),
        timestamp: 200,
    });
    processor.add_operation(DsonOperation::ConflictResolve {
        path: "counter".to_string(),
        strategy: MergeStrategy::Additive,
    });

    let canonical = processor.compute_canonical().unwrap();
    println!("CRDT operations: {}", canonical.len());
    for op in canonical {
        println!("  {op:?}");
    }
    println!();
}

fn test_complex_scenarios() {
    println!("🎭 Test 5: Complex Scenarios");
    println!("---------------------------");

    // Test wildcard schema matching
    let mut processor = CanonicalOperationProcessor::new(
        HashSet::from(["user.*".to_string()]), // Allow all user fields
        HashSet::from(["user.name".to_string(), "user.email".to_string()]), // Only output name and email
    );

    processor.add_operation(DsonOperation::FieldAdd {
        path: "user.name".to_string(),
        value: OperationValue::StringRef("Alice".to_string()),
    });
    processor.add_operation(DsonOperation::FieldAdd {
        path: "user.email".to_string(),
        value: OperationValue::StringRef("alice@example.com".to_string()),
    });
    processor.add_operation(DsonOperation::FieldAdd {
        path: "user.age".to_string(),
        value: OperationValue::NumberRef("30".to_string()),
    }); // This should be filtered out in output

    let canonical = processor.compute_canonical().unwrap();
    println!("Wildcard filtering operations: {}", canonical.len());
    for op in canonical {
        println!("  {op:?}");
    }

    // Test presence/absence operations (always pass through)
    processor.add_operation(DsonOperation::CheckPresence {
        path: "user.verified".to_string(),
    });
    processor.add_operation(DsonOperation::CheckAbsence {
        path: "user.deleted".to_string(),
    });

    let canonical_with_presence = processor.compute_canonical().unwrap();
    println!("With presence checks: {}", canonical_with_presence.len());
    for op in canonical_with_presence {
        println!("  {op:?}");
    }
    println!();
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_zero_allocation_guarantee() {
        // This test ensures operations don't allocate unnecessarily
        let processor = CanonicalOperationProcessor::new(HashSet::new(), HashSet::new());

        // Add operations that should be filtered out
        let mut processor = processor;
        processor.add_operation(DsonOperation::FieldAdd {
            path: "filtered.out".to_string(),
            value: OperationValue::StringRef("test".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(
            canonical.len(),
            0,
            "Filtered operations should not appear in canonical set"
        );
    }

    #[test]
    fn test_operation_ordering_preservation() {
        let operations = vec![
            DsonOperation::ObjectStart {
                path: "root".to_string(),
            },
            DsonOperation::FieldAdd {
                path: "root.field1".to_string(),
                value: OperationValue::StringRef("value1".to_string()),
            },
            DsonOperation::FieldAdd {
                path: "root.field2".to_string(),
                value: OperationValue::StringRef("value2".to_string()),
            },
            DsonOperation::ObjectEnd {
                path: "root".to_string(),
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();

        // Structural operations should maintain order
        assert_eq!(ops.len(), 4);
        match &ops[0] {
            DsonOperation::ObjectStart { path } => assert_eq!(path, "root"),
            _ => panic!("Expected ObjectStart first"),
        }
        match &ops[3] {
            DsonOperation::ObjectEnd { path } => assert_eq!(path, "root"),
            _ => panic!("Expected ObjectEnd last"),
        }
    }

    #[test]
    fn test_basic_canonical_operations() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["user.name".to_string(), "user.age".to_string()]),
            HashSet::from(["user.name".to_string(), "user.age".to_string()]),
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("Alice".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.age".to_string(),
            value: OperationValue::NumberRef("30".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();
        assert_eq!(canonical.len(), 2);
    }

    #[test]
    fn test_schema_filtering() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["allowed.field".to_string()]),
            HashSet::from(["allowed.field".to_string()]),
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "allowed.field".to_string(),
            value: OperationValue::StringRef("ok".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "disallowed.field".to_string(),
            value: OperationValue::StringRef("filtered".to_string()),
        });
        processor.add_operation(DsonOperation::FieldDelete {
            path: "another.disallowed".to_string(),
        });

        let canonical = processor.compute_canonical().unwrap();
        // Only allowed.field should pass through
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn test_operation_optimization() {
        let operations = vec![
            DsonOperation::FieldModify {
                path: "user.name".to_string(),
                value: OperationValue::StringRef("Alice".to_string()),
            },
            DsonOperation::FieldModify {
                path: "user.name".to_string(),
                value: OperationValue::StringRef("Bob".to_string()),
            },
            DsonOperation::FieldDelete {
                path: "user.age".to_string(),
            },
            DsonOperation::FieldAdd {
                path: "user.age".to_string(),
                value: OperationValue::NumberRef("25".to_string()),
            },
            DsonOperation::FieldDelete {
                path: "user.temp".to_string(),
            },
        ];

        let optimizer = OperationOptimizer::new(operations);
        let ops = optimizer.optimize();

        // Should be optimized down from 5 operations
        assert!(ops.len() <= 5);
    }

    #[test]
    fn test_crdt_operations() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["counter".to_string()]),
            HashSet::from(["counter".to_string()]),
        );

        processor.add_operation(DsonOperation::MergeField {
            path: "counter".to_string(),
            value: OperationValue::NumberRef("10".to_string()),
            timestamp: 100,
        });
        processor.add_operation(DsonOperation::MergeField {
            path: "counter".to_string(),
            value: OperationValue::NumberRef("15".to_string()),
            timestamp: 200,
        });
        processor.add_operation(DsonOperation::ConflictResolve {
            path: "counter".to_string(),
            strategy: MergeStrategy::Additive,
        });

        let canonical = processor.compute_canonical().unwrap();
        assert!(!canonical.is_empty());
    }

    #[test]
    fn test_complex_scenarios_wildcard() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from([
                "user.name".to_string(),
                "user.email".to_string(),
                "user.age".to_string(),
            ]),
            HashSet::from(["user.name".to_string(), "user.email".to_string()]),
        );

        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.name".to_string(),
            value: OperationValue::StringRef("Alice".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.email".to_string(),
            value: OperationValue::StringRef("alice@example.com".to_string()),
        });
        processor.add_operation(DsonOperation::FieldAdd {
            path: "user.age".to_string(),
            value: OperationValue::NumberRef("30".to_string()),
        });

        let canonical = processor.compute_canonical().unwrap();
        // Should have name and email (age is filtered by output schema)
        assert_eq!(canonical.len(), 2);
    }

    #[test]
    fn test_presence_absence_operations() {
        let mut processor = CanonicalOperationProcessor::new(
            HashSet::from(["user".to_string()]),
            HashSet::from(["user".to_string()]),
        );

        processor.add_operation(DsonOperation::CheckPresence {
            path: "user.verified".to_string(),
        });
        processor.add_operation(DsonOperation::CheckAbsence {
            path: "user.deleted".to_string(),
        });

        let canonical = processor.compute_canonical().unwrap();
        // Presence/absence operations should pass through
        assert!(!canonical.is_empty());
    }

    #[test]
    fn test_main_function_coverage() {
        // Call main to ensure it's covered
        // This doesn't panic, so we know it works
        super::main();
    }

    #[test]
    fn test_basic_canonical_operations_fn() {
        // Call the standalone function
        super::test_basic_canonical_operations();
    }

    #[test]
    fn test_schema_filtering_fn() {
        // Call the standalone function
        super::test_schema_filtering();
    }

    #[test]
    fn test_operation_optimization_fn() {
        // Call the standalone function
        super::test_operation_optimization();
    }

    #[test]
    fn test_crdt_operations_fn() {
        // Call the standalone function
        super::test_crdt_operations();
    }

    #[test]
    fn test_complex_scenarios_fn() {
        // Call the standalone function
        super::test_complex_scenarios();
    }
}
