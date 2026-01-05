# Point-in-Time & Causal Capabilities

This guide covers the temporal and causal history features of fionn, enabling time-travel, conflict resolution, and history tracking.

## Core Concepts

### Dots (Points in Time)

A `Dot` represents a unique event identifier in a distributed system—a "point in time" for a specific replica.

```rust
pub struct Dot {
    pub replica_id: u64,
    pub sequence: u64,
}
```

### Causal Context

The `CausalContext` represents the "horizon" of knowledge—what events a replica has observed.

```rust
pub struct CausalContext {
    context: HashMap<u64, u64>,  // Replica ID -> Max Sequence
}
```

If a context includes `{ A: 5, B: 3 }`, it means this replica has seen all events from Replica A up to sequence 5, and from Replica B up to sequence 3.

## Usage

### Tracking History

Use the `CausalDotStore` to track operations with their causal history:

```rust
use fionn_crdt::dot_store::{CausalContext, Dot};

// Initialize an empty context
let mut context = CausalContext::new();

// Record a new event
let event = Dot::new(1, 100); // Replica 1, Sequence 100
context.observe(event);

// Check if we have seen an event
assert!(context.has_observed(event));
```

### Checking Causality

Determine the order of events using vector clock logic:

```rust
let mut ctx_a = CausalContext::new();
ctx_a.observe(Dot::new(1, 10));

let mut ctx_b = CausalContext::new();
ctx_b.observe(Dot::new(1, 10));
ctx_b.observe(Dot::new(2, 5)); // B has seen more

// A is a subset of B, so A happened before B
assert!(ctx_a.happened_before(&ctx_b));
```

### Observed-Remove Semantics

fionn uses "Observed-Remove" semantics for concurrent deletions. A field can only be deleted if its addition has been observed. This prevents resurrection of deleted data or accidental deletion of unseen concurrent updates.

```rust
use fionn_crdt::observed_remove::ObservedRemoveProcessor;
use fionn_ops::Operation;

let mut processor = ObservedRemoveProcessor::new();

// Process an Add operation
processor.process_operation(&add_op)?;

// Process a Delete operation
// Only succeeds if the Add was previously observed
processor.process_operation(&delete_op)?;
```

## Implementation Notes

The current implementation uses:
- **Vector Stores**: Dots stored in `Vec` structures
- **Simplified Join**: When merging stores, may pick one non-empty store

Full set-reconciliation merging is planned for a future release.

## Related Documentation

- [Architecture: Causal Dot Store](../architecture/causal-dot.md)
- [Architecture: CRDT Mappings](../architecture/crdt-mappings.md)
