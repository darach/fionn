// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format emitters for tape-to-output transformation
//!
//! Each emitter converts a unified tape into a specific output format.

use super::tape::{TapeNode, TapeValue, UnifiedTape};
use super::{TransformOptions, TransformResult};
use std::io::Write;

/// Trait for format emitters
pub trait Emitter {
    /// Emit tape to new Vec
    ///
    /// # Errors
    ///
    /// Returns an error if the tape cannot be emitted to the target format.
    fn emit(&self, tape: &UnifiedTape<'_>) -> TransformResult<Vec<u8>>;

    /// Emit tape into existing buffer
    ///
    /// # Errors
    ///
    /// Returns an error if the tape cannot be emitted to the target format.
    fn emit_into(&self, tape: &UnifiedTape<'_>, output: &mut Vec<u8>) -> TransformResult<()>;
}

// =============================================================================
// JSON Emitter
// =============================================================================

/// JSON format emitter
pub struct JsonEmitter<'a> {
    options: &'a TransformOptions,
}

impl<'a> JsonEmitter<'a> {
    /// Create new JSON emitter
    #[must_use]
    pub const fn new(options: &'a TransformOptions) -> Self {
        Self { options }
    }

    #[allow(clippy::unused_self)] // Method signature for API consistency; may use self.options in future
    fn emit_value(&self, value: &TapeValue<'_>, output: &mut Vec<u8>) {
        match value {
            TapeValue::Null => output.extend_from_slice(b"null"),
            TapeValue::Bool(true) => output.extend_from_slice(b"true"),
            TapeValue::Bool(false) => output.extend_from_slice(b"false"),
            TapeValue::Int(i) => {
                let _ = write!(output, "{i}");
            }
            TapeValue::Float(f) => {
                if f.is_finite() {
                    let _ = write!(output, "{f}");
                } else {
                    output.extend_from_slice(b"null");
                }
            }
            TapeValue::RawNumber(s) => output.extend_from_slice(s.as_bytes()),
            TapeValue::String(s) => {
                output.push(b'"');
                escape_json_string(s, output);
                output.push(b'"');
            }
        }
    }

    fn emit_indent(&self, output: &mut Vec<u8>, depth: usize) {
        if self.options.pretty {
            output.push(b'\n');
            for _ in 0..depth {
                output.extend_from_slice(self.options.indent.as_bytes());
            }
        }
    }
}

impl Emitter for JsonEmitter<'_> {
    fn emit(&self, tape: &UnifiedTape<'_>) -> TransformResult<Vec<u8>> {
        let mut output =
            Vec::with_capacity(tape.stats.string_bytes * 2 + tape.stats.node_count * 4);
        self.emit_into(tape, &mut output)?;
        Ok(output)
    }

    #[allow(clippy::too_many_lines)] // Format emission handles many node types inline for performance
    fn emit_into(&self, tape: &UnifiedTape<'_>, output: &mut Vec<u8>) -> TransformResult<()> {
        let mut depth = 0usize;
        let mut first_in_container: Vec<bool> = vec![true];
        let mut expect_value = false;
        // Track current tabular field names for object emission
        let mut current_fields: Option<Vec<&str>> = None;

        for node in &tape.nodes {
            match node {
                TapeNode::ObjectStart { .. } => {
                    if !first_in_container.last().copied().unwrap_or(true) && !expect_value {
                        output.push(b',');
                    }
                    if self.options.pretty && depth > 0 && !expect_value {
                        self.emit_indent(output, depth);
                    }
                    output.push(b'{');
                    depth += 1;
                    first_in_container.push(true);
                    expect_value = false;
                }
                TapeNode::ObjectEnd => {
                    depth -= 1;
                    if self.options.pretty && !first_in_container.last().copied().unwrap_or(true) {
                        self.emit_indent(output, depth);
                    }
                    output.push(b'}');
                    first_in_container.pop();
                    if let Some(first) = first_in_container.last_mut() {
                        *first = false;
                    }
                }
                TapeNode::ArrayStart { .. } => {
                    if !first_in_container.last().copied().unwrap_or(true) && !expect_value {
                        output.push(b',');
                    }
                    if self.options.pretty && depth > 0 && !expect_value {
                        self.emit_indent(output, depth);
                    }
                    output.push(b'[');
                    depth += 1;
                    first_in_container.push(true);
                    expect_value = false;
                }
                TapeNode::ArrayEnd => {
                    depth -= 1;
                    if self.options.pretty && !first_in_container.last().copied().unwrap_or(true) {
                        self.emit_indent(output, depth);
                    }
                    output.push(b']');
                    first_in_container.pop();
                    if let Some(first) = first_in_container.last_mut() {
                        *first = false;
                    }
                    // Clear field names when leaving tabular array
                    current_fields = None;
                }
                TapeNode::Key(key) => {
                    if !first_in_container.last().copied().unwrap_or(true) {
                        output.push(b',');
                    }
                    if self.options.pretty {
                        self.emit_indent(output, depth);
                    }
                    output.push(b'"');
                    escape_json_string(key, output);
                    output.push(b'"');
                    output.push(b':');
                    if self.options.pretty {
                        output.push(b' ');
                    }
                    expect_value = true;
                    if let Some(first) = first_in_container.last_mut() {
                        *first = false;
                    }
                }
                TapeNode::Value(value) => {
                    if !expect_value && !first_in_container.last().copied().unwrap_or(true) {
                        output.push(b',');
                    }
                    if !expect_value && self.options.pretty {
                        self.emit_indent(output, depth);
                    }
                    self.emit_value(value, output);
                    expect_value = false;
                    if let Some(first) = first_in_container.last_mut() {
                        *first = false;
                    }
                }
                TapeNode::TabularHeader { fields, .. } => {
                    // Store field names for subsequent TabularRow emissions
                    current_fields = Some(fields.iter().map(AsRef::as_ref).collect());
                }
                TapeNode::TabularRow { values } => {
                    // Emit row - as object if we have field names, array otherwise
                    if !first_in_container.last().copied().unwrap_or(true) {
                        output.push(b',');
                    }
                    if self.options.pretty {
                        self.emit_indent(output, depth);
                    }

                    if let Some(ref fields) = current_fields {
                        // Emit as object with field names
                        output.push(b'{');
                        for (i, val) in values.iter().enumerate() {
                            if i > 0 {
                                output.push(b',');
                            }
                            // Use field name if available, fallback to index
                            let field_name = fields.get(i).copied().unwrap_or("_");
                            output.push(b'"');
                            escape_json_string(field_name, output);
                            output.push(b'"');
                            output.push(b':');
                            self.emit_value(val, output);
                        }
                        output.push(b'}');
                    } else {
                        // Fallback: emit as array
                        output.push(b'[');
                        for (i, val) in values.iter().enumerate() {
                            if i > 0 {
                                output.push(b',');
                            }
                            self.emit_value(val, output);
                        }
                        output.push(b']');
                    }

                    if let Some(first) = first_in_container.last_mut() {
                        *first = false;
                    }
                }
                TapeNode::Comment(_)
                | TapeNode::Reference { .. }
                | TapeNode::Definition { .. }
                | TapeNode::Section { .. } => {
                    // JSON doesn't support these - skip in lossy mode
                }
            }
        }

        Ok(())
    }
}

// =============================================================================
// YAML Emitter
// =============================================================================

/// YAML format emitter
#[cfg(feature = "yaml")]
pub struct YamlEmitter<'a> {
    options: &'a TransformOptions,
}

#[cfg(feature = "yaml")]
impl<'a> YamlEmitter<'a> {
    /// Create a new YAML emitter
    #[must_use]
    pub const fn new(options: &'a TransformOptions) -> Self {
        Self { options }
    }

    #[allow(clippy::unused_self)] // Method signature for API consistency; may use self.options in future
    fn emit_value(&self, value: &TapeValue<'_>, output: &mut Vec<u8>) {
        match value {
            TapeValue::Null => output.extend_from_slice(b"null"),
            TapeValue::Bool(true) => output.extend_from_slice(b"true"),
            TapeValue::Bool(false) => output.extend_from_slice(b"false"),
            TapeValue::Int(i) => {
                let _ = write!(output, "{i}");
            }
            TapeValue::Float(f) => {
                if f.is_finite() {
                    let _ = write!(output, "{f}");
                } else {
                    output.extend_from_slice(b".nan");
                }
            }
            TapeValue::RawNumber(s) => output.extend_from_slice(s.as_bytes()),
            TapeValue::String(s) => {
                if needs_yaml_quoting(s) {
                    output.push(b'"');
                    escape_json_string(s, output);
                    output.push(b'"');
                } else {
                    output.extend_from_slice(s.as_bytes());
                }
            }
        }
    }

    fn emit_indent(&self, output: &mut Vec<u8>, depth: usize) {
        for _ in 0..depth {
            output.extend_from_slice(self.options.indent.as_bytes());
        }
    }
}

#[cfg(feature = "yaml")]
impl Emitter for YamlEmitter<'_> {
    fn emit(&self, tape: &UnifiedTape<'_>) -> TransformResult<Vec<u8>> {
        let mut output =
            Vec::with_capacity(tape.stats.string_bytes * 2 + tape.stats.node_count * 4);
        self.emit_into(tape, &mut output)?;
        Ok(output)
    }

    fn emit_into(&self, tape: &UnifiedTape<'_>, output: &mut Vec<u8>) -> TransformResult<()> {
        let mut depth = 0usize;
        let mut in_array: Vec<bool> = vec![false];
        let mut first_in_container: Vec<bool> = vec![true];

        for node in &tape.nodes {
            match node {
                TapeNode::ObjectStart { .. } => {
                    if *in_array.last().unwrap_or(&false)
                        && !first_in_container.last().copied().unwrap_or(true)
                    {
                        output.push(b'\n');
                        self.emit_indent(output, depth);
                        output.extend_from_slice(b"-");
                    }
                    depth += 1;
                    in_array.push(false);
                    first_in_container.push(true);
                }
                TapeNode::ObjectEnd | TapeNode::ArrayEnd => {
                    depth = depth.saturating_sub(1);
                    in_array.pop();
                    first_in_container.pop();
                }
                TapeNode::ArrayStart { .. } => {
                    depth += 1;
                    in_array.push(true);
                    first_in_container.push(true);
                }
                TapeNode::Key(key) => {
                    if !first_in_container.last().copied().unwrap_or(true) || depth > 1 {
                        output.push(b'\n');
                    }
                    self.emit_indent(output, depth.saturating_sub(1));
                    output.extend_from_slice(key.as_bytes());
                    output.extend_from_slice(b": ");
                    if let Some(first) = first_in_container.last_mut() {
                        *first = false;
                    }
                }
                TapeNode::Value(value) => {
                    if *in_array.last().unwrap_or(&false) {
                        if !first_in_container.last().copied().unwrap_or(true) {
                            output.push(b'\n');
                        }
                        self.emit_indent(output, depth.saturating_sub(1));
                        output.extend_from_slice(b"- ");
                    }
                    self.emit_value(value, output);
                    if let Some(first) = first_in_container.last_mut() {
                        *first = false;
                    }
                }
                TapeNode::Comment(text) => {
                    if self.options.preserve_comments {
                        output.push(b'\n');
                        self.emit_indent(output, depth);
                        output.extend_from_slice(b"# ");
                        output.extend_from_slice(text.as_bytes());
                    }
                }
                TapeNode::TabularRow { values } => {
                    output.push(b'\n');
                    self.emit_indent(output, depth.saturating_sub(1));
                    output.extend_from_slice(b"- [");
                    for (i, val) in values.iter().enumerate() {
                        if i > 0 {
                            output.extend_from_slice(b", ");
                        }
                        self.emit_value(val, output);
                    }
                    output.push(b']');
                }
                _ => {}
            }
        }

        output.push(b'\n');
        Ok(())
    }
}

// =============================================================================
// TOML Emitter
// =============================================================================

/// TOML format emitter
#[cfg(feature = "toml")]
pub struct TomlEmitter<'a> {
    #[allow(dead_code)] // Reserved for future format-specific options
    options: &'a TransformOptions,
}

#[cfg(feature = "toml")]
impl<'a> TomlEmitter<'a> {
    /// Create a new TOML emitter
    #[must_use]
    pub const fn new(options: &'a TransformOptions) -> Self {
        Self { options }
    }

    #[allow(clippy::unused_self)] // Method signature for API consistency; may use self.options in future
    fn emit_value(&self, value: &TapeValue<'_>, output: &mut Vec<u8>) {
        match value {
            TapeValue::Null => output.extend_from_slice(b"\"\""), // TOML has no null
            TapeValue::Bool(true) => output.extend_from_slice(b"true"),
            TapeValue::Bool(false) => output.extend_from_slice(b"false"),
            TapeValue::Int(i) => {
                let _ = write!(output, "{i}");
            }
            TapeValue::Float(f) => {
                if f.is_finite() {
                    let _ = write!(output, "{f}");
                } else if f.is_nan() {
                    output.extend_from_slice(b"nan");
                } else if f.is_infinite() && f.is_sign_positive() {
                    output.extend_from_slice(b"inf");
                } else {
                    output.extend_from_slice(b"-inf");
                }
            }
            TapeValue::RawNumber(s) => output.extend_from_slice(s.as_bytes()),
            TapeValue::String(s) => {
                output.push(b'"');
                escape_json_string(s, output);
                output.push(b'"');
            }
        }
    }
}

#[cfg(feature = "toml")]
impl Emitter for TomlEmitter<'_> {
    fn emit(&self, tape: &UnifiedTape<'_>) -> TransformResult<Vec<u8>> {
        let mut output =
            Vec::with_capacity(tape.stats.string_bytes * 2 + tape.stats.node_count * 4);
        self.emit_into(tape, &mut output)?;
        Ok(output)
    }

    fn emit_into(&self, tape: &UnifiedTape<'_>, output: &mut Vec<u8>) -> TransformResult<()> {
        let mut path: Vec<String> = Vec::new();
        let mut in_inline = false;

        for node in &tape.nodes {
            match node {
                TapeNode::ObjectStart { .. } => {
                    if !path.is_empty() && !in_inline {
                        // Emit section header
                        output.push(b'\n');
                        output.push(b'[');
                        output.extend_from_slice(path.join(".").as_bytes());
                        output.extend_from_slice(b"]\n");
                    }
                }
                TapeNode::ObjectEnd => {
                    if !path.is_empty() {
                        path.pop();
                    }
                }
                TapeNode::Key(key) => {
                    path.push(key.to_string());
                }
                TapeNode::Value(value) => {
                    if let Some(key) = path.last() {
                        output.extend_from_slice(key.as_bytes());
                        output.extend_from_slice(b" = ");
                        self.emit_value(value, output);
                        output.push(b'\n');
                    }
                    path.pop();
                }
                TapeNode::ArrayStart { .. } => {
                    if let Some(key) = path.last() {
                        output.extend_from_slice(key.as_bytes());
                        output.extend_from_slice(b" = [");
                        in_inline = true;
                    }
                }
                TapeNode::ArrayEnd => {
                    output.extend_from_slice(b"]\n");
                    in_inline = false;
                    path.pop();
                }
                TapeNode::TabularRow { values } if in_inline => {
                    output.push(b'[');
                    for (i, val) in values.iter().enumerate() {
                        if i > 0 {
                            output.extend_from_slice(b", ");
                        }
                        self.emit_value(val, output);
                    }
                    output.extend_from_slice(b"], ");
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// =============================================================================
// CSV Emitter
// =============================================================================

/// CSV format emitter
#[cfg(feature = "csv")]
pub struct CsvEmitter<'a> {
    #[allow(dead_code)] // Reserved for future format-specific options
    options: &'a TransformOptions,
}

#[cfg(feature = "csv")]
impl<'a> CsvEmitter<'a> {
    /// Create a new CSV emitter
    #[must_use]
    pub const fn new(options: &'a TransformOptions) -> Self {
        Self { options }
    }

    #[allow(clippy::unused_self)] // Method signature for API consistency; may use self.options in future
    fn emit_value(&self, value: &TapeValue<'_>, output: &mut Vec<u8>) {
        match value {
            TapeValue::Null => {} // Empty field
            TapeValue::Bool(true) => output.extend_from_slice(b"true"),
            TapeValue::Bool(false) => output.extend_from_slice(b"false"),
            TapeValue::Int(i) => {
                let _ = write!(output, "{i}");
            }
            TapeValue::Float(f) => {
                let _ = write!(output, "{f}");
            }
            TapeValue::RawNumber(s) => output.extend_from_slice(s.as_bytes()),
            TapeValue::String(s) => {
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    output.push(b'"');
                    for c in s.bytes() {
                        if c == b'"' {
                            output.push(b'"');
                        }
                        output.push(c);
                    }
                    output.push(b'"');
                } else {
                    output.extend_from_slice(s.as_bytes());
                }
            }
        }
    }
}

#[cfg(feature = "csv")]
impl Emitter for CsvEmitter<'_> {
    fn emit(&self, tape: &UnifiedTape<'_>) -> TransformResult<Vec<u8>> {
        let mut output = Vec::with_capacity(tape.stats.string_bytes + tape.stats.node_count * 2);
        self.emit_into(tape, &mut output)?;
        Ok(output)
    }

    fn emit_into(&self, tape: &UnifiedTape<'_>, output: &mut Vec<u8>) -> TransformResult<()> {
        for node in &tape.nodes {
            match node {
                TapeNode::TabularHeader { fields, .. } => {
                    for (i, field) in fields.iter().enumerate() {
                        if i > 0 {
                            output.push(b',');
                        }
                        output.extend_from_slice(field.as_bytes());
                    }
                    output.push(b'\n');
                }
                TapeNode::TabularRow { values } => {
                    for (i, val) in values.iter().enumerate() {
                        if i > 0 {
                            output.push(b',');
                        }
                        self.emit_value(val, output);
                    }
                    output.push(b'\n');
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// =============================================================================
// ISON Emitter
// =============================================================================

/// ISON format emitter
#[cfg(feature = "ison")]
pub struct IsonEmitter<'a> {
    #[allow(dead_code)] // Reserved for future format-specific options
    options: &'a TransformOptions,
}

#[cfg(feature = "ison")]
impl<'a> IsonEmitter<'a> {
    /// Create a new ISON emitter
    #[must_use]
    pub const fn new(options: &'a TransformOptions) -> Self {
        Self { options }
    }

    #[allow(clippy::unused_self)] // Method signature for API consistency; may use self.options in future
    fn emit_value(&self, value: &TapeValue<'_>, output: &mut Vec<u8>) {
        match value {
            TapeValue::Null => output.extend_from_slice(b"null"),
            TapeValue::Bool(true) => output.extend_from_slice(b"true"),
            TapeValue::Bool(false) => output.extend_from_slice(b"false"),
            TapeValue::Int(i) => {
                let _ = write!(output, "{i}");
            }
            TapeValue::Float(f) => {
                let _ = write!(output, "{f}");
            }
            TapeValue::RawNumber(s) => output.extend_from_slice(s.as_bytes()),
            TapeValue::String(s) => {
                if s.contains(' ') || s.contains('\n') {
                    output.push(b'"');
                    output.extend_from_slice(s.as_bytes());
                    output.push(b'"');
                } else {
                    output.extend_from_slice(s.as_bytes());
                }
            }
        }
    }
}

#[cfg(feature = "ison")]
impl Emitter for IsonEmitter<'_> {
    fn emit(&self, tape: &UnifiedTape<'_>) -> TransformResult<Vec<u8>> {
        let mut output = Vec::with_capacity(tape.stats.string_bytes + tape.stats.node_count * 2);
        self.emit_into(tape, &mut output)?;
        Ok(output)
    }

    #[allow(clippy::match_same_arms)] // Empty arms intentionally document skipped node types
    fn emit_into(&self, tape: &UnifiedTape<'_>, output: &mut Vec<u8>) -> TransformResult<()> {
        let mut path: Vec<String> = Vec::new();
        let mut in_array = false;
        let mut array_index = 0usize;

        for node in &tape.nodes {
            match node {
                TapeNode::ObjectStart { .. } => {
                    // ISON uses flat key paths, no explicit object markers
                }
                TapeNode::ObjectEnd => {
                    if !path.is_empty() {
                        path.pop();
                    }
                }
                TapeNode::ArrayStart { .. } => {
                    in_array = true;
                    array_index = 0;
                }
                TapeNode::ArrayEnd => {
                    in_array = false;
                    if !path.is_empty() {
                        path.pop();
                    }
                }
                TapeNode::Key(key) => {
                    path.push(key.to_string());
                }
                TapeNode::Value(value) => {
                    if in_array {
                        // Array element: path[index]
                        let base_path = path.join(".");
                        if !base_path.is_empty() {
                            output.extend_from_slice(base_path.as_bytes());
                        }
                        let _ = write!(output, "[{array_index}] ");
                        self.emit_value(value, output);
                        output.push(b'\n');
                        array_index += 1;
                    } else {
                        // Object key: key value
                        let full_path = path.join(".");
                        output.extend_from_slice(full_path.as_bytes());
                        output.push(b' ');
                        self.emit_value(value, output);
                        output.push(b'\n');
                        path.pop();
                    }
                }
                TapeNode::Section { path: sec_path } => {
                    output.extend_from_slice(b"table.");
                    output.extend_from_slice(sec_path.join(".").as_bytes());
                    output.push(b'\n');
                }
                TapeNode::TabularHeader { fields, .. } => {
                    for (i, field) in fields.iter().enumerate() {
                        if i > 0 {
                            output.push(b' ');
                        }
                        output.extend_from_slice(field.as_bytes());
                        output.extend_from_slice(b":string");
                    }
                    output.push(b'\n');
                }
                TapeNode::TabularRow { values } => {
                    for (i, val) in values.iter().enumerate() {
                        if i > 0 {
                            output.push(b' ');
                        }
                        self.emit_value(val, output);
                    }
                    output.push(b'\n');
                }
                TapeNode::Comment(text) if self.options.preserve_comments => {
                    output.extend_from_slice(b"// ");
                    output.extend_from_slice(text.as_bytes());
                    output.push(b'\n');
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// =============================================================================
// TOON Emitter
// =============================================================================

/// TOON format emitter
#[cfg(feature = "toon")]
pub struct ToonEmitter<'a> {
    options: &'a TransformOptions,
}

#[cfg(feature = "toon")]
impl<'a> ToonEmitter<'a> {
    /// Create a new TOON emitter
    #[must_use]
    pub const fn new(options: &'a TransformOptions) -> Self {
        Self { options }
    }

    #[allow(clippy::unused_self)] // Method signature for API consistency; may use self.options in future
    fn emit_value(&self, value: &TapeValue<'_>, output: &mut Vec<u8>) {
        match value {
            TapeValue::Null => output.extend_from_slice(b"null"),
            TapeValue::Bool(true) => output.extend_from_slice(b"true"),
            TapeValue::Bool(false) => output.extend_from_slice(b"false"),
            TapeValue::Int(i) => {
                let _ = write!(output, "{i}");
            }
            TapeValue::Float(f) => {
                let _ = write!(output, "{f}");
            }
            TapeValue::RawNumber(s) => output.extend_from_slice(s.as_bytes()),
            TapeValue::String(s) => {
                if needs_toon_quoting(s) {
                    output.push(b'"');
                    escape_json_string(s, output);
                    output.push(b'"');
                } else {
                    output.extend_from_slice(s.as_bytes());
                }
            }
        }
    }

    fn emit_indent(&self, output: &mut Vec<u8>, depth: usize) {
        for _ in 0..depth {
            output.extend_from_slice(self.options.indent.as_bytes());
        }
    }
}

#[cfg(feature = "toon")]
impl Emitter for ToonEmitter<'_> {
    fn emit(&self, tape: &UnifiedTape<'_>) -> TransformResult<Vec<u8>> {
        let mut output = Vec::with_capacity(tape.stats.string_bytes + tape.stats.node_count * 2);
        self.emit_into(tape, &mut output)?;
        Ok(output)
    }

    fn emit_into(&self, tape: &UnifiedTape<'_>, output: &mut Vec<u8>) -> TransformResult<()> {
        let mut depth = 0usize;
        let mut current_key: Option<String> = None;
        let mut in_array = false;
        let mut array_index = 0usize;

        for node in &tape.nodes {
            match node {
                TapeNode::ObjectStart { .. } => {
                    depth += 1;
                }
                TapeNode::ObjectEnd => {
                    depth = depth.saturating_sub(1);
                }
                TapeNode::ArrayStart { .. } => {
                    in_array = true;
                    array_index = 0;
                    depth += 1;
                }
                TapeNode::ArrayEnd => {
                    in_array = false;
                    depth = depth.saturating_sub(1);
                    current_key = None;
                }
                TapeNode::Key(key) => {
                    current_key = Some(key.to_string());
                }
                TapeNode::Value(value) => {
                    if in_array {
                        // Array element
                        if let Some(ref key) = current_key {
                            self.emit_indent(output, depth.saturating_sub(1));
                            output.extend_from_slice(key.as_bytes());
                            let _ = write!(output, "[{array_index}]: ");
                            self.emit_value(value, output);
                            output.push(b'\n');
                        }
                        array_index += 1;
                    } else if let Some(ref key) = current_key {
                        // Object key-value pair
                        self.emit_indent(output, depth.saturating_sub(1));
                        output.extend_from_slice(key.as_bytes());
                        output.extend_from_slice(b": ");
                        self.emit_value(value, output);
                        output.push(b'\n');
                        current_key = None;
                    }
                }
                TapeNode::TabularHeader { fields, delimiter } => {
                    let key = current_key.as_deref().unwrap_or("data");
                    self.emit_indent(output, depth.saturating_sub(1));
                    output.extend_from_slice(key.as_bytes());
                    output.extend_from_slice(b"[");
                    let delim_marker = if *delimiter == b'|' {
                        "|"
                    } else if *delimiter == b'\t' {
                        "\t"
                    } else {
                        ""
                    };
                    output.extend_from_slice(b"0");
                    output.extend_from_slice(delim_marker.as_bytes());
                    output.extend_from_slice(b"]{");
                    output.extend_from_slice(fields.join(",").as_bytes());
                    output.extend_from_slice(b"}:\n");
                }
                TapeNode::TabularRow { values } => {
                    self.emit_indent(output, depth);
                    for (i, val) in values.iter().enumerate() {
                        if i > 0 {
                            output.push(b',');
                        }
                        self.emit_value(val, output);
                    }
                    output.push(b'\n');
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Escape string for JSON output
fn escape_json_string(s: &str, output: &mut Vec<u8>) {
    for c in s.chars() {
        match c {
            '"' => output.extend_from_slice(b"\\\""),
            '\\' => output.extend_from_slice(b"\\\\"),
            '\n' => output.extend_from_slice(b"\\n"),
            '\r' => output.extend_from_slice(b"\\r"),
            '\t' => output.extend_from_slice(b"\\t"),
            c if c.is_control() => {
                let _ = write!(output, "\\u{:04x}", c as u32);
            }
            c => {
                let mut buf = [0u8; 4];
                output.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

/// Check if string needs YAML quoting
#[cfg(feature = "yaml")]
fn needs_yaml_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }

    // Reserved words
    if matches!(
        s,
        "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
    ) {
        return true;
    }

    // Starts with special char
    if s.starts_with(|c: char| {
        matches!(
            c,
            '&' | '*' | '!' | '|' | '>' | '\'' | '"' | '%' | '@' | '`'
        )
    }) {
        return true;
    }

    // Contains special chars
    s.contains(':') || s.contains('#') || s.contains('\n')
}

/// Check if string needs TOON quoting
#[cfg(feature = "toon")]
fn needs_toon_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }

    // Reserved words
    if matches!(s, "true" | "false" | "null") {
        return true;
    }

    // Numeric
    if s.parse::<f64>().is_ok() {
        return true;
    }

    // Special chars
    s.contains(':')
        || s.contains(',')
        || s.contains('[')
        || s.contains(']')
        || s.contains('{')
        || s.contains('}')
        || s.contains('\n')
        || s.starts_with('-')
}
