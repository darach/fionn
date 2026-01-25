# Fionn Python Bindings: Drop-in orjson Replacement + Extended Features

## Executive Summary

This plan outlines how to create Python bindings for fionn that:
1. **Drop-in replace orjson** with identical API compatibility
2. **Extend all fionn features** to Python (multi-format, CRDT, streaming, etc.)
3. **Require zero changes** to the existing Rust codebase
4. **Maintain performance parity** with orjson (or exceed it)

---

## Key Differentiators: Why fionn Python > orjson

```
┌────────────────────────────────────────────────────────────────────────────┐
│                       FIONN PYTHON VALUE PROPOSITION                        │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  1. orjson COMPATIBILITY                                                   │
│     • Drop-in replacement: import fionn as orjson                          │
│     • All 14 OPT_* flags supported                                         │
│     • Same exceptions, same behavior                                       │
│                                                                            │
│  2. ISONL STREAMING (11.9x faster than fastest JSON)                       │
│     • Schema-embedded line format                                          │
│     • SIMD-accelerated field extraction                                    │
│     • 355M cycles vs 4,226M cycles (sonic-rs baseline)                     │
│     • Zero schema inference overhead                                       │
│                                                                            │
│  3. JSONL STREAMING (matches sonic-rs)                                     │
│     • Schema-filtered parsing                                              │
│     • Batch processing with GIL release                                    │
│     • Selective field extraction                                           │
│                                                                            │
│  4. EXTENDED FEATURES (no Python alternatives)                             │
│     • Multi-format: YAML, TOML, CSV, ISON, TOON                            │
│     • Gron: path-based JSON exploration                                    │
│     • Diff/Patch: RFC 6902/7396 compliance                                 │
│     • CRDT: conflict-free distributed merging                              │
│     • Tape API: zero-copy advanced parsing                                 │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### Performance Summary (vs baselines)

| Operation | Baseline | fionn Target | Advantage |
|-----------|----------|--------------|-----------|
| JSON loads | orjson | Match (±5%) | Drop-in compat |
| JSON dumps | orjson | Match (±5%) | Drop-in compat |
| JSONL streaming | sonic-rs | Match | Schema filtering |
| **ISONL streaming** | **sonic-rs** | **11.9x faster** | **Key differentiator** |
| CRDT merge | manual Python | 50-100x | Rust SIMD |
| Gron | Python gron | 10-50x | Rust SIMD |

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           PYTHON LAYER                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  fionn (drop-in orjson replacement)                                  │   │
│  │  ─────────────────────────────────────                               │   │
│  │  • loads(data) → Any                                                 │   │
│  │  • dumps(obj, default=None, option=None) → bytes                     │   │
│  │  • Fragment(data) → Fragment                                         │   │
│  │  • OPT_* flags (14 flags)                                            │   │
│  │  • JSONEncodeError, JSONDecodeError                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  fionn.ext (extended features)                                       │   │
│  │  ──────────────────────────────                                      │   │
│  │  • Multi-format: parse_yaml(), parse_toml(), parse_csv(), etc.       │   │
│  │  • Gron: gron(), ungron(), gron_query()                              │   │
│  │  • Diff/Patch: diff(), patch(), merge()                              │   │
│  │  • CRDT: CrdtDocument, merge_crdt()                                  │   │
│  │  • Streaming: JsonlReader, JsonlWriter, stream_process()             │   │
│  │  • Tape: Tape, TapePool (advanced zero-copy)                         │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ PyO3 Bindings
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RUST LAYER (unchanged)                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  fionn-core   fionn-tape   fionn-simd   fionn-ops   fionn-gron             │
│  fionn-diff   fionn-crdt   fionn-stream fionn-pool                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Part 1: orjson Drop-in Replacement

### 1.1 API Compatibility Matrix

| orjson API | fionn Implementation | Notes |
|------------|---------------------|-------|
| `loads(data)` | `DsonTape::parse()` → Python dict | Zero-copy input via PyBackedBytes |
| `dumps(obj)` | `serde_json` serialization | Return bytes, not str |
| `dumps(obj, default=fn)` | Custom handler chain | 254-level recursion limit |
| `dumps(obj, option=flags)` | Bitflag processing | All 14 OPT_* flags |
| `Fragment(data)` | Passthrough wrapper | Embed pre-serialized JSON |
| `JSONEncodeError` | Custom exception | Subclass of TypeError |
| `JSONDecodeError` | Custom exception | Subclass of ValueError |

### 1.2 Option Flags Implementation

```rust
// crates/fionn-py/src/options.rs (new file in new crate)

pub const OPT_APPEND_NEWLINE: u32      = 1 << 0;
pub const OPT_INDENT_2: u32            = 1 << 1;
pub const OPT_NAIVE_UTC: u32           = 1 << 2;
pub const OPT_NON_STR_KEYS: u32        = 1 << 3;
pub const OPT_OMIT_MICROSECONDS: u32   = 1 << 4;
pub const OPT_PASSTHROUGH_DATACLASS: u32 = 1 << 5;
pub const OPT_PASSTHROUGH_DATETIME: u32  = 1 << 6;
pub const OPT_PASSTHROUGH_SUBCLASS: u32  = 1 << 7;
pub const OPT_SERIALIZE_DATACLASS: u32   = 1 << 8;
pub const OPT_SERIALIZE_NUMPY: u32       = 1 << 9;
pub const OPT_SERIALIZE_UUID: u32        = 1 << 10;
pub const OPT_SORT_KEYS: u32             = 1 << 11;
pub const OPT_STRICT_INTEGER: u32        = 1 << 12;
pub const OPT_UTC_Z: u32                 = 1 << 13;
```

### 1.3 Type Support Matrix

| Python Type | Serialization | Deserialization | Notes |
|-------------|--------------|-----------------|-------|
| `str` | ✓ UTF-8 validated | ✓ | Via serde |
| `bytes` | ✗ | ✓ Zero-copy input | PyBackedBytes |
| `dict` | ✓ | ✓ | Key constraints with flags |
| `list` | ✓ | ✓ | |
| `tuple` | ✓ as array | ✓ as list | |
| `int` | ✓ 64-bit/53-bit | ✓ | OPT_STRICT_INTEGER |
| `float` | ✓ IEEE 754 | ✓ | |
| `bool` | ✓ | ✓ | |
| `None` | ✓ as null | ✓ | |
| `dataclass` | ✓ with flag | N/A | OPT_SERIALIZE_DATACLASS |
| `datetime` | ✓ RFC 3339 | N/A | Multiple flags |
| `date` | ✓ ISO 8601 | N/A | |
| `time` | ✓ ISO 8601 | N/A | OPT_OMIT_MICROSECONDS |
| `uuid.UUID` | ✓ | N/A | OPT_SERIALIZE_UUID |
| `numpy.ndarray` | ✓ with flag | N/A | OPT_SERIALIZE_NUMPY |
| `enum.Enum` | ✓ as value | N/A | |

### 1.4 Performance Targets (vs orjson)

| Operation | orjson | fionn Target | Strategy |
|-----------|--------|--------------|----------|
| `loads()` | 2x faster than json | Match or exceed | SIMD via DsonTape |
| `dumps()` compact | 10x faster than json | Match or exceed | serde + SIMD |
| `dumps()` pretty | 27-54x faster | Match or exceed | Pre-allocated buffers |

---

## Part 2: Extended Features (fionn.ext)

### 2.1 Multi-Format Parsing

```python
# Proposed API
import fionn.ext as fx

# Format-specific parsing
data = fx.parse_yaml(yaml_string)
data = fx.parse_toml(toml_string)
data = fx.parse_csv(csv_string, has_header=True)
data = fx.parse_ison(ison_string)
data = fx.parse_isonl(isonl_string)  # ISONL: schema-embedded line format
data = fx.parse_jsonl(jsonl_string)  # JSONL: JSON Lines
data = fx.parse_toon(toon_string)

# Auto-detect format
data = fx.parse(input_string)  # Returns (data, detected_format)

# Format conversion
yaml_out = fx.to_yaml(data)
toml_out = fx.to_toml(data)
csv_out = fx.to_csv(data)
ison_out = fx.to_ison(data)
isonl_out = fx.to_isonl(data, schema=["id:int", "name:string"])
jsonl_out = fx.to_jsonl(data)
```

### 2.2 Gron Operations

```python
import fionn.ext as fx

# JSON to gron
gron_output = fx.gron('{"a": {"b": 1}}')
# Output: 'json = {};\njson.a = {};\njson.a.b = 1;\n'

# Gron to JSON
json_output = fx.ungron(gron_string)

# Query with gron
results = fx.gron_query(json_string, "$.users[*].name")

# JSONL gron (streaming)
for line in fx.gron_jsonl(jsonl_file):
    process(line)
```

### 2.3 Diff/Patch/Merge

```python
import fionn.ext as fx

# Compute diff (RFC 6902 JSON Patch)
patch = fx.diff(source, target)

# Apply patch
result = fx.patch(document, patch)

# Merge (RFC 7396)
merged = fx.merge(base, overlay)

# Deep merge
merged = fx.deep_merge(base, overlay)

# Three-way merge
result = fx.three_way_merge(base, ours, theirs)
```

### 2.4 CRDT Operations

```python
import fionn.ext as fx

# Create CRDT document
doc = fx.CrdtDocument(initial_data, replica_id="node-1")

# Apply operations
doc.set("users.0.name", "Alice")
doc.delete("users.0.temp_field")

# Merge with remote
conflicts = doc.merge(remote_doc)

# Get merge strategies
doc.set_strategy("counters.*", fx.MergeStrategy.ADDITIVE)
doc.set_strategy("timestamps.*", fx.MergeStrategy.MAX)

# Export state
state = doc.export_state()
delta = doc.export_delta(since_version)
```

### 2.5 Streaming: JSONL & ISONL

**Key Performance Differentiator**: ISONL streaming is fionn's unique advantage.

```
┌────────────────────────────────────────────────────────────────────────────┐
│                    STREAMING FORMAT PERFORMANCE                             │
│                    (vs sonic-rs baseline)                                   │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  JSONL (sonic-rs):    ████████████████████████████████████████  1.0x       │
│  JSONL (simd-json):   ██████████████████████████████████████░░  0.95x      │
│  ISONL (SIMD):        ███░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  0.084x     │
│                                                                            │
│  ISONL = 11.9x faster than fastest JSON parser (sonic-rs)                  │
│  355M cycles vs 4,226M cycles (same data, 1000 lines × 10K iterations)     │
│                                                                            │
│  Hardware Advantages (vs sonic-rs):                                        │
│  • IPC: 5.23 vs 3.39 (54% better CPU utilization)                          │
│  • Cache misses: 11.3K vs 77.7K (6.9x fewer)                               │
│  • Branch misses: 32.7K vs 654K (20x fewer)                                │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

#### JSONL Streaming

```python
import fionn.ext as fx

# Read JSONL with schema filtering
reader = fx.JsonlReader(
    file_path,
    schema=["id", "name", "email"],  # Only parse these fields
    batch_size=1000
)

for batch in reader:
    process(batch)

# Write JSONL
writer = fx.JsonlWriter(output_path)
for record in records:
    writer.write(record)
writer.close()

# Selective field extraction (fastest JSONL path)
for record in fx.JsonlReader(path, schema=["score"]):
    total += record["score"]  # Only 'score' field parsed
```

#### ISONL Streaming (11.9x faster than JSONL)

```python
import fionn.ext as fx

# ISONL format: self-contained schema per line
# table.users|id:int|name:string|1|Alice
# table.users|id:int|name:string|2|Bob

# Read ISONL - schema-embedded, no inference needed
reader = fx.IsonlReader(
    file_path,
    batch_size=1000
)

for batch in reader:
    process(batch)  # Returns dicts with typed values

# Selective field extraction (SIMD-accelerated)
# Only scans to field position, no full parse
for record in fx.IsonlReader(path, fields=["score"]):
    total += record["score"]

# Write ISONL with schema
writer = fx.IsonlWriter(
    output_path,
    table="users",
    schema=["id:int", "name:string", "email:string", "score:int"]
)
for record in records:
    writer.write(record)
writer.close()

# Convert JSONL to ISONL (for 11.9x speedup on repeated reads)
fx.jsonl_to_isonl(
    input_path="data.jsonl",
    output_path="data.isonl",
    table="events",
    infer_schema=True  # Or provide explicit schema
)
```

#### Stream Processing Pipeline

```python
import fionn.ext as fx

# JSONL pipeline
pipeline = fx.Pipeline()
pipeline.filter(lambda x: x["active"])
pipeline.map(lambda x: {"id": x["id"], "score": x["score"] * 2})
pipeline.process_jsonl(input_path, output_path)

# ISONL pipeline (11.9x faster)
pipeline = fx.Pipeline()
pipeline.filter(lambda x: x["score"] > 100)
pipeline.map(lambda x: {"id": x["id"], "rank": x["score"] // 10})
pipeline.process_isonl(input_path, output_path)

# Cross-format pipeline: JSONL → process → ISONL
pipeline.process(
    input_path="input.jsonl",
    input_format="jsonl",
    output_path="output.isonl",
    output_format="isonl",
    output_schema=["id:int", "rank:int"]
)
```

### 2.6 Advanced: Tape API (Zero-Copy)

```python
import fionn.ext as fx

# Parse to tape (zero-copy internal representation)
tape = fx.Tape.parse(json_bytes)

# Navigate without full materialization
value = tape.get("users.0.name")  # Returns TapeValue
all_names = tape.query("$.users[*].name")

# Schema-filtered parsing (skip unneeded fields)
tape = fx.Tape.parse(
    json_bytes,
    schema=fx.Schema(["id", "name"])  # Only materialize these
)

# Tape pooling for repeated parsing
pool = fx.TapePool(strategy="lru", max_tapes=100)
with pool.parse(json_bytes) as tape:
    # tape is reused from pool
    process(tape)
```

---

## Part 3: Implementation Strategy

### 3.1 New Crate Structure

```
fionn/
├── crates/
│   ├── fionn-core/          # Existing (unchanged)
│   ├── fionn-tape/          # Existing (unchanged)
│   ├── fionn-simd/          # Existing (unchanged)
│   ├── fionn-ops/           # Existing (unchanged)
│   ├── fionn-gron/          # Existing (unchanged)
│   ├── fionn-diff/          # Existing (unchanged)
│   ├── fionn-crdt/          # Existing (unchanged)
│   ├── fionn-stream/        # Existing (unchanged)
│   ├── fionn-pool/          # Existing (unchanged)
│   └── fionn-py/            # NEW: Python bindings
│       ├── Cargo.toml
│       ├── pyproject.toml   # maturin configuration
│       └── src/
│           ├── lib.rs       # Module entry point
│           ├── compat.rs    # orjson compatibility layer
│           ├── options.rs   # OPT_* flags
│           ├── types.rs     # Python type conversions
│           ├── exceptions.rs # Custom exceptions
│           ├── ext/
│           │   ├── mod.rs
│           │   ├── formats.rs   # Multi-format parsing
│           │   ├── gron.rs      # Gron operations
│           │   ├── diff.rs      # Diff/patch/merge
│           │   ├── crdt.rs      # CRDT operations
│           │   ├── stream.rs    # Streaming JSONL
│           │   └── tape.rs      # Advanced tape API
│           └── numpy.rs     # NumPy integration (optional)
```

### 3.2 Cargo.toml for fionn-py

```toml
[package]
name = "fionn-py"
version = "0.1.0"
edition = "2024"

[lib]
name = "fionn"
crate-type = ["cdylib"]

[dependencies]
# PyO3 for Python bindings
pyo3 = { version = "0.22", features = ["extension-module", "anyhow"] }

# Internal fionn crates (all existing, unchanged)
fionn-core = { path = "../fionn-core" }
fionn-tape = { path = "../fionn-tape" }
fionn-simd = { path = "../fionn-simd" }
fionn-ops = { path = "../fionn-ops" }
fionn-gron = { path = "../fionn-gron" }
fionn-diff = { path = "../fionn-diff" }
fionn-crdt = { path = "../fionn-crdt" }
fionn-stream = { path = "../fionn-stream" }
fionn-pool = { path = "../fionn-pool" }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Optional NumPy support
numpy = { version = "0.22", optional = true }

[features]
default = []
numpy = ["dep:numpy", "pyo3/abi3-py39"]
all-formats = ["fionn-core/all-formats"]

[build-dependencies]
pyo3-build-config = "0.22"
```

### 3.3 Key Implementation Patterns

#### Zero-Copy Input (loads)

```rust
use pyo3::pybacked::PyBackedBytes;
use fionn_tape::DsonTape;

#[pyfunction]
fn loads(data: PyBackedBytes) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        // Zero-copy: data stays in Python memory
        let tape = py.allow_threads(|| {
            // GIL released for SIMD parsing
            DsonTape::parse(std::str::from_utf8(data.as_ref())?)
        })?;

        // Convert tape to Python dict
        tape_to_pyobject(py, &tape)
    })
}
```

#### Efficient Output (dumps)

```rust
use pyo3::types::PyBytes;

#[pyfunction]
#[pyo3(signature = (obj, default=None, option=None))]
fn dumps(
    py: Python,
    obj: &Bound<'_, PyAny>,
    default: Option<&Bound<'_, PyAny>>,
    option: Option<u32>,
) -> PyResult<Py<PyBytes>> {
    let options = DumpOptions::from_flags(option.unwrap_or(0));

    // Convert Python object to Rust Value
    let value = pyobject_to_value(obj, default, &options)?;

    // Serialize with GIL released
    let bytes = py.allow_threads(|| {
        serialize_to_bytes(&value, &options)
    })?;

    // Return as Python bytes (zero-copy possible with PyBytes::new_with)
    Ok(PyBytes::new_bound(py, &bytes).into())
}
```

#### GIL Release for SIMD Operations

```rust
#[pyfunction]
fn gron(py: Python, json: PyBackedStr) -> PyResult<String> {
    py.allow_threads(|| {
        // All SIMD gron operations happen without GIL
        fionn_gron::gron(json.as_ref(), &GronOptions::default())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    })
}
```

#### Streaming with Chunked GIL Acquisition

```rust
#[pyclass]
struct JsonlReader {
    // Internal state
    inner: RefCell<JsonlReaderInner>,
}

#[pymethods]
impl JsonlReader {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>, py: Python) -> PyResult<Option<PyObject>> {
        let batch = py.allow_threads(|| {
            // Parse batch without GIL
            slf.inner.borrow_mut().next_batch()
        })?;

        match batch {
            Some(records) => {
                // Convert to Python objects with GIL
                let py_records = records_to_pylist(py, records)?;
                Ok(Some(py_records.into()))
            }
            None => Ok(None),
        }
    }
}
```

---

## Part 4: Performance Considerations

### 4.1 Zero-Copy Strategy

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         ZERO-COPY DATA FLOW                                 │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  INPUT (loads):                                                            │
│  ──────────────                                                            │
│  Python bytes ──► PyBackedBytes ──► &[u8] ──► DsonTape (SIMD parse)       │
│       │                                            │                       │
│       └── Memory stays in Python heap ─────────────┘                       │
│                                                                            │
│  OUTPUT (dumps):                                                           │
│  ───────────────                                                           │
│  Rust Vec<u8> ──► PyBytes::new_with() ──► Python bytes                    │
│       │                  │                                                 │
│       └── Single allocation, initialized in place ─┘                       │
│                                                                            │
│  TAPE API:                                                                 │
│  ─────────                                                                 │
│  Python bytes ──► Tape (keeps reference) ──► TapeValue (no copy)          │
│       │                    │                                               │
│       └── Original bytes live as long as Tape ─┘                          │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 GIL Release Points

| Operation | GIL Released | Reason |
|-----------|--------------|--------|
| `loads()` parsing | ✓ | SIMD tape construction |
| `dumps()` serialization | ✓ | JSON string building |
| `gron()` | ✓ | SIMD path traversal |
| `diff()` | ✓ | Comparison algorithms |
| `merge()` | ✓ | CRDT merge logic |
| JSONL batch read | ✓ | Batch parsing |
| Python object construction | ✗ | Requires interpreter |
| Python callback (`default`) | ✗ | User code execution |

### 4.3 Memory Management

```rust
// Frozen classes for performance-critical types
#[pyclass(frozen)]
pub struct Tape {
    // Interior mutability only where needed
    inner: Arc<TapeInner>,
}

// Pool-based allocation for repeated operations
#[pyclass]
pub struct TapePool {
    pool: Arc<Mutex<fionn_pool::ThreadLocalPool>>,
}
```

---

## Part 5: Testing Strategy

### 5.1 orjson Compatibility Tests

```python
# tests/test_orjson_compat.py

import fionn
import orjson
import pytest

class TestLoadsCompat:
    """Ensure fionn.loads() matches orjson.loads() exactly."""

    @pytest.mark.parametrize("input_data", [
        b'{"a": 1}',
        b'[1, 2, 3]',
        b'"string"',
        b'123',
        b'true',
        b'null',
        # ... extensive test cases
    ])
    def test_loads_identical(self, input_data):
        assert fionn.loads(input_data) == orjson.loads(input_data)

class TestDumpsCompat:
    """Ensure fionn.dumps() matches orjson.dumps() exactly."""

    @pytest.mark.parametrize("obj,options", [
        ({"a": 1}, None),
        ({"a": 1}, fionn.OPT_SORT_KEYS),
        ({"a": 1}, fionn.OPT_INDENT_2),
        # ... all flag combinations
    ])
    def test_dumps_identical(self, obj, options):
        assert fionn.dumps(obj, option=options) == orjson.dumps(obj, option=options)
```

### 5.2 Performance Benchmarks

```python
# benchmarks/bench_vs_orjson.py

import fionn
import orjson
import json
import pytest

@pytest.mark.benchmark(group="loads")
def test_fionn_loads(benchmark, sample_json):
    benchmark(fionn.loads, sample_json)

@pytest.mark.benchmark(group="loads")
def test_orjson_loads(benchmark, sample_json):
    benchmark(orjson.loads, sample_json)

@pytest.mark.benchmark(group="loads")
def test_json_loads(benchmark, sample_json):
    benchmark(json.loads, sample_json)
```

---

## Part 6: Risks and Mitigations

### 6.1 Risk Matrix

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| orjson API edge cases | Medium | High | Extensive compatibility test suite |
| NumPy integration complexity | Medium | Medium | Optional feature, phased rollout |
| GIL contention in callbacks | Low | Medium | Document limitations, provide async alternatives |
| Memory leaks in long-running | Low | High | Valgrind testing, Python gc integration |
| SIMD compatibility (ARM) | Low | Medium | Fallback to scalar, test on ARM CI |

### 6.2 API Stability Commitment

- **fionn.loads/dumps**: Stable, matches orjson exactly
- **fionn.ext**: Versioned, may evolve
- **fionn.ext.Tape**: Advanced API, may have breaking changes

---

## Part 7: Implementation Phases

### Phase 1: Core orjson Compatibility (2-3 weeks)

```
┌─────────────────────────────────────────────────────────────────┐
│  PHASE 1: orjson Drop-in                                        │
├─────────────────────────────────────────────────────────────────┤
│  • loads() with PyBackedBytes zero-copy                         │
│  • dumps() with all OPT_* flags                                 │
│  • Fragment support                                             │
│  • JSONEncodeError / JSONDecodeError                            │
│  • Basic type support (dict, list, str, int, float, bool, None) │
│  • datetime, date, time serialization                           │
│  • uuid.UUID serialization                                      │
│  • dataclass serialization                                      │
│  • Comprehensive orjson compatibility tests                     │
└─────────────────────────────────────────────────────────────────┘
```

### Phase 2: Extended Types (1-2 weeks)

```
┌─────────────────────────────────────────────────────────────────┐
│  PHASE 2: Extended Type Support                                 │
├─────────────────────────────────────────────────────────────────┤
│  • NumPy ndarray serialization (optional feature)               │
│  • Enum serialization                                           │
│  • TypedDict support                                            │
│  • default handler with 254-level recursion                     │
│  • Subclass handling with OPT_PASSTHROUGH_SUBCLASS              │
│  • Performance benchmarks vs orjson                             │
└─────────────────────────────────────────────────────────────────┘
```

### Phase 3: Multi-Format Support (2-3 weeks)

```
┌─────────────────────────────────────────────────────────────────┐
│  PHASE 3: fionn.ext.formats                                     │
├─────────────────────────────────────────────────────────────────┤
│  • parse_yaml(), to_yaml()                                      │
│  • parse_toml(), to_toml()                                      │
│  • parse_csv(), to_csv()                                        │
│  • parse_ison(), to_ison()                                      │
│  • parse_toon(), to_toon()                                      │
│  • Auto-detect format via parse()                               │
│  • Format conversion functions                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Phase 4: Gron & Diff (2 weeks)

```
┌─────────────────────────────────────────────────────────────────┐
│  PHASE 4: fionn.ext.gron & fionn.ext.diff                       │
├─────────────────────────────────────────────────────────────────┤
│  • gron(), ungron()                                             │
│  • gron_query() with JSONPath-like syntax                       │
│  • diff() returning RFC 6902 patches                            │
│  • patch() applying patches                                     │
│  • merge() for RFC 7396                                         │
│  • deep_merge() for nested structures                           │
│  • three_way_merge() for conflict resolution                    │
└─────────────────────────────────────────────────────────────────┘
```

### Phase 5: Streaming JSONL & ISONL (2-3 weeks)

**Critical Phase**: This is where fionn's key advantage (11.9x ISONL speedup) becomes accessible.

```
┌─────────────────────────────────────────────────────────────────┐
│  PHASE 5a: JSONL Streaming                                      │
├─────────────────────────────────────────────────────────────────┤
│  • JsonlReader with schema filtering                            │
│  • JsonlWriter                                                  │
│  • Selective field extraction                                   │
│  • Batch processing with GIL release                            │
│  • Performance: match sonic-rs                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│  PHASE 5b: ISONL Streaming (KEY DIFFERENTIATOR)                 │
├─────────────────────────────────────────────────────────────────┤
│  • IsonlReader with SIMD-accelerated parsing                    │
│  • IsonlWriter with schema generation                           │
│  • Selective field extraction (scan to position, no full parse) │
│  • Schema-embedded format support                               │
│  • jsonl_to_isonl() conversion utility                          │
│  • Performance target: 11.9x faster than JSONL                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│  PHASE 5c: Pipeline & CRDT                                      │
├─────────────────────────────────────────────────────────────────┤
│  • Pipeline for stream processing (JSONL + ISONL)               │
│  • Cross-format pipeline (JSONL → ISONL)                        │
│  • CrdtDocument class                                           │
│  • MergeStrategy enum                                           │
│  • Conflict resolution                                          │
│  • Delta export/import                                          │
└─────────────────────────────────────────────────────────────────┘
```

### Phase 6: Advanced Tape API (1-2 weeks)

```
┌─────────────────────────────────────────────────────────────────┐
│  PHASE 6: fionn.ext.tape (Advanced)                             │
├─────────────────────────────────────────────────────────────────┤
│  • Tape class with zero-copy semantics                          │
│  • TapePool for repeated parsing                                │
│  • Schema-filtered parsing                                      │
│  • TapeValue for lazy materialization                           │
│  • Query API on tapes                                           │
└─────────────────────────────────────────────────────────────────┘
```

---

## Part 8: Success Criteria

### 8.1 orjson Compatibility

- [ ] 100% API compatibility with orjson 3.x
- [ ] All 14 OPT_* flags implemented and tested
- [ ] All supported types serialize identically
- [ ] Exception types and messages match
- [ ] `import fionn; fionn.loads == orjson.loads` pattern works

### 8.2 Performance (JSON)

- [ ] `loads()`: ≥ orjson performance (within 5%)
- [ ] `dumps()`: ≥ orjson performance (within 5%)
- [ ] Memory usage: ≤ orjson (no excess allocations)
- [ ] GIL release: All CPU-intensive operations release GIL

### 8.3 Streaming Performance (KEY METRICS)

| Target | Baseline | fionn | Verification |
|--------|----------|-------|--------------|
| JSONL throughput | sonic-rs | ≥ 95% | bench_jsonl.py |
| JSONL selective | sonic-rs | ≥ 100% | bench_jsonl.py |
| **ISONL throughput** | **sonic-rs** | **≥ 10x** | **bench_isonl.py** |
| **ISONL selective** | **sonic-rs** | **≥ 11x** | **bench_isonl.py** |
| ISONL vs JSONL | JSONL | ≥ 10x | bench_isonl.py |

- [ ] `JsonlReader`: match sonic-rs throughput (within 5%)
- [ ] `IsonlReader`: **11.9x faster than sonic-rs** (key differentiator)
- [ ] `IsonlReader` selective: scan-to-position without full parse
- [ ] `jsonl_to_isonl()`: conversion utility for migration
- [ ] Batch processing: configurable batch sizes (default 1000)
- [ ] Memory: O(batch_size) not O(file_size)

### 8.4 Extended Features

- [ ] All fionn-core features accessible from Python
- [ ] All fionn-gron features accessible
- [ ] All fionn-diff features accessible
- [ ] All fionn-crdt features accessible
- [ ] JSONL streaming API with proper iterator protocol
- [ ] ISONL streaming API with proper iterator protocol
- [ ] Cross-format pipeline support (JSONL ↔ ISONL)
- [ ] Documentation with examples for all features

---

## Part 9: Open Questions for Clarification

1. **NumPy Priority**: Should NumPy support be Phase 1 (drop-in parity) or Phase 2 (extended)?

2. **Async API**: Should we provide async versions of streaming operations using `pyo3-asyncio`?

3. **Type Stubs**: Should we generate `.pyi` stub files for IDE support from the start?

4. **Package Name**:
   - `fionn` (matches Rust, clear identity)
   - `fionn-json` (emphasizes JSON focus)
   - Something else?

5. **Minimum Python Version**:
   - Python 3.9+ (wider compatibility)
   - Python 3.10+ (newer features)
   - Python 3.12+ (free-threading ready)

6. **CI/CD**:
   - Build wheels for which platforms? (manylinux, musllinux, macOS, Windows)
   - ARM64 builds?

---

## Appendix A: File Structure Detail

```
crates/fionn-py/
├── Cargo.toml
├── pyproject.toml
├── README.md
├── src/
│   ├── lib.rs                 # #[pymodule] entry point
│   ├── compat/
│   │   ├── mod.rs
│   │   ├── loads.rs           # loads() implementation
│   │   ├── dumps.rs           # dumps() implementation
│   │   ├── fragment.rs        # Fragment class
│   │   └── options.rs         # OPT_* constants
│   ├── types/
│   │   ├── mod.rs
│   │   ├── serialize.rs       # Python → JSON
│   │   ├── deserialize.rs     # JSON → Python
│   │   ├── datetime.rs        # datetime handling
│   │   ├── numpy.rs           # NumPy arrays (optional)
│   │   └── dataclass.rs       # dataclass handling
│   ├── exceptions.rs          # JSONEncodeError, JSONDecodeError
│   └── ext/
│       ├── mod.rs             # fionn.ext submodule
│       ├── formats.rs         # Multi-format parsing (YAML, TOML, CSV, ISON, TOON)
│       ├── gron.rs            # Gron operations
│       ├── diff.rs            # Diff/patch/merge
│       ├── crdt.rs            # CRDT operations
│       ├── jsonl.rs           # JSONL streaming (JsonlReader, JsonlWriter)
│       ├── isonl.rs           # ISONL streaming (IsonlReader, IsonlWriter) - KEY DIFFERENTIATOR
│       ├── pipeline.rs        # Stream processing pipelines
│       └── tape.rs            # Advanced tape API
├── tests/
│   ├── test_compat.py         # orjson compatibility tests
│   ├── test_formats.py        # Multi-format tests
│   ├── test_gron.py           # Gron tests
│   ├── test_diff.py           # Diff/patch tests
│   ├── test_crdt.py           # CRDT tests
│   ├── test_jsonl.py          # JSONL streaming tests
│   ├── test_isonl.py          # ISONL streaming tests
│   └── test_pipeline.py       # Pipeline tests
└── benchmarks/
    ├── bench_loads.py
    ├── bench_dumps.py
    ├── bench_jsonl.py         # JSONL vs sonic-rs
    ├── bench_isonl.py         # ISONL vs JSONL (11.9x target)
    └── bench_vs_orjson.py
```

---

## Appendix B: Example Usage After Implementation

```python
# =============================================================================
# 1. DROP-IN ORJSON REPLACEMENT
# =============================================================================
import fionn

# Identical to orjson - use as: import fionn as orjson
data = fionn.loads(b'{"name": "Alice", "age": 30}')
output = fionn.dumps(data, option=fionn.OPT_INDENT_2)

# =============================================================================
# 2. EXTENDED FEATURES
# =============================================================================
import fionn.ext as fx

# Multi-format
config = fx.parse_toml(toml_string)
yaml_out = fx.to_yaml(config)

# Gron
paths = fx.gron(json_string)
reconstructed = fx.ungron(paths)

# Diff/Merge
patch = fx.diff(old_doc, new_doc)
merged = fx.deep_merge(base, overlay)

# CRDT
doc = fx.CrdtDocument(data, replica_id="node-1")
doc.merge(remote_doc)

# =============================================================================
# 3. JSONL STREAMING (matches sonic-rs performance)
# =============================================================================

# Read JSONL with schema filtering
for batch in fx.JsonlReader("large.jsonl", schema=["id", "name"]):
    process(batch)

# Selective field extraction
total = sum(r["score"] for r in fx.JsonlReader("data.jsonl", schema=["score"]))

# Write JSONL
with fx.JsonlWriter("output.jsonl") as writer:
    for record in records:
        writer.write(record)

# =============================================================================
# 4. ISONL STREAMING (11.9x faster than JSONL - KEY DIFFERENTIATOR)
# =============================================================================

# ISONL format example:
# table.users|id:int|name:string|email:string|1|Alice|alice@example.com

# Read ISONL (schema-embedded, zero inference overhead)
for batch in fx.IsonlReader("large.isonl"):
    process(batch)  # Returns typed dicts

# Selective field extraction (SIMD-accelerated scan-to-position)
# Only scans to field index, no full parse - fastest possible path
total = sum(r["score"] for r in fx.IsonlReader("data.isonl", fields=["score"]))

# Write ISONL with typed schema
with fx.IsonlWriter("output.isonl", table="users",
                    schema=["id:int", "name:string", "score:int"]) as writer:
    for record in records:
        writer.write(record)

# Convert existing JSONL to ISONL for 11.9x speedup on repeated reads
fx.jsonl_to_isonl(
    "input.jsonl",
    "output.isonl",
    table="events",
    infer_schema=True
)

# =============================================================================
# 5. CROSS-FORMAT PIPELINES
# =============================================================================

# Pipeline: filter + map + write
pipeline = fx.Pipeline()
pipeline.filter(lambda x: x["active"])
pipeline.map(lambda x: {"id": x["id"], "score": x["score"] * 2})

# Process JSONL
pipeline.process_jsonl("input.jsonl", "output.jsonl")

# Process ISONL (11.9x faster)
pipeline.process_isonl("input.isonl", "output.isonl")

# Cross-format: read JSONL, write ISONL (migration path)
pipeline.process(
    input_path="legacy.jsonl",
    input_format="jsonl",
    output_path="optimized.isonl",
    output_format="isonl",
    output_schema=["id:int", "score:int"]
)

# =============================================================================
# 6. ADVANCED: ZERO-COPY TAPE API
# =============================================================================

# Parse to tape (no full materialization)
tape = fx.Tape.parse(huge_json_bytes)
value = tape.get("deeply.nested.field")  # Lazy access

# Schema-filtered tape
tape = fx.Tape.parse(json_bytes, schema=fx.Schema(["id", "name"]))
```
