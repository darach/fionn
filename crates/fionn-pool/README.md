# fionn-pool

[![Crates.io](https://img.shields.io/crates/v/fionn-pool.svg)](https://crates.io/crates/fionn-pool)
[![Documentation](https://docs.rs/fionn-pool/badge.svg)](https://docs.rs/fionn-pool)
[![License](https://img.shields.io/crates/l/fionn-pool.svg)](https://github.com/darach/fionn#license)

Reusable tape buffer pooling for reduced allocation overhead.

Parsing JSON into a tape allocates memory. In a high-throughput pipeline that processes
millions of documents, those allocations add up. `fionn-pool` maintains a pool of
pre-allocated tape buffers with LRU and size-limited eviction, so each parse reuses a
buffer instead of allocating a new one. The result is lower latency and less pressure
on the allocator.

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For the full feature set -- SIMD skipping, CRDTs, diff/patch, streaming, and
more -- see the top-level [fionn](https://crates.io/crates/fionn) crate.

## License

MIT OR Apache-2.0
