# fionn-tape

[![Crates.io](https://img.shields.io/crates/v/fionn-tape.svg)](https://crates.io/crates/fionn-tape)
[![Documentation](https://docs.rs/fionn-tape/badge.svg)](https://docs.rs/fionn-tape)
[![License](https://img.shields.io/crates/l/fionn-tape.svg)](https://github.com/darach/fionn#license)

Tape-based JSON representation for fionn.

A tape stores JSON as a flat sequence of tokens rather than a tree of heap-allocated
nodes. This layout is cache-friendly and avoids per-value allocation, making it well
suited to high-throughput pipelines where documents are parsed, filtered, and
re-serialised without long-lived ownership.

`fionn-tape` builds on `simd-json`'s tape model and integrates with the rest of the
fionn ecosystem -- skip operations, schema filtering, and memory pooling all operate
on tapes.

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For the full feature set -- SIMD skipping, CRDTs, diff/patch, streaming, and
more -- see the top-level crate.

## License

MIT OR Apache-2.0
