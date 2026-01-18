// SPDX-License-Identifier: MIT OR Apache-2.0
//! CSV-specific diff semantics
//!
//! CSV diff operations require special handling because:
//! - Rows have implicit identity (by position or by key column)
//! - Column order may or may not be significant
//! - Missing values vs empty strings have semantic differences
//!
//! This module provides:
//! - Row-based diff (compare by position)
//! - Key-based diff (compare by identity column)
//! - Content-addressed diff (compare by row hash)
//!
//! ## Diff Modes
//!
//! ### Positional Mode (default)
//! Rows are compared by their position. Row 0 vs Row 0, etc.
//! - Fast for ordered data
//! - Produces large diffs for insertions/deletions
//!
//! ### Key-Based Mode
//! Rows are matched by a key column (e.g., "id").
//! - Handles insertions/deletions gracefully
//! - Requires unique key column
//!
//! ### Content-Addressed Mode
//! Rows are hashed and compared by content.
//! - Detects moved rows
//! - Higher memory usage

use fionn_core::{TapeNodeKind, TapeSource, TapeValue};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// CSV diff options
#[derive(Debug, Clone)]
pub struct CsvDiffOptions {
    /// How to identify rows for comparison
    pub row_identity: RowIdentityMode,
    /// Whether column order matters
    pub column_order_significant: bool,
    /// Columns to ignore in comparison
    pub ignore_columns: HashSet<String>,
    /// Treat empty string as null
    pub empty_is_null: bool,
    /// Key column for key-based identity
    pub key_column: Option<String>,
}

impl Default for CsvDiffOptions {
    fn default() -> Self {
        Self {
            row_identity: RowIdentityMode::Positional,
            column_order_significant: false,
            ignore_columns: HashSet::new(),
            empty_is_null: false,
            key_column: None,
        }
    }
}

impl CsvDiffOptions {
    /// Create options for key-based row identity
    pub fn with_key_column(key: impl Into<String>) -> Self {
        Self {
            row_identity: RowIdentityMode::KeyBased,
            key_column: Some(key.into()),
            ..Default::default()
        }
    }

    /// Create options for content-addressed diff
    #[must_use]
    pub fn content_addressed() -> Self {
        Self {
            row_identity: RowIdentityMode::ContentAddressed,
            ..Default::default()
        }
    }
}

/// How rows are identified for diff comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowIdentityMode {
    /// Compare rows by position (row 0 vs row 0)
    Positional,
    /// Compare rows by key column value
    KeyBased,
    /// Compare rows by content hash
    ContentAddressed,
}

/// A single CSV diff operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvDiffOp {
    /// Row added at position
    AddRow {
        /// Row index where the row was added
        position: usize,
        /// Column values for the new row
        values: HashMap<String, String>,
    },
    /// Row removed from position
    RemoveRow {
        /// Row index where the row was removed
        position: usize,
        /// Key column value (if key-based identity)
        key: Option<String>,
    },
    /// Row modified
    ModifyRow {
        /// Row index of the modified row
        position: usize,
        /// Key column value (if key-based identity)
        key: Option<String>,
        /// List of cell changes in this row
        changes: Vec<CellChange>,
    },
    /// Row moved (content-addressed only)
    MoveRow {
        /// Original row index
        from_position: usize,
        /// New row index
        to_position: usize,
        /// Key column value (if key-based identity)
        key: Option<String>,
    },
    /// Column added
    AddColumn {
        /// Name of the new column
        name: String,
        /// Column index
        position: usize,
    },
    /// Column removed
    RemoveColumn {
        /// Name of the removed column
        name: String,
        /// Column index
        position: usize,
    },
    /// Column renamed
    RenameColumn {
        /// Original column name
        old_name: String,
        /// New column name
        new_name: String,
        /// Column index
        position: usize,
    },
}

/// Change to a single cell
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChange {
    /// Column name
    pub column: String,
    /// Old value (None if column was added)
    pub old_value: Option<String>,
    /// New value (None if column was removed)
    pub new_value: Option<String>,
}

/// Result of CSV diff
#[derive(Debug, Clone)]
pub struct CsvDiff {
    /// Operations to transform source into target
    pub operations: Vec<CsvDiffOp>,
    /// Statistics about the diff
    pub stats: CsvDiffStats,
}

/// Statistics about a CSV diff
#[derive(Debug, Clone, Default)]
pub struct CsvDiffStats {
    /// Number of rows added
    pub rows_added: usize,
    /// Number of rows removed
    pub rows_removed: usize,
    /// Number of rows modified
    pub rows_modified: usize,
    /// Number of rows unchanged
    pub rows_unchanged: usize,
    /// Number of cells changed
    pub cells_changed: usize,
    /// Number of columns added
    pub columns_added: usize,
    /// Number of columns removed
    pub columns_removed: usize,
}

impl CsvDiff {
    /// Check if the diff is empty (no changes)
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get number of operations
    #[must_use]
    pub const fn len(&self) -> usize {
        self.operations.len()
    }
}

/// Compute diff between two CSV representations
///
/// Both source and target should be JSON values in the CSV gron format:
/// `{"csv": {"rows": [{"col1": "val1", ...}, ...]}}`
#[must_use]
pub fn csv_diff(source: &Value, target: &Value, options: &CsvDiffOptions) -> CsvDiff {
    let source_rows = extract_csv_rows(source);
    let target_rows = extract_csv_rows(target);

    match options.row_identity {
        RowIdentityMode::Positional => diff_positional(&source_rows, &target_rows, options),
        RowIdentityMode::KeyBased => diff_key_based(&source_rows, &target_rows, options),
        RowIdentityMode::ContentAddressed => {
            diff_content_addressed(&source_rows, &target_rows, options)
        }
    }
}

/// Compute diff between two `TapeSource` CSVs
pub fn csv_diff_tapes<S: TapeSource, T: TapeSource>(
    source: &S,
    target: &T,
    options: &CsvDiffOptions,
) -> CsvDiff {
    // Convert tapes to values for diff
    // This is the bridge between tape and diff semantics
    let source_value = tape_to_csv_value(source);
    let target_value = tape_to_csv_value(target);

    csv_diff(&source_value, &target_value, options)
}

// =============================================================================
// Internal Implementation
// =============================================================================

fn extract_csv_rows(value: &Value) -> Vec<HashMap<String, String>> {
    let mut rows = Vec::new();

    // Handle csv.rows[N].col format
    if let Some(csv) = value.get("csv") {
        if let Some(rows_arr) = csv.get("rows").and_then(|r| r.as_array()) {
            for row in rows_arr {
                if let Some(obj) = row.as_object() {
                    let row_map: HashMap<String, String> = obj
                        .iter()
                        .map(|(k, v)| {
                            let val = match v {
                                Value::String(s) => s.clone(),
                                Value::Null => String::new(),
                                other => other.to_string(),
                            };
                            (k.clone(), val)
                        })
                        .collect();
                    rows.push(row_map);
                }
            }
        }
    }
    // Handle direct array of objects
    else if let Some(arr) = value.as_array() {
        for row in arr {
            if let Some(obj) = row.as_object() {
                let row_map: HashMap<String, String> = obj
                    .iter()
                    .map(|(k, v)| {
                        let val = match v {
                            Value::String(s) => s.clone(),
                            Value::Null => String::new(),
                            other => other.to_string(),
                        };
                        (k.clone(), val)
                    })
                    .collect();
                rows.push(row_map);
            }
        }
    }

    rows
}

fn tape_to_csv_value<T: TapeSource>(tape: &T) -> Value {
    // Extract CSV structure from tape
    // This handles the csv.rows[N].col format
    let mut rows = Vec::new();

    // Walk tape looking for row structures
    let mut i = 0;
    while i < tape.len() {
        if let Some(node) = tape.node_at(i) {
            if let TapeNodeKind::ObjectStart { count } = node.kind {
                let mut row = Map::new();
                let obj_end = i + count * 2 + 1; // count key-value pairs + object node
                let mut field_idx = i + 1;

                while field_idx < obj_end && field_idx < tape.len() {
                    if let Some(key) = tape.key_at(field_idx) {
                        field_idx += 1; // Move past key
                        if let Some(value) = tape.value_at(field_idx) {
                            let json_val = match value {
                                TapeValue::String(s) => Value::String(s.to_string()),
                                TapeValue::Int(n) => Value::Number(n.into()),
                                TapeValue::Float(f) => serde_json::Number::from_f64(f)
                                    .map_or(Value::Null, Value::Number),
                                TapeValue::Bool(b) => Value::Bool(b),
                                TapeValue::Null => Value::Null,
                                TapeValue::RawNumber(s) => s
                                    .parse::<i64>()
                                    .map(|n| Value::Number(n.into()))
                                    .or_else(|_| {
                                        s.parse::<f64>()
                                            .ok()
                                            .and_then(serde_json::Number::from_f64)
                                            .map(Value::Number)
                                            .ok_or(())
                                    })
                                    .unwrap_or_else(|()| Value::String(s.to_string())),
                            };
                            row.insert(key.to_string(), json_val);
                        }
                        if let Ok(skip) = tape.skip_value(field_idx) {
                            field_idx = skip;
                        } else {
                            field_idx += 1;
                        }
                    } else {
                        field_idx += 1;
                    }
                }

                if !row.is_empty() {
                    rows.push(Value::Object(row));
                }
                if let Ok(skip) = tape.skip_value(i) {
                    i = skip;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    Value::Array(rows)
}

fn diff_positional(
    source: &[HashMap<String, String>],
    target: &[HashMap<String, String>],
    options: &CsvDiffOptions,
) -> CsvDiff {
    let mut operations = Vec::new();
    let mut stats = CsvDiffStats::default();

    let max_len = source.len().max(target.len());

    for i in 0..max_len {
        match (source.get(i), target.get(i)) {
            (Some(src_row), Some(tgt_row)) => {
                let changes = diff_row(src_row, tgt_row, options);
                if changes.is_empty() {
                    stats.rows_unchanged += 1;
                } else {
                    stats.rows_modified += 1;
                    stats.cells_changed += changes.len();
                    operations.push(CsvDiffOp::ModifyRow {
                        position: i,
                        key: options
                            .key_column
                            .as_ref()
                            .and_then(|k| src_row.get(k).cloned()),
                        changes,
                    });
                }
            }
            (Some(_), None) => {
                stats.rows_removed += 1;
                operations.push(CsvDiffOp::RemoveRow {
                    position: i,
                    key: options
                        .key_column
                        .as_ref()
                        .and_then(|k| source[i].get(k).cloned()),
                });
            }
            (None, Some(tgt_row)) => {
                stats.rows_added += 1;
                operations.push(CsvDiffOp::AddRow {
                    position: i,
                    values: tgt_row.clone(),
                });
            }
            (None, None) => unreachable!(),
        }
    }

    CsvDiff { operations, stats }
}

fn diff_key_based(
    source: &[HashMap<String, String>],
    target: &[HashMap<String, String>],
    options: &CsvDiffOptions,
) -> CsvDiff {
    let Some(key_col) = &options.key_column else {
        return diff_positional(source, target, options);
    };

    let mut operations = Vec::new();
    let mut stats = CsvDiffStats::default();

    // Index source rows by key
    let source_by_key: HashMap<&str, (usize, &HashMap<String, String>)> = source
        .iter()
        .enumerate()
        .filter_map(|(i, row)| row.get(key_col).map(|k| (k.as_str(), (i, row))))
        .collect();

    // Index target rows by key
    let target_by_key: HashMap<&str, (usize, &HashMap<String, String>)> = target
        .iter()
        .enumerate()
        .filter_map(|(i, row)| row.get(key_col).map(|k| (k.as_str(), (i, row))))
        .collect();

    // Find removed rows
    for (key, (pos, _)) in &source_by_key {
        if !target_by_key.contains_key(*key) {
            stats.rows_removed += 1;
            operations.push(CsvDiffOp::RemoveRow {
                position: *pos,
                key: Some((*key).to_string()),
            });
        }
    }

    // Find added and modified rows
    for (key, (pos, tgt_row)) in &target_by_key {
        if let Some((_, src_row)) = source_by_key.get(*key) {
            let changes = diff_row(src_row, tgt_row, options);
            if changes.is_empty() {
                stats.rows_unchanged += 1;
            } else {
                stats.rows_modified += 1;
                stats.cells_changed += changes.len();
                operations.push(CsvDiffOp::ModifyRow {
                    position: *pos,
                    key: Some((*key).to_string()),
                    changes,
                });
            }
        } else {
            stats.rows_added += 1;
            operations.push(CsvDiffOp::AddRow {
                position: *pos,
                values: (*tgt_row).clone(),
            });
        }
    }

    CsvDiff { operations, stats }
}

fn diff_content_addressed(
    source: &[HashMap<String, String>],
    target: &[HashMap<String, String>],
    options: &CsvDiffOptions,
) -> CsvDiff {
    let mut operations = Vec::new();
    let mut stats = CsvDiffStats::default();

    // Hash rows for content addressing
    let source_hashes: HashMap<u64, (usize, &HashMap<String, String>)> = source
        .iter()
        .enumerate()
        .map(|(i, row)| (hash_row(row, options), (i, row)))
        .collect();

    let target_hashes: HashMap<u64, (usize, &HashMap<String, String>)> = target
        .iter()
        .enumerate()
        .map(|(i, row)| (hash_row(row, options), (i, row)))
        .collect();

    // Find moved rows (same content, different position)
    let mut matched_source: HashSet<usize> = HashSet::new();
    let mut matched_target: HashSet<usize> = HashSet::new();

    for (hash, (tgt_pos, _)) in &target_hashes {
        if let Some((src_pos, _)) = source_hashes.get(hash) {
            if *src_pos == *tgt_pos {
                stats.rows_unchanged += 1;
            } else {
                operations.push(CsvDiffOp::MoveRow {
                    from_position: *src_pos,
                    to_position: *tgt_pos,
                    key: options
                        .key_column
                        .as_ref()
                        .and_then(|k| source[*src_pos].get(k).cloned()),
                });
            }
            matched_source.insert(*src_pos);
            matched_target.insert(*tgt_pos);
        }
    }

    // Find removed rows (in source but not in target)
    for (i, _) in source.iter().enumerate() {
        if !matched_source.contains(&i) {
            stats.rows_removed += 1;
            operations.push(CsvDiffOp::RemoveRow {
                position: i,
                key: options
                    .key_column
                    .as_ref()
                    .and_then(|k| source[i].get(k).cloned()),
            });
        }
    }

    // Find added rows (in target but not in source)
    for (i, row) in target.iter().enumerate() {
        if !matched_target.contains(&i) {
            stats.rows_added += 1;
            operations.push(CsvDiffOp::AddRow {
                position: i,
                values: row.clone(),
            });
        }
    }

    CsvDiff { operations, stats }
}

fn diff_row(
    source: &HashMap<String, String>,
    target: &HashMap<String, String>,
    options: &CsvDiffOptions,
) -> Vec<CellChange> {
    let mut changes = Vec::new();

    // All columns from both rows
    let all_cols: HashSet<&str> = source
        .keys()
        .chain(target.keys())
        .map(String::as_str)
        .collect();

    for col in all_cols {
        if options.ignore_columns.contains(col) {
            continue;
        }

        let src_val = source.get(col).map(String::as_str);
        let tgt_val = target.get(col).map(String::as_str);

        // Normalize empty strings if option set
        let src_normalized = if options.empty_is_null {
            src_val.filter(|s| !s.is_empty())
        } else {
            src_val
        };
        let tgt_normalized = if options.empty_is_null {
            tgt_val.filter(|s| !s.is_empty())
        } else {
            tgt_val
        };

        if src_normalized != tgt_normalized {
            changes.push(CellChange {
                column: col.to_string(),
                old_value: src_val.map(std::borrow::ToOwned::to_owned),
                new_value: tgt_val.map(std::borrow::ToOwned::to_owned),
            });
        }
    }

    changes
}

fn hash_row(row: &HashMap<String, String>, options: &CsvDiffOptions) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();

    // Sort keys for consistent hashing
    let mut keys: Vec<&str> = row
        .keys()
        .filter(|k| !options.ignore_columns.contains(*k))
        .map(String::as_str)
        .collect();
    keys.sort_unstable();

    for key in keys {
        key.hash(&mut hasher);
        if let Some(val) = row.get(key) {
            let normalized = if options.empty_is_null && val.is_empty() {
                ""
            } else {
                val.as_str()
            };
            normalized.hash(&mut hasher);
        }
    }

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_csv_diff_positional_identical() {
        let data = json!({
            "csv": {
                "rows": [
                    {"id": "1", "name": "Alice"},
                    {"id": "2", "name": "Bob"}
                ]
            }
        });

        let diff = csv_diff(&data, &data, &CsvDiffOptions::default());
        assert!(diff.is_empty());
        assert_eq!(diff.stats.rows_unchanged, 2);
    }

    #[test]
    fn test_csv_diff_positional_modified() {
        let source = json!({
            "csv": {
                "rows": [
                    {"id": "1", "name": "Alice"},
                    {"id": "2", "name": "Bob"}
                ]
            }
        });
        let target = json!({
            "csv": {
                "rows": [
                    {"id": "1", "name": "Alice"},
                    {"id": "2", "name": "Robert"}
                ]
            }
        });

        let diff = csv_diff(&source, &target, &CsvDiffOptions::default());
        assert_eq!(diff.stats.rows_unchanged, 1);
        assert_eq!(diff.stats.rows_modified, 1);
        assert_eq!(diff.stats.cells_changed, 1);
    }

    #[test]
    fn test_csv_diff_key_based() {
        let source = json!([
            {"id": "1", "name": "Alice"},
            {"id": "2", "name": "Bob"}
        ]);
        let target = json!([
            {"id": "2", "name": "Bob"},
            {"id": "3", "name": "Charlie"}
        ]);

        let options = CsvDiffOptions::with_key_column("id");
        let diff = csv_diff(&source, &target, &options);

        assert_eq!(diff.stats.rows_removed, 1); // id=1 removed
        assert_eq!(diff.stats.rows_added, 1); // id=3 added
        assert_eq!(diff.stats.rows_unchanged, 1); // id=2 unchanged
    }

    #[test]
    fn test_csv_diff_content_addressed_move() {
        let source = json!([
            {"id": "1", "name": "Alice"},
            {"id": "2", "name": "Bob"}
        ]);
        let target = json!([
            {"id": "2", "name": "Bob"},
            {"id": "1", "name": "Alice"}
        ]);

        let options = CsvDiffOptions::content_addressed();
        let diff = csv_diff(&source, &target, &options);

        // Both rows moved
        let move_count = diff
            .operations
            .iter()
            .filter(|op| matches!(op, CsvDiffOp::MoveRow { .. }))
            .count();
        assert!(move_count >= 1);
    }

    #[test]
    fn test_csv_diff_empty_is_null() {
        let source = json!([{"id": "1", "name": ""}]);
        let target = json!([{"id": "1"}]);

        // Without empty_is_null, these are different
        let diff1 = csv_diff(&source, &target, &CsvDiffOptions::default());
        assert!(!diff1.is_empty());

        // With empty_is_null, these are the same
        let options = CsvDiffOptions {
            empty_is_null: true,
            ..Default::default()
        };
        let diff2 = csv_diff(&source, &target, &options);
        assert!(diff2.is_empty());
    }

    #[test]
    fn test_csv_diff_ignore_columns() {
        let source = json!([{"id": "1", "name": "Alice", "updated": "2024-01-01"}]);
        let target = json!([{"id": "1", "name": "Alice", "updated": "2024-01-02"}]);

        let mut options = CsvDiffOptions::default();
        options.ignore_columns.insert("updated".to_string());

        let diff = csv_diff(&source, &target, &options);
        assert!(diff.is_empty());
    }
}
