// SPDX-License-Identifier: MIT OR Apache-2.0
//! fionn CLI binary - A Swiss Army knife for structured data with SIMD acceleration
//!
//! Supports multiple formats: JSON, YAML, TOML, CSV, ISON, TOON
//!
//! This CLI uses the tape-based architecture throughout for maximum performance:
//! - `UnifiedTape::parse()` for parsing all formats
//! - `transform()` for cross-format conversion
//! - `gron_from_tape()` for format-agnostic gron
//! - `diff_tapes()` for tape-native diffing (250x faster than DOM)
//! - `merge_tapes()` for tape-based merging

use clap::{Parser, Subcommand, ValueEnum};
use fionn_core::FormatKind;
use fionn_diff::{
    apply_patch, deep_merge_tapes, diff_tapes, json_diff, json_merge_patch, merge_tapes,
    tape_to_value,
};
use fionn_gron::{
    GronOptions, GronQueryOptions, Query, gron, gron_from_tape, gron_query, ungron_to_value,
};
use fionn_tape::DsonTape;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

// Tape-based transform is available when any format feature is enabled
#[cfg(any(
    feature = "yaml",
    feature = "toml",
    feature = "csv",
    feature = "ison",
    feature = "toon"
))]
use fionn_simd::transform::{TransformOptions, transform};

// ============================================================================
// CLI Format Enum (mirrors FormatKind but for clap)
// ============================================================================

/// Supported data formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Format {
    /// JSON (JavaScript Object Notation)
    Json,
    /// JSONL (Newline-delimited JSON) - streaming format
    Jsonl,
    /// YAML (YAML Ain't Markup Language)
    #[cfg(feature = "yaml")]
    Yaml,
    /// TOML (Tom's Obvious Minimal Language)
    #[cfg(feature = "toml")]
    Toml,
    /// CSV (Comma-Separated Values)
    #[cfg(feature = "csv")]
    Csv,
    /// ISON (Interchange Simple Object Notation)
    #[cfg(feature = "ison")]
    Ison,
    /// ISONL (ISON Lines) - streaming format, 11.9x faster than JSONL
    #[cfg(feature = "ison")]
    Isonl,
    /// TOON (Token-Oriented Object Notation)
    #[cfg(feature = "toon")]
    Toon,
    /// Gron (greppable object notation) - output only
    Gron,
    /// Auto-detect format from content/extension
    #[default]
    Auto,
}

#[allow(dead_code)] // Methods used conditionally based on enabled format features
impl Format {
    /// Convert to [`FormatKind`] (for library interop)
    const fn to_format_kind(self) -> Option<FormatKind> {
        match self {
            Self::Json | Self::Jsonl => Some(FormatKind::Json),
            #[cfg(feature = "yaml")]
            Self::Yaml => Some(FormatKind::Yaml),
            #[cfg(feature = "toml")]
            Self::Toml => Some(FormatKind::Toml),
            #[cfg(feature = "csv")]
            Self::Csv => Some(FormatKind::Csv),
            #[cfg(feature = "ison")]
            Self::Ison | Self::Isonl => Some(FormatKind::Ison),
            #[cfg(feature = "toon")]
            Self::Toon => Some(FormatKind::Toon),
            Self::Gron | Self::Auto => None,
        }
    }

    /// Check if this is a streaming (line-delimited) format
    const fn is_streaming(self) -> bool {
        matches!(self, Self::Jsonl) || {
            #[cfg(feature = "ison")]
            {
                matches!(self, Self::Isonl)
            }
            #[cfg(not(feature = "ison"))]
            {
                false
            }
        }
    }

    /// Check if this is an output-only format
    const fn is_output_only(self) -> bool {
        matches!(self, Self::Gron)
    }
}

// ============================================================================
// Main CLI Structure
// ============================================================================

#[derive(Parser)]
#[command(name = "fionn")]
#[command(
    version,
    about = "A Swiss Army knife for structured data with SIMD acceleration"
)]
#[command(long_about = "fionn - Multi-format data processing tool\n\n\
    Supports: JSON, YAML, TOML, CSV, ISON, TOON\n\
    Operations: gron, diff, patch, merge, query, format, validate, convert")]
#[allow(clippy::struct_excessive_bools)] // CLI args naturally have many boolean flags
struct Args {
    /// Input format (default: auto-detect)
    #[arg(short = 'f', long = "from", global = true, value_name = "FORMAT")]
    from: Option<Format>,

    /// Output format (default: same as input, or json)
    #[arg(short = 't', long = "to", global = true, value_name = "FORMAT")]
    to: Option<Format>,

    /// Pretty-print output
    #[arg(short = 'p', long = "pretty", global = true)]
    pretty: bool,

    /// Compact output (no extra whitespace)
    #[arg(long = "compact", global = true)]
    compact: bool,

    /// Colorized output
    #[arg(long = "color", global = true)]
    color: bool,

    /// Output file (default: stdout)
    #[arg(short = 'o', long = "output", global = true)]
    output: Option<PathBuf>,

    /// Quiet mode - suppress informational messages
    #[arg(short = 'q', long = "quiet", global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Subcommands for fionn CLI
#[derive(Subcommand)]
enum Commands {
    /// Flatten data to gron format (greppable)
    Gron {
        /// Input file (reads from stdin if not provided)
        #[arg(value_name = "FILE")]
        input: Option<PathBuf>,

        /// Reverse mode: convert gron back to structured format
        #[arg(short = 'u', long = "ungron")]
        ungron: bool,

        /// Output compact gron format (no spaces)
        #[arg(short = 'c', long = "compact")]
        compact: bool,

        /// Output only paths (no values)
        #[arg(long = "paths")]
        paths_only: bool,

        /// Output only values (no paths)
        #[arg(long = "values")]
        values_only: bool,

        /// Custom root prefix (default: json)
        #[arg(long = "prefix", default_value = "json")]
        prefix: String,

        /// Query filter (JSONPath-like)
        #[arg(long = "query")]
        query: Option<String>,

        /// Sort output alphabetically
        #[arg(long = "sort")]
        sort: bool,
    },

    /// Compute diff between two files
    Diff {
        /// First file (source)
        file1: PathBuf,
        /// Second file (target)
        file2: PathBuf,

        /// Output format for diff (json-patch, merge-patch, or unified)
        #[arg(long = "diff-format", default_value = "json-patch")]
        diff_format: String,

        /// Ignore array element order
        #[arg(long = "ignore-order")]
        ignore_order: bool,
    },

    /// Apply a patch to a file
    Patch {
        /// File to patch
        file: PathBuf,
        /// Patch file (JSON Patch or Merge Patch format)
        patch: PathBuf,

        /// Patch format (json-patch or merge-patch)
        #[arg(long = "patch-format", default_value = "json-patch")]
        patch_format: String,

        /// Dry run - show result without modifying
        #[arg(long = "dry-run")]
        dry_run: bool,
    },

    /// Merge multiple files
    Merge {
        /// Files to merge (first file is base)
        files: Vec<PathBuf>,

        /// Deep merge nested objects (default: true)
        #[arg(long = "deep", default_value = "true")]
        deep: bool,

        /// Array merge strategy (replace, append, concat)
        #[arg(long = "arrays", default_value = "replace")]
        array_strategy: String,
    },

    /// Query data with JSONPath-like syntax
    Query {
        /// Query string (JSONPath-like)
        query: String,
        /// Input file
        file: Option<PathBuf>,

        /// Output raw values (no JSON encoding for strings)
        #[arg(short = 'r', long = "raw")]
        raw: bool,

        /// Return only first match
        #[arg(long = "first")]
        first: bool,
    },

    /// Convert between formats
    Convert {
        /// Input file
        file: Option<PathBuf>,

        /// Sort object keys alphabetically
        #[arg(long = "sort-keys")]
        sort_keys: bool,
    },

    /// Format/pretty-print data
    Format {
        /// Input file
        file: Option<PathBuf>,

        /// Indentation width (spaces)
        #[arg(short = 'i', long = "indent", default_value = "2")]
        indent: usize,

        /// Sort object keys
        #[arg(long = "sort-keys")]
        sort_keys: bool,
    },

    /// Validate data format
    Validate {
        /// Input file
        file: Option<PathBuf>,

        /// Strict validation mode
        #[arg(long = "strict")]
        strict: bool,
    },

    /// Process streaming data (JSONL, ISONL, multi-doc YAML, CSV rows)
    Stream {
        /// Input file
        file: Option<PathBuf>,

        /// Filter records by query
        #[arg(long = "filter")]
        filter: Option<String>,

        /// Limit number of records
        #[arg(long = "limit")]
        limit: Option<usize>,

        /// Skip first N records
        #[arg(long = "skip")]
        skip: Option<usize>,

        /// Fields to extract (comma-separated for schema filtering)
        #[arg(long = "fields", short = 'F')]
        fields: Option<String>,

        /// DSON operations (JSON array): '[{"FieldAdd":{"path":"x","value":1}}]'
        #[arg(long = "ops")]
        ops: Option<String>,

        /// Use SIMD acceleration (default: auto-detect)
        #[arg(long = "simd", default_value = "true")]
        use_simd: bool,
    },

    /// Infer schema from data
    Schema {
        /// Input file
        file: Option<PathBuf>,

        /// Schema output format (json-schema, typescript, rust)
        #[arg(long = "schema-format", default_value = "json-schema")]
        schema_format: String,
    },

    /// Perform operations on data
    Ops {
        /// Operation: keys, values, length, flatten, type, paths, etc.
        op: String,
        /// Input file
        file: Option<PathBuf>,

        /// Path to operate on (for nested operations)
        #[arg(long = "path")]
        path: Option<String>,
    },

    /// Show statistics about data
    Stats {
        /// Input file
        file: Option<PathBuf>,
    },

    /// Run benchmarks
    Bench {
        /// Benchmark type (parse, gron, diff, all)
        #[arg(default_value = "all")]
        bench_type: String,
    },
}

// ============================================================================
// Format Detection and Parsing
// ============================================================================

/// Detect format from file extension
fn detect_format_from_extension(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "json" | "geojson" => Some(Format::Json),
        "jsonl" | "ndjson" => Some(Format::Jsonl),
        #[cfg(feature = "yaml")]
        "yaml" | "yml" => Some(Format::Yaml),
        #[cfg(feature = "toml")]
        "toml" => Some(Format::Toml),
        #[cfg(feature = "csv")]
        "csv" | "tsv" => Some(Format::Csv),
        #[cfg(feature = "ison")]
        "ison" => Some(Format::Ison),
        #[cfg(feature = "ison")]
        "isonl" => Some(Format::Isonl),
        #[cfg(feature = "toon")]
        "toon" => Some(Format::Toon),
        "gron" => Some(Format::Gron),
        _ => None,
    }
}

/// Detect format from content
fn detect_format_from_content(content: &[u8]) -> Format {
    let result = FormatKind::detect_from_content(content);
    match result.format {
        FormatKind::Json => Format::Json,
        #[cfg(feature = "yaml")]
        FormatKind::Yaml => Format::Yaml,
        #[cfg(feature = "toml")]
        FormatKind::Toml => Format::Toml,
        #[cfg(feature = "csv")]
        FormatKind::Csv => Format::Csv,
        #[cfg(feature = "ison")]
        FormatKind::Ison => Format::Ison,
        #[cfg(feature = "toon")]
        FormatKind::Toon => Format::Toon,
        #[allow(unreachable_patterns)] // Matches feature-gated variants
        _ => Format::Json,
    }
}

/// Resolve format (explicit > extension > content detection)
fn resolve_input_format(explicit: Option<Format>, path: Option<&Path>, content: &[u8]) -> Format {
    // Explicit format takes precedence
    if let Some(fmt) = explicit
        && fmt != Format::Auto
    {
        return fmt;
    }

    // Try file extension
    if let Some(p) = path
        && let Some(fmt) = detect_format_from_extension(p)
    {
        return fmt;
    }

    // Fall back to content detection
    detect_format_from_content(content)
}

/// Resolve output format
fn resolve_output_format(explicit: Option<Format>, input_format: Format) -> Format {
    if let Some(fmt) = explicit
        && fmt != Format::Auto
    {
        return fmt;
    }
    // Default to input format, or JSON if input was gron
    if input_format == Format::Gron {
        Format::Json
    } else {
        input_format
    }
}

// ============================================================================
// Parsing Functions
// ============================================================================

/// Parse content to [`serde_json::Value`] based on format
fn parse_to_value(content: &str, format: Format) -> Result<Value, Box<dyn std::error::Error>> {
    match format {
        Format::Json | Format::Jsonl | Format::Auto => Ok(serde_json::from_str(content)?),
        #[cfg(feature = "yaml")]
        Format::Yaml => Ok(serde_yaml::from_str(content)?),
        #[cfg(feature = "toml")]
        Format::Toml => {
            let toml_value: toml_crate::Value = toml_crate::from_str(content)?;
            // Convert TOML Value to JSON Value
            let json_str = serde_json::to_string(&toml_value)?;
            Ok(serde_json::from_str(&json_str)?)
        }
        #[cfg(feature = "csv")]
        Format::Csv => {
            // Parse CSV to array of objects
            let mut reader = csv::Reader::from_reader(content.as_bytes());
            let headers: Vec<String> = reader.headers()?.iter().map(String::from).collect();
            let mut records = Vec::new();
            for result in reader.records() {
                let record = result?;
                let mut obj = serde_json::Map::new();
                for (i, field) in record.iter().enumerate() {
                    if i < headers.len() {
                        obj.insert(headers[i].clone(), Value::String(field.to_string()));
                    }
                }
                records.push(Value::Object(obj));
            }
            Ok(Value::Array(records))
        }
        Format::Gron => {
            // Parse gron format back to JSON
            let value = ungron_to_value(content)?;
            Ok(value)
        }
        #[cfg(feature = "ison")]
        Format::Ison | Format::Isonl => {
            // ISON/ISONL parsing via transform - uses real ISON parser and JSON emitter
            let opts = TransformOptions::new();
            let (json_bytes, _metrics) = transform(
                content.as_bytes(),
                FormatKind::Ison,
                FormatKind::Json,
                &opts,
            )
            .map_err(|e| format!("ISON parse error: {e}"))?;
            let json_str = String::from_utf8(json_bytes)
                .map_err(|e| format!("ISON output encoding error: {e}"))?;
            Ok(serde_json::from_str(&json_str)?)
        }
        #[cfg(feature = "toon")]
        Format::Toon => {
            // TOON parsing via transform - uses real TOON parser and JSON emitter
            let opts = TransformOptions::new();
            let (json_bytes, _metrics) = transform(
                content.as_bytes(),
                FormatKind::Toon,
                FormatKind::Json,
                &opts,
            )
            .map_err(|e| format!("TOON parse error: {e}"))?;
            let json_str = String::from_utf8(json_bytes)
                .map_err(|e| format!("TOON output encoding error: {e}"))?;
            Ok(serde_json::from_str(&json_str)?)
        }
        #[allow(unreachable_patterns)] // Matches feature-gated variants
        _ => Err("Unsupported input format".into()),
    }
}

// ============================================================================
// Output Functions
// ============================================================================

/// Serialize value to string based on format
fn value_to_string(
    value: &Value,
    format: Format,
    pretty: bool,
    compact: bool,
    indent: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    match format {
        Format::Json | Format::Auto => {
            if compact {
                Ok(serde_json::to_string(value)?)
            } else if pretty {
                Ok(serde_json::to_string_pretty(value)?)
            } else {
                let indent_str = " ".repeat(indent);
                let mut buf = Vec::new();
                let formatter =
                    serde_json::ser::PrettyFormatter::with_indent(indent_str.as_bytes());
                let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
                value.serialize(&mut ser)?;
                Ok(String::from_utf8(buf)?)
            }
        }
        #[cfg(feature = "yaml")]
        Format::Yaml => Ok(serde_yaml::to_string(value)?),
        #[cfg(feature = "toml")]
        Format::Toml => {
            // Convert to TOML - requires the value to be a table
            let toml_str = toml_crate::to_string_pretty(value)?;
            Ok(toml_str)
        }
        #[cfg(feature = "csv")]
        Format::Csv => {
            // Convert array of objects to CSV
            if let Value::Array(arr) = value {
                let mut wtr = csv::Writer::from_writer(vec![]);
                // Get headers from first object
                if let Some(Value::Object(first)) = arr.first() {
                    let headers: Vec<&str> = first.keys().map(String::as_str).collect();
                    wtr.write_record(&headers)?;

                    for item in arr {
                        if let Value::Object(obj) = item {
                            let row: Vec<String> = headers
                                .iter()
                                .map(|h| {
                                    obj.get(*h)
                                        .map(|v| match v {
                                            Value::String(s) => s.clone(),
                                            other => other.to_string(),
                                        })
                                        .unwrap_or_default()
                                })
                                .collect();
                            wtr.write_record(&row)?;
                        }
                    }
                }
                Ok(String::from_utf8(wtr.into_inner()?)?)
            } else {
                Err("CSV output requires an array of objects".into())
            }
        }
        Format::Gron => {
            // Output as gron format
            let json_str = serde_json::to_string(value)?;
            let opts = if compact {
                GronOptions::default().compact()
            } else {
                GronOptions::default()
            };
            Ok(gron(&json_str, &opts)?)
        }
        #[cfg(feature = "ison")]
        Format::Ison => {
            // ISON output via transform - real ISON emitter
            let json_bytes = serde_json::to_vec(value)?;
            let opts = TransformOptions::new().with_pretty(pretty);
            let (output, _metrics) =
                transform(&json_bytes, FormatKind::Json, FormatKind::Ison, &opts)
                    .map_err(|e| format!("ISON transform error: {e}"))?;
            String::from_utf8(output).map_err(Into::into)
        }
        #[cfg(feature = "toon")]
        Format::Toon => {
            // TOON output via transform - real TOON emitter
            let json_bytes = serde_json::to_vec(value)?;
            let opts = TransformOptions::new().with_pretty(pretty);
            let (output, _metrics) =
                transform(&json_bytes, FormatKind::Json, FormatKind::Toon, &opts)
                    .map_err(|e| format!("TOON transform error: {e}"))?;
            String::from_utf8(output).map_err(Into::into)
        }
        #[allow(unreachable_patterns)] // Matches feature-gated variants
        _ => Err("Unsupported output format".into()),
    }
}

// ============================================================================
// I/O Helpers
// ============================================================================

/// Read input from file or stdin
fn read_input(path: Option<&PathBuf>) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(p) = path {
        Ok(fs::read_to_string(p)?)
    } else {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        Ok(input)
    }
}

/// Write output to file or stdout
fn write_output(output: &str, path: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(p) = path {
        fs::write(p, output)?;
    } else {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(output.as_bytes())?;
        if !output.ends_with('\n') {
            handle.write_all(b"\n")?;
        }
    }
    Ok(())
}

// ============================================================================
// Command Handlers
// ============================================================================

fn main() {
    let args = Args::parse();

    let result = match &args.command {
        Commands::Gron { .. } => handle_gron(&args),
        Commands::Diff { .. } => handle_diff(&args),
        Commands::Patch { .. } => handle_patch(&args),
        Commands::Merge { .. } => handle_merge(&args),
        Commands::Query { .. } => handle_query(&args),
        Commands::Convert { .. } => handle_convert(&args),
        Commands::Format { .. } => handle_format(&args),
        Commands::Validate { .. } => handle_validate(&args),
        Commands::Stream { .. } => handle_stream(&args),
        Commands::Schema { .. } => handle_schema(&args),
        Commands::Ops { .. } => handle_ops(&args),
        Commands::Stats { .. } => handle_stats(&args),
        Commands::Bench { .. } => {
            handle_bench(&args);
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn handle_gron(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Gron {
        input,
        ungron,
        compact,
        paths_only,
        values_only,
        prefix,
        query,
        sort,
    } = &args.command
    else {
        unreachable!()
    };

    let content = read_input(input.as_ref())?;
    let input_format = resolve_input_format(args.from, input.as_deref(), content.as_bytes());

    if *ungron {
        // Convert gron back to structured format
        let value = ungron_to_value(&content)?;
        let output_format = resolve_output_format(args.to, Format::Json);
        let output = value_to_string(&value, output_format, args.pretty, args.compact, 2)?;
        write_output(&output, args.output.as_ref())?;
        return Ok(());
    }

    // Build gron options
    let mut opts = GronOptions::with_prefix(prefix);
    if *compact || args.compact {
        opts = opts.compact();
    }
    if *paths_only {
        opts = opts.paths_only();
    }
    if *values_only {
        opts = opts.values_only();
    }
    if args.color {
        opts = opts.color();
    }

    // For JSON input, use tape-based gron for maximum performance
    // For other formats, parse to value first then use value-based gron
    let output = if input_format == Format::Json || input_format == Format::Auto {
        // Try tape-based gron first (fastest path)
        if let Ok(tape) = DsonTape::parse(&content) {
            if let Some(query_str) = query {
                // For query mode, use value-based path
                let json_str = serde_json::to_string(&tape_to_value(&tape)?)?;
                let q = Query::parse(query_str)?;
                let query_opts = GronQueryOptions {
                    gron: opts,
                    max_matches: 0,
                    include_containers: false,
                };
                gron_query(&json_str, &q, &query_opts)?
            } else {
                // Use tape-based gron for maximum performance
                let mut result = gron_from_tape(&tape, &opts)?;
                if *sort {
                    let mut lines: Vec<&str> = result.lines().collect();
                    lines.sort_unstable();
                    result = lines.join("\n");
                }
                result
            }
        } else {
            // Fall back to value-based gron
            let value = parse_to_value(&content, input_format)?;
            let json_str = serde_json::to_string(&value)?;
            let mut result = gron(&json_str, &opts)?;
            if *sort {
                let mut lines: Vec<&str> = result.lines().collect();
                lines.sort_unstable();
                result = lines.join("\n");
            }
            result
        }
    } else {
        // Non-JSON input: parse to value first
        let value = parse_to_value(&content, input_format)?;
        let json_str = serde_json::to_string(&value)?;

        if let Some(query_str) = query {
            let q = Query::parse(query_str)?;
            let query_opts = GronQueryOptions {
                gron: opts,
                max_matches: 0,
                include_containers: false,
            };
            gron_query(&json_str, &q, &query_opts)?
        } else {
            let mut result = gron(&json_str, &opts)?;
            if *sort {
                let mut lines: Vec<&str> = result.lines().collect();
                lines.sort_unstable();
                result = lines.join("\n");
            }
            result
        }
    };

    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_diff(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Diff {
        file1,
        file2,
        diff_format,
        ignore_order,
    } = &args.command
    else {
        unreachable!()
    };

    let content1 = fs::read_to_string(file1)?;
    let content2 = fs::read_to_string(file2)?;

    let format1 = resolve_input_format(args.from, Some(file1), content1.as_bytes());
    let format2 = resolve_input_format(args.from, Some(file2), content2.as_bytes());

    // Parse both files to values (works with all formats)
    let mut value1 = parse_to_value(&content1, format1)?;
    let mut value2 = parse_to_value(&content2, format2)?;

    // If ignore-order is set, sort all arrays recursively
    if *ignore_order {
        value1 = sort_arrays_recursive(&value1);
        value2 = sort_arrays_recursive(&value2);
    }

    // For JSON-only inputs, we can use tape-based operations for better performance
    let both_json = format1 == Format::Json && format2 == Format::Json;

    let output = match diff_format.as_str() {
        "merge-patch" => {
            // RFC 7396 merge patch - just output the target as the patch
            serde_json::to_string_pretty(&value2)?
        }
        "gron" => {
            // Diff as gron (show changed lines)
            let json1 = serde_json::to_string(&value1)?;
            let json2 = serde_json::to_string(&value2)?;

            // Use tape-based gron for JSON, value-based for others
            let (gron1, gron2) = if both_json {
                let tape1 = DsonTape::parse(&content1).map_err(|e| format!("Parse error: {e}"))?;
                let tape2 = DsonTape::parse(&content2).map_err(|e| format!("Parse error: {e}"))?;
                (
                    gron_from_tape(&tape1, &GronOptions::default())?,
                    gron_from_tape(&tape2, &GronOptions::default())?,
                )
            } else {
                (
                    gron(&json1, &GronOptions::default())?,
                    gron(&json2, &GronOptions::default())?,
                )
            };

            let lines1: std::collections::HashSet<&str> = gron1.lines().collect();
            let lines2: std::collections::HashSet<&str> = gron2.lines().collect();

            let mut diff_lines = Vec::new();
            for line in &lines1 {
                if !lines2.contains(line) {
                    diff_lines.push(format!("- {line}"));
                }
            }
            for line in &lines2 {
                if !lines1.contains(line) {
                    diff_lines.push(format!("+ {line}"));
                }
            }
            diff_lines.sort_unstable();
            diff_lines.join("\n")
        }
        "tape" => {
            // Native tape diff - only works for JSON inputs
            if !both_json {
                return Err("--diff-format=tape only works with JSON inputs. Use default json-patch for cross-format diff.".into());
            }
            let tape1 = DsonTape::parse(&content1).map_err(|e| format!("Parse error: {e}"))?;
            let tape2 = DsonTape::parse(&content2).map_err(|e| format!("Parse error: {e}"))?;
            let tape_diff = diff_tapes(&tape1, &tape2)?;
            // Convert TapeDiff operations to JSON array for output
            let ops: Vec<serde_json::Value> = tape_diff
                .operations
                .iter()
                .map(|op| match op {
                    fionn_diff::TapeDiffOp::Add { path, value } => {
                        serde_json::json!({"op": "add", "path": path, "value": format!("{value:?}")})
                    }
                    fionn_diff::TapeDiffOp::Remove { path } => {
                        serde_json::json!({"op": "remove", "path": path})
                    }
                    fionn_diff::TapeDiffOp::Replace { path, value } => {
                        serde_json::json!({"op": "replace", "path": path, "value": format!("{value:?}")})
                    }
                    fionn_diff::TapeDiffOp::Move { from, path } => {
                        serde_json::json!({"op": "move", "from": from, "path": path})
                    }
                    fionn_diff::TapeDiffOp::Copy { from, path } => {
                        serde_json::json!({"op": "copy", "from": from, "path": path})
                    }
                    _ => serde_json::json!({"op": "unknown"}),
                })
                .collect();
            serde_json::to_string_pretty(&ops)?
        }
        _ => {
            // Default: JSON Patch (RFC 6902) - works with all formats
            let patch = json_diff(&value1, &value2);
            serde_json::to_string_pretty(&patch)?
        }
    };

    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_patch(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Patch {
        file,
        patch,
        patch_format,
        dry_run,
    } = &args.command
    else {
        unreachable!()
    };

    let content = fs::read_to_string(file)?;
    let patch_content = fs::read_to_string(patch)?;

    let input_format = resolve_input_format(args.from, Some(file), content.as_bytes());
    let value = parse_to_value(&content, input_format)?;

    let patched = if patch_format.as_str() == "merge-patch" {
        let patch_value: Value = serde_json::from_str(&patch_content)?;
        json_merge_patch(&value, &patch_value)
    } else {
        // JSON Patch (RFC 6902)
        let patch_ops: fionn_diff::JsonPatch = serde_json::from_str(&patch_content)?;
        apply_patch(&value, &patch_ops)?
    };

    let output_format = resolve_output_format(args.to, input_format);
    let output = value_to_string(&patched, output_format, args.pretty, args.compact, 2)?;

    if *dry_run && !args.quiet {
        eprintln!("Dry run - would produce:");
    }

    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_merge(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Merge {
        files,
        deep,
        array_strategy,
    } = &args.command
    else {
        unreachable!()
    };

    if files.is_empty() {
        return Err("No files provided for merge".into());
    }

    // Parse first file as base (works with all formats)
    let content = fs::read_to_string(&files[0])?;
    let input_format = resolve_input_format(args.from, Some(&files[0]), content.as_bytes());
    let mut result = parse_to_value(&content, input_format)?;

    // Check if all files are JSON for potential tape-based optimization
    let all_json = input_format == Format::Json
        && files[1..].iter().all(|f| {
            fs::read(f)
                .map(|c| resolve_input_format(args.from, Some(f), &c) == Format::Json)
                .unwrap_or(false)
        });

    // Only use tape-based merge for simple replace strategy (default behavior)
    let use_tape_optimization = all_json && files.len() == 2 && array_strategy == "replace";

    if use_tape_optimization {
        // Optimize: use tape-based merge for two JSON files
        let overlay_content = fs::read_to_string(&files[1])?;
        let base_tape = DsonTape::parse(&content).map_err(|e| format!("Parse error: {e}"))?;
        let overlay_tape =
            DsonTape::parse(&overlay_content).map_err(|e| format!("Parse error: {e}"))?;

        result = if *deep {
            deep_merge_tapes(&base_tape, &overlay_tape)?
        } else {
            merge_tapes(&base_tape, &overlay_tape)?
        };
    } else {
        // General case: value-based merge with array strategy support
        for file in &files[1..] {
            let file_content = fs::read_to_string(file)?;
            let file_format = resolve_input_format(args.from, Some(file), file_content.as_bytes());
            let overlay_value = parse_to_value(&file_content, file_format)?;

            result = if *deep {
                deep_merge_with_array_strategy(&result, &overlay_value, array_strategy)
            } else {
                json_merge_patch(&result, &overlay_value)
            };
        }
    }

    let output_format = resolve_output_format(args.to, input_format);
    let output = value_to_string(&result, output_format, args.pretty, args.compact, 2)?;
    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_query(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Query {
        query,
        file,
        raw,
        first,
    } = &args.command
    else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());
    let value = parse_to_value(&content, input_format)?;

    // Execute query and collect matching values
    let matches = execute_query(&value, query);

    // Apply --first flag
    let matches = if *first && !matches.is_empty() {
        vec![matches.into_iter().next().unwrap()]
    } else {
        matches
    };

    // Format output
    let output_format = resolve_output_format(args.to, Format::Json);
    let output = if *raw {
        // Raw mode: output values without JSON encoding (strings unquoted)
        matches
            .iter()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Null => "null".to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                other => serde_json::to_string(other).unwrap_or_default(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else if matches.len() == 1 {
        // Single result: output directly
        value_to_string(&matches[0], output_format, args.pretty, args.compact, 2)?
    } else {
        // Multiple results: output as array
        let arr = Value::Array(matches);
        value_to_string(&arr, output_format, args.pretty, args.compact, 2)?
    };

    write_output(&output, args.output.as_ref())?;
    Ok(())
}

/// Execute a JSONPath-like query and return matching values
fn execute_query(value: &Value, query: &str) -> Vec<Value> {
    let mut results = Vec::new();
    let query = query.trim();

    // Handle root query
    if query == "." || query.is_empty() {
        return vec![value.clone()];
    }

    // Parse and execute query segments
    let segments = parse_query_path(query);
    collect_query_matches(value, &segments, 0, &mut results);

    results
}

/// Parse query path into segments
fn parse_query_path(query: &str) -> Vec<QuerySegmentParsed> {
    let mut segments = Vec::new();
    let mut query = query.trim_start_matches('$');

    // Handle leading .. for recursive descent
    if query.starts_with("..") {
        query = &query[2..];
        // Collect field name after ..
        let field_end = query.find(['.', '[']).unwrap_or(query.len());
        let field = &query[..field_end];
        if !field.is_empty() {
            segments.push(QuerySegmentParsed::RecursiveField(field.to_string()));
        }
        query = &query[field_end..];
    } else if query.starts_with('.') {
        query = &query[1..];
    }

    if query.is_empty() {
        return segments;
    }

    let mut chars = query.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !current.is_empty() {
                    segments.push(QuerySegmentParsed::Field(current.clone()));
                    current.clear();
                }
                // Check for recursive descent (..)
                if chars.peek() == Some(&'.') {
                    chars.next();
                    // Collect the field name after ..
                    while let Some(&nc) = chars.peek() {
                        if nc == '.' || nc == '[' {
                            break;
                        }
                        current.push(chars.next().unwrap());
                    }
                    if !current.is_empty() {
                        segments.push(QuerySegmentParsed::RecursiveField(current.clone()));
                        current.clear();
                    }
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(QuerySegmentParsed::Field(current.clone()));
                    current.clear();
                }
                // Parse bracket content
                let mut bracket_content = String::new();
                for bc in chars.by_ref() {
                    if bc == ']' {
                        break;
                    }
                    bracket_content.push(bc);
                }
                // Determine bracket type
                if bracket_content == "*" {
                    segments.push(QuerySegmentParsed::Wildcard);
                } else if let Ok(idx) = bracket_content.parse::<usize>() {
                    segments.push(QuerySegmentParsed::Index(idx));
                } else {
                    // Quoted field name
                    let field = bracket_content.trim_matches('"').trim_matches('\'');
                    segments.push(QuerySegmentParsed::Field(field.to_string()));
                }
            }
            '*' => {
                if !current.is_empty() {
                    segments.push(QuerySegmentParsed::Field(current.clone()));
                    current.clear();
                }
                segments.push(QuerySegmentParsed::Wildcard);
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        segments.push(QuerySegmentParsed::Field(current));
    }

    segments
}

#[derive(Debug, Clone)]
enum QuerySegmentParsed {
    Field(String),
    Index(usize),
    Wildcard,
    RecursiveField(String),
}

/// Collect values matching query segments
fn collect_query_matches(
    value: &Value,
    segments: &[QuerySegmentParsed],
    segment_idx: usize,
    results: &mut Vec<Value>,
) {
    if segment_idx >= segments.len() {
        results.push(value.clone());
        return;
    }

    match &segments[segment_idx] {
        QuerySegmentParsed::Field(name) => {
            if let Value::Object(obj) = value
                && let Some(v) = obj.get(name)
            {
                collect_query_matches(v, segments, segment_idx + 1, results);
            }
        }
        QuerySegmentParsed::Index(idx) => {
            if let Value::Array(arr) = value
                && let Some(v) = arr.get(*idx)
            {
                collect_query_matches(v, segments, segment_idx + 1, results);
            }
        }
        QuerySegmentParsed::Wildcard => match value {
            Value::Array(arr) => {
                for item in arr {
                    collect_query_matches(item, segments, segment_idx + 1, results);
                }
            }
            Value::Object(obj) => {
                for v in obj.values() {
                    collect_query_matches(v, segments, segment_idx + 1, results);
                }
            }
            _ => {}
        },
        QuerySegmentParsed::RecursiveField(name) => {
            // Search recursively for the field
            recursive_field_search(value, name, segments, segment_idx + 1, results);
        }
    }
}

/// Recursively search for a field name
fn recursive_field_search(
    value: &Value,
    field_name: &str,
    segments: &[QuerySegmentParsed],
    next_segment: usize,
    results: &mut Vec<Value>,
) {
    match value {
        Value::Object(obj) => {
            // Check if this object has the field
            if let Some(v) = obj.get(field_name) {
                collect_query_matches(v, segments, next_segment, results);
            }
            // Recurse into all values
            for v in obj.values() {
                recursive_field_search(v, field_name, segments, next_segment, results);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                recursive_field_search(item, field_name, segments, next_segment, results);
            }
        }
        _ => {}
    }
}

fn handle_convert(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Convert { file, sort_keys } = &args.command else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());
    let mut value = parse_to_value(&content, input_format)?;

    if *sort_keys {
        value = sort_json_keys(&value);
    }

    let output_format = args.to.unwrap_or(Format::Json);
    if output_format == Format::Auto {
        return Err("Output format must be specified with --to for convert command".into());
    }

    let output = value_to_string(&value, output_format, args.pretty, args.compact, 2)?;
    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_format(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Format {
        file,
        indent,
        sort_keys,
    } = &args.command
    else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());
    let mut value = parse_to_value(&content, input_format)?;

    if *sort_keys {
        value = sort_json_keys(&value);
    }

    let output_format = resolve_output_format(args.to, input_format);
    let output = value_to_string(&value, output_format, true, args.compact, *indent)?;
    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_validate(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Validate { file, strict } = &args.command else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());

    // Try to parse - will error if invalid
    let value = parse_to_value(&content, input_format)?;

    // Strict mode: additional validation checks
    if *strict {
        let mut warnings = Vec::new();
        validate_strict(&value, "", &mut warnings);

        if !warnings.is_empty() {
            for warning in &warnings {
                eprintln!("Warning: {warning}");
            }
            return Err(format!(
                "Strict validation failed with {} warning(s)",
                warnings.len()
            )
            .into());
        }
    }

    if !args.quiet {
        let format_name = match input_format {
            Format::Json => "JSON",
            Format::Jsonl => "JSONL",
            #[cfg(feature = "yaml")]
            Format::Yaml => "YAML",
            #[cfg(feature = "toml")]
            Format::Toml => "TOML",
            #[cfg(feature = "csv")]
            Format::Csv => "CSV",
            #[cfg(feature = "ison")]
            Format::Ison => "ISON",
            #[cfg(feature = "ison")]
            Format::Isonl => "ISONL",
            #[cfg(feature = "toon")]
            Format::Toon => "TOON",
            Format::Gron => "Gron",
            Format::Auto => "Auto-detected",
        };
        let mode = if *strict { " (strict)" } else { "" };
        println!("{format_name} is valid{mode}");
    }
    Ok(())
}

/// Perform strict validation checks on a value
fn validate_strict(value: &Value, path: &str, warnings: &mut Vec<String>) {
    match value {
        Value::Object(obj) => {
            // Check for empty keys
            for (key, val) in obj {
                if key.is_empty() {
                    warnings.push(format!(
                        "Empty key at {}",
                        if path.is_empty() { "root" } else { path }
                    ));
                }
                // Check for duplicate-looking keys (case insensitive)
                let lower_key = key.to_lowercase();
                let has_case_duplicate = obj
                    .keys()
                    .any(|k| k.to_lowercase() == lower_key && k != key);
                if has_case_duplicate {
                    warnings.push(format!(
                        "Case-insensitive duplicate key '{}' at {}",
                        key,
                        if path.is_empty() { "root" } else { path }
                    ));
                }

                let new_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                validate_strict(val, &new_path, warnings);
            }
        }
        Value::Array(arr) => {
            // Check for mixed types in arrays
            if arr.len() > 1 {
                let first_type = value_type_name(arr.first());
                for (i, item) in arr.iter().enumerate().skip(1) {
                    let item_type = value_type_name(Some(item));
                    if item_type != first_type && first_type != "null" && item_type != "null" {
                        warnings.push(format!(
                            "Mixed array types at {}[{}]: expected {}, found {}",
                            if path.is_empty() { "root" } else { path },
                            i,
                            first_type,
                            item_type
                        ));
                        break; // Only report once per array
                    }
                }
            }
            for (i, item) in arr.iter().enumerate() {
                let new_path = format!("{path}[{i}]");
                validate_strict(item, &new_path, warnings);
            }
        }
        Value::String(s) => {
            // Check for suspicious string patterns
            if s.trim().is_empty() && !s.is_empty() {
                warnings.push(format!(
                    "Whitespace-only string at {}",
                    if path.is_empty() { "root" } else { path }
                ));
            }
        }
        Value::Number(n) => {
            // Check for NaN-like or problematic numbers
            if let Some(f) = n.as_f64()
                && f.is_infinite()
            {
                warnings.push(format!(
                    "Infinite number at {}",
                    if path.is_empty() { "root" } else { path }
                ));
            }
        }
        _ => {}
    }
}

const fn value_type_name(value: Option<&Value>) -> &'static str {
    match value {
        None => "none",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

fn handle_stream(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Stream {
        file,
        filter,
        limit,
        skip,
        fields,
        ops,
        use_simd,
    } = &args.command
    else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());

    // Check if we should use SIMD-accelerated ISONL processing
    #[cfg(feature = "ison")]
    if *use_simd && matches!(input_format, Format::Isonl) {
        return handle_stream_isonl_simd(
            args,
            content.as_bytes(),
            filter.as_deref(),
            *limit,
            skip.unwrap_or(0),
            fields.as_deref(),
            ops.as_deref(),
        );
    }

    // Suppress warning when ison feature is not enabled
    let _ = use_simd;

    // Fall back to standard line-by-line processing
    handle_stream_generic(
        args,
        &content,
        input_format,
        filter,
        limit,
        skip,
        fields,
        ops,
    )
}

/// Generic streaming handler for JSONL and other line-delimited formats
#[allow(clippy::too_many_arguments)] // Stream handler needs multiple configuration parameters
#[allow(clippy::ref_option)] // Pattern matches CLI argument types
fn handle_stream_generic(
    args: &Args,
    content: &str,
    input_format: Format,
    filter: &Option<String>,
    limit: &Option<usize>,
    skip: &Option<usize>,
    fields: &Option<String>,
    ops: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    let mut count = 0;
    let skip_count = skip.unwrap_or(0);

    // Parse field list for selective extraction
    let field_list: Vec<&str> = fields
        .as_ref()
        .map(|f| f.split(',').map(str::trim).collect())
        .unwrap_or_default();

    // Parse DSON operations if provided
    let dson_ops: Vec<serde_json::Value> = ops
        .as_ref()
        .map(|o| serde_json::from_str(o))
        .transpose()?
        .unwrap_or_default();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Skip initial records if requested
        if count < skip_count {
            count += 1;
            continue;
        }

        // Parse line
        let mut value = parse_to_value(line, input_format)?;

        // Apply field filtering if specified
        if !field_list.is_empty() {
            value = extract_fields(&value, &field_list);
        }

        // Apply DSON operations if specified
        if !dson_ops.is_empty() {
            value = apply_dson_ops(value, &dson_ops)?;
        }

        // Apply filter if specified
        if let Some(filter_expr) = filter
            && !evaluate_filter(&value, filter_expr)
        {
            count += 1;
            continue; // Skip non-matching records
        }

        results.push(value);

        // Check limit
        if let Some(lim) = limit
            && results.len() >= *lim
        {
            break;
        }

        count += 1;
    }

    // Output results
    let output_format = resolve_output_format(args.to, Format::Json);
    for value in &results {
        let output = value_to_string(value, output_format, false, true, 0)?;
        println!("{output}");
    }

    if !args.quiet {
        eprintln!("Processed {} records", results.len());
    }
    Ok(())
}

/// SIMD-accelerated ISONL streaming handler
#[cfg(feature = "ison")]
fn handle_stream_isonl_simd(
    args: &Args,
    data: &[u8],
    filter: Option<&str>,
    limit: Option<usize>,
    skip: usize,
    fields: Option<&str>,
    ops: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use fionn_stream::skiptape::isonl::{IsonlBatchResult, SimdIsonlBatchProcessor};

    let mut processor = SimdIsonlBatchProcessor::new();

    // Process entire batch with SIMD acceleration
    let batch_result: IsonlBatchResult = processor
        .process_batch_unfiltered(data)
        .map_err(|e| format!("ISONL processing error: {e}"))?;

    // Parse field list for selective extraction
    let field_list: Vec<&str> = fields
        .map(|f| f.split(',').map(str::trim).collect())
        .unwrap_or_default();

    // Parse DSON operations if provided
    let dson_ops: Vec<serde_json::Value> = ops
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();

    let mut count = 0;
    let mut output_count = 0;
    let output_format = resolve_output_format(args.to, Format::Json);

    for doc in &batch_result.documents {
        // Skip initial records if requested
        if count < skip {
            count += 1;
            continue;
        }

        // Parse the JSON document
        let mut value: Value = serde_json::from_str(doc)?;

        // Apply field filtering if specified
        if !field_list.is_empty() {
            value = extract_fields(&value, &field_list);
        }

        // Apply DSON operations if specified
        if !dson_ops.is_empty() {
            value = apply_dson_ops(value, &dson_ops)?;
        }

        // Apply filter if specified
        if let Some(filter_expr) = filter
            && !evaluate_filter(&value, filter_expr)
        {
            count += 1;
            continue;
        }

        // Output the result
        let output = value_to_string(&value, output_format, false, true, 0)?;
        println!("{output}");
        output_count += 1;

        // Check limit
        if let Some(lim) = limit
            && output_count >= lim
        {
            break;
        }

        count += 1;
    }

    if !args.quiet {
        eprintln!(
            "Processed {} records ({} successful, {} errors) via SIMD",
            batch_result.statistics.total_lines,
            batch_result.statistics.successful_lines,
            batch_result.statistics.failed_lines
        );
    }
    Ok(())
}

/// Extract specific fields from a JSON value
fn extract_fields(value: &Value, fields: &[&str]) -> Value {
    if let Value::Object(obj) = value {
        let mut result = serde_json::Map::new();
        for field in fields {
            // Handle nested paths (e.g., "user.name")
            if field.contains('.') {
                let parts: Vec<&str> = field.splitn(2, '.').collect();
                if let Some(Value::Object(nested_obj)) = obj.get(parts[0])
                    && let Some(v) = nested_obj.get(parts[1])
                {
                    result.insert((*field).to_string(), v.clone());
                }
            } else if let Some(v) = obj.get(*field) {
                result.insert((*field).to_string(), v.clone());
            }
        }
        Value::Object(result)
    } else {
        value.clone()
    }
}

/// Apply DSON operations to a JSON value
fn apply_dson_ops(
    mut value: Value,
    ops: &[serde_json::Value],
) -> Result<Value, Box<dyn std::error::Error>> {
    for op in ops {
        if let Some(field_add) = op.get("FieldAdd") {
            let path = field_add
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("FieldAdd requires 'path'")?;
            let new_value = field_add
                .get("value")
                .ok_or("FieldAdd requires 'value'")?
                .clone();

            if let Value::Object(ref mut obj) = value {
                obj.insert(path.to_string(), new_value);
            }
        } else if let Some(field_modify) = op.get("FieldModify") {
            let path = field_modify
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("FieldModify requires 'path'")?;
            let new_value = field_modify
                .get("value")
                .ok_or("FieldModify requires 'value'")?
                .clone();

            if let Value::Object(ref mut obj) = value
                && obj.contains_key(path)
            {
                obj.insert(path.to_string(), new_value);
            }
        } else if let Some(field_delete) = op.get("FieldDelete") {
            let path = field_delete
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("FieldDelete requires 'path'")?;

            if let Value::Object(ref mut obj) = value {
                obj.remove(path);
            }
        }
    }
    Ok(value)
}

/// Evaluate a filter expression against a value
/// Supports: path queries (existence check), comparisons (>, <, >=, <=, ==, !=)
fn evaluate_filter(value: &Value, filter: &str) -> bool {
    let filter = filter.trim();

    // Try to parse as comparison expression
    for op in &[">=", "<=", "!=", "==", ">", "<"] {
        if let Some(pos) = filter.find(op) {
            let path = filter[..pos].trim();
            let rhs = filter[pos + op.len()..].trim();

            // Get value at path
            let matches = execute_query(value, path);
            if matches.is_empty() {
                return false;
            }

            let lhs = &matches[0];

            // Parse right-hand side
            let rhs_value: Value = if rhs == "null" {
                Value::Null
            } else if rhs == "true" {
                Value::Bool(true)
            } else if rhs == "false" {
                Value::Bool(false)
            } else if let Ok(n) = rhs.parse::<i64>() {
                Value::Number(n.into())
            } else if let Ok(n) = rhs.parse::<f64>() {
                serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
            } else {
                // String (remove quotes if present)
                let s = rhs.trim_matches('"').trim_matches('\'');
                Value::String(s.to_string())
            };

            return compare_values(lhs, &rhs_value, op);
        }
    }

    // No comparison operator - treat as existence check
    let matches = execute_query(value, filter);
    !matches.is_empty()
}

/// Compare two JSON values with an operator
fn compare_values(lhs: &Value, rhs: &Value, op: &str) -> bool {
    match op {
        "==" => lhs == rhs,
        "!=" => lhs != rhs,
        ">" | ">=" | "<" | "<=" => {
            // Numeric comparison
            let lhs_num = match lhs {
                Value::Number(n) => n.as_f64(),
                _ => None,
            };
            let rhs_num = match rhs {
                Value::Number(n) => n.as_f64(),
                _ => None,
            };

            if let (Some(l), Some(r)) = (lhs_num, rhs_num) {
                match op {
                    ">" => l > r,
                    ">=" => l >= r,
                    "<" => l < r,
                    "<=" => l <= r,
                    _ => false,
                }
            } else {
                // String comparison fallback
                let lhs_str = match lhs {
                    Value::String(s) => s.as_str(),
                    _ => return false,
                };
                let rhs_str = match rhs {
                    Value::String(s) => s.as_str(),
                    _ => return false,
                };
                match op {
                    ">" => lhs_str > rhs_str,
                    ">=" => lhs_str >= rhs_str,
                    "<" => lhs_str < rhs_str,
                    "<=" => lhs_str <= rhs_str,
                    _ => false,
                }
            }
        }
        _ => false,
    }
}

fn handle_schema(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Schema {
        file,
        schema_format,
    } = &args.command
    else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());
    let value = parse_to_value(&content, input_format)?;

    let schema = infer_schema(&value);

    let output = match schema_format.as_str() {
        "typescript" => schema_to_typescript(&schema),
        "rust" => schema_to_rust(&schema),
        _ => serde_json::to_string_pretty(&schema)?,
    };

    write_output(&output, args.output.as_ref())?;
    Ok(())
}

#[allow(clippy::too_many_lines)] // Ops handler dispatches many operation types inline
fn handle_ops(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Ops { op, file, path } = &args.command else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());
    let value = parse_to_value(&content, input_format)?;

    // Navigate to path if specified
    let target = if let Some(p) = path {
        get_at_path(&value, p).ok_or_else(|| format!("Path not found: {p}"))?
    } else {
        &value
    };

    let result: Value = match op.as_str() {
        "keys" => {
            if let Value::Object(obj) = target {
                Value::Array(obj.keys().map(|k| Value::String(k.clone())).collect())
            } else {
                return Err("keys requires an object".into());
            }
        }
        "values" => {
            if let Value::Object(obj) = target {
                Value::Array(obj.values().cloned().collect())
            } else {
                return Err("values requires an object".into());
            }
        }
        "entries" => {
            if let Value::Object(obj) = target {
                Value::Array(obj.iter().map(|(k, v)| serde_json::json!([k, v])).collect())
            } else {
                return Err("entries requires an object".into());
            }
        }
        "length" | "len" | "count" => match target {
            Value::Array(arr) => serde_json::json!(arr.len()),
            Value::Object(obj) => serde_json::json!(obj.len()),
            Value::String(s) => serde_json::json!(s.len()),
            _ => return Err("length not applicable to this type".into()),
        },
        "type" => {
            let type_name = match target {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            Value::String(type_name.to_string())
        }
        "flatten" => flatten_value(target, ""),
        "paths" => {
            let paths = collect_paths(target, "");
            Value::Array(paths.into_iter().map(Value::String).collect())
        }
        "first" => {
            if let Value::Array(arr) = target {
                arr.first().cloned().unwrap_or(Value::Null)
            } else {
                return Err("first requires an array".into());
            }
        }
        "last" => {
            if let Value::Array(arr) = target {
                arr.last().cloned().unwrap_or(Value::Null)
            } else {
                return Err("last requires an array".into());
            }
        }
        "reverse" => {
            if let Value::Array(arr) = target {
                let mut reversed = arr.clone();
                reversed.reverse();
                Value::Array(reversed)
            } else {
                return Err("reverse requires an array".into());
            }
        }
        "sort" => {
            if let Value::Array(arr) = target {
                let mut sorted = arr.clone();
                sorted.sort_by(|a, b| {
                    let a_str = a.to_string();
                    let b_str = b.to_string();
                    a_str.cmp(&b_str)
                });
                Value::Array(sorted)
            } else {
                return Err("sort requires an array".into());
            }
        }
        "unique" => {
            if let Value::Array(arr) = target {
                let mut seen = std::collections::HashSet::new();
                let unique: Vec<Value> = arr
                    .iter()
                    .filter(|v| {
                        let s = v.to_string();
                        seen.insert(s)
                    })
                    .cloned()
                    .collect();
                Value::Array(unique)
            } else {
                return Err("unique requires an array".into());
            }
        }
        _ => return Err(format!("Unknown operation: {op}").into()),
    };

    let output_format = resolve_output_format(args.to, Format::Json);
    let output = value_to_string(&result, output_format, args.pretty, args.compact, 2)?;
    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_stats(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::Stats { file } = &args.command else {
        unreachable!()
    };

    let content = read_input(file.as_ref())?;
    let input_format = resolve_input_format(args.from, file.as_deref(), content.as_bytes());
    let value = parse_to_value(&content, input_format)?;

    let stats = compute_stats(&value);
    let output = serde_json::to_string_pretty(&stats)?;
    write_output(&output, args.output.as_ref())?;
    Ok(())
}

fn handle_bench(args: &Args) {
    use std::time::Instant;

    let Commands::Bench { bench_type } = &args.command else {
        unreachable!()
    };

    let data = r#"{"test": "data", "number": 42, "array": [1,2,3], "nested": {"a": 1, "b": 2}}"#
        .repeat(1000);

    if matches!(bench_type.as_str(), "parse" | "all") {
        let start = Instant::now();
        for _ in 0..100 {
            let _: Value = serde_json::from_str(&data).unwrap();
        }
        println!("Parse: 100 iterations in {:?}", start.elapsed());
    }

    if matches!(bench_type.as_str(), "gron" | "all") {
        let start = Instant::now();
        for _ in 0..100 {
            let _ = gron(&data, &GronOptions::default()).unwrap();
        }
        println!("Gron: 100 iterations in {:?}", start.elapsed());
    }

    if matches!(bench_type.as_str(), "diff" | "all") {
        let value1: Value = serde_json::from_str(&data).unwrap();
        let mut value2 = value1.clone();
        if let Value::Object(ref mut obj) = value2 {
            obj.insert("new_field".to_string(), serde_json::json!("new_value"));
        }
        let start = Instant::now();
        for _ in 0..100 {
            let _ = json_diff(&value1, &value2);
        }
        println!("Diff: 100 iterations in {:?}", start.elapsed());
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Sort JSON object keys recursively
fn sort_json_keys(value: &Value) -> Value {
    match value {
        Value::Object(obj) => {
            let mut sorted: Vec<_> = obj.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let sorted_map: serde_json::Map<String, Value> = sorted
                .into_iter()
                .map(|(k, v)| (k.clone(), sort_json_keys(v)))
                .collect();
            Value::Object(sorted_map)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_json_keys).collect()),
        other => other.clone(),
    }
}

/// Sort all arrays recursively (for ignore-order diff)
fn sort_arrays_recursive(value: &Value) -> Value {
    match value {
        Value::Object(obj) => {
            let sorted_map: serde_json::Map<String, Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), sort_arrays_recursive(v)))
                .collect();
            Value::Object(sorted_map)
        }
        Value::Array(arr) => {
            // First, recursively sort nested structures
            let mut sorted: Vec<Value> = arr.iter().map(sort_arrays_recursive).collect();
            // Then sort the array elements themselves by their string representation
            sorted.sort_by_key(ToString::to_string);
            Value::Array(sorted)
        }
        other => other.clone(),
    }
}

/// Deep merge with configurable array strategy
fn deep_merge_with_array_strategy(base: &Value, overlay: &Value, array_strategy: &str) -> Value {
    match (base, overlay) {
        (Value::Object(base_obj), Value::Object(overlay_obj)) => {
            let mut result = base_obj.clone();
            for (key, overlay_val) in overlay_obj {
                let merged = result.get(key).map_or_else(
                    || overlay_val.clone(),
                    |base_val| {
                        deep_merge_with_array_strategy(base_val, overlay_val, array_strategy)
                    },
                );
                result.insert(key.clone(), merged);
            }
            Value::Object(result)
        }
        (Value::Array(base_arr), Value::Array(overlay_arr)) => {
            match array_strategy {
                "append" => {
                    // Append overlay array to base array
                    let mut result = base_arr.clone();
                    result.extend(overlay_arr.iter().cloned());
                    Value::Array(result)
                }
                "concat" => {
                    // Same as append but with deduplication
                    let mut result = base_arr.clone();
                    for item in overlay_arr {
                        if !result.iter().any(|x| x == item) {
                            result.push(item.clone());
                        }
                    }
                    Value::Array(result)
                }
                _ => {
                    // "replace" - default behavior: overlay replaces base
                    Value::Array(overlay_arr.clone())
                }
            }
        }
        // For non-matching types or primitives, overlay wins
        (_, overlay_val) => overlay_val.clone(),
    }
}

/// Get value at JSON path
fn get_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        match current {
            Value::Object(obj) => {
                current = obj.get(segment)?;
            }
            Value::Array(arr) => {
                let idx: usize = segment.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Flatten nested object to dotted keys
fn flatten_value(value: &Value, prefix: &str) -> Value {
    let mut result = serde_json::Map::new();
    flatten_recursive(value, prefix, &mut result);
    Value::Object(result)
}

fn flatten_recursive(value: &Value, prefix: &str, result: &mut serde_json::Map<String, Value>) {
    match value {
        Value::Object(obj) => {
            for (k, v) in obj {
                let new_prefix = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_recursive(v, &new_prefix, result);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let new_prefix = format!("{prefix}[{i}]");
                flatten_recursive(v, &new_prefix, result);
            }
        }
        _ => {
            if !prefix.is_empty() {
                result.insert(prefix.to_string(), value.clone());
            }
        }
    }
}

/// Collect all paths in a value
fn collect_paths(value: &Value, prefix: &str) -> Vec<String> {
    let mut paths = Vec::new();
    if !prefix.is_empty() {
        paths.push(prefix.to_string());
    }

    match value {
        Value::Object(obj) => {
            for (k, v) in obj {
                let new_prefix = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                paths.extend(collect_paths(v, &new_prefix));
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let new_prefix = format!("{prefix}[{i}]");
                paths.extend(collect_paths(v, &new_prefix));
            }
        }
        _ => {}
    }
    paths
}

/// Infer JSON Schema from value
fn infer_schema(value: &Value) -> Value {
    match value {
        Value::Null => serde_json::json!({"type": "null"}),
        Value::Bool(_) => serde_json::json!({"type": "boolean"}),
        Value::Number(n) => {
            if n.is_i64() {
                serde_json::json!({"type": "integer"})
            } else {
                serde_json::json!({"type": "number"})
            }
        }
        Value::String(_) => serde_json::json!({"type": "string"}),
        Value::Array(arr) => {
            if arr.is_empty() {
                serde_json::json!({"type": "array"})
            } else {
                let item_schema = infer_schema(&arr[0]);
                serde_json::json!({"type": "array", "items": item_schema})
            }
        }
        Value::Object(obj) => {
            let mut properties = serde_json::Map::new();
            let required: Vec<Value> = obj.keys().map(|k| Value::String(k.clone())).collect();
            for (k, v) in obj {
                properties.insert(k.clone(), infer_schema(v));
            }
            serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required
            })
        }
    }
}

/// Convert schema type to TypeScript type string
fn schema_type_to_ts(schema: &Value) -> String {
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("null") => "null".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("integer" | "number") => "number".to_string(),
        Some("string") => "string".to_string(),
        Some("array") => {
            let item_type = schema
                .get("items")
                .map_or_else(|| "unknown".to_string(), schema_type_to_ts);
            format!("{item_type}[]")
        }
        Some("object") => schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map_or_else(
                || "object".to_string(),
                |props| {
                    let fields: Vec<String> = props
                        .iter()
                        .map(|(k, v)| format!("  {k}: {};", schema_type_to_ts(v)))
                        .collect();
                    format!("{{\n{}\n}}", fields.join("\n"))
                },
            ),
        _ => "unknown".to_string(),
    }
}

/// Convert schema to TypeScript type definition
fn schema_to_typescript(schema: &Value) -> String {
    format!("type Root = {};", schema_type_to_ts(schema))
}

/// Convert schema type to Rust type string
fn schema_type_to_rust(schema: &Value, name: &str) -> String {
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("null") => "()".to_string(),
        Some("boolean") => "bool".to_string(),
        Some("integer") => "i64".to_string(),
        Some("number") => "f64".to_string(),
        Some("string") => "String".to_string(),
        Some("array") => {
            let item_type = schema.get("items").map_or_else(
                || "serde_json::Value".to_string(),
                |s| schema_type_to_rust(s, "Item"),
            );
            format!("Vec<{item_type}>")
        }
        Some("object") => name.to_string(),
        _ => "serde_json::Value".to_string(),
    }
}

/// Convert schema to Rust type definition
fn schema_to_rust(schema: &Value) -> String {
    use std::fmt::Write;

    let mut output = String::from("#[derive(Debug, Serialize, Deserialize)]\n");
    output.push_str("pub struct Root {\n");

    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
        for (k, v) in props {
            let field_type = schema_type_to_rust(v, &to_pascal_case(k));
            let _ = writeln!(output, "    pub {k}: {field_type},");
        }
    }

    output.push_str("}\n");
    output
}

/// Convert string to `PascalCase`
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        })
        .collect()
}

/// Count types recursively in a JSON value
fn count_types(value: &Value, counts: &mut std::collections::HashMap<&'static str, usize>) {
    let type_name = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    *counts.entry(type_name).or_insert(0) += 1;

    match value {
        Value::Array(arr) => {
            for v in arr {
                count_types(v, counts);
            }
        }
        Value::Object(obj) => {
            for v in obj.values() {
                count_types(v, counts);
            }
        }
        _ => {}
    }
}

/// Compute max depth of a JSON value
fn max_depth(value: &Value, current: usize) -> usize {
    match value {
        Value::Array(arr) => arr
            .iter()
            .map(|v| max_depth(v, current + 1))
            .max()
            .unwrap_or(current),
        Value::Object(obj) => obj
            .values()
            .map(|v| max_depth(v, current + 1))
            .max()
            .unwrap_or(current),
        _ => current,
    }
}

/// Compute statistics about a value
fn compute_stats(value: &Value) -> Value {
    let mut stats = serde_json::Map::new();
    let mut type_counts = std::collections::HashMap::new();
    count_types(value, &mut type_counts);

    let types_obj: serde_json::Map<String, Value> = type_counts
        .into_iter()
        .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
        .collect();

    stats.insert("types".to_string(), Value::Object(types_obj));
    stats.insert(
        "max_depth".to_string(),
        serde_json::json!(max_depth(value, 0)),
    );

    if let Value::Object(obj) = value {
        stats.insert("top_level_keys".to_string(), serde_json::json!(obj.len()));
    }
    if let Value::Array(arr) = value {
        stats.insert("array_length".to_string(), serde_json::json!(arr.len()));
    }

    Value::Object(stats)
}
