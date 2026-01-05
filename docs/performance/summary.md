# Performance Summary

Benchmark results from `cargo bench`. Results vary by hardware and workload.

## The Key Value Proposition: Skip Without Parsing

fionn's core advantage is **SIMD-accelerated skip operations** - traverse JSON structure at 8+ GiB/s without building a DOM.

### Skip vs Full Parse (253KB API Response, 1000 users)

| Method | Time | Throughput | Speedup |
|--------|------|------------|---------|
| serde_json full parse | 913 µs | 277 MiB/s | baseline |
| sonic_rs full parse | 195 µs | 1.26 GiB/s | 4.7x |
| **fionn AVX2 skip** | **31 µs** | **8.0 GiB/s** | **29.5x** |
| fionn JsonSki skip | 218 µs | 1.13 GiB/s | 4.2x |

**fionn is 29.5x faster than serde_json when you don't need to parse everything.**

## Skip Strategy Comparison

Skip strategies for nested JSON (50 levels deep, 610 bytes):

| Strategy | Time | Throughput | Notes |
|----------|------|------------|-------|
| **AVX2** | **140 ns** | **4.4 GiB/s** | SIMD-accelerated (best) |
| Scalar | 401 ns | 1.5 GiB/s | Baseline byte-by-byte |
| JsonSki | 624 ns | 1.0 GiB/s | Bracket counting |
| Langdale | 632 ns | 987 MiB/s | XOR prefix algorithm |

## Skip Scaling (AVX2)

AVX2 skip throughput scales with document size:

### Nested Objects
| Depth | Time | Throughput |
|-------|------|------------|
| 10 levels | 37 ns | 3.4 GiB/s |
| 50 levels | 140 ns | 4.4 GiB/s |
| 100 levels | 286 ns | 4.3 GiB/s |
| 200 levels | 532 ns | 4.7 GiB/s |

### Wide Objects
| Fields | Time | Throughput |
|--------|------|------------|
| 10 fields | 78 ns | 6.3 GiB/s |
| 100 fields | 651 ns | 7.9 GiB/s |
| 500 fields | 3.3 µs | 8.2 GiB/s |
| 1000 fields | 6.6 µs | **8.3 GiB/s** |

## Full Parsing Comparison

When you DO need to parse everything (100 users, 24KB):

| Library | Time | Throughput |
|---------|------|------------|
| serde_json | 88.6 µs | 277 MiB/s |
| simd-json | 21.4 µs | 1.13 GiB/s |
| sonic-rs | 19.7 µs | **1.22 GiB/s** |

## JSONL Streaming (1000 lines, 105KB)

| Method | Time | Throughput | Speedup |
|--------|------|------------|---------|
| serde_json line-by-line | 291 µs | 360 MiB/s | baseline |
| **fionn batch filtered** | **202 µs** | **523 MiB/s** | **1.45x** |

## CRDT Merge Operations

Raw merge function performance (pre-parsed values):

| Strategy | 1000 ops | Throughput |
|----------|----------|------------|
| LWW | 251 ns | 3.99 Gelem/s |
| Max | 312 ns | 3.21 Gelem/s |
| Additive | 234 ns | 4.27 Gelem/s |

## When to Use What

| Use Case | Recommendation | Expected Throughput |
|----------|----------------|---------------------|
| Skip unwanted fields | **fionn AVX2 skip** | 8+ GiB/s |
| JSONL streaming | **fionn batch** | 523 MiB/s |
| Full parse (small docs) | serde_json or sonic-rs | 277 MiB/s - 1.2 GiB/s |
| Full parse (large docs) | simd-json or sonic-rs | 1.1-1.2 GiB/s |
| Distributed sync + CRDT | **fionn + delta CRDT** | N/A |

## Trade-offs

**fionn costs**:
- ~4% parsing overhead vs simd-json for full DOM building
- Learning curve for skip-based programming model

**fionn benefits**:
- **29.5x faster** than serde_json for skip-only operations
- **8+ GiB/s** skip throughput on wide documents
- **1.45x faster** JSONL streaming
- CRDT merge without re-serialization

## Running Benchmarks

```bash
cargo bench --bench schema_selectivity  # Skip performance
cargo bench --bench comprehensive_benchmarks  # Full parse comparison
cargo bench --bench optimized_merge  # CRDT merge
cargo bench --bench gron_benchmark  # Gron transformation
cargo bench --bench simd_jsonl_bench  # JSONL streaming
```
