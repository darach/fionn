# fionn-core

[![Crates.io](https://img.shields.io/crates/v/fionn-core.svg)](https://crates.io/crates/fionn-core)
[![Documentation](https://docs.rs/fionn-core/badge.svg)](https://docs.rs/fionn-core)
[![License](https://img.shields.io/crates/l/fionn-core.svg)](https://github.com/darach/fionn#license)

Core types, error handling, and traits for the fionn JSON processing ecosystem.

Every other fionn crate depends on this one. It defines the shared vocabulary: error
types, path representations, operation enums, and the trait interfaces that higher-level
crates implement. If you build on fionn directly, you rarely import `fionn-core` yourself
-- the umbrella [`fionn`](https://crates.io/crates/fionn) crate re-exports what you need.

## What it provides

- **Error types** -- a unified error hierarchy used across all fionn crates.
- **Path resolution** -- `ParsedPath` for navigating JSON structures by dotted or bracketed notation.
- **Operation model** -- the `Operation` and `OperationValue` enums that represent JSON mutations.
- **Shared traits** -- `Patchable`, `Mergeable`, and other interfaces that crates like `fionn-diff` and `fionn-crdt` build on.

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For SIMD skip operations, CRDT sync, diff/patch, streaming, and more, see the
top-level crate.

## License

MIT OR Apache-2.0
