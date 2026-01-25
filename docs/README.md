# fionn Documentation

Documentation for fionn, a SIMD-accelerated multi-format data toolkit.

## Architecture

- [DOMless Processing](architecture/domless-processing.md) - Tape-based parsing without DOM overhead
- [Tape-to-Tape Transformation](tape-to-tape.md) - Cross-format transformations via unified tape
- [Traits](architecture/traits.md) - The trait ecosystem and parallel processing
- [CRDT Mappings](architecture/crdt-mappings.md) - JSON to CRDT type mapping
- [Delta CRDTs](architecture/delta-crdt-mappings.md) - Delta state synchronization
- [Causal Dot Store](architecture/causal-dot.md) - Time-travel and causality tracking

## Formats

- [Multi-Format Parsing](formats/multi-format-parsing.md) - SIMD techniques for all formats
- [Transformation Matrix](transformation-matrix.md) - Cross-format conversion fidelity

## Query

- [Query Extensions](query-extensions.md) - Kind and context filtering (`::string`, `::yaml:anchor`)

## Performance

- [Optimization Guide](performance/optimization-guide.md) - Choosing the right approach
- [Benchmarks](performance/benchmarks.md) - Current metrics and optimization history
- [Benchmark Analysis](benchmark-analysis.md) - Detailed performance breakdown
- [Benchmark Grid](benchmark-grid.md) - Format × operation matrix
- [Skip Parsing Analysis](skip-parsing-analysis.md) - Schema-guided skip performance

## Capabilities

- [Point-in-Time](capabilities/point-in-time.md) - Causal contexts and history

## Design Documents

- [Line-Oriented CRDT](line-oriented-crdt.md) - CRDT design for line-based formats
- [Multi-Format DSON CRDT](multi-format-dson-crdt.md) - Unified CRDT across formats
- [Schema Future Work](schema-future-work.md) - JSON Schema, Avro, JTD analysis
- [Python Bindings Plan](python-bindings-plan.md) - PyO3 bindings design

## Research

- [Architecture Comparison](research/comparison.md) - How fionn relates to its foundations
- [Performance Analysis](research/performance-analysis.md) - Value analysis techniques
- [Merge Optimization](research/merge-optimization.md) - CRDT merge strategies
- [Skip Strategies](research/skip-strategies.md) - JSON skip implementation research

### Papers

| Paper | PDF |
|-------|-----|
| [Research Brief](research/papers/brief.md) | [PDF](research/papers/brief.pdf) |
| [Schema-Aware Skip Processing](research/papers/paper-full.md) | [PDF](research/papers/paper-full.pdf) |
| [SIMD-Accelerated Gron](research/papers/paper-fionn-gron.md) | [PDF](research/papers/paper-fionn-gron.pdf) |
| [SIMD Diff/Patch/Merge](research/papers/paper-fionn-diffpatchmerge.md) | [PDF](research/papers/paper-fionn-diffpatchmerge.pdf) |
| [Terascale JSON Parsing](research/papers/paper-terascale.md) | [PDF](research/papers/paper-terascale.pdf) |
| [Gron Beyond Trees](research/papers/gron-beyond-trees.md) | [PDF](research/papers/gron-beyond-trees.pdf) |

## Quick Links

- [Main README](../README.md)
- [API Documentation](https://docs.rs/fionn)
- [Development Guide](../AGENTS.md)
