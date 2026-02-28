# fionn-py

[![PyPI](https://img.shields.io/pypi/v/fionn.svg)](https://pypi.org/project/fionn/)
[![License](https://img.shields.io/crates/l/fionn.svg)](https://github.com/darach/fionn#license)

Python bindings for [fionn](https://crates.io/crates/fionn), a SIMD-accelerated
JSON toolkit written in Rust.

Every call crosses into native Rust code compiled with PyO3. The Python layer adds
no parsing of its own -- it hands bytes to the same AVX2 skip engine, tape
representation, CRDT processor, and transaction runtime that the Rust crates use
directly. The result is C-level throughput with a Python-native API.

## Installation

```bash
pip install fionn
```

## Quick start

fionn works as a drop-in replacement for orjson:

```python
import fionn

data = fionn.loads(b'{"name": "Alice", "age": 30}')
output = fionn.dumps(data, option=fionn.OPT_INDENT_2)
```

Extended features live under `fionn.ext`:

```python
import fionn.ext as fx
```

---

## Modules

The sections below map to the Rust crates that power each feature. All are
accessible through the single `fionn` Python package.

### SIMD skip and tape -- `fionn-simd`, `fionn-tape`, `fionn-pool`

The tape API parses JSON into a flat token sequence without building a DOM.
Fields can be accessed lazily, and tape buffers can be pooled to avoid repeated
allocation in tight loops.

```python
# Parse once, access fields without full materialisation
tape = fx.Tape.parse(huge_json_bytes)
name = tape.get("users.0.name")

# Pool tape buffers for high-throughput pipelines
pool = fx.TapePool(strategy="lru", max_tapes=100)
tape = pool.parse(json_bytes)
```

Underneath, `fionn-simd` drives skip operations at up to 8 GiB/s on x86_64
(AVX2). `fionn-pool` manages the buffer lifecycle so each parse reuses memory
rather than allocating afresh.

### Streaming -- `fionn-stream`

Process newline-delimited JSON in batches with optional schema filtering.
ISONL -- a schema-embedded format -- eliminates per-line schema inference and
reaches 11.9x the throughput of sonic-rs on repeated reads.

```python
# JSONL batch processing
for batch in fx.JsonlReader("data.jsonl", schema=["id", "name"], batch_size=1000):
    for record in batch:
        process(record)

# ISONL: convert once, read many times at 11.9x speed
fx.jsonl_to_isonl("input.jsonl", "output.isonl", table="events", infer_schema=True)

for batch in fx.IsonlReader("output.isonl"):
    for record in batch:
        process(record)
```

| Operation | Baseline | fionn | Speedup |
|-----------|----------|-------|---------|
| JSON loads | orjson | match | 1x |
| JSONL streaming | sonic-rs | match | 1x |
| **ISONL streaming** | sonic-rs | **11.9x faster** | **11.9x** |

### Gron -- `fionn-gron`

Flatten JSON into discrete `path = value` assignments that grep, awk, and sort
handle naturally. Reassemble with ungron.

```python
text = fx.gron('{"a": {"b": 1}}')
# json = {};
# json.a = {};
# json.a.b = 1;

obj = fx.ungron(text)  # back to {"a": {"b": 1}}

result = fx.gron_query('{"users": [{"name": "Alice"}]}', "users.0.name")
```

### Diff, patch, and merge -- `fionn-diff`

Compute structural diffs (RFC 6902), apply patches, and run three-way merges
with configurable conflict-resolution strategies.

```python
ops = fx.diff({"a": 1}, {"a": 2, "b": 3})
patched = fx.patch({"a": 1}, ops)

merged = fx.three_way_merge(base, ours, theirs)
```

### CRDTs -- `fionn-crdt`

Conflict-free Replicated Data Types let multiple replicas edit the same JSON
document concurrently. Replicas converge deterministically without coordination.

```python
doc = fx.CrdtDocument({"counter": 0}, replica_id="node-1")
doc.set("counter", 10)
doc.set_strategy("counter", fx.MergeStrategy.Additive)

conflicts = doc.merge(other_doc)
```

The Python `CrdtDocument` wraps the Rust `StrictDsonProcessor`, which implements
the Strict DSON model with dot-based causal contexts and delta-state
dissemination.

### Transactions -- `fionn-tx`

CRDTs guarantee convergence, but many workloads need stronger guarantees around
groups of operations. `fionn-tx` provides nine transaction protocols, each
suited to a different consistency-availability tradeoff. The Python API exposes
the full protocol set.

**One-shot convenience API:**

```python
events = fx.quick_tx(
    fx.TxMode.Ramp,
    {"user": "Alice", "balance": 1000},
)
```

**Explicit lifecycle:**

```python
runtime = fx.TxRuntime(replica_id=1)
tx = runtime.begin(fx.TxMode.Calvin)
tx.set("order_id", "ORD-001")
tx.set("total", 99.99)
bundle = runtime.commit_tx(tx)
```

**Multi-replica replication:**

```python
r1 = fx.TxRuntime(replica_id=1)
r2 = fx.TxRuntime(replica_id=2)

tx = r1.begin(fx.TxMode.Ramp)
tx.set("key", "value")
bundle = r1.commit_tx(tx)

# Deliver events to the other replica
r2.apply_events([bundle.to_event_data()])
```

#### Transaction modes

| Mode | Protocol | Use case |
|------|----------|----------|
| `Ramp` | Read-Atomic Multi-Partition | Multi-partition reads that must see all or none of a write batch |
| `Rola` | RAMP + Compare-And-Swap | Atomic swap with version-vector conflict detection |
| `Psi` | Parallel Snapshot Isolation | Concurrent writers with write-write conflict detection |
| `Tcb` | Transactional Causal Broadcast | Causal ordering across replicas -- no effect before cause |
| `Calvin` | Deterministic Execution | Agreed execution order, identical output on every node |
| `Saga` | Compensating Transactions | Multi-step pipelines with automatic rollback on failure |
| `EscrowCounters` | Bounded Distributed Counters | Local decrements within a pre-allocated budget |
| `SsiLite` | Serializable Snapshot Isolation | Full serializability via read-write anti-dependency tracking |
| `MaterializedViews` | Atomic Base + View Updates | Base document and derived views updated in one transaction |

### Format conversion -- `fionn-core`, `fionn-ops`

Convert between JSON, YAML, TOML, CSV, ISON, and TOON:

```python
yaml_str = fx.to_yaml({"key": "value"})
obj = fx.parse_toml('[server]\nport = 8080')
rows = fx.parse_csv("id,name\n1,Alice", has_header=True)
```

### Pipelines

Chain filter, map, and format-conversion stages into a streaming pipeline:

```python
pipeline = fx.Pipeline()
pipeline.filter(lambda x: x["active"])
pipeline.map(lambda x: {"id": x["id"]})
pipeline.process_isonl("input.isonl", "output.isonl")
```

---

## Rust internals

Every feature above is a thin Python wrapper around a Rust crate:

| Python API | Rust crate | What it does |
|------------|------------|--------------|
| `fionn.loads` / `fionn.dumps` | `fionn-simd`, `fionn-core` | SIMD-accelerated JSON parse and serialise |
| `fx.Tape`, `fx.TapePool` | `fionn-tape`, `fionn-pool` | Zero-copy tape representation and buffer pooling |
| `fx.JsonlReader`, `fx.IsonlReader` | `fionn-stream` | Batch streaming with schema filtering |
| `fx.gron`, `fx.ungron` | `fionn-gron` | Greppable JSON |
| `fx.diff`, `fx.patch`, `fx.merge` | `fionn-diff` | RFC 6902 diff/patch and three-way merge |
| `fx.CrdtDocument` | `fionn-crdt` | Delta-state CRDTs (Strict DSON) |
| `fx.TxRuntime`, `fx.TxMode` | `fionn-tx` | Nine transaction protocols over CRDTs |
| `fx.parse_yaml`, `fx.to_toml`, ... | `fionn-ops`, `fionn-core` | Multi-format conversion |

The top-level Rust crate is [`fionn`](https://crates.io/crates/fionn) on
crates.io. See its README for the full architecture and sub-crate map.

## Development

```bash
# Build
maturin develop

# Test
pytest

# Lint and type-check
ruff check .
mypy .
```

## License

MIT OR Apache-2.0
