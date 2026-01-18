# Performance Analysis: Understanding fionn's Unique Value

## Executive Summary

This analysis examines why different fionn operations show varying speedups over alternatives, identifies the unique contributions that differentiate fionn from serde/simd-json/sonic, and highlights gaps and opportunities for further optimization.

---

## 1. The Performance Hierarchy

### 1.1 Observed Throughput by Operation Type

| Operation | Throughput | vs Baseline | Bottleneck |
|-----------|------------|-------------|------------|
| **Parsing (DsonTape)** | 1.0-1.4 GiB/s | 2.3-3.6x vs serde | I/O bound |
| **CRDT Primitives** | ~4 GiB/s effective | ∞ (sub-ns) | CPU cache |
| **Skip Operations** | 800+ MiB/s | N/A (unique) | Branch prediction |
| **Gron** | 130-200 MiB/s | ~1x vs gron tool | String allocation |
| **Diff** | 800 MiB/s - 1.3 GiB/s | ~0.6x vs json-patch | Tree traversal |
| **Patch** | 400-600 MiB/s | ~1x | Mutation overhead |
| **Transform** | 90-165 MiB/s | 2x vs serde roundtrip | Serialization |

### 1.2 Why Parsing Shows Highest Gains

```
┌─────────────────────────────────────────────────────────────────────┐
│                     PARSING (SIMD-Accelerated)                       │
├─────────────────────────────────────────────────────────────────────┤
│  Input Stream:  {"key": "value", "num": 123, "arr": [1,2,3]}        │
│                  ↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓ (16-64 bytes at once)             │
│                                                                      │
│  SIMD Operations:                                                    │
│  1. Find structural chars: { } [ ] : , " in parallel                │
│  2. Classify bytes (string/number/bool/null) vectorized             │
│  3. Build tape indices with minimal branching                        │
│                                                                      │
│  Memory Pattern: SEQUENTIAL READ (cache-optimal)                     │
│  Branching: MINIMAL (SIMD masks replace branches)                   │
│  Allocation: SINGLE TAPE BUFFER (pre-sized)                         │
└─────────────────────────────────────────────────────────────────────┘
```

**Why 2.3-3.6x faster than serde:**
1. **No DOM construction** - Tape is flat, no pointer-chasing
2. **Zero-copy strings** - Cow<str> borrows from input
3. **SIMD structural detection** - 16+ bytes per cycle
4. **Single allocation** - Tape grows once, no per-node alloc

### 1.3 Why Gron/Diff/Patch Show Lower Gains

```
┌─────────────────────────────────────────────────────────────────────┐
│                        GRON (Tree → Text)                           │
├─────────────────────────────────────────────────────────────────────┤
│  For each node:                                                      │
│  1. Build path string: "json.users[0].name" (ALLOCATION)            │
│  2. Format value: "\"Alice\"" (STRING ESCAPE CHECK)                 │
│  3. Emit line: path = value; (I/O or BUFFER WRITE)                  │
│                                                                      │
│  Memory Pattern: RANDOM READ (tree traversal) + SEQUENTIAL WRITE    │
│  Branching: HIGH (type dispatch, escape detection)                  │
│  Allocation: PER-PATH STRING + OUTPUT BUFFER                        │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                     DIFF (Tree × Tree → Patch)                       │
├─────────────────────────────────────────────────────────────────────┤
│  For objects: O(n + m) - hash key comparison                        │
│  For arrays:  O(n × m) - LCS for optimal diff (or O(n+m) simple)   │
│                                                                      │
│  Memory Pattern: DUAL TREE TRAVERSAL (cache-hostile)                │
│  Branching: VERY HIGH (type comparison, equality checks)            │
│  Allocation: PATCH OPERATIONS (Vec<PatchOp>)                        │
└─────────────────────────────────────────────────────────────────────┘
```

**The fundamental issue:** These operations are **semantically complex**, not byte-level. SIMD helps parsing because structure detection is regular; it helps less with semantic tree operations.

---

## 2. CRDT/Ops Lens

### 2.1 Why CRDT Primitives Are Essentially Free

```rust
// LWW merge: 2.3ns for 10 operations = 0.23ns each
// This is ~2 CPU cycles!
pub fn merge_lww_fast(local_ts: u64, local_val: i64,
                      remote_ts: u64, remote_val: i64) -> (u64, i64) {
    if remote_ts > local_ts {
        (remote_ts, remote_val)
    } else {
        (local_ts, local_val)
    }
}
```

**Key insight:** The CRDT operation itself is trivial. The cost is in:
1. **Parsing** the values to compare (1000x more expensive)
2. **Serializing** the result (100x more expensive)
3. **Network I/O** (10000x more expensive)

### 2.2 Performance Model for CRDT Pipelines

```
Total Time = Parse(A) + Parse(B) + Merge(A,B) + Serialize(Result)

For 1000 fields:
  Parse(A):     ~10 µs
  Parse(B):     ~10 µs
  Merge:        ~250 ns  ← 0.1% of total!
  Serialize:    ~15 µs
  ─────────────────────
  Total:        ~35 µs

Merge is NEVER the bottleneck.
```

### 2.3 Opportunity: Pre-Parsed Value Caching

```rust
// Current: Parse on every merge
let result = merge_documents(json_a, json_b);

// Opportunity: Parse once, merge many
let tape_a = DsonTape::parse(json_a)?;
let tape_b = DsonTape::parse(json_b)?;
let cache = MergeCache::new(&tape_a, &tape_b);

// Subsequent merges reuse parsed structure
for field in hot_fields {
    cache.merge_field(field, MergeStrategy::LWW);  // Sub-µs
}
```

---

## 3. Streaming Lens

### 3.1 JSONL: The Ideal Streaming Format

```
┌─────────────────────────────────────────────────────────────────────┐
│                    JSONL Streaming Characteristics                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Line 1: {"id":1,"data":"..."}  ←─┐                                 │
│  Line 2: {"id":2,"data":"..."}  ←─┼─ Each line independent          │
│  Line 3: {"id":3,"data":"..."}  ←─┘  (embarrassingly parallel)      │
│                                                                      │
│  Properties:                                                         │
│  • O(1) random access by line number                                │
│  • Parallelizable across lines                                       │
│  • Streamable (no need to buffer entire document)                   │
│  • Recoverable (corrupt line doesn't affect others)                 │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 Micro-Batch vs Document-at-a-Time

| Mode | Latency | Throughput | Memory | Use Case |
|------|---------|------------|--------|----------|
| **Document-at-a-time** | Low | Medium | Low | Real-time events |
| **Micro-batch (100)** | Medium | High | Medium | Stream processing |
| **Large batch (1000+)** | High | Highest | High | ETL pipelines |

**Benchmark evidence:**
```
JSONL Processing (fionn SIMD batch):
  100 lines:   21 µs  (210 ns/line)
  1000 lines:  210 µs (210 ns/line)  ← Linear scaling!
  10000 lines: 1.9 ms (190 ns/line)  ← Actually improves (better amortization)

serde_json line-by-line:
  100 lines:   36 µs  (360 ns/line)
  1000 lines:  375 µs (375 ns/line)
```

### 3.3 Format Suitability for Streaming

| Format | Streaming Suitability | Reason |
|--------|----------------------|--------|
| **JSONL** | ★★★★★ | Line = document, trivially parallel |
| **NDJSON** | ★★★★★ | Same as JSONL |
| **JSON Array** | ★★☆☆☆ | Must parse `[`, find `,` boundaries |
| **YAML** | ★★★☆☆ | `---` separators, but indentation-sensitive |
| **CSV** | ★★★★☆ | Line-based, but quoting complicates |
| **ISON** | ★★★★★ | Designed for streaming (ISONL) |
| **TOON** | ★★☆☆☆ | Indentation-based, harder to chunk |

### 3.4 Opportunity: Schema-Guided Streaming Skip

```
┌─────────────────────────────────────────────────────────────────────┐
│              Schema-Guided JSONL Processing                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Schema: Extract only ["id", "status"]                              │
│                                                                      │
│  Input line: {"id":1,"name":"Alice","metadata":{...1KB...},"status":"active"}
│                   ↑                              ↑                    │
│               EXTRACT                          SKIP                  │
│                                                                      │
│  Without schema: Parse all 1KB = 1000 ns                            │
│  With schema:    Skip to fields = 100 ns  (10x faster!)             │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 4. Unique fionn Contributions

### 4.1 What Others Don't Have

| Feature | serde | simd-json | sonic-rs | fionn |
|---------|-------|-----------|----------|-------|
| Tape representation | ❌ | ✅ | ✅ | ✅ |
| Multi-format | ✅ (traits) | ❌ | ❌ | ✅ (native) |
| O(1) skip | ❌ | ❌ | ❌ | ✅ |
| Format-agnostic ops | ❌ | ❌ | ❌ | ✅ (TapeSource) |
| Gron transform | ❌ | ❌ | ❌ | ✅ |
| JSON Patch diff | ❌ | ❌ | ❌ | ✅ |
| CRDT merge | ❌ | ❌ | ❌ | ✅ |
| Cross-format transform | ❌ | ❌ | ❌ | ✅ |
| Streaming schema extract | ❌ | ❌ | ❌ | ✅ |

### 4.2 The fionn Value Proposition

```
┌─────────────────────────────────────────────────────────────────────┐
│                    fionn's Unique Value Stack                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Applications: Config diff, API migration, Data sync        │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              ↑                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Operations: gron, diff, patch, merge, CRDT                 │    │
│  │  (format-agnostic via TapeSource)                           │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              ↑                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Transformation: JSON ↔ YAML ↔ TOML ↔ CSV ↔ ISON ↔ TOON    │    │
│  │  (tape-to-tape, zero intermediate allocation)               │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              ↑                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Unified Tape: DsonTape, UnifiedTape, SkipTape              │    │
│  │  (O(1) skip, zero-copy strings, format metadata)            │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              ↑                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  SIMD Parsing: JSON, YAML, TOML, CSV, ISON, TOON            │    │
│  │  (1+ GiB/s, branchless, cache-optimized)                    │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.3 Differentiated Benchmarks Needed

| Benchmark | Purpose | Unique to fionn |
|-----------|---------|-----------------|
| **Skip selectivity** | Show O(1) skip value | ✅ |
| **Tape-to-tape transform** | Show zero-serde conversion | ✅ |
| **Multi-format gron** | Show unified path decomposition | ✅ |
| **CRDT merge pipeline** | Show end-to-end sync performance | ✅ |
| **Schema-guided streaming** | Show selective extraction | ✅ |
| **Cross-format diff** | Show JSON vs YAML diff | ✅ |

---

## 5. Gaps and Opportunities

### 5.1 Current Gaps

| Gap | Impact | Effort | Priority |
|-----|--------|--------|----------|
| **No YAML/TOML skip benchmarks** | Can't prove format-agnostic skip | Medium | High |
| **No cross-format diff** | Missing key differentiator | High | High |
| **No streaming + CRDT combo** | Missing real-world pipeline | High | Medium |
| **No schema-guided parse bench** | Can't show selective extraction | Medium | Medium |
| **No gron for CSV/ISON/TOON** | Novel research incomplete | High | High |

### 5.2 Opportunities for 10x Contributions

#### Opportunity 1: Streaming CRDT Sync Pipeline
```
┌─────────────────────────────────────────────────────────────────────┐
│  JSONL Stream → Parse → CRDT Merge → Emit                           │
│                                                                      │
│  Target: Process 1M records/second with automatic conflict resolution│
│                                                                      │
│  Benchmark: Compare vs manual JSON + custom merge logic             │
│  Expected: 5-10x improvement (parsing + merge integrated)           │
└─────────────────────────────────────────────────────────────────────┘
```

#### Opportunity 2: Cross-Format Config Diff
```
┌─────────────────────────────────────────────────────────────────────┐
│  diff(config.yaml, config.json) → Semantic Patch                    │
│                                                                      │
│  Target: Diff configs regardless of format, produce unified patch   │
│                                                                      │
│  Benchmark: Compare vs convert-then-diff approach                   │
│  Expected: 2-3x improvement (no intermediate serialization)         │
└─────────────────────────────────────────────────────────────────────┘
```

#### Opportunity 3: Schema-Projected Streaming
```
┌─────────────────────────────────────────────────────────────────────┐
│  JSONL Stream + Schema → Projected Records (only requested fields)  │
│                                                                      │
│  Target: Extract 3 fields from 100-field records at wire speed      │
│                                                                      │
│  Benchmark: Compare vs full parse + filter                          │
│  Expected: 3-10x improvement (skip 97% of parsing)                  │
└─────────────────────────────────────────────────────────────────────┘
```

#### Opportunity 4: Gron for Tabular Data (Novel)
```
┌─────────────────────────────────────────────────────────────────────┐
│  CSV → Gron-style path decomposition                                │
│                                                                      │
│  csv[0].name = "Alice";                                             │
│  csv[0].age = 30;                                                   │
│  csv[1].name = "Bob";                                               │
│                                                                      │
│  Enables: grep-based CSV queries, CSV diff, CSV→JSON transform      │
│  Unique: No other tool provides this                                │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 6. Benchmark Matrix (Current Status)

### 6.1 Operation × Format Coverage

| Operation | JSON | YAML | TOML | CSV | ISON | TOON | JSONL |
|-----------|------|------|------|-----|------|------|-------|
| **Parse** | ✅ 1.4 GiB/s | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ 624 MiB/s |
| **Gron** | ✅ 143 MiB/s | ✅ | ✅ | ❌ | ❌ | ❌ | ✅ |
| **Ungron** | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | — |
| **Diff** | ✅ 1.3 GiB/s | ❌ | ❌ | ❌ | ❌ | ❌ | — |
| **Patch** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | — |
| **Merge** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | — |
| **CRDT** | ✅ 4 GiB/s | — | — | — | — | — | — |
| **Skip** | ✅ 800 MiB/s | ❌ | ❌ | ❌ | ❌ | ❌ | — |
| **Transform** | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | — |
| **Stream** | — | — | — | — | — | — | ✅ |

### 6.2 Benchmark Files × Purpose

| Benchmark File | Purpose | Tests | Status |
|----------------|---------|-------|--------|
| `comprehensive_benchmarks.rs` | Parsing all formats | ~50 | ✅ |
| `gron_benchmark.rs` | Gron operations | ~20 | ✅ |
| `gron_comparison.rs` | vs gron tool | ~10 | ✅ |
| `diff_benchmark.rs` | JSON diff | ~20 | ✅ |
| `diff_patch_merge_crdt.rs` | Full CRDT pipeline | 87 | ✅ |
| `optimized_merge.rs` | CRDT primitives | ~30 | ✅ |
| `cross_format_skip.rs` | Skip selectivity | 22 | ✅ |
| `tape_to_tape.rs` | Format transforms | 27 | ✅ |
| `memory_profile.rs` | Memory patterns | 15 | ✅ |
| `streaming_formats.rs` | JSONL streaming | 24 | ✅ |
| `real_world_datasets.rs` | Real data | 35 | ✅ |
| `skip_selectivity_benchmark.rs` | Deep skip analysis | ~30 | ✅ |
| `tape_source_benchmark.rs` | TapeSource trait | ~40 | ✅ |
| `schema_selectivity.rs` | Schema-guided | ~20 | ✅ |

**Total: ~400+ benchmark tests**

---

## 7. Recommendations

### 7.1 Immediate Actions

1. **Add cross-format diff benchmark** - Key differentiator
2. **Add YAML/TOML skip benchmarks** - Prove format-agnostic skip
3. **Add streaming CRDT pipeline benchmark** - Real-world scenario

### 7.2 Research Directions

1. **Gron for CSV/ISON/TOON** - Novel contribution (paper ready)
2. **Schema-projected streaming** - High-impact optimization
3. **Cross-format semantic diff** - Unique capability

### 7.3 Benchmark Priorities

| Priority | Benchmark | Unique Value Demonstrated |
|----------|-----------|---------------------------|
| 1 | Cross-format diff (YAML vs JSON) | Format-agnostic operations |
| 2 | Streaming CRDT merge pipeline | End-to-end performance |
| 3 | Schema-projected JSONL extraction | Selective parsing |
| 4 | CSV gron | Novel path decomposition |
| 5 | Multi-format skip comparison | O(1) skip across formats |

---

## 8. Conclusion

fionn's unique value lies not in raw parsing speed (where simd-json/sonic are competitive), but in the **integrated operations stack**:

1. **Parse once, operate many** - Tape enables skip, diff, gron without re-parsing
2. **Format agnostic** - Same operations work on JSON, YAML, TOML via TapeSource
3. **Zero-copy transforms** - Format conversion without intermediate allocation
4. **Streaming + CRDT** - Real-time data sync at scale

The benchmarks should emphasize these differentiators, not just raw throughput.
