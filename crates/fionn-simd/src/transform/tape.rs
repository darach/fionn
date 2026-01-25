// SPDX-License-Identifier: MIT OR Apache-2.0
//! Unified tape representation for cross-format transformation
//!
//! This module provides a format-agnostic tape structure that can represent
//! any supported format's data, enabling efficient transformations without
//! intermediate DOM allocation.

use super::{TransformError, TransformResult};
use fionn_core::format::{FormatKind, NodeKind};
use std::borrow::Cow;

/// A node in the unified tape
#[derive(Debug, Clone)]
pub enum TapeNode<'a> {
    /// Object/mapping start with element count
    ObjectStart {
        /// Number of elements in the object
        count: usize,
    },
    /// Object/mapping end
    ObjectEnd,
    /// Array/sequence start with element count
    ArrayStart {
        /// Number of elements in the array
        count: usize,
    },
    /// Array/sequence end
    ArrayEnd,
    /// Key in object (always string-like)
    Key(Cow<'a, str>),
    /// Scalar value
    Value(TapeValue<'a>),
    /// Comment (preserved in semantic/strict modes)
    Comment(Cow<'a, str>),
    /// Reference (YAML alias, ISON ref)
    Reference {
        /// Reference kind
        kind: RefKind,
        /// Reference target
        target: Cow<'a, str>,
    },
    /// Definition (YAML anchor, ISON table)
    Definition {
        /// Definition kind
        kind: DefKind,
        /// Definition name
        name: Cow<'a, str>,
    },
    /// Section header (TOML, ISON blocks)
    Section {
        /// Section path components
        path: Vec<Cow<'a, str>>,
    },
    /// Tabular header with field names
    TabularHeader {
        /// Field names
        fields: Vec<Cow<'a, str>>,
        /// Field delimiter
        delimiter: u8,
    },
    /// Tabular row
    TabularRow {
        /// Row values
        values: Vec<TapeValue<'a>>,
    },
}

/// Scalar values in the tape
#[derive(Debug, Clone, PartialEq)]
pub enum TapeValue<'a> {
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// Floating point value
    Float(f64),
    /// String value
    String(Cow<'a, str>),
    /// Raw number (preserves original representation)
    RawNumber(Cow<'a, str>),
}

impl TapeValue<'_> {
    /// Get the node kind for this value
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        match self {
            Self::Null => NodeKind::Null,
            Self::Bool(_) => NodeKind::Boolean,
            Self::Int(_) | Self::Float(_) | Self::RawNumber(_) => NodeKind::Number,
            Self::String(_) => NodeKind::String,
        }
    }

    /// Check if this value is null
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Convert to owned version
    #[must_use]
    pub fn into_owned(self) -> TapeValue<'static> {
        match self {
            Self::Null => TapeValue::Null,
            Self::Bool(b) => TapeValue::Bool(b),
            Self::Int(i) => TapeValue::Int(i),
            Self::Float(f) => TapeValue::Float(f),
            Self::String(s) => TapeValue::String(Cow::Owned(s.into_owned())),
            Self::RawNumber(s) => TapeValue::RawNumber(Cow::Owned(s.into_owned())),
        }
    }
}

/// Reference kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// YAML alias (*name)
    #[cfg(feature = "yaml")]
    YamlAlias,
    /// ISON reference (:type:id)
    #[cfg(feature = "ison")]
    IsonRef,
    /// Generic reference
    Generic,
}

/// Definition kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    /// YAML anchor (&name)
    #[cfg(feature = "yaml")]
    YamlAnchor,
    /// ISON table definition
    #[cfg(feature = "ison")]
    IsonTable,
    /// Generic definition
    Generic,
}

/// Unified tape structure
#[derive(Debug)]
pub struct UnifiedTape<'a> {
    /// Source format
    pub source_format: FormatKind,
    /// Tape nodes
    pub nodes: Vec<TapeNode<'a>>,
    /// Anchors/definitions for reference resolution
    pub definitions: Vec<(Cow<'a, str>, usize)>,
    /// Memory statistics
    pub stats: TapeStats,
}

/// Statistics about the tape
#[derive(Debug, Clone, Default)]
pub struct TapeStats {
    /// Total node count
    pub node_count: usize,
    /// Object count
    pub object_count: usize,
    /// Array count
    pub array_count: usize,
    /// String count
    pub string_count: usize,
    /// Total string bytes
    pub string_bytes: usize,
    /// Number count
    pub number_count: usize,
    /// Maximum nesting depth
    pub max_depth: usize,
    /// Reference count
    pub reference_count: usize,
    /// Comment count
    pub comment_count: usize,
}

impl<'a> UnifiedTape<'a> {
    /// Create a new empty tape
    #[must_use]
    pub fn new(source_format: FormatKind) -> Self {
        Self {
            source_format,
            nodes: Vec::new(),
            definitions: Vec::new(),
            stats: TapeStats::default(),
        }
    }

    /// Create tape with capacity
    #[must_use]
    pub fn with_capacity(source_format: FormatKind, capacity: usize) -> Self {
        Self {
            source_format,
            nodes: Vec::with_capacity(capacity),
            definitions: Vec::new(),
            stats: TapeStats::default(),
        }
    }

    /// Parse input into unified tape
    ///
    /// # Errors
    ///
    /// Returns an error if the input cannot be parsed as the specified format.
    pub fn parse(input: &'a [u8], format: FormatKind) -> TransformResult<Self> {
        match format {
            FormatKind::Json => Self::parse_json(input),
            #[cfg(feature = "yaml")]
            FormatKind::Yaml => Self::parse_yaml(input),
            #[cfg(feature = "toml")]
            FormatKind::Toml => Self::parse_toml(input),
            #[cfg(feature = "csv")]
            FormatKind::Csv => Self::parse_csv(input),
            #[cfg(feature = "ison")]
            FormatKind::Ison => Self::parse_ison(input),
            #[cfg(feature = "toon")]
            FormatKind::Toon => Self::parse_toon(input),
        }
    }

    /// Parse JSON into unified tape
    #[allow(clippy::items_after_statements)] // Nested helper functions for recursive descent parsing
    #[allow(clippy::missing_transmute_annotations)] // serde_json internal transmutes
    fn parse_json(input: &'a [u8]) -> TransformResult<Self> {
        let input_str = std::str::from_utf8(input).map_err(|e| TransformError::ParseError {
            format: FormatKind::Json,
            message: e.to_string(),
        })?;

        let mut tape = Self::with_capacity(FormatKind::Json, input.len() / 10);
        let mut depth = 0usize;
        let mut max_depth = 0usize;

        // Use serde_json for reliable parsing
        let value: serde_json::Value =
            serde_json::from_str(input_str).map_err(|e| TransformError::ParseError {
                format: FormatKind::Json,
                message: e.to_string(),
            })?;

        fn emit_value<'a>(
            tape: &mut UnifiedTape<'a>,
            value: &'a serde_json::Value,
            depth: &mut usize,
            max_depth: &mut usize,
        ) {
            match value {
                serde_json::Value::Null => {
                    tape.push_value(TapeValue::Null);
                }
                serde_json::Value::Bool(b) => {
                    tape.push_value(TapeValue::Bool(*b));
                }
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        tape.push_value(TapeValue::Int(i));
                    } else if let Some(f) = n.as_f64() {
                        tape.push_value(TapeValue::Float(f));
                    } else {
                        tape.push_value(TapeValue::RawNumber(Cow::Owned(n.to_string())));
                    }
                }
                serde_json::Value::String(s) => {
                    tape.stats.string_bytes += s.len();
                    tape.push_value(TapeValue::String(Cow::Owned(s.clone())));
                }
                serde_json::Value::Array(arr) => {
                    *depth += 1;
                    *max_depth = (*max_depth).max(*depth);
                    tape.push_array_start(arr.len());
                    for item in arr {
                        emit_value(tape, item, depth, max_depth);
                    }
                    tape.push_array_end();
                    *depth -= 1;
                }
                serde_json::Value::Object(obj) => {
                    *depth += 1;
                    *max_depth = (*max_depth).max(*depth);
                    tape.push_object_start(obj.len());
                    for (key, val) in obj {
                        tape.push_key(Cow::Owned(key.clone()));
                        emit_value(tape, val, depth, max_depth);
                    }
                    tape.push_object_end();
                    *depth -= 1;
                }
            }
        }

        // We need to use owned values since serde_json::Value owns the strings
        // This is a temporary solution - a proper implementation would use simd-json
        let value_ref: &serde_json::Value = &value;
        // SAFETY: We're using Cow::Owned for all strings, so lifetime is fine
        emit_value(
            unsafe { std::mem::transmute(&mut tape) },
            unsafe { std::mem::transmute::<&serde_json::Value, &'a serde_json::Value>(value_ref) },
            &mut depth,
            &mut max_depth,
        );

        tape.stats.max_depth = max_depth;
        tape.finalize_stats();
        Ok(tape)
    }

    /// Parse YAML into unified tape
    #[cfg(feature = "yaml")]
    fn parse_yaml(input: &'a [u8]) -> TransformResult<Self> {
        let input_str = std::str::from_utf8(input).map_err(|e| TransformError::ParseError {
            format: FormatKind::Yaml,
            message: e.to_string(),
        })?;

        // For now, parse as JSON-compatible subset
        // A full implementation would use a proper YAML parser
        let mut tape = Self::with_capacity(FormatKind::Yaml, input.len() / 10);

        // Simple line-by-line YAML parsing (JSON-like subset)
        let mut depth = 0usize;
        let mut max_depth = 0usize;
        let mut indent_stack: Vec<usize> = vec![0];

        for line in input_str.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(comment_text) = trimmed.strip_prefix('#') {
                tape.nodes.push(TapeNode::Comment(Cow::Owned(
                    comment_text.trim().to_string(),
                )));
                tape.stats.comment_count += 1;
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            // Handle key: value pairs
            if let Some(colon_pos) = trimmed.find(':') {
                let key = trimmed[..colon_pos].trim();
                let value_part = trimmed[colon_pos + 1..].trim();

                // Adjust depth based on indentation
                while indent_stack.len() > 1 && indent < indent_stack[indent_stack.len() - 1] {
                    indent_stack.pop();
                    tape.push_object_end();
                    depth -= 1;
                }

                tape.push_key(Cow::Owned(key.to_string()));

                if value_part.is_empty() {
                    // Nested object/array coming
                    tape.push_object_start(0);
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    indent_stack.push(indent + 2);
                } else {
                    // Inline value
                    tape.push_value(parse_yaml_value(value_part));
                }
            }
        }

        // Close any open objects
        while indent_stack.len() > 1 {
            indent_stack.pop();
            tape.push_object_end();
        }

        tape.stats.max_depth = max_depth;
        tape.finalize_stats();
        Ok(tape)
    }

    /// Parse TOML into unified tape
    #[cfg(feature = "toml")]
    fn parse_toml(input: &'a [u8]) -> TransformResult<Self> {
        let input_str = std::str::from_utf8(input).map_err(|e| TransformError::ParseError {
            format: FormatKind::Toml,
            message: e.to_string(),
        })?;

        let mut tape = Self::with_capacity(FormatKind::Toml, input.len() / 10);

        // Parse using toml crate for reliability
        let value: toml::Value =
            toml::from_str(input_str).map_err(|e| TransformError::ParseError {
                format: FormatKind::Toml,
                message: e.to_string(),
            })?;

        #[allow(clippy::items_after_statements)] // Nested helper function for recursive descent parsing
        fn emit_toml_value(
            tape: &mut UnifiedTape<'_>,
            value: &toml::Value,
            depth: &mut usize,
            max_depth: &mut usize,
        ) {
            match value {
                toml::Value::String(s) => {
                    tape.stats.string_bytes += s.len();
                    tape.push_value(TapeValue::String(Cow::Owned(s.clone())));
                }
                toml::Value::Integer(i) => {
                    tape.push_value(TapeValue::Int(*i));
                }
                toml::Value::Float(f) => {
                    tape.push_value(TapeValue::Float(*f));
                }
                toml::Value::Boolean(b) => {
                    tape.push_value(TapeValue::Bool(*b));
                }
                toml::Value::Datetime(dt) => {
                    tape.push_value(TapeValue::String(Cow::Owned(dt.to_string())));
                }
                toml::Value::Array(arr) => {
                    *depth += 1;
                    *max_depth = (*max_depth).max(*depth);
                    tape.push_array_start(arr.len());
                    for item in arr {
                        emit_toml_value(tape, item, depth, max_depth);
                    }
                    tape.push_array_end();
                    *depth -= 1;
                }
                toml::Value::Table(table) => {
                    *depth += 1;
                    *max_depth = (*max_depth).max(*depth);
                    tape.push_object_start(table.len());
                    for (key, val) in table {
                        tape.push_key(Cow::Owned(key.clone()));
                        emit_toml_value(tape, val, depth, max_depth);
                    }
                    tape.push_object_end();
                    *depth -= 1;
                }
            }
        }

        let mut depth = 0;
        let mut max_depth = 0;
        emit_toml_value(&mut tape, &value, &mut depth, &mut max_depth);

        tape.stats.max_depth = max_depth;
        tape.finalize_stats();
        Ok(tape)
    }

    /// Parse CSV into unified tape
    #[cfg(feature = "csv")]
    fn parse_csv(input: &'a [u8]) -> TransformResult<Self> {
        let mut tape = Self::with_capacity(FormatKind::Csv, input.len() / 20);

        let mut lines = input.split(|&b| b == b'\n');

        // Parse header row
        let header = lines.next().ok_or_else(|| TransformError::ParseError {
            format: FormatKind::Csv,
            message: "Empty CSV".to_string(),
        })?;

        let header_str = std::str::from_utf8(header).map_err(|e| TransformError::ParseError {
            format: FormatKind::Csv,
            message: e.to_string(),
        })?;

        let fields: Vec<Cow<'_, str>> = header_str
            .split(',')
            .map(|s| Cow::Owned(s.trim().trim_matches('"').to_string()))
            .collect();

        tape.nodes.push(TapeNode::TabularHeader {
            fields: fields.clone(),
            delimiter: b',',
        });

        // Parse data rows
        tape.push_array_start(0); // Unknown count

        for line in lines {
            if line.is_empty() || line == b"\r" {
                continue;
            }

            let line_str = std::str::from_utf8(line).map_err(|e| TransformError::ParseError {
                format: FormatKind::Csv,
                message: e.to_string(),
            })?;

            let values: Vec<TapeValue<'_>> = line_str
                .split(',')
                .map(|s| {
                    let trimmed = s.trim().trim_matches('"');
                    if trimmed.is_empty() {
                        TapeValue::Null
                    } else if let Ok(i) = trimmed.parse::<i64>() {
                        TapeValue::Int(i)
                    } else if let Ok(f) = trimmed.parse::<f64>() {
                        TapeValue::Float(f)
                    } else if trimmed == "true" {
                        TapeValue::Bool(true)
                    } else if trimmed == "false" {
                        TapeValue::Bool(false)
                    } else {
                        TapeValue::String(Cow::Owned(trimmed.to_string()))
                    }
                })
                .collect();

            tape.nodes.push(TapeNode::TabularRow { values });
        }

        tape.push_array_end();
        tape.finalize_stats();
        Ok(tape)
    }

    /// Parse ISON into unified tape
    #[cfg(feature = "ison")]
    fn parse_ison(input: &'a [u8]) -> TransformResult<Self> {
        let input_str = std::str::from_utf8(input).map_err(|e| TransformError::ParseError {
            format: FormatKind::Ison,
            message: e.to_string(),
        })?;

        let mut tape = Self::with_capacity(FormatKind::Ison, input.len() / 10);
        let mut current_block: Option<String> = None;
        let mut field_names: Vec<String> = Vec::new();
        let mut in_simple_object = false;
        let mut array_keys: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for line in input_str.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Comment
            if trimmed.starts_with("//") || trimmed.starts_with('#') {
                tape.nodes.push(TapeNode::Comment(Cow::Owned(
                    trimmed[2..].trim().to_string(),
                )));
                tape.stats.comment_count += 1;
                continue;
            }

            // Block header (table.name or object.name)
            if trimmed.starts_with("table.") || trimmed.starts_with("object.") {
                // Close simple object if we were in one
                if in_simple_object {
                    tape.push_object_end();
                    in_simple_object = false;
                }
                if current_block.is_some() {
                    tape.push_array_end();
                }
                let name = trimmed.split_once('.').map_or("", |x| x.1);
                current_block = Some(name.to_string());
                tape.nodes.push(TapeNode::Section {
                    path: vec![Cow::Owned(name.to_string())],
                });
                tape.push_array_start(0);
                field_names.clear();
                continue;
            }

            // Check if this is a field declaration line (in tabular block)
            if current_block.is_some() && field_names.is_empty() && trimmed.contains(':') {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.iter().all(|p| p.contains(':')) {
                    field_names = parts
                        .iter()
                        .map(|p| p.split(':').next().unwrap_or("").to_string())
                        .collect();
                    tape.nodes.push(TapeNode::TabularHeader {
                        fields: field_names.iter().map(|s| Cow::Owned(s.clone())).collect(),
                        delimiter: b' ',
                    });
                    continue;
                }
            }

            // Data row (in tabular block)
            if current_block.is_some() && !field_names.is_empty() {
                let values: Vec<TapeValue<'_>> =
                    trimmed.split_whitespace().map(parse_ison_value).collect();
                tape.nodes.push(TapeNode::TabularRow { values });
                continue;
            }

            // Simple key-value format: "key value" or "key[index] value"
            if let Some(space_pos) = trimmed.find(' ') {
                let key_part = &trimmed[..space_pos];
                let value_part = trimmed[space_pos + 1..].trim();

                // Start simple object if not already
                if !in_simple_object && current_block.is_none() {
                    tape.push_object_start(0);
                    in_simple_object = true;
                }

                // Check for array syntax: key[index]
                if let Some(bracket_pos) = key_part.find('[')
                    && let Some(end_bracket) = key_part.find(']')
                {
                    let key = &key_part[..bracket_pos];
                    let _index: usize = key_part[bracket_pos + 1..end_bracket].parse().unwrap_or(0);

                    // Track array start
                    let array_count = array_keys.entry(key.to_string()).or_insert(0);
                    if *array_count == 0 {
                        tape.nodes.push(TapeNode::Key(Cow::Owned(key.to_string())));
                        tape.push_array_start(0);
                    }
                    *array_count += 1;

                    tape.nodes
                        .push(TapeNode::Value(parse_ison_value(value_part)));
                    continue;
                }

                // Simple key-value
                tape.nodes
                    .push(TapeNode::Key(Cow::Owned(key_part.to_string())));
                tape.nodes
                    .push(TapeNode::Value(parse_ison_value(value_part)));
            }
        }

        // Close any open arrays
        for _ in array_keys.values() {
            tape.push_array_end();
        }

        // Close simple object if we were in one
        if in_simple_object {
            tape.push_object_end();
        }

        if current_block.is_some() {
            tape.push_array_end();
        }

        tape.finalize_stats();
        Ok(tape)
    }

    /// Parse TOON into unified tape
    #[cfg(feature = "toon")]
    #[allow(clippy::too_many_lines)] // Complex format parsing requires handling many cases inline
    fn parse_toon(input: &'a [u8]) -> TransformResult<Self> {
        let input_str = std::str::from_utf8(input).map_err(|e| TransformError::ParseError {
            format: FormatKind::Toon,
            message: e.to_string(),
        })?;

        let mut tape = Self::with_capacity(FormatKind::Toon, input.len() / 10);
        let mut depth = 0usize;
        #[allow(unused_assignments)] // max_depth is set unconditionally after initialization
        let mut max_depth = 0usize;
        let mut indent_stack: Vec<usize> = vec![0];
        let mut in_tabular: Option<(Vec<String>, u8)> = None; // (fields, delimiter)

        // TOON documents are implicitly objects at the root level
        tape.push_object_start(0); // count unknown yet
        depth += 1;
        max_depth = 1;

        for line in input_str.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            // Check for tabular array header: name[N]{field,field}:
            if let Some(bracket_start) = trimmed.find('[')
                && let Some(colon_pos) = trimmed.rfind(':')
                && colon_pos > bracket_start
            {
                let bracket_end = trimmed.find(']').unwrap_or(bracket_start);
                let count_str = &trimmed[bracket_start + 1..bracket_end];
                let delimiter = if count_str.ends_with('|') {
                    b'|'
                } else if count_str.ends_with('\t') {
                    b'\t'
                } else {
                    b','
                };

                // Extract field names if present
                #[allow(clippy::option_if_let_else)]
                // More readable with if-let for nested Option unwrap
                let fields = if let Some(brace_start) = trimmed.find('{') {
                    let brace_end = trimmed.find('}').unwrap_or(brace_start);
                    trimmed[brace_start + 1..brace_end]
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect()
                } else {
                    Vec::new()
                };

                let key = trimmed[..bracket_start].trim();
                tape.push_key(Cow::Owned(key.to_string()));
                tape.nodes.push(TapeNode::TabularHeader {
                    fields: fields.iter().map(|s| Cow::Owned(s.clone())).collect(),
                    delimiter,
                });
                tape.push_array_start(0);
                in_tabular = Some((fields, delimiter));
                indent_stack.push(indent + 2);
                depth += 1;
                max_depth = max_depth.max(depth);
                continue;
            }

            // Handle tabular rows
            if let Some((ref _fields, delimiter)) = in_tabular {
                if indent >= *indent_stack.last().unwrap_or(&0) && !trimmed.contains(':') {
                    let delim_char = delimiter as char;
                    let values: Vec<TapeValue<'_>> = trimmed
                        .split(delim_char)
                        .map(|s| parse_yaml_value(s.trim()))
                        .collect();
                    tape.nodes.push(TapeNode::TabularRow { values });
                    continue;
                }
                // End tabular mode
                tape.push_array_end();
                in_tabular = None;
                indent_stack.pop();
                depth -= 1;
            }

            // Adjust depth based on indentation
            while indent_stack.len() > 1 && indent < *indent_stack.last().unwrap_or(&0) {
                indent_stack.pop();
                tape.push_object_end();
                depth -= 1;
            }

            // Handle key: value pairs with folded keys
            if let Some(colon_pos) = trimmed.find(':') {
                let key = trimmed[..colon_pos].trim();
                let value_part = trimmed[colon_pos + 1..].trim();

                // Handle folded keys (a.b.c: value)
                if key.contains('.') && !key.starts_with('"') {
                    let parts: Vec<&str> = key.split('.').collect();
                    for (i, part) in parts.iter().enumerate() {
                        tape.push_key(Cow::Owned((*part).to_string()));
                        if i < parts.len() - 1 {
                            tape.push_object_start(1);
                            depth += 1;
                            max_depth = max_depth.max(depth);
                        }
                    }
                    if !value_part.is_empty() {
                        tape.push_value(parse_yaml_value(value_part));
                    }
                    // Close nested objects
                    for _ in 0..parts.len() - 1 {
                        tape.push_object_end();
                        depth -= 1;
                    }
                } else {
                    tape.push_key(Cow::Owned(key.to_string()));
                    if value_part.is_empty() {
                        tape.push_object_start(0);
                        depth += 1;
                        max_depth = max_depth.max(depth);
                        indent_stack.push(indent + 2);
                    } else {
                        tape.push_value(parse_yaml_value(value_part));
                    }
                }
            }
        }

        // Close any open tabular array
        if in_tabular.is_some() {
            tape.push_array_end();
            indent_stack.pop(); // Pop the indent that was pushed when tabular started
        }

        // Close any open structures
        while indent_stack.len() > 1 {
            indent_stack.pop();
            tape.push_object_end();
            #[allow(unused_assignments)] // depth tracks structure but value unused after loop
            {
                depth -= 1;
            }
        }

        // Close the root object we opened at the start
        tape.push_object_end();

        tape.stats.max_depth = max_depth;
        tape.finalize_stats();
        Ok(tape)
    }

    /// Push object start
    pub fn push_object_start(&mut self, count: usize) {
        self.nodes.push(TapeNode::ObjectStart { count });
        self.stats.object_count += 1;
    }

    /// Push object end
    pub fn push_object_end(&mut self) {
        self.nodes.push(TapeNode::ObjectEnd);
    }

    /// Push array start
    pub fn push_array_start(&mut self, count: usize) {
        self.nodes.push(TapeNode::ArrayStart { count });
        self.stats.array_count += 1;
    }

    /// Push array end
    pub fn push_array_end(&mut self) {
        self.nodes.push(TapeNode::ArrayEnd);
    }

    /// Push key
    pub fn push_key(&mut self, key: Cow<'a, str>) {
        self.stats.string_bytes += key.len();
        self.nodes.push(TapeNode::Key(key));
    }

    /// Push value
    pub fn push_value(&mut self, value: TapeValue<'a>) {
        match &value {
            TapeValue::String(s) => {
                self.stats.string_count += 1;
                self.stats.string_bytes += s.len();
            }
            TapeValue::Int(_) | TapeValue::Float(_) | TapeValue::RawNumber(_) => {
                self.stats.number_count += 1;
            }
            _ => {}
        }
        self.nodes.push(TapeNode::Value(value));
    }

    /// Finalize statistics
    #[allow(clippy::missing_const_for_fn)] // Vec::len() is not const
    fn finalize_stats(&mut self) {
        self.stats.node_count = self.nodes.len();
    }

    /// Get node count
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::len() is not const
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if tape is empty
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::is_empty() is not const
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Parse a simple YAML-like value
fn parse_yaml_value(s: &str) -> TapeValue<'_> {
    let trimmed = s.trim();

    if trimmed.is_empty() || trimmed == "null" || trimmed == "~" {
        return TapeValue::Null;
    }

    if trimmed == "true" {
        return TapeValue::Bool(true);
    }
    if trimmed == "false" {
        return TapeValue::Bool(false);
    }

    // Try integer
    if let Ok(i) = trimmed.parse::<i64>() {
        return TapeValue::Int(i);
    }

    // Try float
    if let Ok(f) = trimmed.parse::<f64>() {
        return TapeValue::Float(f);
    }

    // String (remove quotes if present)
    let unquoted = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    TapeValue::String(Cow::Owned(unquoted.to_string()))
}

/// Parse ISON value with reference support
#[cfg(feature = "ison")]
fn parse_ison_value(s: &str) -> TapeValue<'static> {
    let trimmed = s.trim();

    // Reference :type:id
    if trimmed.starts_with(':') {
        return TapeValue::String(Cow::Owned(trimmed.to_string()));
    }

    if trimmed.is_empty() || trimmed == "null" {
        return TapeValue::Null;
    }

    if trimmed == "true" {
        return TapeValue::Bool(true);
    }
    if trimmed == "false" {
        return TapeValue::Bool(false);
    }

    if let Ok(i) = trimmed.parse::<i64>() {
        return TapeValue::Int(i);
    }

    if let Ok(f) = trimmed.parse::<f64>() {
        return TapeValue::Float(f);
    }

    // Quoted string
    let unquoted = if trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    TapeValue::String(Cow::Owned(unquoted.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_simple() {
        let input = br#"{"name": "test", "value": 42}"#;
        let tape = UnifiedTape::parse(input, FormatKind::Json).unwrap();
        assert!(!tape.is_empty());
        assert_eq!(tape.stats.object_count, 1);
    }

    #[test]
    fn test_parse_json_nested() {
        let input = br#"{"user": {"name": "test", "age": 30}}"#;
        let tape = UnifiedTape::parse(input, FormatKind::Json).unwrap();
        assert_eq!(tape.stats.object_count, 2);
        assert_eq!(tape.stats.max_depth, 2);
    }

    #[test]
    fn test_tape_value_kind() {
        assert_eq!(TapeValue::Null.kind(), NodeKind::Null);
        assert_eq!(TapeValue::Bool(true).kind(), NodeKind::Boolean);
        assert_eq!(TapeValue::Int(42).kind(), NodeKind::Number);
        assert_eq!(
            TapeValue::String(Cow::Borrowed("test")).kind(),
            NodeKind::String
        );
    }
}
