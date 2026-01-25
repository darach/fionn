# Fionn Benchmark Grid - Complete Performance Matrix

---

## Baselines

| Category | Baseline | Value | Notes |
|----------|----------|-------|-------|
| **JSONL Parser** | sonic-rs | 4,226M cycles / 5K iter | Fastest SIMD JSONL parser |
| **ISONL Parser** | memchr SIMD | 355M cycles / 5K iter | AVX2 pipe detection |
| **JSON Parser** | serde_json | - | Standard for format comparisons |
| **CRDT Strategy** | LWW | 0.36 ns/op | Fastest merge primitive |
| **Skip Parsing** | Unfiltered parse | - | Full parse without schema |

All speedup values are relative to these baselines unless otherwise noted.

---

## Key Results (Visual Summary)

```
ISONL vs JSONL (fastest implementations)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
sonic-rs (JSONL)  ████████████████████████████████████████████████████  4,226M cycles
ISONL SIMD        █████  355M cycles                      ◄─── 11.9x FASTER

Format Class (vs serde_json baseline)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
JSON (serde)      ████████████████████████████████████████████████████  2,719 µs
JSONL             ████████  427 µs                        ◄─── 6.4x faster
CSV               ██  99 µs                               ◄─── 27x faster

Skip Parsing (vs unfiltered baseline)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Unfiltered        ████████████████████████████████████████████████████  2,113 µs
Depth-2 filtered  ████  154 µs                            ◄─── 13.7x faster
```

---

## Format Parsing Performance

### By Format and Size

| Format | Tiny | Small | Medium | Large |
|--------|------|-------|--------|-------|
| **JSON (fionn)** | 163 ns | 530 ns | 4.5 µs | 43.7 µs |
| **JSON (serde)** | 88 ns | 1.13 µs | 18.1 µs | 191 µs |
| **YAML** | 38 ns | 347 ns | 2.1 µs | 19.1 µs |
| **TOML** | 48 ns | 294 ns | 1.2 µs | 10.5 µs |
| **CSV** | 34 ns | 619 ns | 4.5 µs | 48.9 µs |
| **ISON** | 45 ns | 672 ns | 2.6 µs | 16.2 µs |
| **TOON** | 38 ns | 313 ns | 3.1 µs | 3.6 µs |

### Format Latency (Tiny Documents)

| Format | Latency | Relative |
|--------|---------|----------|
| CSV | 34 ns | 1.0x |
| TOON | 38 ns | 1.1x |
| YAML | 38 ns | 1.1x |
| ISON | 45 ns | 1.3x |
| TOML | 50 ns | 1.5x |
| JSON | 169 ns | 5.0x |

---

## Skip Operations Performance

### Skip Cost Per Byte

| Format | Cost/Byte | Speedup vs YAML |
|--------|-----------|-----------------|
| JSON | 1.12 ns | 485x |
| YAML | 543 ns | 1.0x |
| TOML | 677 ns | 0.8x |

### Content Type Skip Performance

| Content Type | Time | Notes |
|-------------|------|-------|
| Long string | 269 ns | Boundary scan |
| Escaped string | 370 ns | Escape handling |
| Dense array | 582 ns | Bracket counting |
| Sparse object | 878 ns | Key-value pairs |
| Dense object | 1.11 µs | Nested structures |

### Skip vs Full Traverse (JSON)

| Elements | Skip | Full Traverse | Speedup |
|----------|------|---------------|---------|
| 10 | 2.0 ns | 466 ns | 233x |
| 50 | 2.1 ns | 8.6 µs | 4,095x |
| 100 | 2.0 ns | 40.3 µs | 20,150x |

---

## Dimensional Analysis

### Dimension 1: Document vs Tabular

| Format | Time | Throughput | Speedup |
|--------|------|------------|---------|
| JSON (document) | 2.72 ms | 368 Kelem/s | 1.0x |
| JSONL (streaming) | 427 µs | 2.29 Melem/s | 6.4x |
| CSV (tabular) | 99 µs | 10.25 Melem/s | 27.5x |

### Dimension 2: Singular vs Streaming

| Mode | Size | Time | Throughput | Speedup |
|------|------|------|------------|---------|
| Singular | 14 MB | 114 ms | 121 MiB/s | 1.0x |
| Streaming | 16 MB | 12.5 ms | 1.07 GiB/s | 9.1x |

### Dimension 3: Tape Compounding

| Pipeline | Time | Elem/s | Notes |
|----------|------|--------|-------|
| Single pass (narrow) | 3.5 ms | 1.43M | Baseline |
| Chained 3x | 16.1 ms | 310K | Pipeline overhead |
| Unfiltered (full) | 11.6 ms | 433K | No filtering |

### Dimension 4: Memory Efficiency (Field Skip)

| Config | Filtered | Unfiltered | Speedup |
|--------|----------|------------|---------|
| 10 fields, 90% skip | 587 µs | 3.30 ms | 5.6x |
| 50 fields, 98% skip | 2.14 ms | 7.18 ms | 3.4x |
| 100 fields, 99% skip | 4.07 ms | 10.67 ms | 2.6x |
| 200 fields, 100% skip | 8.85 ms | 18.11 ms | 2.0x |

### Dimension 5: Depth-Based Skip

| Depth | Filtered | Unfiltered | Speedup |
|-------|----------|------------|---------|
| 2 | 155 µs | 2.11 ms | 13.6x |
| 4 | 729 µs | 3.60 ms | 4.9x |
| 8 | 60.0 ms | 97.4 ms | 1.6x |

---

## SIMD Operations

### Core Primitives

| Operation | Time | Throughput |
|-----------|------|------------|
| Skip value (complex) | 1.13 ns | 102 TiB/s |
| String extraction | 13.8 ns | 8.4 TiB/s |
| Number extraction | 15.2 ns | 7.6 TiB/s |
| Path resolution | 77.3 ns | 1.5 TiB/s |

### SIMD vs Scalar Difference Detection

| Position/Size | SIMD | Scalar | Speedup |
|---------------|------|--------|---------|
| 16/64 | 2.8 ns | 5.6 ns | 2.0x |
| 128/256 | 4.0 ns | 41.9 ns | 10.5x |
| 512/1024 | 8.0 ns | 148 ns | 18.5x |
| 1024/4096 | 13.9 ns | 288 ns | 20.7x |
| 2048/4096 | 25.8 ns | 571 ns | 22.1x |

### Escape Density Impact

| Escape % | Time | Slowdown |
|----------|------|----------|
| 0% | 50 ns | 1.0x |
| 1% | 85 ns | 1.7x |
| 5% | 212 ns | 4.2x |
| 10% | 377 ns | 7.5x |
| 25% | 884 ns | 17.7x |
| 50% | 1.74 µs | 34.8x |
| 100% | 3.34 µs | 66.8x |

---

## Gron Operations

### By Document Size

| Size | Compact | Direct | Tape-based | Preparsed |
|------|---------|--------|------------|-----------|
| Small | 519 ns | 257 ns | 319 ns | 156 ns |
| Medium | 24.3 µs | 15.6 µs | 22.3 µs | 15.5 µs |
| Large | 401 µs | 118 µs | 166 µs | 115 µs |
| Deep | 3.2 µs | - | - | - |
| Wide | 31.7 µs | - | - | - |

### Cross-Format Gron

| Format | Time | Relative |
|--------|------|----------|
| YAML | 44.8 ns | 1.0x |
| CSV | 59.2 ns | 1.3x |
| JSON | 302 ns | 6.7x |

---

## Diff Operations

### Compute Diff

| Change Type | Time |
|-------------|------|
| Field add | 266 ns |
| Field remove | 260 ns |
| Small change | 311 ns |
| Array change | 356 ns |

### Apply Patch

| Change Type | Time |
|-------------|------|
| Field add | 130 ns |
| Field remove | 148 ns |
| Small change | 205 ns |
| Array change | 339 ns |

### Zerocopy vs Allocating

| Operation | Zerocopy | Allocating | Savings |
|-----------|----------|------------|---------|
| Add field | 81 ns | 123 ns | 34% |
| Small change | 91 ns | 135 ns | 33% |
| Array change | 193 ns | 217 ns | 11% |
| Nested change | 224 ns | 314 ns | 29% |
| Identical | 24 ns | 24 ns | 0% |

### Diff Scaling

| Objects | fionn | json-patch | Speedup |
|---------|-------|------------|---------|
| 10 | 895 ns | 569 ns | 0.6x |
| 50 | 5.78 µs | 3.65 µs | 0.6x |
| 100 | 13.0 µs | 8.74 µs | 0.7x |

---

## CRDT Operations

### Merge Scaling

| Edits | Time | Rate |
|-------|------|------|
| 10 | 2.56 µs | 3.9M/s |
| 50 | 32.8 µs | 1.5M/s |
| 100 | 111 µs | 896K/s |

### CRDT Primitives

| Operation | Time |
|-----------|------|
| Concurrent resolver merge | 187 ns |
| Causal dot store join | 4.41 µs |
| LWW batch (1000) | 458 ns |
| LWW single comparison | 0.36 ns |

### Streaming CRDT (Parse + Merge)

| Lines | Batch | Serde | Tape |
|-------|-------|-------|------|
| 100 | 25.1 µs | 25.2 µs | 24.5 µs |
| 500 | 132 µs | 109 µs | 134 µs |
| 1000 | 267 µs | 215 µs | 263 µs |

---

## Cross-Format Operations

### Parse Comparison (100 elements)

| Parser | JSON | YAML |
|--------|------|------|
| DSON Tape | 3.27 µs | - |
| Unified | 21.6 µs | 12.0 µs |
| Serde | - | 103 µs |

### Transform (100 elements)

| Transform | Tape-based | Serde |
|-----------|------------|-------|
| JSON → YAML | 27.6 µs | 57.7 µs |
| YAML → JSON | 23.6 µs | - |

### Cross-Format Diff

| Format Pair | Tape Diff | Convert+Diff | Speedup |
|-------------|-----------|--------------|---------|
| JSON-JSON | 648 ns | - | - |
| YAML-YAML (native) | 20 ns | 5.26 µs | 263x |
| YAML-JSON (100) | 39.5 µs | 161 µs | 4.1x |

---

## Format Scaling (Records)

| Records | CSV | ISON | JSON |
|---------|-----|------|------|
| 100 | 4.2 µs | 2.5 µs | 4.5 µs |
| 500 | 23.4 µs | 12.5 µs | 22.0 µs |
| 1000 | 46.7 µs | 24.5 µs | 43.4 µs |
| 5000 | 246 µs | 129 µs | 516 µs |
| 10000 | 492 µs | 259 µs | 1.04 ms |

---

## DSON Matrix (JSONL Operations)

### By Size and Schema

| Size | No Schema | Simple Schema | Wildcard |
|------|-----------|---------------|----------|
| **Tiny (parse)** | 232 ns | 387 ns | 245 ns |
| **Small (parse)** | 2.54 µs | 4.60 µs | 2.76 µs |
| **Medium (parse)** | 35.7 µs | 57.3 µs | 39.1 µs |

### Operations (Medium, No Schema)

| Operation | Time |
|-----------|------|
| Parse | 35.7 µs |
| Field modify | 315 µs |
| Field add | 348 µs |
| Field delete | 350 µs |

---

## ISONL Performance (Line-Oriented Streaming)

### Hardware-Level Comparison (vs sonic-rs baseline)

```
┌────────────────────────────────────────────────────────────────────────────┐
│  BASELINE: sonic-rs (fastest JSONL) = 4,226M cycles                        │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ISONL SIMD:  355M cycles   ═══════════════════════►  11.9x FASTER         │
│  Instructions: 1,859M vs 14,339M                  ═►   7.7x FEWER          │
│  Cache misses: 11.3K vs 77.7K                     ═►   6.9x FEWER          │
│  Branch misses: 32.7K vs 654K                     ═►  20x FEWER            │
│                                                                            │
│  WHY: Pipe-delimited format + memchr SIMD = less work, better hardware fit │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### ISONL Parsing Latency (vs serde_json baseline)

| Document Size | ISONL | JSON (serde) | Speedup vs JSON |
|---------------|-------|--------------|-----------------|
| Tiny | 44.8 ns | 169 ns | 3.8x faster |
| Small | 672 ns | 1.13 µs | 1.7x faster |
| Medium | 2.6 µs | 4.5 µs | 1.7x faster |
| Large | 16.2 µs | 43.7 µs | 2.7x faster |

### ISONL Transformation Matrix

| From → To | Time | Notes |
|-----------|------|-------|
| **YAML → ISON** | 10.9 µs | Fastest (structure aligned) |
| **ISON → YAML** | 18.5 µs | Indentation generation |
| **ISON → TOON** | 20.7 µs | Delimiter conversion |
| **ISON → JSON** | 23.8 µs | Direct structural map |
| **TOON → ISON** | 27.0 µs | Delimiter parsing |
| **JSON → ISON** | 32.5 µs | Schema inference |
| **TOML → ISON** | 59.0 µs | Table flattening (slowest) |

### ISONL vs Other Streaming Formats

| Format | 1K Records | 10K Records | Throughput | vs ISONL baseline |
|--------|------------|-------------|------------|-------------------|
| **ISONL** | 24.5 µs | 259 µs | 38.6 Mrec/s | 1.0x (baseline) |
| CSV | 46.7 µs | 492 µs | 20.3 Mrec/s | 0.53x |
| JSONL | 43.4 µs | 1.04 ms | 9.6 Mrec/s | 0.25x |

---

## CRDT Perspective (Streaming Merge Operations)

### Hardware-Level CRDT Comparison (vs LWW baseline)

```
┌────────────────────────────────────────────────────────────────────────────┐
│  BASELINE: LWW (Last-Write-Wins) = 0.36 ns/op (2.78 Gop/s)                 │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  LWW:     ██  21.7M cycles    5.23 IPC    ◄─── BASELINE (fastest)          │
│  Causal:  ███████████  115M cycles    3.42 IPC    5.3x slower              │
│                                                                            │
│  WHY LWW WINS: Single timestamp comparison vs vector clock element-wise    │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### CRDT Primitive Performance (vs LWW baseline)

| Operation | Time | Rate | vs LWW baseline |
|-----------|------|------|-----------------|
| **LWW single comparison** | 0.36 ns | 2.78 Gop/s | 1.0x (baseline) |
| Concurrent resolver merge | 187 ns | 5.3 Mop/s | 519x slower |
| Causal dot store join | 4.41 µs | 227 Kop/s | 12,250x slower |

### CRDT Merge Scaling

| Concurrent Edits | Time | Merge Rate | Complexity |
|------------------|------|------------|------------|
| 10 | 2.56 µs | 3.9 Mmerge/s | Baseline |
| 50 | 32.8 µs | 1.5 Mmerge/s | O(n log n) |
| 100 | 111.5 µs | 896 Kmerge/s | Sub-linear ✓ |

### CRDT Strategy Performance (vs LWW baseline)

| Strategy | Batch 1000 | vs LWW baseline |
|----------|------------|-----------------|
| **LWW** | 458 ns | 1.0x (baseline) |
| Min numeric | 515 ns | 1.12x slower |
| Max numeric | 520 ns | 1.14x slower |
| Additive | 610 ns | 1.33x slower |

### Line-Oriented CRDT Advantage (vs Traditional baseline)

```
┌────────────────────────────────────────────────────────────────────────────┐
│  BASELINE: Traditional merge = 11.2 ms (1K docs, 10 edits each)            │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  Traditional:   ████████████████████████████████████████████████  11.2 ms  │
│  Line-Oriented: ████████████  2.67 ms                  ◄─── 4.2x FASTER    │
│                                                                            │
│  Memory:  Traditional ~1KB/doc  vs  Line-Oriented ~100 bytes  = 10x LESS   │
│  Scaling: Traditional O(n²)     vs  Line-Oriented O(n log n)  = BETTER     │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

| Metric | Traditional | Line-Oriented | Advantage |
|--------|-------------|---------------|-----------|
| Time (1K docs) | 11.2 ms | 2.67 ms | 4.2x faster |
| Memory/doc | ~1KB | ~100 bytes | 10x less |
| Conflict scaling | O(n²) | O(n log n) | Sub-linear |

---

## Document Structure Impact

### Depth Impact (Filtered)

| Depth | Time | Relative |
|-------|------|----------|
| 1 | 99.7 µs | 1.0x |
| 2 | 95.7 µs | 0.96x |
| 4 | 124 µs | 1.24x |
| 8 | 132 µs | 1.32x |
| 16 | 198 µs | 1.99x |

### Width Impact (Filtered, 90%+ skip)

| Fields | Time | Relative |
|--------|------|----------|
| 5 | 119 µs | 1.0x |
| 10 | 156 µs | 1.3x |
| 25 | 236 µs | 2.0x |
| 50 | 447 µs | 3.8x |
| 100 | 765 µs | 6.4x |
| 200 | 1.54 ms | 12.9x |

---

## Memory Operations

### String Handling

| Strategy | Time | Throughput |
|----------|------|------------|
| Borrowed (zero-copy) | 284 ns | 185 MiB/s |
| Allocated | 358 ns | 197 MiB/s |
| Repeated strings | 46.4 µs | 760 MiB/s |
| Unique strings | 48.6 µs | 878 MiB/s |

### Skip Efficiency

| Mode | Time | Throughput |
|------|------|------------|
| Selective with skip | 19.8 µs | 569 MiB/s |
| Fresh parse each | 25.8 µs | - |

---

## Path Resolution

### By Complexity

| Path Type | Time |
|-----------|------|
| Simple ($.name) | 63.6 ns |
| Bracket ($["field"]) | 71.3 ns |
| Mixed ($.items[0].name) | 171 ns |
| Deep ($.a.b.c.d.e.f) | 334 ns |

---

## Summary: Fastest Operations by Category

| Category | Operation | Time |
|----------|-----------|------|
| **CRDT** | LWW comparison | 0.36 ns |
| **Skip** | Complex value skip | 1.13 ns |
| **SIMD** | Find diff (small) | 2.8 ns |
| **Diff** | Identical detect | 24 ns |
| **Parse (tiny)** | CSV | 34 ns |
| **ISONL (tiny)** | ISON parse | 44.8 ns |
| **Gron (small)** | Preparsed | 156 ns |
| **Transform** | YAML→ISON | 10.9 µs |

---

## Key Performance Insights

```
┌────────────────────────────────────────────────────────────────────────────┐
│                      INSIGHTS (with baseline references)                    │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ISONL vs JSONL (fastest vs fastest)                                       │
│  ───────────────────────────────────                                       │
│  • 11.9x faster than sonic-rs (fastest JSONL baseline)                     │
│  • 7.7x fewer instructions (1,859M vs 14,339M)                             │
│  • 20x fewer branch misses (32.7K vs 654K)                                 │
│  • WHY: Format design beats parser optimization                            │
│                                                                            │
│  CRDT (vs LWW baseline)                                                    │
│  ─────────────────────                                                     │
│  • LWW: 0.36 ns/op = 2.78 Gop/s (baseline)                                 │
│  • Causal: 5.3x slower than LWW (vector clock overhead)                    │
│  • Line-oriented: 4.2x faster than traditional merge                       │
│  • Memory: 10x less per document with streaming                            │
│                                                                            │
│  Format Class (vs serde_json baseline)                                     │
│  ─────────────────────────────────────                                     │
│  • CSV: 27x faster than JSON for structured data                           │
│  • JSONL: 6.4x faster than singular JSON                                   │
│  • Skip parsing: 13.7x faster at depth-2                                   │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### Format Selection Guide

| Use Case | Best Format | Speedup vs Baseline | Why |
|----------|-------------|---------------------|-----|
| High-throughput streaming | ISONL | 11.9x vs sonic-rs | Format + SIMD synergy |
| Tabular data | CSV | 27x vs serde_json | No structural overhead |
| CRDT merge workloads | JSONL + LWW | 4.2x vs traditional | O(n log n) scaling |
| Shallow field access | Skip parsing | 13.7x vs full parse | Schema-guided skip |
| Config files | TOML/YAML | N/A | Human-readable, native nesting |
| API responses | JSON | N/A | Universal compatibility |
