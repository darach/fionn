# Skip Strategies Research

Research attribution and algorithm details for JSON skip implementations.

> **See also**: [Concept Graph](papers/sources/concept_graph.md) for complete source attribution and terminology distinctions.

## Critical Terminology Distinction

This project carefully distinguishes between three related but distinct concepts:

| Term | Meaning | Source |
|------|---------|--------|
| **Structural Scanning** | SIMD-accelerated preprocessing that identifies ALL structural characters | simdjson (Langdale & Lemire) |
| **Fast-Forwarding** | Query-driven skipping of irrelevant substructures during streaming | JSONSki (Jiang & Zhao) |
| **Schema-Aware Skipping** | Inferred input/output schema determines which fields to process | **This project (novel)** |

**Structural scanning** processes every byte to build an index. **Fast-forwarding** uses queries to guide skipping. **Schema-aware skipping** uses operation-based schema inference to skip fields not touched by processing.

## Overview

The `skip` module provides multiple strategies for skipping JSON values during on-demand parsing. Each strategy trades off simplicity, performance, and SIMD-readiness.

## Research Foundation

### simdjson / Langdale-Lemire

**Paper**: "Parsing Gigabytes of JSON per Second" (Langdale & Lemire, 2019)

- **Venue**: VLDB Journal
- **DOI**: https://doi.org/10.1007/s00778-019-00578-5

**Key Contributions**:
- XOR prefix for in-string state detection
- Branchless escape sequence handling
- 64-byte chunk processing aligned to cache lines

**Implementation**: `LangdaleSkip` in `src/skip/langdale.rs`

### JSONSki

**Paper**: "Streaming semi-structured data with bit-parallel fast-forwarding" (ASPLOS 2022)

- **Authors**: Lin Jiang, Junqiao Qiu, Zhijia Zhao
- **Venue**: ASPLOS '22
- **DOI**: https://doi.org/10.1145/3503222.3507719

**Key Contributions**:
- Bracket counting with string mask
- Single-pass container skipping
- Optimized for on-demand parsing (skip without parse)

**Implementation**: `JsonSkiSkip` in `src/skip/jsonski.rs`

### Eisel-Lemire (Related)

**Paper**: "Number Parsing at a Gigabyte per Second" (Lemire, 2021)

- **Venue**: Software: Practice and Experience
- **DOI**: https://doi.org/10.1002/spe.2984

*Note*: Float parsing is handled by simd-json; referenced for completeness.

## Algorithm Details

### XOR Prefix String Detection

Computes cumulative XOR across quote positions to determine "inside string" state:

```
Input quotes:     0b10000100  (positions 2 and 7)
XOR prefix:       0b01111100  (positions 2-6 are inside string)
```

The key insight: each quote flips the in-string state for all subsequent positions.

```rust
fn prefix_xor(bitmask: u64) -> u64 {
    let mut m = bitmask;
    m ^= m << 1;
    m ^= m << 2;
    m ^= m << 4;
    m ^= m << 8;
    m ^= m << 16;
    m ^= m << 32;
    m
}
```

### Branchless Escape Detection

Determines which characters follow an odd number of backslashes:

```rust
const EVEN_BITS: u64 = 0x5555_5555_5555_5555;

let follows_escape = (backslash << 1) | *prev_escaped;
let odd_sequence_starts = backslash & !EVEN_BITS & !follows_escape;
let (seq_even, overflow) = odd_sequence_starts.overflowing_add(backslash);
*prev_escaped = u64::from(overflow);
```

Uses carry propagation to count consecutive backslashes without branches.

### Bracket Counting Skip

JSONSki's core optimization: count open/close brackets in a 64-byte chunk, excluding those inside strings:

```rust
// Build bitmasks for brackets
let open_bits = /* bitmask of '{' or '[' positions */;
let close_bits = /* bitmask of '}' or ']' positions */;

// Exclude brackets inside strings
open_bits &= !instring;
close_bits &= !instring;

// Process each close bracket
while close_bits != 0 {
    close_count += 1;
    let close_pos = close_bits.trailing_zeros();
    // Count opens before this close
    open_count = (open_bits & ((1u64 << close_pos) - 1)).count_ones();

    if open_count < close_count {
        return Some(close_pos + 1); // Container closed
    }
    close_bits &= close_bits - 1; // Clear lowest bit
}
```

## Strategy Comparison

| Strategy | Approach | Chunk Size | SIMD-Ready |
|----------|----------|------------|------------|
| Scalar | Byte-by-byte | 1 byte | No |
| Langdale | XOR prefix + escape | 64 bytes | Yes |
| JSONSki | Bracket counting | 64 bytes | Yes |

### Performance Characteristics

- **Scalar**: Baseline. Branches on every byte. Good for small inputs (<64 bytes).
- **Langdale**: XOR prefix minimizes branches. Good for string-heavy JSON.
- **JSONSki**: Optimized for container skipping. Best for deeply nested structures.

### Use Cases

| Scenario | Recommended Strategy |
|----------|---------------------|
| Small documents (<64 bytes) | Scalar |
| String-heavy, flat objects | Langdale |
| Deeply nested structures | JSONSki |
| On-demand field access | JSONSki (default) |

## SIMD Acceleration

The current implementations use scalar loops for bit manipulation. Future work:

### x86_64 (SSE2/AVX2)
- `_mm256_cmpeq_epi8` for character matching
- `_mm256_movemask_epi8` for bitmask extraction
- `_mm_clmulepi64_si128` for fast prefix XOR (with PCLMULQDQ)

### aarch64 (NEON)
- `vceqq_u8` for character matching
- `vshrn_n_u16` + shift for bitmask extraction
- PMULL for polynomial multiplication (prefix XOR)

## Benchmarks

Run available benchmarks:

```bash
cargo bench --bench comprehensive_benchmarks
cargo bench --bench gron_benchmark
```

See [Performance Summary](../performance/summary.md) for current benchmark results.

## Schema-Aware Skip Tape (Novel Contribution)

Beyond the skip strategies derived from simdjson and JSONSki, this project introduces **schema-aware skip processing**:

### Key Innovations

1. **Compiled Schema with Hash-Based Fast Rejection**
   - Location: `src/skiptape/schema.rs`, `src/core/schema.rs`
   - Pre-computes field path hashes for O(1) rejection
   - Glob/wildcard patterns compiled to regex
   - `could_match_children` for early subtree pruning

2. **Skip Tape Format**
   - Location: `src/skiptape/tape.rs`
   - 64-byte aligned `SkipNode` structures
   - `SkipMarker` nodes for skipped regions (not byte offsets)
   - Arena-based string deduplication

3. **CRDT Operation Integration**
   - Location: `src/crdt/observed_remove.rs`
   - Observed-remove semantics for JSON field operations
   - Operation-based schema inference

### Why This Matters

| Approach | Processes | Output |
|----------|-----------|--------|
| simdjson tape | ALL bytes | Full structural index |
| JSONSki streaming | Query-matching paths | Extracted values |
| **Skip tape** | Schema-matching paths | Filtered tape with skip markers |

The skip tape approach achieves 100-200x speedup over full document parsing by:
- Skipping non-schema-matching subtrees entirely
- Counting brackets instead of parsing values
- Pre-rejecting paths via hash lookup before string comparison

## See Also

- [Architecture Comparison](comparison.md) - How skip fits in fionn
- [Performance Summary](../performance/summary.md) - Benchmark data
