// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tape-to-tape transformation engine
//!
//! This module provides zero-allocation transformations between format tapes,
//! enabling efficient cross-format conversion without intermediate DOM structures.
//!
//! # Architecture
//!
//! The transformation engine operates on a unified tape representation that can
//! hold any format's structural information. Transformations are performed by:
//!
//! 1. Parsing source format into unified tape
//! 2. Applying schema filters (optional)
//! 3. Emitting target format from tape
//!
//! # Fidelity Modes
//!
//! - **Lossless**: Full round-trip preservation
//! - **Semantic**: Data-equivalent transformation
//! - **Lossy**: Comments/references may be dropped

pub mod emitter;
pub mod metrics;
pub mod tape;
mod tape_source_impl;

pub use emitter::{Emitter, JsonEmitter};
pub use metrics::{MemoryMetrics, TransformMetrics};
pub use tape::{TapeNode, TapeValue, UnifiedTape};
pub use tape_source_impl::{UnifiedArrayElementIterator, UnifiedObjectFieldIterator};

#[cfg(feature = "yaml")]
pub use emitter::YamlEmitter;

#[cfg(feature = "toml")]
pub use emitter::TomlEmitter;

#[cfg(feature = "csv")]
pub use emitter::CsvEmitter;

#[cfg(feature = "ison")]
pub use emitter::IsonEmitter;

#[cfg(feature = "toon")]
pub use emitter::ToonEmitter;

use fionn_core::format::FormatKind;

/// Transformation fidelity modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformFidelity {
    /// Strict lossless transformation - error on any data loss
    Strict,
    /// Semantic equivalence - data values preserved, syntax may change
    #[default]
    Semantic,
    /// Lossy transformation - comments/refs dropped silently
    Lossy,
}

/// Transformation options
#[derive(Debug, Clone, Default)]
pub struct TransformOptions {
    /// Fidelity mode
    pub fidelity: TransformFidelity,
    /// Pretty print output
    pub pretty: bool,
    /// Indent string for pretty printing
    pub indent: String,
    /// Expand references inline
    pub expand_refs: bool,
    /// Preserve comments where possible
    pub preserve_comments: bool,
}

impl TransformOptions {
    /// Create new options with defaults
    #[must_use]
    pub fn new() -> Self {
        Self {
            indent: "  ".to_string(),
            ..Default::default()
        }
    }

    /// Set fidelity mode
    #[must_use]
    pub const fn with_fidelity(mut self, fidelity: TransformFidelity) -> Self {
        self.fidelity = fidelity;
        self
    }

    /// Enable pretty printing
    #[must_use]
    pub const fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    /// Set indent string
    #[must_use]
    pub fn with_indent(mut self, indent: impl Into<String>) -> Self {
        self.indent = indent.into();
        self
    }
}

/// Transform error types
#[derive(Debug, Clone)]
pub enum TransformError {
    /// Source format parsing error
    ParseError {
        /// The format that failed to parse
        format: FormatKind,
        /// The error message
        message: String,
    },
    /// Information loss in strict mode
    InformationLoss {
        /// Path where information was lost
        path: String,
        /// Description of the lost element
        lost_element: String,
        /// Source format
        source_format: FormatKind,
        /// Target format
        target_format: FormatKind,
    },
    /// Unsupported transformation
    UnsupportedTransformation {
        /// Source format
        source: FormatKind,
        /// Target format
        target: FormatKind,
        /// Reason the transformation is unsupported
        reason: String,
    },
    /// Invalid input
    InvalidInput {
        /// The error message
        message: String,
    },
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError { format, message } => {
                write!(f, "Parse error in {format}: {message}")
            }
            Self::InformationLoss {
                path,
                lost_element,
                source_format,
                target_format,
            } => {
                write!(
                    f,
                    "Information loss at '{path}': {lost_element} cannot be represented when converting {source_format} to {target_format}"
                )
            }
            Self::UnsupportedTransformation {
                source,
                target,
                reason,
            } => {
                write!(f, "Cannot transform {source} to {target}: {reason}")
            }
            Self::InvalidInput { message } => {
                write!(f, "Invalid input: {message}")
            }
        }
    }
}

impl std::error::Error for TransformError {}

/// Result type for transformations
pub type TransformResult<T> = Result<T, TransformError>;

/// Main transformation function
///
/// Transforms input from source format to target format.
///
/// # Arguments
///
/// * `input` - Source data as bytes
/// * `source_format` - Format of the input
/// * `target_format` - Desired output format
/// * `options` - Transformation options
///
/// # Returns
///
/// Transformed data and metrics
///
/// # Errors
///
/// Returns [`TransformError`] if parsing fails or the transformation is unsupported.
pub fn transform(
    input: &[u8],
    source_format: FormatKind,
    target_format: FormatKind,
    options: &TransformOptions,
) -> TransformResult<(Vec<u8>, TransformMetrics)> {
    let mut metrics = TransformMetrics::new(source_format, target_format);
    metrics.start();

    // Parse source into unified tape
    let tape = UnifiedTape::parse(input, source_format)?;
    metrics.record_parse(input.len());

    // Same format optimization - minimal transformation
    if source_format == target_format {
        metrics.finish();
        return Ok((input.to_vec(), metrics));
    }

    // Create appropriate emitter and emit
    let output = match target_format {
        FormatKind::Json => {
            let emitter = JsonEmitter::new(options);
            emitter.emit(&tape)?
        }
        #[cfg(feature = "yaml")]
        FormatKind::Yaml => {
            let emitter = YamlEmitter::new(options);
            emitter.emit(&tape)?
        }
        #[cfg(feature = "toml")]
        FormatKind::Toml => {
            let emitter = TomlEmitter::new(options);
            emitter.emit(&tape)?
        }
        #[cfg(feature = "csv")]
        FormatKind::Csv => {
            let emitter = CsvEmitter::new(options);
            emitter.emit(&tape)?
        }
        #[cfg(feature = "ison")]
        FormatKind::Ison => {
            let emitter = IsonEmitter::new(options);
            emitter.emit(&tape)?
        }
        #[cfg(feature = "toon")]
        FormatKind::Toon => {
            let emitter = ToonEmitter::new(options);
            emitter.emit(&tape)?
        }
    };

    metrics.record_emit(output.len());
    metrics.finish();

    Ok((output, metrics))
}

/// Transform with pre-allocated output buffer for zero-allocation
///
/// # Errors
///
/// Returns [`TransformError`] if parsing fails or the transformation is unsupported.
pub fn transform_into(
    input: &[u8],
    source_format: FormatKind,
    target_format: FormatKind,
    options: &TransformOptions,
    output: &mut Vec<u8>,
) -> TransformResult<TransformMetrics> {
    let mut metrics = TransformMetrics::new(source_format, target_format);
    metrics.start();

    let tape = UnifiedTape::parse(input, source_format)?;
    metrics.record_parse(input.len());

    output.clear();

    match target_format {
        FormatKind::Json => {
            let emitter = JsonEmitter::new(options);
            emitter.emit_into(&tape, output)?;
        }
        #[cfg(feature = "yaml")]
        FormatKind::Yaml => {
            let emitter = YamlEmitter::new(options);
            emitter.emit_into(&tape, output)?;
        }
        #[cfg(feature = "toml")]
        FormatKind::Toml => {
            let emitter = TomlEmitter::new(options);
            emitter.emit_into(&tape, output)?;
        }
        #[cfg(feature = "csv")]
        FormatKind::Csv => {
            let emitter = CsvEmitter::new(options);
            emitter.emit_into(&tape, output)?;
        }
        #[cfg(feature = "ison")]
        FormatKind::Ison => {
            let emitter = IsonEmitter::new(options);
            emitter.emit_into(&tape, output)?;
        }
        #[cfg(feature = "toon")]
        FormatKind::Toon => {
            let emitter = ToonEmitter::new(options);
            emitter.emit_into(&tape, output)?;
        }
    }

    metrics.record_emit(output.len());
    metrics.finish();

    Ok(metrics)
}
