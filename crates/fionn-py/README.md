# fionn-py

Python bindings for fionn - Fast JSON library with SIMD acceleration.

## Features

- **Drop-in orjson replacement**: `import fionn` works like `import orjson`
- **ISONL streaming**: 11.9x faster than fastest JSON parser (sonic-rs)
- **JSONL streaming**: Schema-filtered parsing with batch processing
- **Multi-format**: YAML, TOML, CSV, ISON, TOON parsing
- **Extended ops**: Gron, Diff/Patch, CRDT, Tape API

## Installation

```bash
# Using uv (recommended)
uv pip install fionn

# Using pip
pip install fionn
```

## Quick Start

### orjson-compatible API

```python
import fionn

# Parse JSON
data = fionn.loads(b'{"name": "Alice", "age": 30}')

# Serialize JSON
output = fionn.dumps(data, option=fionn.OPT_INDENT_2)
```

### ISONL Streaming (11.9x faster)

```python
import fionn.ext as fx

# Read ISONL (schema-embedded, zero inference overhead)
for batch in fx.IsonlReader("data.isonl"):
    for record in batch:
        process(record)

# Convert JSONL to ISONL for 11.9x speedup on repeated reads
fx.jsonl_to_isonl("input.jsonl", "output.isonl", table="events", infer_schema=True)
```

## Performance

| Operation | Baseline | fionn | Speedup |
|-----------|----------|-------|---------|
| JSON loads | orjson | match | 1x |
| JSONL streaming | sonic-rs | match | 1x |
| **ISONL streaming** | sonic-rs | **11.9x faster** | **11.9x** |

## Development

```bash
# Install dev dependencies
uv sync --dev

# Build
maturin develop

# Test
pytest

# Lint
ruff check .

# Type check
mypy .
```

## License

MIT OR Apache-2.0
