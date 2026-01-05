# Trait Abstractions

## Overview

fionn uses traits to define a common interface for document processing. This enables comparison across implementations and consistent CRDT semantics.

## Core Traits

### DocumentProcessor

The fundamental trait for processing JSON documents.

```rust
pub trait DocumentProcessor {
    fn process(&mut self, input: &str) -> Result<String>;
    fn apply_operation(&mut self, op: &Operation) -> Result<()>;
    fn apply_operations(&mut self, ops: &[Operation]) -> Result<()>;
    fn output(&self) -> Result<String>;
}
```

### FieldOperations

CRUD operations on document fields.

```rust
pub trait FieldOperations {
    fn field_add(&mut self, path: &str, value: Value) -> Result<()>;
    fn field_modify(&mut self, path: &str, value: Value) -> Result<()>;
    fn field_delete(&mut self, path: &str) -> Result<()>;
    fn field_read(&self, path: &str) -> Result<Option<Value>>;
    fn field_exists(&self, path: &str) -> bool;
}
```

### ArrayOperations

Operations for JSON arrays.

```rust
pub trait ArrayOperations {
    fn array_insert(&mut self, path: &str, index: usize, value: Value) -> Result<()>;
    fn array_remove(&mut self, path: &str, index: usize) -> Result<()>;
    fn array_replace(&mut self, path: &str, index: usize, value: Value) -> Result<()>;
    fn array_len(&self, path: &str) -> Result<usize>;
    fn array_filter(&mut self, path: &str, predicate: &Predicate) -> Result<()>;
    fn array_map(&mut self, path: &str, transform: &Transform) -> Result<()>;
}
```

### SchemaAware

Schema-based filtering.

```rust
pub trait SchemaAware {
    fn matches_input_schema(&self, path: &str) -> bool;
    fn matches_output_schema(&self, path: &str) -> bool;
    fn input_schema(&self) -> Vec<String>;
    fn output_schema(&self) -> Vec<String>;
}
```

## CRDT Traits

### VectorClock

Tracks causality across replicas.

```rust
pub struct VectorClock {
    pub clocks: BTreeMap<String, u64>,
}

impl VectorClock {
    pub fn increment(&mut self, replica_id: &str);
    pub fn merge(&mut self, other: &VectorClock);
    pub fn happened_before(&self, other: &VectorClock) -> bool;
    pub fn concurrent_with(&self, other: &VectorClock) -> bool;
}
```

### CrdtMerge

Core trait for CRDT merge operations.

```rust
pub trait CrdtMerge {
    fn merge_operation(&mut self, op: CrdtOperation) -> Result<Option<Conflict>>;
    fn merge_field(&mut self, path: &str, value: Value, timestamp: u64, strategy: &MergeStrategy) -> Result<Option<Conflict>>;
    fn vector_clock(&self) -> &VectorClock;
    fn replica_id(&self) -> &str;
}
```

### DeltaCrdt

Delta-state CRDT for efficient synchronization.

```rust
pub trait DeltaCrdt: CrdtMerge {
    type Delta;
    fn generate_delta(&self, since: &VectorClock) -> Self::Delta;
    fn apply_delta(&mut self, delta: Self::Delta) -> Result<Vec<Conflict>>;
    fn compact(&mut self);
}
```

## Merge Strategies

### LastWriteWins

The operation with the higher timestamp wins.

```
Local:  counter = 10 @ t=5
Remote: counter = 20 @ t=8
Result: counter = 20 (remote wins)
```

### Max

The maximum value wins.

```
Local:  counter = 100 @ t=5
Remote: counter = 50  @ t=8
Result: counter = 100 (local wins)
```

### Additive

Values combine.

```
Local:  counter = 10
Remote: counter = 20
Result: counter = 30
```

### Union

For sets, take the union of elements.

```
Local:  tags = ["a", "b"]
Remote: tags = ["b", "c"]
Result: tags = ["a", "b", "c"]
```

## Usage

### Basic Processing

```rust
let mut proc = Processor::new("replica_1");

// Process input
proc.process(r#"{"name": "Alice", "age": 30}"#)?;

// Read and modify
let name = proc.field_read("name")?;
proc.field_modify("age", Value::from(31))?;

// Output
let result = proc.output()?;
```

### CRDT Operations

```rust
let mut proc_a = Processor::new("replica_a");
let mut proc_b = Processor::new("replica_b");

// Both process same document
proc_a.process(r#"{"counter": 0}"#)?;
proc_b.process(r#"{"counter": 0}"#)?;

// Concurrent modifications
proc_a.field_modify("counter", Value::from(10))?;
proc_b.field_modify("counter", Value::from(20))?;

// Merge with strategy
let conflict = proc_b.merge_field(
    "counter",
    Value::from(10),
    timestamp,
    &MergeStrategy::Max,
)?;
// Result: counter = 20 (max wins)
```

### Parallel Processing

```rust
let proc = Processor::new("replica_1").with_parallel(true);

// Process documents in parallel
let results = proc.process_batch_parallel(documents)?;

// Apply operations in parallel
let conflicts = proc.apply_operations_parallel(ops)?;
```

## Characteristics

| Feature | Support |
|---------|---------|
| Zero-copy parsing | Yes |
| SIMD acceleration | Yes |
| Schema filtering | Yes |
| CRDT support | Yes |
| Streaming | Yes |
| Parallel processing | Yes |

## Performance Tips

1. **Batch operations**: Use `apply_operations()` for multiple changes
2. **Schema filtering**: Define schemas to skip unnecessary parsing
3. **Vector clock compaction**: Call `compact()` periodically
4. **Buffered delivery**: Use `buffer_operation()` for out-of-order messages
