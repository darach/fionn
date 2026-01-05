# fionn Documentation

Documentation for fionn, a SIMD-accelerated JSON toolkit with CRDT semantics.

## Architecture

Internal design and data structures.

- [DOMless Processing](architecture/domless-processing.md) - Tape-based parsing without DOM overhead
- [Traits](architecture/traits.md) - The trait ecosystem and parallel processing
- [CRDT Mappings](architecture/crdt-mappings.md) - JSON to CRDT type mapping
- [Delta CRDTs](architecture/delta-crdt-mappings.md) - Delta state synchronization
- [Causal Dot Store](architecture/causal-dot.md) - Time-travel and causality tracking

## Capabilities

- [Point-in-Time](capabilities/point-in-time.md) - Causal contexts and history
- [Parallel Processing](architecture/traits.md#parallel-processing) - rayon-based parallelism
- [GPU Acceleration](architecture/domless-processing.md#gpu-acceleration) - wgpu-based pre-scan

## Performance

- [Optimization Guide](performance/optimization-guide.md) - Choosing the right approach
- [Benchmark Summary](performance/summary.md) - Current metrics

## Research

Technical analysis and algorithm research.

- [Architecture Comparison](research/comparison.md) - How fionn relates to its foundations
- [Merge Optimization](research/merge-optimization.md) - CRDT merge strategies
- [Skip Strategies](research/skip-strategies.md) - JSON skip implementation research

### Academic Papers

- [Full Paper](research/papers/paper-full.md) - Schema-aware skip processing for JSON stream analytics
- [SIMD Diff/Patch/Merge](research/papers/paper-simd-diffpatchmerge.md) - Structural differencing
- [SIMD Gron](research/papers/paper-simd-gron.md) - Greppable JSON transformation
- [Terascale Processing](research/papers/paper-terascale.md) - Scaling to terabytes

## Quick Links

- [Main README](../README.md)
- [API Documentation](https://docs.rs/fionn)
- [Development Guide](../AGENTS.md) - Build commands and code style
