# Architecture Comparison

How fionn relates to its foundations and where each approach fits.

## Foundations

| Project | Role | License |
|---------|------|---------|
| [simd-json](https://github.com/simd-lite/simd-json) | SIMD parsing, tape structure | Apache-2.0/MIT |
| [serde_json](https://github.com/serde-rs/json) | Baseline JSON, serde integration | Apache-2.0/MIT |
| [sonic-rs](https://github.com/cloudwego/sonic-rs) | Rust port of the bytedance sonic json library | Apache-2.0 |
| [helsing-ai/dson](https://github.com/helsing-ai/dson) | Delta-CRDT semantics | Apache-2.0 |

## Approach Comparison

| Aspect | serde_json | simd-json | DSON | fionn |
|--------|------------|-----------|------|-------|
| **Parsing** | Scalar | SIMD | Scalar | SIMD |
| **Data model** | DOM | Tape | DOM | Skip tape |
| **CRDT** | No | No | Yes | Yes |
| **Schema filtering** | No | No | No | Yes |
| **JSONL streaming** | Manual | Manual | No | Native |

## When Each Wins

**serde_json**:
- Small documents (<1KB)
- Type-safe deserialization to structs
- Ecosystem compatibility

**simd-json**:
- Large documents (>1KB)
- Read-heavy workloads
- Maximum parse throughput

**sonic-rs**:
- High-performance SIMD parsing with lazy evaluation
- On-demand value access without full DOM construction
- Get-path API for selective field extraction

**DSON**:
- Pure CRDT semantics
- Simpler codebase
- No SIMD dependency

**fionn**:
- Schema-filtered extraction
- JSONL batch processing
- CRDT + performance
- Memory-constrained environments

## Novel Components

### Skip Tape

Schema-compiled parsing that skips irrelevant fields during parse, not after:

```
Traditional: Parse all -> Filter -> Serialize subset
Skip tape:   Parse subset -> Serialize subset
```

Memory and CPU scale with selected fields, not document size.

### SIMD Path Resolution

Field lookup using SIMD string matching:

```rust
tape.resolve_path("user.profile.settings.theme")
```

O(1) lookup after initial parse vs O(n) re-traversal.

### Delta-CRDT Integration

Operations tracked with causal context for distributed merge:

- Last-write-wins (LWW)
- Max/Min numeric
- Additive counters
- Observed-remove sets

## Trade-offs

| Aspect | Advantage | Cost |
|--------|-----------|------|
| SIMD parsing | 2-3x throughput on large docs | Setup overhead on small docs |
| Skip tape | Memory scales with schema | Schema compilation cost |
| CRDT tracking | Distributed merge | Causal context memory |
| Zero-copy | No allocation during access | Lifetime constraints |

## See Also

- [Performance Summary](../performance/summary.md) - Benchmark data
- [Merge Optimization](merge-optimization.md) - CRDT merge strategies
