# Line-Oriented Delta-CRDT

This document describes fionn's novel line-oriented delta-CRDT system for streaming formats (JSONL, ISONL, CSV).

## Prior Art Gap

Academic delta-CRDT work (Almeida, Shoker, Baquero 2015-2018) focuses on:
- JSON documents
- In-memory state
- Document-level merge
- Sequential processing

**None address:**
- Line-oriented streaming formats
- Cross-format CRDT
- Streaming delta-sync without buffering
- Schema-per-line evolution

## Line-Oriented Properties

```
LINE-ORIENTED: JSONL, ISONL, CSV
DOCUMENT-ORIENTED: JSON, YAML, TOML, TOON

┌─────────────────────────────────────────────────────────────────────────┐
│ ✅ Self-contained lines    - No cross-line dependencies                 │
│ ✅ Stateless parsing       - No anchor resolution, no forward refs      │
│ ✅ Bounded memory          - O(max_line_size) not O(document_size)      │
│ ✅ Streamable              - Emit output before input complete          │
│ ✅ Parallelizable          - Each line independent                      │
│ ✅ Resumable               - Checkpoint at any line boundary            │
│ ✅ Appendable              - New data = append lines (no rewrite)       │
│ ✅ Diffable                - Line-level diff (git-friendly)             │
└─────────────────────────────────────────────────────────────────────────┘
```

## Exploitable Generalizations

### 1. SIMD Parallel Line Processing

```
Step 1: SIMD scan for \n boundaries (all lines found in parallel)
Step 2: Parallel dispatch - N threads process N lines simultaneously
Step 3: Ordered reassembly (if order matters) or unordered emit

Throughput: line-oriented achieves N× speedup on N cores
```

Document formats cannot do this due to:
- YAML anchors defined before use
- TOML table context
- JSON nested structure

### 2. Streaming CRDT (No Buffering)

```
Document CRDT:  Must load entire document → merge → write entire doc
                └─────── O(doc_size) memory ───────┘

Line CRDT:      Read line → merge line → emit line (constant memory)
                └─────── O(line_size) memory ─────┘
```

Line-level operations:
- `LineAppend` - Grow-only set (trivially convergent)
- `LineModify` - LWW per line (identified by line hash or position)
- `LineDelete` - Tombstone per line
- `LineReorder` - Sequence CRDT (RGA/LSEQ)

Delta sync: "lines 1000-2000 since clock X" (not "entire document")

### 3. Append-Only Log Semantics

Line formats naturally model append-only logs:

```jsonl
{"ts":1,"event":"click"}
{"ts":2,"event":"view"}
{"ts":3,"event":"purchase"}  ← just append
```

CRDT implication: Append-only = Grow-Only Set (G-Set)
- Trivially convergent (no conflicts possible)
- No tombstones needed
- Merge = union of lines

### 4. Schema-Per-Line (ISONL Unique)

ISONL embeds schema in every line:

```
table.users|id:int|name:str|1|Alice
table.users|id:int|name:str|2|Bob
table.users|id:int|name:str|age:int|3|Carol|30  ← EVOLVED SCHEMA
```

Enables:
- Schema evolution mid-stream (no migration)
- Self-describing lines (no external schema file)
- Heterogeneous streams (different schemas interleaved)
- Late binding (interpret schema at read time)

### 5. Chunked Network Transfer

```
Line-oriented:
  Client ←── {"id":1}\n ── Server (emit immediately)
         ←── {"id":2}\n ──        (emit immediately)
         ←── {"id":3}\n ──        (emit immediately)

Document-oriented:
  Client ←── (wait) ────── Server (buffer entire array)
         ←── (wait) ──────        (buffer entire array)
         ←── [{...},...] ──       (send all at once)

Time-to-first-byte: Line = O(1), Document = O(N)
```

### 6. Memory-Mapped Parallel Scan

```
mmap(file) → SIMD find all \n → partition into chunks → parallel parse

1GB JSONL file on 8 cores:
  • Split into 8 × 128MB regions
  • Each core processes its region independently
  • No coordination needed (lines are independent)

1GB JSON file on 8 cores:
  • Single core must parse sequentially
  • Other cores idle
```

### 7. Incremental/Tail Processing

```bash
tail -f access.log | fionn --from csv --filter '$.status >= 500'
```

Line formats enable:
- `tail -f` (follow) processing
- Resume from byte offset
- Partial file processing (lines 1M to 2M)
- Live streaming analytics

## Unified Line-Oriented Trait

```rust
pub trait LineOriented: Format {
    /// Parse single line to tape segment
    fn parse_line(&self, line: &[u8]) -> Result<TapeSegment>;

    /// Lines are independent (no cross-line refs)
    const STATELESS: bool = true;

    /// Can process in parallel
    const PARALLELIZABLE: bool = true;

    /// Supports append without rewrite
    const APPENDABLE: bool = true;

    /// Memory bound per line
    fn max_line_memory(&self) -> usize;
}

impl LineOriented for Jsonl { ... }
impl LineOriented for Isonl { ... }
impl LineOriented for Csv { ... }
// JSON, YAML, TOML, TOON do NOT implement this
```

## Line-Oriented CRDT Optimizations

### 1. Line-ID CRDT

Each line has unique ID:
- Insert/delete/modify by line ID
- Tombstones are line-level (smaller than field-level)

### 2. Positional CRDT (RGA/LSEQ)

For line ordering:
- Concurrent inserts at same position → deterministic order
- Reorder without rewriting content

### 3. Append-Only CRDT (G-Set)

For event logs:
- No conflicts possible
- Merge = set union
- Perfect for audit trails

### 4. Chunked Delta Sync

```
"Send lines 500-1000 since vector clock X"
Receiver applies lines independently
No document-level lock needed
```

## Transformation Optimization

```
Line → Line:  JSONL ↔ ISONL ↔ CSV
  • Zero-copy possible (reinterpret, don't rebuild)
  • Streaming transform (no buffering)
  • Parallel transform (each line independent)

Line → Document:  JSONL → JSON
  • Must buffer (build array)
  • Sequential assembly

Document → Line:  JSON → JSONL
  • Must parse entire doc first
  • Then emit lines (if top-level is array)
```

## Theoretical Properties

1. **Convergence**: Line-CRDT converges iff underlying line CRDTs converge
2. **Commutativity**: Line order independence for associative operations
3. **Memory bound**: O(max_line_size) for streaming operations
4. **Parallelism**: O(N/P) merge time for P processors, N lines
5. **Losslessness**: Round-trip preservation with OriginalSyntax
6. **Schema monotonicity**: ISONL schema can only grow (fields added)

## Novel Contributions

| # | Contribution | Novelty vs Prior Art |
|---|--------------|---------------------|
| 1 | Line-oriented delta-CRDT | First streaming CRDT model |
| 2 | Unified multi-format tape | First format-agnostic CRDT |
| 3 | Schema-per-line (ISONL) | First schema-evolving CRDT |
| 4 | Streaming CRDT operations | LineAppend/Filter/Map/Reduce |
| 5 | SIMD-parallel CRDT merge | First parallel line CRDT |
| 6 | Cross-format CRDT sync | YAML↔JSON↔CSV unified merge |
| 7 | OriginalSyntax preservation | Lossless cross-format CRDT |
| 8 | Chunked byte-range delta sync | "Lines N-M since clock X" |

## Key Insight

Line-oriented formats form a **closed set** under streaming operations:

`JSONL ↔ ISONL ↔ CSV` transforms can be:
- Zero-buffering
- Parallelized per-line
- Incrementally checkpointed
- Append-only extended

Document formats break this closure - any involvement of JSON/YAML/TOML/TOON requires buffering the entire structure.
