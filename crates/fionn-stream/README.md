# fionn-stream

[![Crates.io](https://img.shields.io/crates/v/fionn-stream.svg)](https://crates.io/crates/fionn-stream)
[![Documentation](https://docs.rs/fionn-stream/badge.svg)](https://docs.rs/fionn-stream)
[![License](https://img.shields.io/crates/l/fionn-stream.svg)](https://github.com/darach/fionn#license)

Streaming JSON and JSONL processing for fionn.

`fionn-stream` processes newline-delimited JSON (JSONL) in batches rather than
line-by-line, using SIMD skip operations and schema filtering to extract only the
fields you need. Batch processing is 1.45x faster than line-by-line parsing. The
crate also supports ISONL -- a schema-embedded format that eliminates per-line schema
inference, reaching 11.9x the throughput of sonic-rs on repeated reads.

## Capabilities

- **JSONL batch processing** -- parse batches of newline-delimited records with schema filtering.
- **ISONL** -- a schema-embedded streaming format for repeated-read workloads.
- **Multi-format** -- optional YAML, TOML, CSV, ISON, and TOON support via feature flags.

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For the full feature set -- SIMD skipping, CRDTs, diff/patch, and more -- see
the top-level [fionn](https://crates.io/crates/fionn) crate.

## License

MIT OR Apache-2.0
