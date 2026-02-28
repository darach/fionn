# fionn-crdt

[![Crates.io](https://img.shields.io/crates/v/fionn-crdt.svg)](https://crates.io/crates/fionn-crdt)
[![Documentation](https://docs.rs/fionn-crdt/badge.svg)](https://docs.rs/fionn-crdt)
[![License](https://img.shields.io/crates/l/fionn-crdt.svg)](https://github.com/darach/fionn#license)

Delta-state CRDTs for distributed JSON synchronisation.

When multiple replicas edit the same JSON document concurrently, conflicts are
inevitable. `fionn-crdt` resolves them deterministically using Conflict-free
Replicated Data Types. It implements the Strict DSON model -- a JSON CRDT built on
delta-state dissemination -- so replicas converge to the same document without
coordination.

## Data structures

- **`DotObservedRemoveMap`** -- an add-wins observed-remove map keyed by JSON paths, with dot-based causal contexts for precise delta generation.
- **`ReplicatedGrowableArray`** -- an RGA for ordered sequences (JSON arrays), supporting concurrent insert, delete, and move.
- **Lattice merge** -- all types implement a join-semilattice merge so that applying the same delta twice is harmless.

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For transactional envelopes over these CRDTs, see
[fionn-tx](https://crates.io/crates/fionn-tx). For the full feature set, see the
top-level [fionn](https://crates.io/crates/fionn) crate.

## License

MIT OR Apache-2.0
