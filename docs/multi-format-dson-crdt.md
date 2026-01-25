# Multi-Format DSON/CRDT System

This document describes fionn's novel multi-format delta-CRDT system with schema-guided skip parsing, unified tape representation, and streaming line-oriented merge.

## Three-Level Operational Model

```
╔═════════════════════════════════════════════════════════════════════════╗
║  LEVEL 1: SCHEMA (META)          Inferred/Declared structure            ║
║  ─────────────────────────────────────────────────────────────────────  ║
║  LEVEL 2: DOCUMENT (BATCH)       Full document / batch of documents     ║
║  ─────────────────────────────────────────────────────────────────────  ║
║  LEVEL 3: LINE (STREAM)          Per-line stateless processing          ║
╚═════════════════════════════════════════════════════════════════════════╝
```

### Level 1: Schema Operations (Meta)

| Category | Operations |
|----------|------------|
| Inference | `SchemaInfer`, `SchemaValidate`, `SchemaCompile`, `SchemaProject` |
| Mutation | `SchemaFieldAdd`, `SchemaFieldDrop`, `SchemaFieldRename`, `SchemaFieldRetype`, `SchemaMerge` |
| CRDT Merge | `SchemaUnion` (grow-only), `SchemaIntersect`, `SchemaLUB` (least upper bound) |

Schema is itself a CRDT (grow-only set of fields):
- Fields can be added, never removed (tombstone = "optional")
- Type conflicts → widen to union type
- Cross-replica schema merge → union of all fields

### Level 2: Document/Batch Operations

| Category | Operations |
|----------|------------|
| Structural | `ObjectStart`, `ObjectEnd`, `ArrayStart`, `ArrayEnd` |
| Field | `FieldAdd`, `FieldModify`, `FieldDelete`, `FieldRead` |
| Array | `ArrayInsert`, `ArrayRemove`, `ArrayReplace`, `ArrayBuild`, `ArrayFilter`, `ArrayMap`, `ArrayReduce` |
| Presence | `CheckPresence`, `CheckAbsence`, `CheckNull`, `CheckNotNull` |
| CRDT | `MergeField`, `ConflictResolve` |
| Batch | `BatchExecute`, `StreamBuild`, `StreamFilter`, `StreamMap`, `StreamEmit` |

Document CRDT: Per-field LWW register with vector clock.

### Level 3: Line/Stream Operations

| Category | Operations |
|----------|------------|
| Line CRUD | `LineAppend`, `LineModify`, `LineDelete`, `LineRead` |
| Line CRDT | `LineMerge`, `LineTombstone`, `LineReorder` (RGA) |
| Stream | `ChunkedDelta`, `ByteRangeSync`, `TailFollow`, `Checkpoint`, `Resume` |

Line CRDT: Per-line identity with position CRDT (RGA/LSEQ for ordering).

## Merge Strategies

| Strategy | Behavior | Use Case |
|----------|----------|----------|
| `LastWriteWins` | Higher timestamp wins | Default |
| `Max` | Larger numeric value | Counters, high-water marks |
| `Min` | Smaller numeric value | Low-water marks |
| `Additive` | Sum numbers, concat strings | Counters, logs |
| `Union` | Set union for arrays | Tags, permissions |
| `Custom(name)` | User-defined function | Domain-specific |

### Strategy Applicability by Level

| Level | LWW | MAX | MIN | ADD | UNION | CUSTOM | G-SET | RGA |
|-------|-----|-----|-----|-----|-------|--------|-------|-----|
| Schema | - | - | - | - | ✅ | - | ✅ | - |
| Document | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | - | - |
| Line | ✅ | - | - | - | - | - | ✅ | ✅ |

## Delta-CRDT Sync

| Level | Delta Unit | Sync Granularity | Memory |
|-------|------------|------------------|--------|
| Schema | (path, type) pair | Per-field addition | O(num_fields) |
| Document | Field operation | Per-path change | O(doc_size) |
| Line | Line operation | Per-line or range | O(line_size) |

## Unified Tape Representation

All formats emit a unified tape structure with format-specific markers:

```rust
pub enum NodeType {
    // Universal (all formats)
    ObjectStart, ObjectEnd,
    ArrayStart, ArrayEnd,
    String(u16), Number(f64), Bool(bool), Null,
    SkipMarker,

    // YAML-specific
    YamlDocumentStart,
    YamlAnchor(String),
    YamlAlias(String),

    // TOML-specific
    TomlTableStart,
    TomlInlineTableStart,
    TomlArrayTableStart,

    // CSV-specific
    CsvRowStart, CsvRowEnd,
    CsvHeaderRow,

    // ISON-specific
    IsonTableBlock,
    IsonObjectBlock,
    IsonReference,

    // TOON-specific
    ToonTabularArray,
    ToonFoldedKey,
}
```

### Original Syntax Preservation

```rust
pub enum OriginalSyntax {
    // YAML
    YamlAnchor { name: String },
    YamlAlias { target: String },
    YamlFlowStyle,

    // TOML
    TomlDottedKey { full_key: String },
    TomlTripleQuotedString,

    // CSV
    CsvQuotedValue { has_quotes: bool },
    CsvDelimiter { delimiter: char },
    CsvNewlineStyle { style: NewlineStyle },

    // ISON
    IsonReference { kind: ReferenceKind },

    // TOON
    ToonFoldedKey { path: String },
    ToonArrayHeader { header_text: String },
}
```

## Schema Evolution

Schemas can evolve across batches. The system handles this implicitly:

1. **Schema Inference Per Batch**: `S_acc' = S_acc ∪ Sₙ` (grow-only)
2. **Nullable by Default**: New/missing fields = `Optional<T>`
3. **Type Widening**: `int` + `string` → `int | string`
4. **Skip Tape Adaptation**: Compiled filter adapts per-batch

### Processing Contract

**Invariants (must hold):**
- Schema monotonicity: `S_batch_n ⊆ S_batch_n+1`
- Null-safe operations: Missing field = null, not error
- Type-widening safe: `int → int|string`, not error
- Order independence: Batches can arrive out-of-order
- Idempotent merge: Reprocessing batch = same result

**Future (explicit support):**
- Schema versioning
- Compatibility checking
- Migration scripts
- Schema registry

## Cross-Level Interaction

```
Schema change ──────► Propagates to document validation
     │
     ▼
Document op ────────► Filtered by schema (skip non-matching)
     │
     ▼
Line op ────────────► Inherits doc schema OR self-describes (ISONL)
```

## Format Applicability

| Level | JSON | YAML | TOML | CSV | ISON | TOON | JSONL | ISONL |
|-------|------|------|------|-----|------|------|-------|-------|
| Schema | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Document | ✅ | ✅ | ✅ | - | ✅ | ✅ | - | - |
| Line | - | - | - | ✅ | - | - | ✅ | ✅ |

## Novel Contributions

| # | Contribution | Level | Prior Art |
|---|--------------|-------|-----------|
| 1 | Schema-guided skip parsing | Meta | None |
| 2 | Schema as grow-only CRDT | Meta | None |
| 3 | Unified multi-format tape | Doc | None |
| 4 | Cross-format delta-CRDT | Doc | JSON only |
| 5 | Line-oriented delta-CRDT | Line | None |
| 6 | Schema-per-line (ISONL) | Line | None |
| 7 | SIMD-parallel CRDT merge | All | None |
| 8 | Streaming O(1) memory CRDT | Line | None |

## References

- Almeida, Shoker, Baquero: "Delta State Replicated Data Types" (2015-2018)
- Shapiro et al.: "Conflict-free Replicated Data Types" (2011)
- Langdale & Lemire: "Parsing Gigabytes of JSON per Second"
