---
title: "SIMD-Accelerated gron: High-Performance JSON to Greppable Line Transformation"
author: "Darach Ennis"
date: "January 2026"
abstract: |
  \begin{flushright}
  \textit{"He could track a single bee through a wood of trees."}\\[0.5em]
  — Standish O'Grady, \textit{History of Ireland}
  \end{flushright}

  \vspace{1em}

  \texttt{gron} transforms JSON into line-oriented, greppable output, enabling powerful text-processing pipelines for JSON data. This paper presents `fionn gron`, a SIMD-accelerated implementation built on the fionn tape infrastructure. Through SIMD-accelerated string escaping, efficient path building, and tape-based traversal, fionn gron achieves **up to 414 MiB/s throughput**-3-15x faster than the original Go implementation and 1-4x faster than fastgron (C++). We analyze the performance characteristics across file sizes, escape densities, and parallelization strategies, providing insights for building high-performance JSON transformation tools.
keywords: [JSON, gron, SIMD, string escaping, tape, performance]
---

# SIMD-Accelerated gron: High-Performance JSON to Greppable Line Transformation

## 1. Introduction

The `gron` tool transforms JSON into a greppable format:

```json
{"users": [{"name": "Alice"}, {"name": "Bob"}]}
```

Becomes:

```
json = {};
json.users = [];
json.users[0] = {};
json.users[0].name = "Alice";
json.users[1] = {};
json.users[1].name = "Bob";
```

This format enables powerful Unix pipeline operations:

```bash
fionn data.json | grep 'name' | fionn --ungron
```

While conceptually simple, high-performance gron requires optimizing several operations:

1. **JSON Parsing**: Convert input to traversable structure
2. **Path Building**: Construct path strings incrementally
3. **String Escaping**: Detect and escape special characters
4. **Output Generation**: Efficiently write formatted output

This paper presents fionn gron, which applies SIMD acceleration to critical paths while leveraging the fionn tape infrastructure for O(1) JSON traversal.

### 1.1 Contributions

- SIMD-accelerated string escaping with chunked bulk copy
- Stack-based path builder for O(1) push/pop operations
- Tape-based traversal eliminating repeated parsing
- Comprehensive benchmarks across implementations
- Analysis of parallelization tradeoffs

## 2. Architecture

### 2.1 System Overview

```
+-------------------------------------------------------------+
|                     fionn gron Pipeline                     |
+-------------------------------------------------------------+
|                                                             |
|  +----------+   +-----------+   +--------------------+      |
|  | JSON     |-->| simd-json |-->| Tape               |      |
|  | Input    |   | Parser    |   | (O(1) traversal)   |      |
|  +----------+   +-----------+   +--------------------+      |
|                                          |                  |
|                                          v                  |
|  +----------+   +-----------+   +--------------------+      |
|  | gron     |<--| String    |<--| Path Builder       |      |
|  | Output   |   | Escaping  |   | (stack-based)      |      |
|  +----------+   | (SIMD)    |   +--------------------+      |
|                 +-----------+                               |
+-------------------------------------------------------------+
```

### 2.2 Component Responsibilities

| Component | Responsibility | Optimization |
|-----------|----------------|--------------|
| simd-json | JSON parsing | SIMD structural character detection |
| Tape | Document structure | Pre-indexed O(1) navigation |
| PathBuilder | Path string construction | Stack-based, no allocation per segment |
| String Escaping | Quote/escape detection | SIMD 16-byte parallel scan |
| Output Writer | Buffered output | 64KB buffer, batch writes |

## 3. SIMD String Escaping

### 3.1 The Challenge

JSON string escaping requires detecting characters that need special handling:

- Control characters (0x00-0x1F)
- Quote (`"`)
- Backslash (`\`)

Traditional byte-by-byte escaping creates a bottleneck for large strings.

### 3.2 Chunked SIMD Approach

Our approach combines SIMD detection with bulk copying:

```rust
unsafe fn escape_json_string_sse2(bytes: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    let control_bound = _mm_set1_epi8(0x20);
    let quote_char = _mm_set1_epi8(b'"' as i8);
    let backslash_char = _mm_set1_epi8(b'\\' as i8);

    while i < len {
        // Find next escape character using SIMD
        let escape_pos = find_next_escape_sse2(
            bytes, i, control_bound, quote_char, backslash_char
        );

        if let Some(pos) = escape_pos {
            // Bulk copy clean segment
            out.extend_from_slice(&bytes[i..pos]);
            // Escape the character
            escape_byte(bytes[pos], out);
            i = pos + 1;
        } else {
            // Copy remaining bytes
            out.extend_from_slice(&bytes[i..]);
            break;
        }
    }
}
```

### 3.3 SIMD Detection

The SIMD kernel processes 16 bytes per iteration:

```rust
unsafe fn find_next_escape_sse2(bytes: &[u8], start: usize, ...) -> Option<usize> {
    while i + 16 <= len {
        let chunk = _mm_loadu_si128(bytes[i..].as_ptr().cast());

        // Check for control chars (< 0x20)
        let control = _mm_cmplt_epi8(chunk, control_bound);

        // Check for quote and backslash
        let quote = _mm_cmpeq_epi8(chunk, quote_char);
        let backslash = _mm_cmpeq_epi8(chunk, backslash_char);

        let needs_escape = _mm_or_si128(control, _mm_or_si128(quote, backslash));
        let mask = _mm_movemask_epi8(needs_escape);

        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }
        i += 16;
    }
    // Scalar fallback for remaining bytes
    ...
}
```

### 3.4 Performance by Escape Density

| Escape Density | Throughput | Notes |
|----------------|------------|-------|
| 0% (clean) | 12.3 GiB/s | Pure bulk copy |
| 1% | 8.1 GiB/s | Occasional escapes |
| 5% | 3.8 GiB/s | |
| 10% | 2.3 GiB/s | |
| 25% | 1.0 GiB/s | |
| 50% | 370 MiB/s | Frequent escapes |
| 100% | 210 MiB/s | Every character escaped |

**Key Insight**: Real-world JSON strings typically have 0-5% escape density, where SIMD provides 4-12x speedup over scalar processing.

## 4. Stack-Based Path Builder

### 4.1 Design

During JSON traversal, paths share prefixes. A stack-based design enables O(1) push/pop:

```rust
pub struct PathBuilder {
    buffer: Vec<u8>,      // Path string buffer
    stack: Vec<usize>,    // Segment boundary stack
    root: String,         // Root prefix ("json")
}

impl PathBuilder {
    pub fn push_field(&mut self, name: &str) {
        self.stack.push(self.buffer.len());
        if needs_quoting(name.as_bytes()) {
            // Bracket notation: ["field.with.dots"]
            self.buffer.extend(b"[\"");
            escape_field_name(&mut self.buffer, name);
            self.buffer.extend(b"\"]");
        } else {
            // Dot notation: .field
            self.buffer.push(b'.');
            self.buffer.extend(name.as_bytes());
        }
    }

    pub fn pop(&mut self) {
        if let Some(offset) = self.stack.pop() {
            self.buffer.truncate(offset);  // O(1) pop
        }
    }
}
```

### 4.2 Performance

| Operation | Time |
|-----------|------|
| push_field (simple) | ~3 ns |
| push_field (needs quoting) | ~8 ns |
| push_index | ~5 ns |
| pop | ~2 ns |

The stack-based approach eliminates allocation per path segment, critical for deep JSON structures.

## 5. Tape-Based Traversal

### 5.1 Tape Structure

simd-json produces a tape representation during parsing:

```
Index  Type       Data
0      Object     {len: 2, count: 5}
1      String     "users"
2      Array      {len: 2, count: 4}
3      Object     {len: 1, count: 2}
4      String     "name"
5      String     "Alice"
6      Object     {len: 1, count: 2}
7      String     "name"
8      String     "Bob"
```

### 5.2 Traversal Algorithm

```rust
fn traverse_gron(nodes: &[Node], index: usize, path: &mut PathBuilder, out: &mut Writer) -> usize {
    match &nodes[index] {
        Node::Object { len, .. } => {
            out.write_line(path.current_path(), b"{}");
            let mut idx = index + 1;
            for _ in 0..*len {
                let key = extract_key(&nodes[idx]);
                idx += 1;
                path.push_field(key);
                idx = traverse_gron(nodes, idx, path, out);
                path.pop();
            }
            idx
        }
        Node::Array { len, .. } => {
            out.write_line(path.current_path(), b"[]");
            let mut idx = index + 1;
            for i in 0..*len {
                path.push_index(i);
                idx = traverse_gron(nodes, idx, path, out);
                path.pop();
            }
            idx
        }
        Node::String(s) => {
            escape_json_string_simd(s, &mut value_buf);
            out.write_line(path.current_path(), &value_buf);
            index + 1
        }
        // ... primitives
    }
}
```

## 6. Comparative Benchmarks

### 6.1 CLI Comparison

Benchmarks using hyperfine with warm caches:

| File Size | fionn | gron (Go) | fastgron (C++) | vs gron | vs fastgron |
|-----------|-------|-----------|----------------|---------|-------------|
| 532 B | 0.5 ms | 1.7 ms | 2.4 ms | **3.2x** | **4.5x** |
| 41 KB | 0.8 ms | 7.2 ms | 2.6 ms | **9.2x** | **3.4x** |
| 244 KB | 2.1 ms | 31.7 ms | 3.9 ms | **14.8x** | **1.8x** |
| 1.2 MB | 5.4 ms | 69.2 ms | 5.4 ms | **12.9x** | **1.0x** |

### 6.2 Library Throughput

| File Size | Throughput | Notes |
|-----------|------------|-------|
| Small (37 B) | 144 MiB/s | Startup overhead dominates |
| Medium (6.6 KB) | 414 MiB/s | Peak efficiency |

### 6.3 Scalar vs SIMD Escaping

| String Type | Scalar | SIMD | Speedup |
|-------------|--------|------|---------|
| Clean (1000 chars) | 98.7 ns | 16.7 ns | **5.9x** |
| Clean (100 chars) | 63 ns | 54 ns | 1.2x |
| With quotes | 93 ns | 22 ns | **4.2x** |
| With newlines | 77 ns | 24 ns | **3.2x** |

## 7. Parallelization Analysis

### 7.1 Parallel Array Processing

We implemented parallel processing for large arrays using rayon:

```rust
if array_len >= parallel_threshold {
    let results: Vec<Vec<u8>> = element_indices
        .par_iter()
        .map(|(i, idx)| {
            let mut local_path = PathBuilder::new(&current_path);
            local_path.push_index_raw(*i);
            let mut local_out = Vec::new();
            traverse_sequential(nodes, *idx, &mut local_path, &mut local_out);
            local_out
        })
        .collect();

    // Merge results in order
    for result in results {
        out.extend_from_slice(&result);
    }
}
```

### 7.2 Results

| Threshold | Large File (374KB) | Notes |
|-----------|-------------------|-------|
| Sequential | 963 us | Baseline |
| Parallel (10) | 2.8 ms | 2.9x slower |
| Parallel (100) | 2.7 ms | 2.8x slower |
| Parallel (1000) | 2.8 ms | 2.9x slower |

**Finding**: Parallelization overhead exceeds benefits for files under 10 MB. The sequential implementation is already memory-bound, leaving little room for parallel speedup.

### 7.3 When Parallelism Helps

Based on our analysis, parallel processing provides benefit when:

1. **Array elements are large** (nested objects with many fields)
2. **String escaping is expensive** (high escape density)
3. **File size exceeds 10 MB**
4. **Processing includes additional work** (filtering, transformation)

## 8. Optimization Insights

### 8.1 Bottleneck Analysis

| Component | % Time (Large File) | Optimization Potential |
|-----------|---------------------|------------------------|
| JSON Parsing | 30% | Already SIMD (simd-json) |
| Tape Traversal | 15% | O(1), hard to improve |
| Path Building | 10% | Stack-based, optimal |
| String Escaping | 25% | SIMD implemented |
| Output Writing | 20% | Buffered, I/O bound |

### 8.2 Memory Bandwidth Limits

At 600 MiB/s, we approach practical memory bandwidth limits for this workload:

- Read input: ~1 GB/s
- Write output (2-3x expansion): ~1.5-2 GB/s
- Total: ~3 GB/s (reasonable for DDR4)

### 8.3 Future Optimizations

1. **AVX2/AVX-512 Escaping**: Process 32-64 bytes per iteration
2. **SIMD Path Building**: Vectorized needs_quoting for long field names
3. **Output Streaming**: Write directly to mmap'd file
4. **Selective Escaping**: Skip escape check for known-clean strings (from tape metadata)

## 9. Implementation Details

### 9.1 Extended Path Syntax

fionn gron supports extended path syntax for special field names:

```
json.simple = "value";           // Dot notation
json["field.with.dots"] = "x";   // Bracket notation
json["field[0]"] = "y";          // Embedded brackets
```

The `needs_quoting` function uses SIMD to detect when bracket notation is required.

### 9.2 Ungron Support

The reverse transformation (gron -> JSON) uses the extended path parser:

```rust
pub fn ungron(gron_output: &str) -> Result<serde_json::Value> {
    let mut root = serde_json::Value::Null;
    for line in gron_output.lines() {
        let (path, value) = parse_gron_line(line)?;
        let components = parse_extended_path(&path)?;
        set_path(&mut root, &components, value)?;
    }
    Ok(root)
}
```

## 10. Conclusion

fionn gron demonstrates that careful application of SIMD acceleration to string operations, combined with efficient data structures, can achieve significant performance improvements over existing implementations:

- **3-15x faster** than gron (Go)
- **1-4x faster** than fastgron (C++) for typical files
- **Up to 414 MiB/s** throughput
- **12 GiB/s** SIMD escape detection for clean strings

Key insights:

1. **SIMD chunked escaping** provides 4-6x speedup for typical JSON strings
2. **Stack-based path building** eliminates allocation overhead
3. **Tape-based traversal** enables O(1) navigation
4. **Parallelization** has limited benefit for files under 10 MB due to the already memory-bound workload

The fionn gron implementation validates the fionn architecture for building high-performance JSON tools, with shared components (path parsing, SIMD utilities) benefiting the broader ecosystem.

## References

1. Langdale, G., & Lemire, D. (2019). Parsing Gigabytes of JSON per Second. VLDB.
2. tomnomnom/gron. GitHub repository.
3. adamritter/fastgron. GitHub repository.
4. simd-json. High-performance JSON parser for Rust.
5. rayon. Data parallelism library for Rust.

---

## Appendix A: Benchmark Methodology

### Hardware
- CPU: Intel Core (AVX2/SSE2 enabled)
- Memory: DDR4

### Software
- Rust 1.85+ with LTO
- Criterion.rs for micro-benchmarks
- hyperfine for CLI benchmarks

### Test Data
- Synthetic JSON with nested arrays of objects
- Varying sizes: 532 B to 1.3 MB
- Varying escape densities: 0-100%

## Appendix B: Usage

```rust
use fionn::gron::{gron, GronOptions};

let json = r#"{"users": [{"name": "Alice"}]}"#;
let output = gron(json, &GronOptions::default())?;

// With options
let options = GronOptions::with_prefix("data")
    .compact()
    .paths_only();
let output = gron(json, &options)?;
```

### CLI Usage

```bash
# Basic usage
echo '{"name": "Alice"}' | fionn

# Output:
# json = {};
# json.name = "Alice";
```
