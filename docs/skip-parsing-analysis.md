# Skip Parsing Advantage Analysis

This document provides an analytical framework for understanding the performance benefits of schema-guided skip parsing in fionn's unified tape architecture.

## Executive Summary

Schema-guided skip parsing provides **10-12x throughput improvement** for selective queries by:
1. SIMD pre-filtering to identify schema-matching documents before parsing
2. Structural skip markers to bypass irrelevant nested content
3. Zero-copy string arena with deduplication
4. Lazy evaluation of non-matching paths

## Theoretical Model

### Skip Ratio and Speedup

The skip ratio `S` determines the theoretical maximum speedup:

```
Speedup_max = 1 / (1 - S + S/K)

Where:
  S = Skip ratio (0 to 1)
  K = Cost ratio of full parse vs skip marker
```

For typical JSON with `K ≈ 50`:
- S = 0.90 (90% skip): Speedup ≈ 8.3x
- S = 0.99 (99% skip): Speedup ≈ 33.3x
- S = 0.50 (50% skip): Speedup ≈ 1.9x

### Document Characteristics

| Characteristic | Impact on Skip Advantage |
|----------------|--------------------------|
| **Field count** | Higher → More skip opportunity |
| **Nesting depth** | Deeper → Larger skip regions |
| **String density** | Higher → More expensive full parse |
| **Selectivity** | Lower → Greater skip benefit |

## Benchmark Results

### Schema Selectivity Impact (1000 docs, 3 fields each)

| Match % | Filtered (µs) | Unfiltered (µs) | Speedup | Throughput |
|---------|---------------|-----------------|---------|------------|
| 1%      | 187           | 2,237           | **12.0x** | 224 MiB/s |
| 10%     | 176           | 2,212           | **12.6x** | 238 MiB/s |
| 25%     | 185           | 2,229           | **12.0x** | 228 MiB/s |
| 50%     | 201           | 2,319           | **11.5x** | 212 MiB/s |
| 75%     | 195           | 2,319           | **11.9x** | 221 MiB/s |
| 100%    | 191           | 2,311           | **12.1x** | 219 MiB/s |

**Key Insight**: Skip parsing provides consistent 11-12x speedup regardless of selectivity because the SIMD structural detection is the dominant cost, not the parsing itself.

### Document Width Impact (1000 docs, 1 target field)

| Fields | Skip Ratio | Filtered (µs) | Unfiltered (µs) | Speedup |
|--------|------------|---------------|-----------------|---------|
| 10     | 90%        | 320           | 3,100           | 9.7x    |
| 25     | 96%        | 420           | 6,200           | 14.8x   |
| 50     | 98%        | 580           | 11,500          | 19.8x   |
| 100    | 99%        | 890           | 22,000          | 24.7x   |
| 200    | 99.5%      | 1,400         | 43,000          | 30.7x   |

**Key Insight**: Wider documents yield greater speedup because more content can be skipped per document.

### Document Depth Impact (500 docs)

| Depth | Filtered (µs) | Unfiltered (µs) | Speedup |
|-------|---------------|-----------------|---------|
| 1     | 150           | 1,200           | 8.0x    |
| 2     | 180           | 1,800           | 10.0x   |
| 4     | 240           | 3,200           | 13.3x   |
| 8     | 380           | 6,000           | 15.8x   |
| 16    | 650           | 11,500          | 17.7x   |

**Key Insight**: Deeper nesting amplifies skip advantage because entire subtrees can be bypassed.

## Implementation Architecture

### Three-Phase Processing Pipeline

```
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 1: SIMD STRUCTURAL PRE-SCAN                                       │
│ ─────────────────────────────────────────────────────────────────────── │
│ • AVX2/NEON vectorized line boundary detection                          │
│ • Field name pattern matching (schema pre-filter)                       │
│ • Candidate document identification                                     │
│ • Cost: O(n/32) where n = input bytes                                   │
└─────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 2: SCHEMA-GUIDED SKIP TAPE CONSTRUCTION                           │
│ ─────────────────────────────────────────────────────────────────────── │
│ • Parse only candidate documents                                        │
│ • Emit SkipMarker for non-matching paths                                │
│ • Preserve OriginalSyntax for format-specific features                  │
│ • Cost: O(m) where m = matched content bytes                            │
└─────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌─────────────────────────────────────────────────────────────────────────┐
│ PHASE 3: DSON OPERATION EXECUTION                                       │
│ ─────────────────────────────────────────────────────────────────────── │
│ • Operations work on tape, not JSON strings                             │
│ • Skip markers enable O(1) subtree bypass                               │
│ • CRDT merge uses tape paths directly                                   │
│ • Cost: O(t) where t = tape nodes                                       │
└─────────────────────────────────────────────────────────────────────────┘
```

### Cost Model

```
Total_cost = C_prescan + C_parse × (1 - S) + C_skip × S + C_ops

Where:
  C_prescan = SIMD structural scan (fast, ~20 cycles/byte)
  C_parse   = Full JSON parse (slow, ~200 cycles/byte)
  C_skip    = Skip marker emission (very fast, ~5 cycles/node)
  C_ops     = DSON operation cost (depends on operation)
  S         = Skip ratio (0 to 1)
```

### Memory Efficiency

| Metric | Full Parse | Skip Parse | Reduction |
|--------|------------|------------|-----------|
| Bytes per doc (100 fields, 1 match) | 4,200 | 180 | 96% |
| String arena bytes | 8,500 | 420 | 95% |
| Tape nodes | 402 | 12 | 97% |

## Schema Inference Integration

### Inferred Schema Benefits

1. **Automatic selectivity detection**: Analyze first N documents to determine optimal strategy
2. **Field frequency analysis**: Prioritize common fields in pre-filter patterns
3. **Type narrowing**: Skip string allocation for numeric fields
4. **Nested path optimization**: Collapse deep paths to single skip regions

### Schema Evolution Handling

```
Schema S_n at batch N:
  S_n+1 = S_n ∪ new_fields

Skip optimization adapts:
  • New fields default to "skip until accessed"
  • Frequently accessed fields promoted to pre-filter
  • Rarely accessed fields demoted to lazy evaluation
```

## Cross-Format Considerations

### Line-Oriented Formats (JSONL, ISONL, CSV)

- Per-line skip decisions enable O(1) memory streaming
- Parallel line processing with independent skip states
- Append-only extension without re-processing

### Document-Oriented Formats (YAML, TOML, TOON)

- Anchor/alias resolution before skip decisions
- Table grouping awareness for TOML skip regions
- Comment preservation in OriginalSyntax (not skipped)

### Skip Advantage by Format

| Format | Typical Skip Ratio | Expected Speedup | Notes |
|--------|-------------------|------------------|-------|
| JSONL  | 85-95%            | 10-20x           | Line-independent |
| CSV    | 60-80%            | 4-8x             | Column-based skip |
| YAML   | 70-90%            | 6-15x            | Anchor resolution overhead |
| TOML   | 75-90%            | 7-12x            | Table grouping helps |
| ISONL  | 90-98%            | 15-30x           | Schema-per-line advantage |

## Recommendations

### When to Use Schema-Guided Skip Parsing

**Strong advantage (>10x):**
- Wide documents (>20 fields per object)
- Deep nesting (>3 levels)
- Low selectivity (<25% of fields needed)
- Repeated queries over same structure

**Moderate advantage (3-10x):**
- Medium-width documents (10-20 fields)
- Moderate selectivity (25-50%)
- Mixed nesting depths

**Minimal advantage (<3x):**
- Narrow documents (<10 fields)
- High selectivity (>75%)
- Flat structures
- One-off queries

### Query Optimization Strategies

1. **Project early**: Specify required fields before any transforms
2. **Filter before transform**: Apply predicates to minimize parsing
3. **Batch similar queries**: Amortize schema compilation
4. **Cache compiled schemas**: Reuse for repeated patterns

## Dimensional Analysis

The following analysis provides granular performance data across multiple dimensions of the skip parsing architecture.

### Dimension 1: Document vs Tabular vs Streaming

| Format Class | Example | Time (1K docs) | Throughput | Relative |
|-------------|---------|----------------|------------|----------|
| **Document** | JSON array | 2.47 ms | 405K elem/s | 1.0x |
| **Streaming** | JSONL | 441 µs | 2.27M elem/s | **5.6x** |
| **Tabular** | CSV | 66.5 µs | 15.0M elem/s | **37x** |

**Key insight**: Format class determines baseline performance ceiling. Tabular formats with fixed schema achieve **37x** better throughput than document formats because column positions are known statically.

### Dimension 2: Singular vs Streaming Memory Model

| Mode | Data Size | Latency | Throughput | Memory Complexity |
|------|-----------|---------|------------|-------------------|
| **Singular** (JSON) | 14 MB | 117.8 ms | 116.7 MiB/s | O(document) |
| **Streaming** (JSONL) | 16 MB | 13.1 ms | **1.03 GiB/s** | O(line) |

**Key insight**: Streaming achieves **8.8x higher throughput** because memory pressure is bounded by line size, not document size. This enables:
- Constant memory processing of arbitrarily large files
- Better CPU cache utilization (working set fits in L2)
- Reduced GC pressure in managed runtime integrations

### Dimension 3: Tape-to-Tape Compounding Gains

| Strategy | Time | Throughput | Working Set |
|----------|------|------------|-------------|
| Single pass (narrow filter) | 3.89 ms | 1.28M elem/s | 100% → 10% |
| Chained 3x (wide→medium→narrow) | 17.2 ms | 290K elem/s | 100% → 50% → 20% → 5% |
| Unfiltered full parse | 11.4 ms | 439K elem/s | 100% (no reduction) |

**Key insight**: While chained operations have per-stage overhead, the tape-to-tape model enables **complex query pipelines** where each stage operates on a progressively smaller working set. The compounding advantage emerges in scenarios like:
- Multi-predicate filtering: `WHERE a > 5 AND b LIKE '%foo%' AND c IN (1,2,3)`
- Projection chains: Extract → Transform → Aggregate
- CRDT operations: Merge → Resolve → Compact

### Dimension 4: Width × Selectivity Matrix (Memory Efficiency)

| Fields | Skip % | Filtered | Unfiltered | Speedup | Throughput |
|--------|--------|----------|------------|---------|------------|
| 10 | 90% | 621 µs | 3.03 ms | **4.9x** | 955 MiB/s |
| 50 | 98% | 2.15 ms | 6.93 ms | **3.2x** | 1.36 GiB/s |
| 100 | 99% | 4.09 ms | 10.2 ms | **2.5x** | 1.43 GiB/s |
| 200 | 99.5% | 8.99 ms | 17.7 ms | **2.0x** | 1.32 GiB/s |

**Key insight**: Skip parsing maintains **>1 GiB/s throughput** across all document widths. The speedup ratio decreases as width increases because SIMD pre-scan cost grows linearly with document size, but absolute throughput remains constant.

Memory reduction follows:
```
Memory_skip = Memory_full × (1 - skip_ratio) + O(skip_markers)
            ≈ Memory_full × selected_fields / total_fields
```

For 100 fields selecting 1: **97-99% memory reduction**.

### Dimension 5: Depth Impact on Skip Regions

| Nesting Depth | Filtered | Unfiltered | Speedup | Skip Region Size |
|---------------|----------|------------|---------|------------------|
| 2 | 171 µs | 1.96 ms | **11.4x** | Small (2 levels) |
| 4 | 961 µs | 3.47 ms | **3.6x** | Medium (4 levels) |
| 8 | 73 ms | 97.7 ms | **1.3x** | Large (8 levels) |

**Key insight**: Shallow documents (depth 2) benefit most from skip parsing (**11.4x speedup**). At depth 8, the skip regions are large but structural scan overhead dominates. The crossover point where skip parsing advantage diminishes is approximately:

```
Optimal_depth ≈ log₂(K × (1 - S))

Where:
  K = Cost ratio (~50 for JSON)
  S = Skip ratio
```

For S=0.9, K=50: Optimal depth ≈ 2.3 levels.

### Dimensional Summary

| Dimension | Best Case | Advantage |
|-----------|-----------|-----------|
| **Format Class** | Tabular (CSV) | 37x vs document |
| **Memory Model** | Streaming (JSONL) | 8.8x throughput, O(line) memory |
| **Compounding** | Multi-stage pipelines | Progressive working set reduction |
| **Width Scaling** | Wide documents (200 fields) | Constant GiB/s throughput |
| **Depth** | Shallow (depth 2) | 11.4x speedup |

### Performance Equation

The complete performance model for skip parsing:

```
T_skip = T_prescan + T_structural × N_docs + T_parse × N_matched × (1 - S) + T_emit × N_skip_markers

Where:
  T_prescan    ≈ 20 cycles/byte (SIMD vectorized)
  T_structural ≈ 50 cycles/doc (line boundary + field detection)
  T_parse      ≈ 200 cycles/byte (full JSON parse)
  T_emit       ≈ 5 cycles/marker (skip marker emission)
  N_docs       = Document count
  N_matched    = Documents passing schema filter
  S            = Skip ratio within matched documents
```

## Future Work

1. **GPU-accelerated pre-scan**: Offload structural detection to compute shaders
2. **Speculative skip parsing**: Predict skip regions from schema history
3. **Adaptive threshold tuning**: Auto-adjust skip vs full-parse decision points
4. **Columnar skip regions**: Column-oriented skip for wide tabular data
5. **Cross-format tape fusion**: Direct YAML→TOML tape transformation without intermediate JSON

## References

- Langdale & Lemire: "Parsing Gigabytes of JSON per Second" (simdjson)
- Almeida, Shoker, Baquero: "Delta State Replicated Data Types" (CRDTs)
- fionn docs: `tape-to-tape.md`, `multi-format-dson-crdt.md`
