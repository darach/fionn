# Performance Benchmarks

fionn achieves multi-GiB/s throughput through SIMD acceleration and tape-based architecture. This document tracks current baselines and the optimization journey.

## Evolution: How We Got Here

### Phase 1: SIMD Parsing Foundation

Initial simd-json integration yielded 800 MiB/s JSON parsing. Key insight: tape representation enables O(1) skip operations.

**Achievement**: 3.5x faster than serde_json for medium/large inputs.

### Phase 2: Skip Index Optimization

Added pre-computed skip indices to tape nodes. Direct pointer arithmetic replaces tree traversal.

**Achievement**: 10-11 GiB/s skip throughput. Bottleneck shifted from CPU to memory bandwidth.

### Phase 3: TapeSource Abstraction

Unified interface across formats. Enabled format-agnostic diff/merge/patch.

**Achievement**: <5% overhead vs direct tape access. Pre-parsed tapes 10-47% faster than re-parsing.

### Phase 4: Multi-Format Extension

Extended SIMD techniques to YAML, TOML, CSV, ISON, TOON. Each format required custom delimiter detection.

**Achievement**: 7-9x faster YAML parsing than serde_yaml. ISON at 1.8 GiB/s.

### Phase 5: Cross-Format Operations

Tape-native diff eliminates DOM construction. Compare tape indices directly.

**Achievement**: 250x speedup for cross-format diff (tape index vs DOM traversal).

---

## Current Baselines

### Parsing Throughput

| Format | Throughput | vs Baseline | SIMD Technique |
|--------|------------|-------------|----------------|
| JSON | 1.02 GiB/s | 3.5x serde | structural char scan |
| YAML | 276 MiB/s | 7-9x serde | indent counting |
| TOML | 38 MiB/s | ~1x toml | bracket detection |
| CSV | 290 MiB/s | competitive | delimiter scan |
| ISON | 1.8 GiB/s | N/A (novel) | block header detect |
| TOON | 252 MiB/s | N/A (novel) | indent + array headers |

### Skip Performance

| Metric | Value | Note |
|--------|-------|------|
| Raw skip throughput | 10-11 GiB/s | Memory bandwidth limited |
| String skip throughput | 34 GiB/s | Just finds closing quote |
| Skip vs full parse | 4-5x | Consistent across sizes |
| TapeSource skip | 1.8ns | O(1) constant time |
| Nesting depth impact | <20% | Depth 1 → 50 |

### Diff/Patch/Merge

| Operation | Latency | vs Naive | Note |
|-----------|---------|----------|------|
| Small doc diff | 500ns | N/A | <1KB documents |
| Tape diff | 11.7ps | 250x | Index comparison |
| Cross-format diff | 85ns | 258x | YAML↔JSON average |
| JSON Patch apply | 800ns | 10x | Direct tape mutation |
| CRDT LWW merge | 2.1ns | N/A | Single operation |
| Batch merge (100) | 165ns | N/A | 606M ops/s |

### Streaming

| Mode | Throughput | Optimization |
|------|------------|--------------|
| JSONL sequential | 1.1 GiB/s | Baseline |
| JSONL + schema filter | 3.2 GiB/s | Skip non-matching |
| JSONL + SIMD batch | 10.5x vs serde | Amortized setup |
| Multi-doc YAML | 180 MiB/s | Document boundary |
| Processor reuse | +7.1% | Zero allocation/batch |

---

## Detailed Measurements

### Skip Selectivity

How much do we gain by skipping portions of content?

| Strategy | Time | vs Full Traverse |
|----------|------|------------------|
| Full traverse | 20.8 µs | baseline |
| Skip first 25% | 15.1 µs | 27% faster |
| Skip first 50% | 10.8 µs | 48% faster |
| Skip first 75% | 7.8 µs | 62% faster |
| Skip first 90% | 5.5 µs | 74% faster |

**Rule of thumb**: Speedup ≈ 0.8 × (percentage skipped)

### Content Type Impact

| Content Type | Throughput | Notes |
|--------------|------------|-------|
| Long string (no escapes) | 34.5 GiB/s | Fastest - just find `"` |
| Escaped string | 23.5 GiB/s | Escape detection overhead |
| Dense array | 10.4 GiB/s | Many brackets to count |
| Sparse object | 10.8 GiB/s | Large string values |
| Dense object | 10.9 GiB/s | Many key-value pairs |

### Cross-Format Comparison (Medium Size)

| Format | Parse Time | Throughput | Relative |
|--------|------------|------------|----------|
| ISON | 2.20 µs | 1.46 GiB/s | 0.35x faster |
| JSON | 6.24 µs | 1.03 GiB/s | 1.0x baseline |
| CSV | 13.3 µs | 247 MiB/s | 2.1x slower |
| TOON | 13.8 µs | 252 MiB/s | 2.2x slower |
| JSONL | 23.9 µs | 276 MiB/s | 3.8x slower |
| YAML | 26.8 µs | 268 MiB/s | 4.3x slower |
| TOML | 214 µs | 35 MiB/s | 34x slower |

**Ranking**: ISON > JSON > JSONL ≈ YAML ≈ CSV ≈ TOON >> TOML

### Core TapeSource Operations

| Operation | Latency | Scaling |
|-----------|---------|---------|
| `len()` | 360 ps | O(1) |
| `skip_value()` | 520 ps | O(1) |
| `value_kind()` | 365 ps | O(1) |
| `equals()` (scalar) | 1.7 ns | O(1) |
| `equals()` (object) | 16 ns | O(fields) |
| `deep_clone()` (nested) | 93 ns | O(n) |
| `node_iteration` | varies | O(n) |

---

## Algorithm Details

### SIMD Skip (AVX2/AVX-512)

64-byte chunk processing:

1. Load 64 bytes (single memory access)
2. SIMD compare for `"`, `\`, `{`, `}`, `[`, `]`
3. Compute string mask via prefix-XOR
4. Branchless escape detection
5. Bracket counting with early exit

At ~10 GiB/s: 160 million 64-byte chunks per second.

### Complexity Analysis

| Metric | Value |
|--------|-------|
| Time complexity | O(n) in bytes |
| Space complexity | O(1) - no allocation |
| Branches per 64 bytes | ~2-3 (early exit) |
| Memory accesses | 1 per 64 bytes |

### Memory Model

```
Zero-Copy Pipeline:
Input Bytes → SIMD Parser → Tape → TapeSource → Output
   [owned]    [borrowed]   [owned]  [borrowed]  [owned]

Allocations:    0           1         0          1
Copies:         0           0         0          0*
```
*Output construction only

---

## Practical Guidelines

### When to Use Skip

| Scenario | Recommendation |
|----------|----------------|
| Extract single field from large JSON | Use skip - huge wins |
| Process all fields | Full parse is simpler |
| Extract 10-20% of fields | Skip provides 10-20% speedup |
| Multiple operations on same data | Parse once, TapeSource skip O(1) |
| Streaming large files | Skip irrelevant records |

### Format Selection

| Use Case | Recommended Format | Key Metric |
|----------|-------------------|------------|
| High throughput | JSON or CSV | 1+ GiB/s |
| Config files | YAML or TOML | Readability |
| LLM pipelines | ISON or TOON | Token efficiency |
| Cross-format ops | Any via tape | 250x speedup |
| Streaming | JSONL | 10x vs serde |

### Optimization Checklist

1. **Pre-parse when reusing**: Multi-operation workflows benefit from tape caching
2. **Prefer JSON/ISON**: Maximum throughput for format-flexible systems
3. **Enable schema selectivity**: Skip fields you don't need
4. **Reuse processors**: Avoid per-batch allocation in streaming
5. **Batch JSONL**: Amortize SIMD setup across lines

---

## Summary

| Metric | Value | Note |
|--------|-------|------|
| Peak parse throughput | 1.8 GiB/s | ISON |
| Peak skip throughput | 34 GiB/s | String content |
| Cross-format diff speedup | 250x | Tape vs DOM |
| YAML acceleration | 7-9x | vs serde_yaml |
| TapeSource overhead | <5% | vs direct |
| Core operations | <1ns | O(1) constant |

The tape-native architecture delivers consistent multi-GiB/s performance across formats while enabling format-agnostic operations.
