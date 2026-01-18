# fionn Documentation

Documentation for fionn, a SIMD-accelerated multi-format data toolkit.

## Architecture

Internal design and data structures.

- [DOMless Processing](architecture/domless-processing.md) - Tape-based parsing without DOM overhead
- [Tape-to-Tape Transformation](tape-to-tape.md) - Cross-format transformations via unified tape
- [Traits](architecture/traits.md) - The trait ecosystem and parallel processing
- [CRDT Mappings](architecture/crdt-mappings.md) - JSON to CRDT type mapping
- [Delta CRDTs](architecture/delta-crdt-mappings.md) - Delta state synchronization
- [Causal Dot Store](architecture/causal-dot.md) - Time-travel and causality tracking

## Formats

Multi-format SIMD parsing (JSON, YAML, TOML, CSV, ISON, TOON).

- [Multi-Format Parsing](formats/multi-format-parsing.md) - SIMD techniques for all formats

## Query

Extended JSONPath with kind predicates.

- [Query Extensions](query-extensions.md) - Kind and context filtering (`::string`, `::yaml:anchor`)

## Performance

- [Optimization Guide](performance/optimization-guide.md) - Choosing the right approach
- [Benchmarks](performance/benchmarks.md) - Current metrics and optimization history

## Capabilities

- [Point-in-Time](capabilities/point-in-time.md) - Causal contexts and history

## Future Work

- [Schema Validation](schema-future-work.md) - JSON Schema, Avro, JTD analysis

## Research

Technical analysis and algorithm research.

- [Architecture Comparison](research/comparison.md) - How fionn relates to its foundations
- [Performance Analysis](research/performance-analysis.md) - Value analysis techniques
- [Merge Optimization](research/merge-optimization.md) - CRDT merge strategies
- [Skip Strategies](research/skip-strategies.md) - JSON skip implementation research

### Academic Papers

- [Gron Beyond Trees](research/papers/gron-beyond-trees.md) - Path-value decomposition for non-hierarchical formats
- [Full Paper](research/papers/paper-full.md) - Schema-aware skip processing for JSON stream analytics
- [SIMD Diff/Patch/Merge](research/papers/paper-simd-diffpatchmerge.md) - Structural differencing
- [SIMD Gron](research/papers/paper-simd-gron.md) - Greppable JSON transformation
- [Terascale Processing](research/papers/paper-terascale.md) - Scaling to terabytes

## Quick Links

- [Main README](../README.md)
- [API Documentation](https://docs.rs/fionn)
- [Development Guide](../AGENTS.md) - Build commands and code style
