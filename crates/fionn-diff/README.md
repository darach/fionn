# fionn-diff

[![Crates.io](https://img.shields.io/crates/v/fionn-diff.svg)](https://crates.io/crates/fionn-diff)
[![Documentation](https://docs.rs/fionn-diff/badge.svg)](https://docs.rs/fionn-diff)
[![License](https://img.shields.io/crates/l/fionn-diff.svg)](https://github.com/darach/fionn#license)

JSON diff, patch, and three-way merge.

`fionn-diff` computes the structural difference between two JSON documents, produces
RFC 6902 JSON Patch operations, and applies patches to produce new documents. It also
supports three-way merge with configurable conflict-resolution strategies, which
`fionn-crdt` uses for distributed synchronisation.

## Capabilities

- **Diff** -- compute the minimal set of operations that transform one document into another.
- **Patch** -- apply an RFC 6902 patch to a document.
- **Three-way merge** -- merge concurrent edits from a common ancestor, with pluggable strategies (`Additive`, `Max`, `LastWriterWins`, and others).

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For SIMD skip operations, CRDT sync, streaming, and more, see the top-level
[fionn](https://crates.io/crates/fionn) crate.

## License

MIT OR Apache-2.0
