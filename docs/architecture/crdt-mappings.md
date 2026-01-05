# CRDT Mappings

## Overview

This document defines how Conflict-free Replicated Data Types (CRDTs) map to JSON structures. These mappings enable distributed, concurrent document operations with automatic conflict resolution.

## CRDT Types

### LWW Register (Last-Write-Wins)

Stores a single value. Later timestamp wins conflicts.

```json
{
  "_crdt": "lww_register",
  "value": "current_value",
  "timestamp": 1640995200000
}
```

**Properties**: Idempotent, commutative.

### MV Register (Multi-Value)

Preserves all concurrent values. Application resolves conflicts.

```json
{
  "_crdt": "mv_register",
  "values": [
    {"value": "alice_value", "timestamp": 1640995200000, "source": "alice"},
    {"value": "bob_value", "timestamp": 1640995201000, "source": "bob"}
  ]
}
```

### G-Set (Grow-Only Set)

Only add operations. No removes. Union of all replicas.

```json
{
  "_crdt": "g_set",
  "elements": ["item1", "item2", "item3"]
}
```

### 2P-Set (Two-Phase Set)

Add set plus remove set. Add wins over remove for same element.

```json
{
  "_crdt": "2p_set",
  "adds": ["item1", "item2", "item3"],
  "removes": ["item2"]
}
```

### OR-Set (Observed-Remove Set)

Elements have unique tags. Remove requires observing the element.

```json
{
  "_crdt": "or_set",
  "elements": [
    {"value": "item1", "tag": "unique_tag_1"},
    {"value": "item2", "tag": "unique_tag_2"}
  ],
  "tombstones": ["unique_tag_3"]
}
```

### G-Counter (Grow-Only Counter)

Sum of all replica increments. No decrement.

```json
{
  "_crdt": "g_counter",
  "replicas": {
    "replica_a": 5,
    "replica_b": 3,
    "replica_c": 8
  }
}
```

### PN-Counter (Positive-Negative Counter)

Increment and decrement. Net = increments - decrements.

```json
{
  "_crdt": "pn_counter",
  "increments": {"replica_a": 10, "replica_b": 5},
  "decrements": {"replica_a": 2, "replica_b": 1}
}
```

### LWW-Map

Map with LWW registers for each key.

```json
{
  "_crdt": "lww_map",
  "entries": {
    "key1": {"value": "data1", "timestamp": 1640995200000},
    "key2": {"value": "data2", "timestamp": 1640995201000}
  }
}
```

## Conflict Resolution

| Strategy | When Applied | Algorithm |
|----------|--------------|-----------|
| LastWriteWins | LWW types | Max timestamp |
| Additive | Counters, sets | Sum values |
| Max | Numeric values | Maximum |
| Min | Numeric values | Minimum |
| Union | Sets | Merge all |
| Custom | Application-specific | User-defined |

## Convergence Guarantees

All CRDT types guarantee eventual convergence:

| CRDT Type | Convergence | Commutativity | Idempotency |
|-----------|-------------|---------------|-------------|
| LWW Register | Yes | Yes | Yes |
| MV Register | Yes | Yes | Yes |
| G-Set | Yes | Yes | Yes |
| 2P-Set | Yes | Yes | Yes |
| OR-Set | Yes | Yes | Yes |
| G-Counter | Yes | Yes | Yes |
| PN-Counter | Yes | Yes | Yes |
| LWW-Map | Yes | Yes | Yes |

## Usage

### Replica Synchronization

```rust
pub struct Replica {
    replica_id: String,
    local_operations: Vec<Operation>,
    applied_operations: HashSet<OperationId>,
}

impl Replica {
    pub fn sync_with(&mut self, other: &Replica) {
        // Exchange operations not seen by other
        let missing = self.local_operations.iter()
            .filter(|op| !other.applied_operations.contains(&op.id))
            .cloned()
            .collect::<Vec<_>>();

        // Apply missing operations
        for op in missing {
            self.apply_operation(op);
        }
    }
}
```

### Conflict Resolution

```rust
pub fn resolve_conflict(
    local: &Value,
    remote: &Value,
    strategy: &MergeStrategy
) -> Value {
    match strategy {
        MergeStrategy::LastWriteWins => remote.clone(),
        MergeStrategy::Additive => {
            let a = local.as_number().unwrap_or(0);
            let b = remote.as_number().unwrap_or(0);
            Value::from(a + b)
        }
        MergeStrategy::Max => {
            let a = local.as_number().unwrap_or(0);
            let b = remote.as_number().unwrap_or(0);
            Value::from(a.max(b))
        }
        MergeStrategy::Union => {
            // Merge arrays
            local.clone()
        }
    }
}
```

## Memory Overhead

| CRDT Type | Memory per Element | Growth Rate |
|-----------|-------------------|-------------|
| LWW Register | 16 bytes | O(1) |
| MV Register | 24 bytes | O(conflicts) |
| G-Set | 8 bytes | O(elements) |
| 2P-Set | 16 bytes | O(operations) |
| OR-Set | 32 bytes | O(elements) |
| G-Counter | 16 bytes | O(replicas) |
| PN-Counter | 32 bytes | O(replicas) |

## Optimization

1. **Delta encoding**: Send only changes since last sync
2. **Compression**: Compress operation payloads
3. **Batching**: Group operations into single messages
4. **Prioritization**: Send critical updates first
