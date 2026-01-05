# Delta-CRDT Operations

## Overview

This document describes operations, their semantics, and optimization strategies for schema-aware JSON processing.

## Operation Types

### Structural Operations

| Operation | Description | Zero-Alloc |
|-----------|-------------|------------|
| ObjectStart | Begin object construction | Yes |
| ObjectEnd | Complete object construction | Yes |
| ArrayStart | Begin array construction | Yes |
| ArrayEnd | Complete array construction | Yes |

### Field Operations

| Operation | Description | Error Condition |
|-----------|-------------|-----------------|
| FieldAdd | Add field if not present | Path not in schema |
| FieldModify | Update existing field | Field doesn't exist |
| FieldDelete | Remove field | Path not in schema |

### Array Operations

| Operation | Description | Validation |
|-----------|-------------|------------|
| ArrayInsert | Insert at index | Bounds checked |
| ArrayRemove | Remove at index | Bounds checked |
| ArrayReplace | Replace at index | Bounds checked |

### CRDT Operations

| Operation | Description | Timestamp |
|-----------|-------------|-----------|
| MergeField | Merge with timestamp | Required |
| ConflictResolve | Resolve conflicts | N/A |

## Canonical Transformations

Operations combine and simplify:

**Delete + Add → Modify**
```
Input:  [FieldDelete("age"), FieldAdd("age", 25)]
Output: [FieldModify("age", 25)]
```

**Add + Modify → Modify**
```
Input:  [FieldAdd("name", "Alice"), FieldModify("name", "Bob")]
Output: [FieldModify("name", "Bob")]
```

**Modify + Delete → Delete**
```
Input:  [FieldModify("age", 30), FieldDelete("age")]
Output: [FieldDelete("age")]
```

**Invalid: Delete + Modify**
```
Input:  [FieldDelete("age"), FieldModify("age", 30)]
Output: Error - cannot modify deleted field
```

## Processing Pipeline

```
Input JSON → Parse → Input Schema Filter → Canonical Transform
           → Redundancy Eliminate → Coalesce → Output Schema Filter → Serialize
```

## Optimization

### Schema Filtering

**Input filtering**: Process only fields in schema. 40-60% operation reduction.

**Output filtering**: Serialize only schema fields. 50-80% output reduction.

### Redundancy Elimination

Multiple deletes become one:
```rust
// Before
[FieldDelete("x"), FieldDelete("x"), FieldDelete("x")]
// After
[FieldDelete("x")]
```

### Operation Coalescing

Multiple modifies keep last value:
```rust
// Before
[FieldModify("name", "Alice"), FieldModify("name", "Bob"), FieldModify("name", "Charlie")]
// After
[FieldModify("name", "Charlie")]
```

## Performance

| Operation | Time | Throughput |
|-----------|------|------------|
| FieldAdd | 25 ns | 40M/sec |
| FieldModify | 30 ns | 33M/sec |
| FieldDelete | 20 ns | 50M/sec |
| Schema Filter | 15 ns | 66M/sec |
| Canonical Transform | 50 ns | 20M/sec |

## Value Types

```rust
pub enum OperationValue {
    StringRef(&'tape str),    // Reference to tape
    NumberRef(&'tape str),    // Reference to tape
    BoolRef(bool),            // Direct copy
    Null,                     // Unit
    ObjectRef { start, end }, // Tape range
    ArrayRef { start, end },  // Tape range
}
```

Zero allocation: values reference the tape directly.
