# fionn-simd

[![Crates.io](https://img.shields.io/crates/v/fionn-simd.svg)](https://crates.io/crates/fionn-simd)
[![Documentation](https://docs.rs/fionn-simd/badge.svg)](https://docs.rs/fionn-simd)
[![License](https://img.shields.io/crates/l/fionn-simd.svg)](https://github.com/darach/fionn#license)

SIMD-accelerated JSON skip operations.

Most JSON libraries parse every byte of a document, even the fields you never read.
`fionn-simd` takes a different approach: it uses AVX2 and AVX-512 instructions to
skip past JSON values at up to 8 GiB/s, without building a DOM or allocating memory
proportional to document size.

## Skip strategies

| Strategy | Throughput | Notes |
|----------|------------|-------|
| **AVX2** | 4--8 GiB/s | x86_64 with AVX2 (Haswell+) |
| **Scalar** | 1.5 GiB/s | Portable fallback, always available |
| **JsonSki** | 1.0 GiB/s | Good general-purpose default |
| **Langdale** | 987 MiB/s | Handles escape-heavy content well |

```rust
use fionn_simd::{SkipStrategy, Skip};

let strategy = SkipStrategy::best_simd();
let skipper = strategy.skipper();
let result = skipper.skip_object(&json_bytes[1..]);
```

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For the full feature set -- streaming, CRDTs, diff/patch, and more -- see the
top-level crate.

## License

MIT OR Apache-2.0
