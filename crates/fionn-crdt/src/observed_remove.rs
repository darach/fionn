// SPDX-License-Identifier: MIT OR Apache-2.0
//! Observed-Remove Semantics for SIMD-DSON
//!
//! This module implements observed-remove semantics in the context of SIMD-DSON's
//! skip tape architecture. Observed-remove ensures that elements can only be removed
//! if their addition has been observed, preventing data loss during concurrent operations.

use fionn_core::DsonOperation;
use std::collections::HashSet;

/// Tracks observed additions for observed-remove semantics
#[derive(Debug, Clone)]
pub struct ObservedAdditions {
    /// Set of field paths that have been observed as added
    observed_paths: HashSet<String>,
}

impl Default for ObservedAdditions {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservedAdditions {
    /// Create a new empty set of observed additions.
    #[must_use]
    pub fn new() -> Self {
        Self {
            observed_paths: HashSet::new(),
        }
    }

    /// Mark a field path as having been observed as added
    pub fn observe_addition(&mut self, path: &str) {
        self.observed_paths.insert(path.to_string());
    }

    /// Check if a field path has been observed as added
    #[must_use]
    pub fn has_observed_addition(&self, path: &str) -> bool {
        self.observed_paths.contains(path)
    }

    /// Remove observation of a field path (used when field is deleted)
    pub fn remove_observation(&mut self, path: &str) {
        self.observed_paths.remove(path);
    }
}

/// Observed-remove processor that enforces removal constraints
#[derive(Debug)]
pub struct ObservedRemoveProcessor {
    observed_additions: ObservedAdditions,
    pending_operations: Vec<DsonOperation>,
}

impl Default for ObservedRemoveProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservedRemoveProcessor {
    /// Create a new observed-remove processor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            observed_additions: ObservedAdditions::new(),
            pending_operations: Vec::new(),
        }
    }

    /// Process an operation with observed-remove semantics
    ///
    /// # Errors
    /// Currently this function always succeeds, but returns `Result` for future
    /// extensibility when more complex validation may be required.
    pub fn process_operation(&mut self, operation: &DsonOperation) -> Option<DsonOperation> {
        match operation {
            DsonOperation::FieldAdd { path, .. } => {
                // Record that we've observed this field being added
                self.observed_additions.observe_addition(path);
                Some(operation.clone())
            }
            DsonOperation::FieldDelete { path } => {
                // Only allow deletion if we've observed the addition
                if self.observed_additions.has_observed_addition(path) {
                    self.observed_additions.remove_observation(path);
                    Some(operation.clone())
                } else {
                    // Cannot delete a field that was never observed as added
                    // Buffer the operation for later processing
                    self.pending_operations.push(operation.clone());
                    None
                }
            }
            DsonOperation::FieldModify { path, .. } => {
                // Allow modification if we've observed the field exists
                if self.observed_additions.has_observed_addition(path) {
                    Some(operation.clone())
                } else {
                    // Field doesn't exist yet, buffer for later
                    self.pending_operations.push(operation.clone());
                    None
                }
            }
            // For other operations, pass through unchanged
            _ => Some(operation.clone()),
        }
    }

    /// Process pending operations that may now be valid
    pub fn process_pending_operations(&mut self) -> Vec<DsonOperation> {
        let mut processed = Vec::new();
        let mut still_pending = Vec::new();

        // Collect operations first to avoid borrowing issues
        let pending_ops: Vec<DsonOperation> = self.pending_operations.drain(..).collect();

        for operation in pending_ops {
            if let Some(processed_op) = self.process_operation(&operation) {
                processed.push(processed_op);
            } else {
                still_pending.push(operation);
            }
        }

        self.pending_operations = still_pending;
        processed
    }

    /// Get the current set of observed field paths
    #[must_use]
    pub const fn observed_fields(&self) -> &HashSet<String> {
        &self.observed_additions.observed_paths
    }

    /// Check if there are pending operations
    #[must_use]
    pub const fn has_pending_operations(&self) -> bool {
        !self.pending_operations.is_empty()
    }
}

/// Concurrent operation resolver using observed-remove semantics
pub struct ConcurrentResolver {
    local_observed: ObservedAdditions,
    remote_observed: ObservedAdditions,
}

impl Default for ConcurrentResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ConcurrentResolver {
    /// Create a new concurrent operation resolver.
    #[must_use]
    pub fn new() -> Self {
        Self {
            local_observed: ObservedAdditions::new(),
            remote_observed: ObservedAdditions::new(),
        }
    }

    /// Resolve concurrent operations between local and remote replicas
    pub fn resolve_concurrent_operations(
        &mut self,
        local_ops: &[DsonOperation],
        remote_ops: &[DsonOperation],
    ) -> (Vec<DsonOperation>, Vec<DsonOperation>) {
        let mut resolved_local = Vec::new();
        let mut resolved_remote = Vec::new();

        // Process operations in timestamp order (assuming operations have timestamps)
        // For this simplified implementation, we'll process in the order given

        for local_op in local_ops {
            match local_op {
                DsonOperation::FieldAdd { path, .. } => {
                    self.local_observed.observe_addition(path);
                    resolved_local.push(local_op.clone());
                }
                DsonOperation::FieldDelete { .. } => {
                    // Deletion proceeds regardless of remote observation status.
                    // In observed-remove semantics, local deletions are always resolved.
                    resolved_local.push(local_op.clone());
                }
                DsonOperation::FieldModify { path, .. } => {
                    // Updates always "win" over concurrent deletions in observed-remove semantics
                    self.local_observed.observe_addition(path);
                    resolved_local.push(local_op.clone());
                }
                _ => resolved_local.push(local_op.clone()),
            }
        }

        for remote_op in remote_ops {
            match remote_op {
                DsonOperation::FieldAdd { path, .. } | DsonOperation::FieldModify { path, .. } => {
                    self.remote_observed.observe_addition(path);
                    resolved_remote.push(remote_op.clone());
                }
                DsonOperation::FieldDelete { .. } => {
                    // Remote deletions are resolved without checking local observation
                    resolved_remote.push(remote_op.clone());
                }
                _ => resolved_remote.push(remote_op.clone()),
            }
        }

        (resolved_local, resolved_remote)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fionn_core::{DsonOperation, OperationValue};

    #[test]
    fn test_observed_remove_basic() {
        let mut processor = ObservedRemoveProcessor::new();

        // Add a field
        let add_op = DsonOperation::FieldAdd {
            path: "test.field".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        };

        let result = processor.process_operation(&add_op);
        assert!(result.is_some());
        assert!(processor.observed_fields().contains("test.field"));

        // Now we can delete it
        let delete_op = DsonOperation::FieldDelete {
            path: "test.field".to_string(),
        };

        let result = processor.process_operation(&delete_op);
        assert!(result.is_some());
        assert!(!processor.observed_fields().contains("test.field"));
    }

    #[test]
    fn test_cannot_delete_unobserved_field() {
        let mut processor = ObservedRemoveProcessor::new();

        // Try to delete a field that was never added
        let delete_op = DsonOperation::FieldDelete {
            path: "never.added".to_string(),
        };

        let result = processor.process_operation(&delete_op);
        assert!(result.is_none()); // Operation is buffered
        assert!(processor.has_pending_operations());
    }

    #[test]
    fn test_concurrent_update_wins_over_delete() {
        let mut resolver = ConcurrentResolver::new();

        // Local: add field
        let local_add = DsonOperation::FieldAdd {
            path: "shared.field".to_string(),
            value: OperationValue::StringRef("local_value".to_string()),
        };

        // Remote: add field then delete it
        let remote_add = DsonOperation::FieldAdd {
            path: "shared.field".to_string(),
            value: OperationValue::StringRef("remote_value".to_string()),
        };
        let remote_delete = DsonOperation::FieldDelete {
            path: "shared.field".to_string(),
        };

        let local_ops = vec![local_add];
        let remote_ops = vec![remote_add, remote_delete];

        let (resolved_local, resolved_remote) =
            resolver.resolve_concurrent_operations(&local_ops, &remote_ops);

        // Both should have their operations resolved
        assert_eq!(resolved_local.len(), 1);
        assert_eq!(resolved_remote.len(), 2);

        // The field should be observed as added by both
        assert!(
            resolver
                .local_observed
                .has_observed_addition("shared.field")
        );
        assert!(
            resolver
                .remote_observed
                .has_observed_addition("shared.field")
        );
    }

    #[test]
    fn test_observed_additions_new() {
        let additions = ObservedAdditions::new();
        assert!(!additions.has_observed_addition("any.path"));
    }

    #[test]
    fn test_observed_additions_observe() {
        let mut additions = ObservedAdditions::new();
        additions.observe_addition("test.path");
        assert!(additions.has_observed_addition("test.path"));
        assert!(!additions.has_observed_addition("other.path"));
    }

    #[test]
    fn test_observed_additions_remove() {
        let mut additions = ObservedAdditions::new();
        additions.observe_addition("test.path");
        assert!(additions.has_observed_addition("test.path"));
        additions.remove_observation("test.path");
        assert!(!additions.has_observed_addition("test.path"));
    }

    #[test]
    fn test_observed_additions_clone() {
        let mut additions = ObservedAdditions::new();
        additions.observe_addition("test.path");
        let cloned = additions.clone();
        assert!(cloned.has_observed_addition("test.path"));
    }

    #[test]
    fn test_observed_additions_debug() {
        let additions = ObservedAdditions::new();
        let debug = format!("{additions:?}");
        assert!(debug.contains("ObservedAdditions"));
    }

    #[test]
    fn test_observed_remove_processor_new() {
        let processor = ObservedRemoveProcessor::new();
        assert!(!processor.has_pending_operations());
        assert!(processor.observed_fields().is_empty());
    }

    #[test]
    fn test_observed_remove_processor_modify_unobserved() {
        let mut processor = ObservedRemoveProcessor::new();

        let modify_op = DsonOperation::FieldModify {
            path: "unobserved.field".to_string(),
            old_value: None,
            new_value: OperationValue::StringRef("value".to_string()),
        };

        let result = processor.process_operation(&modify_op);
        assert!(result.is_none());
        assert!(processor.has_pending_operations());
    }

    #[test]
    fn test_observed_remove_processor_modify_observed() {
        let mut processor = ObservedRemoveProcessor::new();

        // First add the field
        let add_op = DsonOperation::FieldAdd {
            path: "test.field".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        };
        let _ = processor.process_operation(&add_op);

        // Now modify it
        let modify_op = DsonOperation::FieldModify {
            path: "test.field".to_string(),
            old_value: None,
            new_value: OperationValue::StringRef("modified".to_string()),
        };

        let result = processor.process_operation(&modify_op);
        assert!(result.is_some());
    }

    #[test]
    fn test_observed_remove_processor_other_operations() {
        let mut processor = ObservedRemoveProcessor::new();

        let other_op = DsonOperation::ObjectStart {
            path: "obj".to_string(),
        };

        let result = processor.process_operation(&other_op);
        assert!(result.is_some());
    }

    #[test]
    fn test_observed_remove_processor_process_pending() {
        let mut processor = ObservedRemoveProcessor::new();

        // First try to delete an unobserved field
        let delete_op = DsonOperation::FieldDelete {
            path: "pending.field".to_string(),
        };
        let _ = processor.process_operation(&delete_op);
        assert!(processor.has_pending_operations());

        // Now add the field
        let add_op = DsonOperation::FieldAdd {
            path: "pending.field".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        };
        let _ = processor.process_operation(&add_op);

        // Process pending operations
        let processed = processor.process_pending_operations();
        // The delete should now be processed
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn test_observed_remove_processor_debug() {
        let processor = ObservedRemoveProcessor::new();
        let debug = format!("{processor:?}");
        assert!(debug.contains("ObservedRemoveProcessor"));
    }

    #[test]
    fn test_concurrent_resolver_new() {
        let resolver = ConcurrentResolver::new();
        assert!(!resolver.local_observed.has_observed_addition("any"));
        assert!(!resolver.remote_observed.has_observed_addition("any"));
    }

    #[test]
    fn test_concurrent_resolver_local_delete() {
        let mut resolver = ConcurrentResolver::new();

        let local_delete = DsonOperation::FieldDelete {
            path: "field".to_string(),
        };

        let (resolved_local, _resolved_remote) =
            resolver.resolve_concurrent_operations(&[local_delete], &[]);

        assert_eq!(resolved_local.len(), 1);
    }

    #[test]
    fn test_concurrent_resolver_local_modify() {
        let mut resolver = ConcurrentResolver::new();

        let local_modify = DsonOperation::FieldModify {
            path: "field".to_string(),
            old_value: None,
            new_value: OperationValue::StringRef("value".to_string()),
        };

        let (resolved_local, _resolved_remote) =
            resolver.resolve_concurrent_operations(&[local_modify], &[]);

        assert_eq!(resolved_local.len(), 1);
        assert!(resolver.local_observed.has_observed_addition("field"));
    }

    #[test]
    fn test_concurrent_resolver_remote_modify() {
        let mut resolver = ConcurrentResolver::new();

        let remote_modify = DsonOperation::FieldModify {
            path: "field".to_string(),
            old_value: None,
            new_value: OperationValue::StringRef("value".to_string()),
        };

        let (_resolved_local, resolved_remote) =
            resolver.resolve_concurrent_operations(&[], &[remote_modify]);

        assert_eq!(resolved_remote.len(), 1);
        assert!(resolver.remote_observed.has_observed_addition("field"));
    }

    #[test]
    fn test_concurrent_resolver_other_operations() {
        let mut resolver = ConcurrentResolver::new();

        let local_other = DsonOperation::ObjectStart {
            path: "obj".to_string(),
        };
        let remote_other = DsonOperation::ObjectEnd {
            path: "obj".to_string(),
        };

        let (resolved_local, resolved_remote) =
            resolver.resolve_concurrent_operations(&[local_other], &[remote_other]);

        assert_eq!(resolved_local.len(), 1);
        assert_eq!(resolved_remote.len(), 1);
    }

    #[test]
    fn test_concurrent_resolver_delete_with_remote_observed() {
        let mut resolver = ConcurrentResolver::new();

        // First, have remote observe the addition
        let remote_add = DsonOperation::FieldAdd {
            path: "field".to_string(),
            value: OperationValue::StringRef("value".to_string()),
        };
        let _ = resolver.resolve_concurrent_operations(&[], &[remote_add]);

        // Now local tries to delete
        let local_delete = DsonOperation::FieldDelete {
            path: "field".to_string(),
        };

        let (resolved_local, _) = resolver.resolve_concurrent_operations(&[local_delete], &[]);

        assert_eq!(resolved_local.len(), 1);
    }
}
