// SPDX-License-Identifier: MIT OR Apache-2.0
//! fionn CLI tool - SIMD-accelerated gron implementation
//!
//! Converts JSON to greppable, line-oriented output format.

use clap::Parser;
use fionn_gron::{
    ErrorMode, GronJsonlOptions, GronOptions, GronQueryOptions, IndexFormat, Query, gron,
    gron_jsonl, gron_query, ungron_to_value,
};
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// SIMD-accelerated gron - make JSON greppable
#[derive(Parser, Debug)]
#[command(name = "fionn")]
#[command(version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)] // CLI args naturally have many boolean flags
struct Args {
    /// Input file (reads from stdin if not provided)
    #[arg(value_name = "FILE")]
    input: Option<PathBuf>,

    /// Reverse mode: convert gron output back to JSON
    #[arg(short = 'u', long = "ungron")]
    ungron: bool,

    /// Output compact gron format (no spaces around =)
    #[arg(short = 'c', long = "compact")]
    compact: bool,

    /// Output only paths (no values)
    #[arg(long = "paths")]
    paths_only: bool,

    /// Output only values (no paths)
    #[arg(long = "values")]
    values_only: bool,

    /// Custom root prefix (default: "json")
    #[arg(short = 'p', long = "prefix", default_value = "json")]
    prefix: String,

    /// Process input as JSONL (newline-delimited JSON)
    #[arg(long = "jsonl")]
    jsonl: bool,

    /// JSONL index format: bracket (json\[0\]), dot (json.0), or none
    #[arg(long = "jsonl-index", default_value = "bracket")]
    jsonl_index: String,

    /// Error handling for JSONL: fail, skip, or comment
    #[arg(long = "jsonl-errors", default_value = "fail")]
    jsonl_errors: String,

    /// Query filter (JSONPath-like syntax: .field, \[0\], \[*\], ..field)
    #[arg(short = 'q', long = "query")]
    query: Option<String>,

    /// Maximum number of query matches (0 = unlimited)
    #[arg(long = "max-matches", default_value = "0")]
    max_matches: usize,

    /// Include container declarations in query output
    #[arg(long = "include-containers")]
    include_containers: bool,

    /// Output as colorized (for terminal)
    #[arg(long = "color")]
    color: bool,

    /// Stream output (don't buffer)
    #[arg(short = 's', long = "stream")]
    stream: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Read input
    let input = read_input(&args.input)?;

    // Handle ungron mode
    if args.ungron {
        return handle_ungron(&input);
    }

    // Build gron options
    let mut gron_opts = GronOptions::with_prefix(&args.prefix);
    if args.compact {
        gron_opts = gron_opts.compact();
    }
    if args.paths_only {
        gron_opts = gron_opts.paths_only();
    }
    if args.values_only {
        gron_opts = gron_opts.values_only();
    }
    if args.color {
        gron_opts = gron_opts.color();
    }

    // Handle query mode
    if let Some(query_str) = &args.query {
        return handle_query(&input, query_str, &gron_opts, &args);
    }

    // Handle JSONL mode
    if args.jsonl {
        return handle_jsonl(input.as_bytes(), &gron_opts, &args);
    }

    // Standard gron
    let output = gron(&input, &gron_opts)?;
    write_output(&output)?;

    Ok(())
}

#[allow(clippy::ref_option)] // Pattern matches CLI argument types
fn read_input(path: &Option<PathBuf>) -> Result<String, Box<dyn std::error::Error>> {
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

fn handle_ungron(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let value = ungron_to_value(input)?;
    let output = serde_json::to_string_pretty(&value)?;
    println!("{output}");
    Ok(())
}

fn handle_query(
    input: &str,
    query_str: &str,
    gron_opts: &GronOptions,
    args: &Args,
) -> Result<(), Box<dyn std::error::Error>> {
    let query = Query::parse(query_str)?;

    let mut query_opts = GronQueryOptions {
        gron: gron_opts.clone(),
        max_matches: args.max_matches,
        include_containers: args.include_containers,
    };

    if args.compact {
        query_opts = query_opts.compact();
    }

    let output = gron_query(input, &query, &query_opts)?;
    write_output(&output)?;

    Ok(())
}

fn handle_jsonl(
    input: &[u8],
    gron_opts: &GronOptions,
    args: &Args,
) -> Result<(), Box<dyn std::error::Error>> {
    let index_format = match args.jsonl_index.as_str() {
        "bracket" => IndexFormat::Bracketed,
        "dot" => IndexFormat::Dotted,
        "none" => IndexFormat::None,
        other => {
            return Err(
                format!("Invalid JSONL index format: {other}. Use: bracket, dot, or none").into(),
            );
        }
    };

    let error_mode = match args.jsonl_errors.as_str() {
        "fail" => ErrorMode::Fail,
        "skip" => ErrorMode::Skip,
        "comment" => ErrorMode::Comment,
        other => {
            return Err(
                format!("Invalid JSONL error mode: {other}. Use: fail, skip, or comment").into(),
            );
        }
    };

    let mut jsonl_opts = GronJsonlOptions {
        gron: gron_opts.clone(),
        index_format,
        error_mode,
    };

    if args.compact {
        jsonl_opts = jsonl_opts.compact();
    }

    let output = gron_jsonl(input, &jsonl_opts)?;
    write_output(&output)?;

    Ok(())
}
