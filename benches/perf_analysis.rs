// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - binary for perf analysis
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Helper functions may be defined for future use
#![allow(dead_code)]
//! Perf analysis binary for hardware-level performance insights
//!
//! Run with:
//!   cargo build --release --features "all-formats" --bin perf_analysis
//!   perf stat -e cycles,instructions,cache-references,cache-misses,branches,branch-misses \
//!     ./target/release/perf_analysis \<test_name\> \<iterations\>
//!
//! Tests:
//!   - `isonl_parse`: ISONL parsing (schema-embedded)
//!   - `jsonl_parse`: JSONL parsing (schema inference)
//!   - `crdt_lww`: LWW CRDT merge operations
//!   - `crdt_causal`: Causal CRDT merge operations
//!   - `skip_shallow`: Skip parsing at depth 2
//!   - `skip_deep`: Skip parsing at depth 8

use simd_json::prelude::*;
use sonic_rs::JsonValueTrait;
use std::env;
use std::hint::black_box;

// Test data sizes
const SMALL_LINES: usize = 1000;
const ITERATIONS_DEFAULT: usize = 10000;

fn generate_jsonl(num_lines: usize) -> String {
    let mut jsonl = String::with_capacity(num_lines * 100);
    for i in 0..num_lines {
        jsonl.push_str(&format!(
            r#"{{"id":{},"user":"user_{}","email":"user{}@example.com","age":{},"active":{},"score":{}}}"#,
            i, i, i, 20 + (i % 50), i % 2 == 0, i * 10
        ));
        jsonl.push('\n');
    }
    jsonl
}

fn generate_isonl(num_lines: usize) -> String {
    let mut isonl = String::with_capacity(num_lines * 80);
    // ISONL format: table.name|field:type|field:type|value|value
    for i in 0..num_lines {
        isonl.push_str(&format!(
            "table.users|id:int|user:string|email:string|age:int|active:bool|score:int|{}|user_{}|user{}@example.com|{}|{}|{}\n",
            i, i, i, 20 + (i % 50), i % 2 == 0, i * 10
        ));
    }
    isonl
}

fn bench_jsonl_parse(data: &str, iterations: usize) {
    for _ in 0..iterations {
        let mut count = 0usize;
        for line in data.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                && v.get("id").is_some()
            {
                count += 1;
            }
        }
        black_box(count);
    }
}

fn bench_jsonl_simd(data: &str, iterations: usize) {
    // simd-json requires mutable buffer
    let mut buffer = data.as_bytes().to_vec();
    let original = data.as_bytes().to_vec();

    for _ in 0..iterations {
        let mut count = 0usize;
        let mut pos = 0;

        // Find line boundaries and parse each line
        while pos < buffer.len() {
            let line_end = memchr::memchr(b'\n', &buffer[pos..]).map_or(buffer.len(), |p| pos + p);

            if line_end > pos {
                // simd-json needs mutable slice
                let line_slice = &mut buffer[pos..line_end];
                if let Ok(v) = simd_json::to_borrowed_value(line_slice)
                    && v.get("id").is_some()
                {
                    count += 1;
                }
            }
            pos = line_end + 1;
        }
        // Reset buffer for next iteration (simd-json mutates in place)
        buffer.copy_from_slice(&original);
        black_box(count);
    }
}

fn bench_jsonl_sonic(data: &str, iterations: usize) {
    for _ in 0..iterations {
        let mut count = 0usize;
        for line in data.lines() {
            if let Ok(v) = sonic_rs::from_str::<sonic_rs::Value>(line)
                && v.get("id").is_some()
            {
                count += 1;
            }
        }
        black_box(count);
    }
}

// Selective parsing: only extract specific field, skip rest
fn bench_jsonl_simd_selective(data: &str, iterations: usize) {
    let mut buffer = data.as_bytes().to_vec();
    let original = data.as_bytes().to_vec();

    for _ in 0..iterations {
        let mut sum = 0i64;
        let mut pos = 0;

        while pos < buffer.len() {
            let line_end = memchr::memchr(b'\n', &buffer[pos..]).map_or(buffer.len(), |p| pos + p);

            if line_end > pos {
                let line_slice = &mut buffer[pos..line_end];
                if let Ok(v) = simd_json::to_borrowed_value(line_slice) {
                    // Selective: only access 'score' field
                    if let Some(score) = v
                        .get("score")
                        .and_then(simd_json::prelude::ValueAsScalar::as_i64)
                    {
                        sum += score;
                    }
                }
            }
            pos = line_end + 1;
        }
        buffer.copy_from_slice(&original);
        black_box(sum);
    }
}

fn bench_jsonl_sonic_selective(data: &str, iterations: usize) {
    for _ in 0..iterations {
        let mut sum = 0i64;
        for line in data.lines() {
            if let Ok(v) = sonic_rs::from_str::<sonic_rs::Value>(line) {
                // Selective: only access 'score' field
                if let Some(score_val) = v.get("score")
                    && let Some(score) = score_val.as_i64()
                {
                    sum += score;
                }
            }
        }
        black_box(sum);
    }
}

// ISONL selective: extract specific field by index (schema-aware)
fn bench_isonl_selective(data: &[u8], iterations: usize) {
    // Schema: table.users|id:int|user:string|email:string|age:int|active:bool|score:int
    // Value indices: 7=id, 8=user, 9=email, 10=age, 11=active, 12=score
    const SCORE_INDEX: usize = 12;

    for _ in 0..iterations {
        let mut sum = 0i64;
        let mut pos = 0;

        while pos < data.len() {
            let line_end = memchr::memchr(b'\n', &data[pos..]).map_or(data.len(), |p| pos + p);

            // Count pipes to find score field
            let mut pipe_count = 0;
            let mut field_start = pos;

            for i in pos..line_end {
                if data[i] == b'|' {
                    pipe_count += 1;
                    if pipe_count == SCORE_INDEX {
                        field_start = i + 1;
                    } else if pipe_count == SCORE_INDEX + 1 {
                        // Parse score field
                        if let Ok(s) = std::str::from_utf8(&data[field_start..i])
                            && let Ok(score) = s.parse::<i64>()
                        {
                            sum += score;
                        }
                        break;
                    }
                }
            }
            // Handle last field (no trailing pipe)
            if pipe_count == SCORE_INDEX
                && let Ok(s) = std::str::from_utf8(&data[field_start..line_end])
                && let Ok(score) = s.parse::<i64>()
            {
                sum += score;
            }

            pos = line_end + 1;
        }
        black_box(sum);
    }
}

fn bench_isonl_parse(data: &str, iterations: usize) {
    for _ in 0..iterations {
        let mut count = 0usize;
        for line in data.lines() {
            // ISONL parsing: split by pipe, schema in first fields
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() > 6 {
                // Schema: table.users|id:int|user:string|...
                // Values start at index after schema fields
                // For this schema, values start at index 7
                if let Ok(_id) = parts.get(7).unwrap_or(&"").parse::<i64>() {
                    count += 1;
                }
            }
        }
        black_box(count);
    }
}

fn bench_isonl_simd_friendly(data: &[u8], iterations: usize) {
    for _ in 0..iterations {
        let mut count = 0usize;
        let mut pos = 0;
        while pos < data.len() {
            // Find newline using byte scan (SIMD-friendly)
            let line_end = memchr::memchr(b'\n', &data[pos..]).map_or(data.len(), |p| pos + p);

            // Find first pipe (field boundary)
            if let Some(_first_pipe) = memchr::memchr(b'|', &data[pos..line_end]) {
                // Count pipes to find value section
                let pipe_count = memchr::memchr_iter(b'|', &data[pos..line_end]).count();
                if pipe_count > 6 {
                    count += 1;
                }
            }
            pos = line_end + 1;
        }
        black_box(count);
    }
}

fn bench_crdt_lww(iterations: usize) {
    // Simulate LWW merge: just timestamp comparison
    let timestamps_a: Vec<u64> = (0..1000).map(|i| i * 2).collect();
    let timestamps_b: Vec<u64> = (0..1000).map(|i| i * 2 + 1).collect();

    for _ in 0..iterations {
        let mut winners = Vec::with_capacity(1000);
        for (a, b) in timestamps_a.iter().zip(timestamps_b.iter()) {
            // LWW: higher timestamp wins
            winners.push(if a > b { *a } else { *b });
        }
        black_box(winners);
    }
}

fn bench_crdt_causal(iterations: usize) {
    // Simulate causal merge: vector clock comparison (more complex)
    let clocks_a: Vec<[u64; 3]> = (0..1000).map(|i| [i, i + 1, i + 2]).collect();
    let clocks_b: Vec<[u64; 3]> = (0..1000).map(|i| [i + 1, i, i + 3]).collect();

    for _ in 0..iterations {
        let mut merged = Vec::with_capacity(1000);
        for (a, b) in clocks_a.iter().zip(clocks_b.iter()) {
            // Causal merge: element-wise max of vector clocks
            let m = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
            // Check for concurrent (neither dominates)
            let a_dominates = a[0] >= b[0] && a[1] >= b[1] && a[2] >= b[2];
            let b_dominates = b[0] >= a[0] && b[1] >= a[1] && b[2] >= a[2];
            let concurrent = !a_dominates && !b_dominates;
            merged.push((m, concurrent));
        }
        black_box(merged);
    }
}

fn bench_skip_shallow(data: &str, iterations: usize) {
    // Simulate shallow skip: only parse first 2 levels
    for _ in 0..iterations {
        let mut count = 0usize;
        for line in data.lines() {
            // Shallow: just find first field without full parse
            if let Some(colon_pos) = line.find(':')
                && colon_pos < 20
            {
                count += 1;
            }
        }
        black_box(count);
    }
}

fn bench_skip_deep(data: &str, iterations: usize) {
    // Simulate deep parse: full JSON parse
    for _ in 0..iterations {
        let mut count = 0usize;
        for line in data.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                // Deep: access nested field
                if v.get("score")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0)
                    > 0
                {
                    count += 1;
                }
            }
        }
        black_box(count);
    }
}

fn bench_memory_sequential(iterations: usize) {
    // Sequential memory access pattern (cache-friendly)
    let data: Vec<u64> = (0..100_000).collect();

    for _ in 0..iterations {
        let mut sum = 0u64;
        for val in &data {
            sum = sum.wrapping_add(*val);
        }
        black_box(sum);
    }
}

fn bench_memory_random(iterations: usize) {
    // Random memory access pattern (cache-unfriendly)
    let data: Vec<u64> = (0..100_000).collect();
    let indices: Vec<usize> = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        (0..100_000)
            .map(|i| {
                let mut h = DefaultHasher::new();
                i.hash(&mut h);
                (h.finish() as usize) % 100_000
            })
            .collect()
    };

    for _ in 0..iterations {
        let mut sum = 0u64;
        for &idx in &indices {
            sum = sum.wrapping_add(data[idx]);
        }
        black_box(sum);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <test_name> [iterations]", args[0]);
        eprintln!("JSONL parsers:");
        eprintln!("  jsonl_parse      - serde_json baseline");
        eprintln!("  jsonl_simd       - simd-json");
        eprintln!("  jsonl_sonic      - sonic-rs");
        eprintln!("  jsonl_simd_sel   - simd-json selective field");
        eprintln!("  jsonl_sonic_sel  - sonic-rs selective field");
        eprintln!("ISONL parsers:");
        eprintln!("  isonl_parse      - string split baseline");
        eprintln!("  isonl_simd       - memchr SIMD");
        eprintln!("  isonl_selective  - schema-aware selective field");
        eprintln!("Other:");
        eprintln!("  crdt_lww, crdt_causal, skip_shallow, skip_deep, mem_sequential, mem_random");
        std::process::exit(1);
    }

    let test_name = &args[1];
    let iterations = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(ITERATIONS_DEFAULT);

    eprintln!("Running {test_name} for {iterations} iterations...");

    match test_name.as_str() {
        "jsonl_parse" => {
            let data = generate_jsonl(SMALL_LINES);
            bench_jsonl_parse(&data, iterations);
        }
        "jsonl_simd" => {
            let data = generate_jsonl(SMALL_LINES);
            bench_jsonl_simd(&data, iterations);
        }
        "jsonl_sonic" => {
            let data = generate_jsonl(SMALL_LINES);
            bench_jsonl_sonic(&data, iterations);
        }
        "jsonl_simd_sel" => {
            let data = generate_jsonl(SMALL_LINES);
            bench_jsonl_simd_selective(&data, iterations);
        }
        "jsonl_sonic_sel" => {
            let data = generate_jsonl(SMALL_LINES);
            bench_jsonl_sonic_selective(&data, iterations);
        }
        "isonl_parse" => {
            let data = generate_isonl(SMALL_LINES);
            bench_isonl_parse(&data, iterations);
        }
        "isonl_simd" => {
            let data = generate_isonl(SMALL_LINES);
            bench_isonl_simd_friendly(data.as_bytes(), iterations);
        }
        "isonl_selective" => {
            let data = generate_isonl(SMALL_LINES);
            bench_isonl_selective(data.as_bytes(), iterations);
        }
        "crdt_lww" => {
            bench_crdt_lww(iterations);
        }
        "crdt_causal" => {
            bench_crdt_causal(iterations);
        }
        "skip_shallow" => {
            let data = generate_jsonl(SMALL_LINES);
            bench_skip_shallow(&data, iterations);
        }
        "skip_deep" => {
            let data = generate_jsonl(SMALL_LINES);
            bench_skip_deep(&data, iterations);
        }
        "mem_sequential" => {
            bench_memory_sequential(iterations);
        }
        "mem_random" => {
            bench_memory_random(iterations);
        }
        _ => {
            eprintln!("Unknown test: {test_name}");
            std::process::exit(1);
        }
    }

    eprintln!("Done.");
}
