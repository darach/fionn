// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error types for SIMD-JSONL Skip Tape processing

use std::fmt;

/// Errors that can occur during skip tape processing
#[derive(Debug, Clone)]
pub enum SkipTapeError {
    /// JSON parsing error
    ParseError(String),
    /// Schema compilation error
    SchemaError(String),
    /// Memory allocation error
    MemoryError(String),
    /// SIMD processing error
    SimdError(String),
    /// I/O error during batch processing
    IoError(String),
    /// Schema validation error
    ValidationError(String),
}

impl fmt::Display for SkipTapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::SchemaError(msg) => write!(f, "Schema error: {msg}"),
            Self::MemoryError(msg) => write!(f, "Memory error: {msg}"),
            Self::SimdError(msg) => write!(f, "SIMD error: {msg}"),
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
        }
    }
}

impl std::error::Error for SkipTapeError {}

impl From<SkipTapeError> for fionn_core::DsonError {
    fn from(error: SkipTapeError) -> Self {
        Self::SkipTapeError(error.to_string())
    }
}

impl From<fionn_core::DsonError> for SkipTapeError {
    fn from(error: fionn_core::DsonError) -> Self {
        Self::ParseError(error.to_string())
    }
}

/// Result type alias for skip tape operations
pub type Result<T> = std::result::Result<T, SkipTapeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error() {
        let error = SkipTapeError::ParseError("test".to_string());
        assert!(error.to_string().contains("test"));
    }

    #[test]
    fn test_schema_error() {
        let error = SkipTapeError::SchemaError("schema".to_string());
        assert!(error.to_string().contains("schema"));
    }

    #[test]
    fn test_memory_error() {
        let error = SkipTapeError::MemoryError("mem".to_string());
        assert!(error.to_string().contains("mem"));
    }

    #[test]
    fn test_simd_error() {
        let error = SkipTapeError::SimdError("simd".to_string());
        assert!(error.to_string().contains("simd"));
    }

    #[test]
    fn test_io_error() {
        let error = SkipTapeError::IoError("io".to_string());
        assert!(error.to_string().contains("io"));
    }

    #[test]
    fn test_validation_error() {
        let error = SkipTapeError::ValidationError("val".to_string());
        assert!(error.to_string().contains("val"));
    }

    #[test]
    fn test_error_clone() {
        let error = SkipTapeError::ParseError("clone".to_string());
        let cloned = error.clone();
        assert_eq!(error.to_string(), cloned.to_string());
    }

    #[test]
    fn test_error_debug() {
        let error = SkipTapeError::ParseError("debug".to_string());
        let debug = format!("{error:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_from_dson_error() {
        let dson_error = fionn_core::DsonError::ParseError("dson parse".to_string());
        let skip_error: SkipTapeError = dson_error.into();
        assert!(matches!(skip_error, SkipTapeError::ParseError(_)));
        assert!(skip_error.to_string().contains("Parse error"));
    }

    #[test]
    fn test_error_trait_impl() {
        // Test that SkipTapeError implements std::error::Error
        let error: Box<dyn std::error::Error> =
            Box::new(SkipTapeError::ParseError("error trait".to_string()));
        assert!(error.to_string().contains("error trait"));
    }

    #[test]
    fn test_all_variants_clone() {
        let errors = vec![
            SkipTapeError::ParseError("p".to_string()),
            SkipTapeError::SchemaError("s".to_string()),
            SkipTapeError::MemoryError("m".to_string()),
            SkipTapeError::SimdError("i".to_string()),
            SkipTapeError::IoError("o".to_string()),
            SkipTapeError::ValidationError("v".to_string()),
        ];
        for error in errors {
            let cloned = error.clone();
            assert_eq!(error.to_string(), cloned.to_string());
        }
    }
}
