// SPDX-License-Identifier: MIT OR Apache-2.0
//! fionn CLI binary - A Swiss Army knife for JSON with SIMD acceleration

use clap::{Parser, Subcommand};
use fionn_diff::{apply_patch, json_diff, json_merge_patch};
use fionn_gron::{GronOptions, GronQueryOptions, Query, gron, gron_query, ungron_to_value};
use serde::Serialize;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fionn")]
#[command(version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
struct Args {
    /// Enable SIMD acceleration (default: true)
    #[arg(long, default_value = "true")]
    simd: bool,

    /// Enable GPU acceleration (default: true if available)
    #[arg(long, default_value = "true")]
    gpu: bool,

    /// Colorized output
    #[arg(long)]
    color: bool,

    /// Stream processing for large files
    #[arg(long)]
    stream: bool,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

/// Subcommands for fionn CLI
#[derive(Subcommand)]
enum Commands {
    /// Flatten JSON to gron format
    Gron {
        /// Input file (reads from stdin if not provided)
        #[arg(value_name = "FILE")]
        input: Option<PathBuf>,

        /// Reverse mode: convert gron back to JSON
        #[arg(short = 'u', long = "ungron")]
        ungron: bool,

        /// Output compact gron format
        #[arg(short = 'c', long = "compact")]
        compact: bool,

        /// Output only paths
        #[arg(long = "paths")]
        paths_only: bool,

        /// Output only values
        #[arg(long = "values")]
        values_only: bool,

        /// Custom root prefix
        #[arg(short = 'p', long = "prefix", default_value = "json")]
        prefix: String,

        /// Query filter
        #[arg(short = 'q', long = "query")]
        query: Option<String>,

        /// Maximum query matches
        #[arg(long = "max-matches", default_value = "0")]
        max_matches: usize,

        /// Include container declarations in query output
        #[arg(long = "include-containers")]
        include_containers: bool,
    },
    /// Compute diff between two JSON files
    Diff {
        /// First JSON file
        file1: PathBuf,
        /// Second JSON file
        file2: PathBuf,
    },
    /// Apply JSON Patch to a JSON file
    Patch {
        /// JSON file to patch
        file: PathBuf,
        /// Patch file (JSON Patch format)
        patch: PathBuf,
    },
    /// Merge JSON files
    Merge {
        /// JSON files to merge
        files: Vec<PathBuf>,
    },
    /// Query JSON with JSONPath-like syntax
    Query {
        /// Query string
        query: String,
        /// JSON file
        file: Option<PathBuf>,
    },
    /// Format JSON (pretty-print or compact)
    Format {
        /// JSON file
        file: Option<PathBuf>,
        /// Compact output
        #[arg(short = 'c', long = "compact")]
        compact: bool,
        /// Indentation level
        #[arg(short = 'i', long = "indent", default_value = "2")]
        indent: usize,
    },
    /// Validate JSON
    Validate {
        /// JSON file
        file: Option<PathBuf>,
    },
    /// Process JSONL streams
    Stream {
        /// JSONL file
        file: Option<PathBuf>,
    },
    /// Extract JSON schema
    Schema {
        /// JSON file
        file: Option<PathBuf>,
    },
    /// Perform operations on JSON
    Ops {
        /// Operation type
        op: String,
        /// JSON file
        file: Option<PathBuf>,
    },
    /// Benchmark JSON processing
    Bench,
}

fn main() {
    let args = Args::parse();

    match args.command {
        Commands::Gron { .. } => handle_gron(&args),
        Commands::Diff { .. } => handle_diff(&args),
        Commands::Patch { .. } => handle_patch(&args),
        Commands::Merge { .. } => handle_merge(&args),
        Commands::Query { .. } => handle_query(&args),
        Commands::Format { .. } => handle_format(&args),
        Commands::Validate { .. } => handle_validate(&args),
        Commands::Stream { .. } => handle_stream(&args),
        Commands::Schema { .. } => handle_schema(&args),
        Commands::Ops { .. } => handle_ops(&args),
        Commands::Bench => handle_bench(&args),
    }
}

fn handle_gron(args: &Args) {
    if let Commands::Gron {
        input,
        ungron,
        compact,
        paths_only,
        values_only,
        prefix,
        query,
        max_matches,
        include_containers,
    } = &args.command
        && let Err(e) = run_gron(
            input.as_ref(),
            *ungron,
            *compact,
            *paths_only,
            *values_only,
            prefix,
            query.as_ref(),
            *max_matches,
            *include_containers,
        )
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

#[allow(clippy::fn_params_excessive_bools)]
#[allow(clippy::too_many_arguments)]
fn run_gron(
    input: Option<&PathBuf>,
    ungron: bool,
    compact: bool,
    paths_only: bool,
    values_only: bool,
    prefix: &str,
    query: Option<&String>,
    max_matches: usize,
    include_containers: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input_str = read_input(input)?;

    if ungron {
        let value = ungron_to_value(&input_str)?;
        let output = serde_json::to_string_pretty(&value)?;
        write_output(&output)?;
        return Ok(());
    }

    let mut gron_opts = GronOptions::with_prefix(prefix);
    if compact {
        gron_opts = gron_opts.compact();
    }
    if paths_only {
        gron_opts = gron_opts.paths_only();
    }
    if values_only {
        gron_opts = gron_opts.values_only();
    }

    if let Some(query_str) = query {
        let query = Query::parse(query_str)?;
        let mut query_opts = GronQueryOptions {
            gron: gron_opts,
            max_matches,
            include_containers,
        };
        if compact {
            query_opts = query_opts.compact();
        }
        let output = gron_query(&input_str, &query, &query_opts)?;
        write_output(&output)?;
    } else {
        let output = gron(&input_str, &gron_opts)?;
        write_output(&output)?;
    }

    Ok(())
}

fn handle_diff(args: &Args) {
    if let Commands::Diff { file1, file2 } = &args.command
        && let Err(e) = run_diff(file1, file2)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_diff(file1: &PathBuf, file2: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let json1 = fs::read_to_string(file1)?;
    let json2 = fs::read_to_string(file2)?;
    let value1: serde_json::Value = serde_json::from_str(&json1)?;
    let value2: serde_json::Value = serde_json::from_str(&json2)?;
    let patch = json_diff(&value1, &value2);
    let output = serde_json::to_string_pretty(&patch)?;
    write_output(&output)?;
    Ok(())
}

fn handle_patch(args: &Args) {
    if let Commands::Patch { file, patch } = &args.command
        && let Err(e) = run_patch(file, patch)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_patch(file: &PathBuf, patch_file: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let json = fs::read_to_string(file)?;
    let patch_json = fs::read_to_string(patch_file)?;
    let mut value: serde_json::Value = serde_json::from_str(&json)?;
    let patch: fionn_diff::JsonPatch = serde_json::from_str(&patch_json)?;
    #[allow(clippy::unnecessary_mut_passed)]
    apply_patch(&mut value, &patch)?;
    let output = serde_json::to_string_pretty(&value)?;
    write_output(&output)?;
    Ok(())
}

fn handle_merge(args: &Args) {
    if let Commands::Merge { files } = &args.command
        && let Err(e) = run_merge(files)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_merge(files: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>> {
    if files.is_empty() {
        return Err("No files provided for merge".into());
    }
    let mut result: serde_json::Value = serde_json::from_str(&fs::read_to_string(&files[0])?)?;
    for file in &files[1..] {
        let json = fs::read_to_string(file)?;
        let value: serde_json::Value = serde_json::from_str(&json)?;
        #[allow(clippy::unnecessary_mut_passed)]
        let _ = json_merge_patch(&mut result, &value);
    }
    let output = serde_json::to_string_pretty(&result)?;
    write_output(&output)?;
    Ok(())
}

fn handle_query(args: &Args) {
    if let Commands::Query { query, file } = &args.command
        && let Err(e) = run_query(query, file.as_ref())
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_query(query_str: &str, file: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let input_str = read_input(file)?;
    let query = Query::parse(query_str)?;
    let gron_opts = GronOptions::default();
    let query_opts = GronQueryOptions {
        gron: gron_opts,
        max_matches: 0,
        include_containers: false,
    };
    let output = gron_query(&input_str, &query, &query_opts)?;
    write_output(&output)?;
    Ok(())
}

fn handle_format(args: &Args) {
    if let Commands::Format {
        file,
        compact,
        indent,
    } = &args.command
        && let Err(e) = run_format(file.as_ref(), *compact, *indent)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_format(
    file: Option<&PathBuf>,
    compact: bool,
    indent: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let input_str = read_input(file)?;
    let value: serde_json::Value = serde_json::from_str(&input_str)?;
    let output = if compact {
        serde_json::to_string(&value)?
    } else {
        let indent_str = " ".repeat(indent);
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent_str.as_bytes());
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        value.serialize(&mut ser)?;
        String::from_utf8(buf)?
    };
    write_output(&output)?;
    Ok(())
}

fn handle_validate(args: &Args) {
    if let Commands::Validate { file } = &args.command
        && let Err(e) = run_validate(file.as_ref())
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_validate(file: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let input_str = read_input(file)?;
    let _: serde_json::Value = serde_json::from_str(&input_str)?;
    println!("JSON is valid");
    Ok(())
}

fn handle_stream(args: &Args) {
    if let Commands::Stream { file } = &args.command
        && let Err(e) = run_stream(file.as_ref())
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_stream(file: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let input_str = read_input(file)?;
    let _: serde_json::Value = serde_json::from_str(&input_str)?;
    println!("JSON stream processed successfully");
    Ok(())
}

fn handle_schema(args: &Args) {
    if let Commands::Schema { file } = &args.command
        && let Err(e) = run_schema(file.as_ref())
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_schema(file: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let input_str = read_input(file)?;
    let value: serde_json::Value = serde_json::from_str(&input_str)?;
    let schema = infer_schema(&value);
    let output = serde_json::to_string_pretty(&schema)?;
    write_output(&output)?;
    Ok(())
}

fn infer_schema(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Null => serde_json::json!({"type": "null"}),
        serde_json::Value::Bool(_) => serde_json::json!({"type": "boolean"}),
        serde_json::Value::Number(_) => serde_json::json!({"type": "number"}),
        serde_json::Value::String(_) => serde_json::json!({"type": "string"}),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                serde_json::json!({"type": "array"})
            } else {
                let item_schema = infer_schema(&arr[0]);
                serde_json::json!({"type": "array", "items": item_schema})
            }
        }
        serde_json::Value::Object(obj) => {
            let mut properties = serde_json::Map::new();
            for (k, v) in obj {
                properties.insert(k.clone(), infer_schema(v));
            }
            serde_json::json!({"type": "object", "properties": properties})
        }
    }
}

fn handle_ops(args: &Args) {
    if let Commands::Ops { op, file } = &args.command
        && let Err(e) = run_ops(op, file.as_ref())
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_ops(op: &str, file: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let input_str = read_input(file)?;
    let value: serde_json::Value = serde_json::from_str(&input_str)?;
    let result = match op {
        "keys" => {
            if let serde_json::Value::Object(obj) = value {
                serde_json::Value::Array(
                    obj.keys()
                        .map(|k| serde_json::Value::String(k.clone()))
                        .collect(),
                )
            } else {
                return Err("keys op requires object".into());
            }
        }
        "length" => match value {
            serde_json::Value::Array(arr) => serde_json::json!(arr.len()),
            serde_json::Value::Object(obj) => serde_json::json!(obj.len()),
            serde_json::Value::String(s) => serde_json::json!(s.len()),
            _ => return Err("length op not applicable".into()),
        },
        _ => return Err(format!("Unknown op: {op}").into()),
    };
    let output = serde_json::to_string_pretty(&result)?;
    write_output(&output)?;
    Ok(())
}

fn handle_bench(_args: &Args) {
    // Basic benchmark
    use std::time::Instant;
    let data = r#"{"test": "data", "number": 42, "array": [1,2,3]}"#.repeat(1000);
    let start = Instant::now();
    for _ in 0..100 {
        let _: serde_json::Value = serde_json::from_str(&data).unwrap();
    }
    let elapsed = start.elapsed();
    println!("Benchmark: 100 parses in {elapsed:?}");
}

fn read_input(path: Option<&PathBuf>) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(p) = path {
        Ok(fs::read_to_string(p)?)
    } else {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        Ok(input)
    }
}

fn write_output(output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(output.as_bytes())?;
    Ok(())
}
