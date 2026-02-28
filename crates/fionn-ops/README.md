# fionn-ops

[![Crates.io](https://img.shields.io/crates/v/fionn-ops.svg)](https://crates.io/crates/fionn-ops)
[![Documentation](https://docs.rs/fionn-ops/badge.svg)](https://docs.rs/fionn-ops)
[![License](https://img.shields.io/crates/l/fionn-ops.svg)](https://github.com/darach/fionn#license)

Operations, processors, and transformations for fionn.

`fionn-ops` sits between the low-level tape and the high-level streaming layer. It
provides the `BlackBoxProcessor` for schema-filtered JSON processing, the
`OptimizedMergeProcessor` for applying merge strategies, and the query-language
evaluator that powers fionn's jq-like expressions. If you need to filter, transform,
or aggregate JSON without writing a full pipeline, this is the crate to reach for.

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For the full feature set -- SIMD skipping, CRDTs, diff/patch, streaming, and
more -- see the top-level [fionn](https://crates.io/crates/fionn) crate.

## License

MIT OR Apache-2.0
