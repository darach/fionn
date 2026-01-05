# DOMless Processing

## Overview

Traditional JSON parsers build a Document Object Model (DOM) in memory. fionn skips this step. Instead, it uses schema-based filtering to parse only the fields an application needs.

The benefit: process a 100 MB document while using only kilobytes of memory.

## The Problem with DOM

```
100MB JSON → Parse Full DOM → Process → Serialize → Output
     ↓            ↓              ↓          ↓          ↓
 Network      100MB RAM       100MB      100MB     Network
```

DOM parsing wastes resources:
- **Memory**: The entire document sits in RAM
- **CPU**: Every byte is processed, even irrelevant data
- **Scalability**: RAM limits document size

## Schema-Filtered Processing

```
100MB JSON → Schema Filter → Parse Relevant → Process → Output
     ↓            ↓               ↓            ↓          ↓
 Network    Input Schema       1KB RAM       1KB      Network
```

fionn inverts the model:
- **Memory**: O(schema size), not O(document size)
- **CPU**: Only parse matching paths
- **Scalability**: Process documents larger than RAM

## Architecture

### Schema Filter

```rust
pub struct SchemaFilter {
    paths: Vec<CompiledJsonPath>,
    matcher: PathMatcher,
}
```

The filter compiles JSON paths into efficient matchers. When parsing, it checks each field against the schema. Non-matching fields are skipped without parsing their values.

### Streaming Tape Processor

```rust
pub struct StreamingTapeProcessor {
    input_filter: SchemaFilter,
    output_filter: SchemaFilter,
    operations: Vec<DsonOperation>,
}
```

The processor reads JSON as a stream of events. It tracks the current path and applies operations only to matching fields.

### Operations

```rust
pub enum DsonOperation {
    FieldAdd { path: String, value: OperationValue },
    FieldModify { path: String, value: OperationValue },
    FieldDelete { path: String },
    ArrayInsert { path: String, index: usize, value: OperationValue },
    ArrayFilter { path: String, predicate: FilterPredicate },
    ArrayMap { path: String, transform: TransformFunction },
}
```

Operations compose. Apply them in sequence, or let the canonical processor eliminate redundant work.

## Performance

| Scenario | DOM Parser | fionn | Improvement |
|----------|-----------|-----|-------------|
| 1KB JSON, 1 field | ~10KB | ~1KB | 10× |
| 100MB JSON, 1 field | 100MB | ~1KB | 100,000× |
| 1GB JSON, 10 fields | 1GB+ | ~10KB | 100,000× |

These figures illustrate magnitude, not guarantees. Results depend on document structure and field access patterns.

## Use Cases

### Large-Scale Data Processing

```rust
let input_schema = vec!["users[*].id", "orders[*].total"];
let output_schema = vec!["users[*].id", "orders[*].total"];

let processor = StreamingTapeProcessor::new(input_schema, output_schema);
// Memory: ~1KB instead of 100MB
```

### API Processing

```rust
let input_schema = vec!["user.id", "request.action"];
let output_schema = vec!["user.id", "response.status"];

let processor = StreamingTapeProcessor::new(input_schema, output_schema);
// Only process relevant fields
```

### Stream Processing

```rust
let processor = StreamingTapeProcessor::new(
    vec!["events[*].type", "events[*].data"],
    vec!["events[*].id", "events[*].processed"]
);

for chunk in json_stream {
    let result = processor.process_chunk(chunk)?;
    output.send(result);
}
// Memory bounded regardless of stream size
```

## Parallel Processing

fionn supports parallel processing via rayon:

```rust
let proc = Processor::new("replica_1").with_parallel(true);

// Process documents in parallel
let results = proc.process_batch_parallel(documents)?;

// Apply operations in parallel (groups by path to avoid conflicts)
let conflicts = proc.apply_operations_parallel(ops)?;
```

## GPU Acceleration

Enable with `--features gpu`. Uses wgpu for GPU compute shaders:

- Line boundary detection via GPU scan
- Structural character detection
- Returns compact bitmasks per 64-byte chunk

```bash
cargo build --features gpu
```

## Path Notation

Paths use dot notation:
- `user.name` - nested field
- `users[0]` - array index
- `users[*].id` - wildcard (planned)

## Conclusion

DOMless processing eliminates wasted work. Parse what you need; skip the rest. The result: memory efficiency and throughput impossible with traditional DOM parsers.
