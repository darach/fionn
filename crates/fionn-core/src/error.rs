// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error types for fionn operations

use thiserror::Error;

/// Errors that can occur during DSON processing
#[derive(Error, Debug)]
pub enum DsonError {
    /// JSON parsing error
    #[error("JSON parse error: {0}")]
    ParseError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid field access
    #[error("Invalid field access: {0}")]
    InvalidField(String),

    /// Schema validation error
    #[error("Schema validation error: {0}")]
    SchemaError(String),

    /// CRDT merge conflict
    #[error("CRDT merge conflict: {0}")]
    MergeConflict(String),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Serde serialization/deserialization error
    #[error("Serde error: {0}")]
    SerdeError(String),

    /// JSON serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// SIMD-JSONL processing error
    #[error("SIMD-JSONL error: {0}")]
    SimdJsonlError(String),

    /// Skip tape processing error
    #[error("Skip tape error: {0}")]
    SkipTapeError(String),

    /// Memory allocation error
    #[error("Memory error: {0}")]
    MemoryError(String),

    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Result type alias for DSON operations
pub type Result<T> = std::result::Result<T, DsonError>;

impl From<DsonError> for String {
    fn from(error: DsonError) -> Self {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error() {
        let error = DsonError::ParseError("test".to_string());
        assert!(error.to_string().contains("test"));
    }

    #[test]
    fn test_invalid_field() {
        let error = DsonError::InvalidField("field".to_string());
        assert!(error.to_string().contains("field"));
    }

    #[test]
    fn test_schema_error() {
        let error = DsonError::SchemaError("schema".to_string());
        assert!(error.to_string().contains("schema"));
    }

    #[test]
    fn test_merge_conflict() {
        let error = DsonError::MergeConflict("conflict".to_string());
        assert!(error.to_string().contains("conflict"));
    }

    #[test]
    fn test_invalid_operation() {
        let error = DsonError::InvalidOperation("op".to_string());
        assert!(error.to_string().contains("op"));
    }

    #[test]
    fn test_serde_error() {
        let error = DsonError::SerdeError("serde".to_string());
        assert!(error.to_string().contains("serde"));
    }

    #[test]
    fn test_serialization_error() {
        let error = DsonError::SerializationError("ser".to_string());
        assert!(error.to_string().contains("ser"));
    }

    #[test]
    fn test_simd_jsonl_error() {
        let error = DsonError::SimdJsonlError("simd".to_string());
        assert!(error.to_string().contains("simd"));
    }

    #[test]
    fn test_error_to_string_conversion() {
        let error = DsonError::ParseError("test".to_string());
        let s: String = error.into();
        assert!(s.contains("test"));
    }

    #[test]
    fn test_from_io_error() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let dson_error: DsonError = io_error.into();
        assert!(matches!(dson_error, DsonError::IoError(_)));
        assert!(dson_error.to_string().contains("I/O error"));
    }

    #[test]
    fn test_skiptape_error_variant() {
        let error = DsonError::SkipTapeError("skip".to_string());
        assert!(error.to_string().contains("skip"));
    }

    #[test]
    fn test_memory_error_variant() {
        let error = DsonError::MemoryError("mem".to_string());
        assert!(error.to_string().contains("mem"));
    }

    #[test]
    fn test_validation_error_variant() {
        let error = DsonError::ValidationError("val".to_string());
        assert!(error.to_string().contains("val"));
    }

    #[test]
    fn test_io_error_display() {
        let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let dson_error: DsonError = io_error.into();
        let s = dson_error.to_string();
        assert!(s.contains("access denied") || s.contains("I/O"));
    }

    #[test]
    fn test_all_error_variants_debug() {
        let errors: Vec<DsonError> = vec![
            DsonError::ParseError("p".to_string()),
            DsonError::InvalidField("f".to_string()),
            DsonError::SchemaError("s".to_string()),
            DsonError::MergeConflict("m".to_string()),
            DsonError::InvalidOperation("o".to_string()),
            DsonError::SerdeError("d".to_string()),
            DsonError::SerializationError("r".to_string()),
            DsonError::SimdJsonlError("j".to_string()),
            DsonError::SkipTapeError("k".to_string()),
            DsonError::MemoryError("m".to_string()),
            DsonError::ValidationError("v".to_string()),
        ];
        for error in errors {
            let debug = format!("{error:?}");
            assert!(!debug.is_empty());
        }
    }
}
