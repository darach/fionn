# fionn

[![Crates.io](https://img.shields.io/crates/v/fionn.svg)](https://crates.io/crates/fionn)
[![Documentation](https://docs.rs/fionn/badge.svg)](https://docs.rs/fionn)
[![PyPI](https://img.shields.io/pypi/v/fionn.svg)](https://pypi.org/project/fionn/)
[![License](https://img.shields.io/crates/l/fionn.svg)](https://github.com/darach/fionn#license)

A Swiss Army knife for JSON, featuring SIMD acceleration, schema inference,
diff/patch/merge, delta-state CRDTs, and transactional envelopes.

This is the umbrella crate. It re-exports the individual fionn crates so you can
depend on one package and reach everything:

| Crate | Purpose |
|-------|---------|
| [`fionn-core`](https://crates.io/crates/fionn-core) | Core types, errors, and traits |
| [`fionn-simd`](https://crates.io/crates/fionn-simd) | SIMD skip operations (AVX2, AVX-512) |
| [`fionn-tape`](https://crates.io/crates/fionn-tape) | Tape-based JSON representation |
| [`fionn-diff`](https://crates.io/crates/fionn-diff) | RFC 6902 diff, patch, three-way merge |
| [`fionn-crdt`](https://crates.io/crates/fionn-crdt) | Delta-state CRDTs for distributed sync |
| [`fionn-tx`](https://crates.io/crates/fionn-tx) | Transactional envelopes for CRDTs |
| [`fionn-ops`](https://crates.io/crates/fionn-ops) | Processors, schema filtering, queries |
| [`fionn-gron`](https://crates.io/crates/fionn-gron) | Make JSON greppable |
| [`fionn-stream`](https://crates.io/crates/fionn-stream) | JSONL and ISONL streaming |
| [`fionn-pool`](https://crates.io/crates/fionn-pool) | Reusable tape buffer pooling |
| [`fionn-cli`](https://crates.io/crates/fionn-cli) | Command-line interface |

## Python bindings

Python users can install fionn from PyPI. The [`fionn`](https://pypi.org/project/fionn/)
Python package provides a drop-in orjson replacement, ISONL streaming at 11.9x the
throughput of sonic-rs, and access to the full Rust feature set. See the
[fionn-py README](https://github.com/darach/fionn/tree/main/crates/fionn-py) for details.

```bash
pip install fionn
```

## Quick start

```rust
use fionn_simd::{SkipStrategy, Skip};

// Skip an entire JSON value at up to 8 GiB/s
let strategy = SkipStrategy::best_simd();
let skipper = strategy.skipper();
let result = skipper.skip_object(&json_bytes[1..]);
```

## License

MIT OR Apache-2.0
