# Causal Dot Store

## Overview

Causal dot stores enable distributed document synchronization with conflict resolution. They combine:

- **Dot store**: Tracks event identifiers for CRDT operations
- **Causal context**: Tracks observed events across replicas
- **Join operations**: Merges state deterministically

## Core Concepts

### Dots

A dot is a unique event identifier: `(replica_id, sequence_number)`.

```rust
pub struct Dot {
    pub replica: String,
    pub sequence: u64,
}
```

### Causal Context

Tracks which dots have been observed:

```rust
pub struct CausalContext {
    observed: BTreeMap<String, u64>, // replica → max observed sequence
}
```

### Causal Dot Store

Combines a dot store with causal context:

```rust
pub struct CausalDotStore<T> {
    pub store: T,
    pub context: CausalContext,
}
```

## Observed-Remove Semantics

Elements can only be removed if their addition was observed. Concurrent updates win over concurrent removals.

**Example**: Alice updates a field while Bob removes it. Alice's update survives.

This prevents data loss during concurrent operations.

## Integration with Skip Tapes

```rust
pub struct CausalSkipTape<'arena> {
    skip_tape: SkipTape<'arena>,
    causal_context: CausalContext,
    dot_store: DotStore,
}
```

Benefits:
- **Delta sync**: Transmit only changes since last sync
- **Concurrent safety**: Causal ordering prevents inconsistencies
- **Memory efficiency**: Reuses zero-allocation architecture

## Operations

### Remove with Observed Semantics

```rust
fn remove_field_observed(&mut self, path: &str, dot: Dot) -> Result<()> {
    // Only remove if addition was observed
    if self.context.has_observed(&dot) {
        self.remove_field(path)?;
    }
    // Concurrent updates preserve the field
    Ok(())
}
```

### Merge with Causal Ordering

```rust
fn merge(&mut self, other: &Self) -> Result<()> {
    // Merge stores with causal ordering
    self.store.join(&other.store);
    self.context.merge(&other.context);
    Ok(())
}
```

## Performance

| Metric | Baseline | With Causal | Impact |
|--------|----------|-------------|--------|
| Parsing | 25M ops/s | 24M ops/s | -4% |
| Memory | Zero-copy | +8 bytes/op | Minimal |
| Concurrent safety | None | Strong | Major |

## Implementation Roadmap

### Phase 1: Core Integration
- Add `CausalContext` to tape metadata
- Implement dot tracking for field operations
- Feature flag: `crdt`

### Phase 2: Observed-Remove
- Implement observed-remove logic
- Add concurrent update preservation
- Extend to arrays and objects

### Phase 3: Schema Evolution
- Track schema changes with causal dots
- Implement compatibility checking
- Support concurrent schema modifications

## References

- Almeida et al., "Delta State Replicated Data Types" (2018)
- Kleppmann and Bieniusa, "A Conflict-Free Replicated JSON Datatype" (2017)
- Rinberg et al., "DSON: JSON CRDT Using Delta-Mutations" (2022)
