# fionn Optimization Guide

How to choose the right processing approach for your workload.

## Processing Approaches

| Approach | Best For | Trade-off |
|----------|----------|-----------|
| `serde_json` | Small docs (<1KB), type-safe deserialization | Standard overhead |
| simd-json tape | Large docs, read-heavy workloads | Setup cost on small docs |
| fionn | CRDT + large docs | ~3% overhead vs simd-json |
| Skip tape + schema | Selective field extraction | Schema compilation cost |
| JSONL batch | Streaming JSONL workloads | Batch-oriented only |

## Decision Guide

### By Document Size

| Size | Recommendation |
|------|----------------|
| < 1KB | `serde_json` (lowest setup overhead) |
| 1KB - 10KB | simd-json or fionn |
| > 10KB | fionn with skip tape |
| Streaming | JSONL batch processor |

### By Use Case

**Single document, type-safe parsing**
```rust
use serde_json;
let data: MyStruct = serde_json::from_str(json)?;
```

**Large document, field extraction**
```rust
use fionn_stream::skiptape::{SkipTapeProcessor, CompiledSchema};

let schema = CompiledSchema::compile(&[
    "user.id".to_string(),
    "user.name".to_string(),
])?;
let mut processor = SkipTapeProcessor::new();
let tape = processor.process_line(json, &schema)?;
```

**JSONL streaming**
```rust
use fionn_stream::skiptape::SimdJsonlBatchProcessor;

let mut processor = SimdJsonlBatchProcessor::new();
let results = processor.process_batch_raw_simd(jsonl_data)?;
```

**Distributed sync with CRDT**
```rust
use fionn_crdt::merge::{MergeStrategy, OptimizedMergeProcessor};

let mut processor = OptimizedMergeProcessor::new();
processor.set_default_strategy(MergeStrategy::LastWriteWins);
let result = processor.merge_field("field", &local_value, &remote_value);
```

## Feature Trade-offs

| Feature | Benefit | Cost |
|---------|---------|------|
| SIMD parsing | 2-3x throughput on large docs | Setup overhead on small docs |
| Skip tape | Memory scales with schema | Schema compilation |
| CRDT tracking | Distributed merge | Causal context memory |
| Zero-copy | No allocation during access | Lifetime constraints |
| Batch JSONL | Amortized parsing cost | Batch-only processing |

## Parallel Processing

Enable rayon-based parallelism for batch operations:

```rust
use fionn_crdt::merge::OptimizedMergeProcessor;

let processor = OptimizedMergeProcessor::new();
let results = processor.merge_batch_parallel(&entries);
```

JSONL processing uses rayon for parallel line processing by default.

## GPU Acceleration (Experimental)

GPU acceleration is an experimental feature that is currently disabled in the default build.
When enabled, it provides:
- Line boundary detection via GPU scan
- Structural character classification
- Returns bitmasks per 64-byte chunk

Best for very large JSONL files where GPU transfer overhead is amortized.

Note: The `gpu` feature requires `wgpu`, `pollster`, and `bytemuck` dependencies.

## Benchmarking

Run benchmarks to validate performance for your workload:

```bash
cargo bench
cargo bench --bench comprehensive_benchmarks
cargo bench --bench path_parsing
cargo bench --bench gron_benchmark
```

See [Performance Summary](summary.md) for current benchmark data.
