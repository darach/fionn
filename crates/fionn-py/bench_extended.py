#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Extended Benchmarks: gron, diff, merge, selective access
Missing dimensions from the comprehensive analysis.
"""

import gc
import json
import statistics
import timeit
from dataclasses import dataclass
from typing import Callable, Optional

import fionn
import fionn.ext as fx
import orjson

# =============================================================================
# BENCHMARK INFRASTRUCTURE
# =============================================================================


@dataclass
class BenchResult:
    """Benchmark result with statistics."""

    name: str
    ops_per_sec: float
    ns_per_op: float
    std_dev: float


def bench(
    name: str, func: Callable, iterations: int = 1000, warmup: int = 100, runs: int = 5
) -> BenchResult:
    """Run benchmark with warmup, multiple runs, and statistics."""
    for _ in range(warmup):
        func()

    gc.collect()

    samples = []
    for _ in range(runs):
        gc.disable()
        t = timeit.timeit(func, number=iterations)
        gc.enable()
        samples.append(iterations / t)

    mean = statistics.mean(samples)
    std = statistics.stdev(samples) if len(samples) > 1 else 0

    return BenchResult(name=name, ops_per_sec=mean, ns_per_op=1e9 / mean, std_dev=std)


def print_header(title: str) -> None:
    print()
    print("=" * 78)
    print(f"  {title}")
    print("=" * 78)


def print_comparison(results: list[BenchResult], baseline_name: Optional[str] = None) -> None:
    if not results:
        return

    baseline = next((r for r in results if r.name == baseline_name), results[0])
    baseline_ops = baseline.ops_per_sec

    results = sorted(results, key=lambda r: -r.ops_per_sec)

    max_name_len = max(len(r.name) for r in results)

    for r in results:
        ratio = r.ops_per_sec / baseline_ops
        bar_len = int(min(ratio, 10) * 4)
        bar = "\u2588" * bar_len

        print(
            f"  {r.name:<{max_name_len}}  {r.ops_per_sec:>12,.0f} ops/sec  "
            f"({r.ns_per_op:>8.1f} ns)  {ratio:>6.2f}x  {bar}"
        )


# =============================================================================
# GRON/UNGRON BENCHMARKS
# =============================================================================


def bench_gron():
    """Benchmark gron transformation."""
    print_header("GRON TRANSFORMATION")

    # Small JSON
    small = {"name": "Alice", "age": 30, "active": True}
    small_json = json.dumps(small)

    # Medium JSON
    medium = {
        "users": [{"id": i, "name": f"User{i}", "email": f"user{i}@example.com"} for i in range(50)]
    }
    medium_json = json.dumps(medium)

    # Large JSON
    large = {
        "records": [
            {
                "id": i,
                "name": f"Record{i}",
                "data": {"x": i, "y": i * 2, "z": i * 3},
            }
            for i in range(500)
        ]
    }
    large_json = json.dumps(large)

    print(f"\n  SMALL ({len(small_json)} bytes):")
    results = [
        bench("fx.gron()", lambda: fx.gron(small_json), iterations=10000),
        bench("fx.gron_bytes()", lambda: fx.gron_bytes(small_json), iterations=10000),
    ]
    print_comparison(results)

    print(f"\n  MEDIUM ({len(medium_json)} bytes, 50 users):")
    results = [
        bench("fx.gron()", lambda: fx.gron(medium_json), iterations=2000),
        bench("fx.gron(compact)", lambda: fx.gron(medium_json, compact=True), iterations=2000),
    ]
    print_comparison(results)

    print(f"\n  LARGE ({len(large_json):,} bytes, 500 records):")
    results = [
        bench("fx.gron()", lambda: fx.gron(large_json), iterations=500),
        bench("fx.gron(compact)", lambda: fx.gron(large_json, compact=True), iterations=500),
    ]
    print_comparison(results)


def bench_ungron():
    """Benchmark ungron transformation."""
    print_header("UNGRON TRANSFORMATION")

    # Generate gron data
    small = {"name": "Alice", "age": 30}
    small_gron = fx.gron(json.dumps(small))

    medium = {"users": [{"id": i, "name": f"User{i}"} for i in range(50)]}
    medium_gron = fx.gron(json.dumps(medium))

    large = {"records": [{"id": i, "value": i * 1.5} for i in range(500)]}
    large_gron = fx.gron(json.dumps(large))

    print(f"\n  SMALL ({len(small_gron)} bytes gron):")
    results = [
        bench("fx.ungron()", lambda: fx.ungron(small_gron), iterations=10000),
    ]
    print_comparison(results)

    print(f"\n  MEDIUM ({len(medium_gron):,} bytes gron):")
    results = [
        bench("fx.ungron()", lambda: fx.ungron(medium_gron), iterations=2000),
    ]
    print_comparison(results)

    print(f"\n  LARGE ({len(large_gron):,} bytes gron):")
    results = [
        bench("fx.ungron()", lambda: fx.ungron(large_gron), iterations=500),
    ]
    print_comparison(results)


def bench_gron_roundtrip():
    """Benchmark gron + ungron roundtrip vs JSON parse + serialize."""
    print_header("GRON ROUNDTRIP VS JSON ROUNDTRIP")

    data = {"users": [{"id": i, "name": f"User{i}", "score": i * 1.5} for i in range(100)]}
    json_str = json.dumps(data)
    json_bytes = json_str.encode()

    print(f"\n  100 users ({len(json_bytes):,} bytes):")

    def gron_roundtrip():
        gron_str = fx.gron(json_str)
        return fx.ungron(gron_str)

    def json_roundtrip_fionn():
        return fionn.dumps(fionn.loads(json_bytes))

    def json_roundtrip_orjson():
        return orjson.dumps(orjson.loads(json_bytes))

    results = [
        bench("orjson roundtrip", json_roundtrip_orjson, iterations=2000),
        bench("fionn roundtrip", json_roundtrip_fionn, iterations=2000),
        bench("gron roundtrip", gron_roundtrip, iterations=500),
    ]
    print_comparison(results, "orjson roundtrip")


# =============================================================================
# DIFF/PATCH/MERGE BENCHMARKS
# =============================================================================


def bench_diff():
    """Benchmark diff computation."""
    print_header("DIFF COMPUTATION")

    # Small change
    small_source = {"name": "Alice", "age": 30}
    small_target = {"name": "Alice", "age": 31}

    # Medium change (add/remove fields)
    medium_source = {f"field_{i}": i for i in range(50)}
    medium_target = {f"field_{i}": i + 1 for i in range(25, 75)}

    # Large change (nested arrays)
    large_source = {"records": [{"id": i, "value": i} for i in range(100)]}
    large_target = {"records": [{"id": i, "value": i * 2} for i in range(100)]}

    print("\n  SMALL (single field change):")
    results = [
        bench("fx.diff()", lambda: fx.diff(small_source, small_target), iterations=10000),
        bench(
            "fx.diff_bytes()", lambda: fx.diff_bytes(small_source, small_target), iterations=10000
        ),
    ]
    print_comparison(results)

    print("\n  MEDIUM (50 field changes):")
    results = [
        bench("fx.diff()", lambda: fx.diff(medium_source, medium_target), iterations=2000),
    ]
    print_comparison(results)

    print("\n  LARGE (100 nested changes):")
    results = [
        bench("fx.diff()", lambda: fx.diff(large_source, large_target), iterations=500),
    ]
    print_comparison(results)


def bench_patch():
    """Benchmark patch application."""
    print_header("PATCH APPLICATION")

    doc = {"name": "Alice", "age": 30, "scores": [100, 95, 88]}

    # Single operation
    single_patch = [{"op": "replace", "path": "/age", "value": 31}]

    # Multiple operations
    multi_patch = [
        {"op": "replace", "path": "/age", "value": 31},
        {"op": "add", "path": "/city", "value": "NYC"},
        {"op": "remove", "path": "/scores/1"},
    ]

    print("\n  SINGLE OPERATION:")
    results = [
        bench("fx.patch()", lambda: fx.patch(doc, single_patch), iterations=10000),
    ]
    print_comparison(results)

    print("\n  MULTIPLE OPERATIONS (3 ops):")
    results = [
        bench("fx.patch()", lambda: fx.patch(doc, multi_patch), iterations=5000),
    ]
    print_comparison(results)


def bench_merge():
    """Benchmark merge operations."""
    print_header("MERGE OPERATIONS")

    base = {"name": "Alice", "age": 30, "prefs": {"theme": "dark", "lang": "en"}}
    overlay = {"age": 31, "city": "NYC", "prefs": {"theme": "light"}}

    print("\n  RFC 7396 MERGE:")
    results = [
        bench("fx.merge()", lambda: fx.merge(base, overlay), iterations=10000),
    ]
    print_comparison(results)

    print("\n  DEEP MERGE:")
    results = [
        bench("fx.deep_merge()", lambda: fx.deep_merge(base, overlay), iterations=10000),
    ]
    print_comparison(results)

    # Three-way merge
    base_3way = {"name": "Alice", "age": 30}
    ours = {"name": "Alice", "age": 31}
    theirs = {"name": "Bob", "age": 30}

    print("\n  THREE-WAY MERGE:")
    results = [
        bench(
            "fx.three_way_merge()",
            lambda: fx.three_way_merge(base_3way, ours, theirs),
            iterations=5000,
        ),
    ]
    print_comparison(results)


def bench_batch_diff():
    """Benchmark batch diff operations."""
    print_header("BATCH DIFF (AMORTIZED)")

    # Generate pairs
    pairs = [({"id": i, "value": i}, {"id": i, "value": i + 1}) for i in range(100)]

    def individual_diffs():
        return [fx.diff(s, t) for s, t in pairs]

    print("\n  100 document pairs:")
    results = [
        bench("fx.batch_diff()", lambda: fx.batch_diff(pairs), iterations=500),
        bench("individual fx.diff()", individual_diffs, iterations=200),
    ]
    print_comparison(results, "individual fx.diff()")


# =============================================================================
# SELECTIVE ACCESS (SCHEMA-FILTERED)
# =============================================================================


def bench_selective_access():
    """Benchmark selective/schema-filtered access patterns."""
    print_header("SELECTIVE ACCESS (SCHEMA-FILTERED)")

    # Large document
    doc = {
        "metadata": {
            "version": "1.0",
            "author": "system",
            "timestamp": 12345678,
        },
        "records": [
            {
                "id": i,
                "name": f"Record{i}",
                "data": {
                    "x": i * 1.1,
                    "y": i * 2.2,
                    "z": i * 3.3,
                },
                "tags": ["a", "b", "c"],
                "large_field": "x" * 100,  # Field we want to skip
            }
            for i in range(500)
        ],
    }
    json_str = json.dumps(doc)
    json_bytes = json_str.encode()

    # Parse into tape once
    tape = fx.Tape.parse(json_str)

    print(f"\n  Document: {len(json_bytes):,} bytes, 500 records")
    print(f"  Tape nodes: {tape.node_count}")

    # Access specific paths
    paths = ["metadata.version", "records[0].name", "records[499].data.x"]

    print("\n  ACCESS 3 SPECIFIC PATHS:")

    def tape_access():
        return [tape.get(p) for p in paths]

    def parse_access_fionn():
        d = fionn.loads(json_bytes)
        return [
            d["metadata"]["version"],
            d["records"][0]["name"],
            d["records"][499]["data"]["x"],
        ]

    def parse_access_orjson():
        d = orjson.loads(json_bytes)
        return [
            d["metadata"]["version"],
            d["records"][0]["name"],
            d["records"][499]["data"]["x"],
        ]

    results = [
        bench("Tape.get() x3", tape_access, iterations=5000),
        bench("fx.batch_resolve()", lambda: fx.batch_resolve(tape, paths), iterations=5000),
        bench("orjson + access", parse_access_orjson, iterations=1000),
        bench("fionn + access", parse_access_fionn, iterations=500),
    ]
    print_comparison(results, "orjson + access")

    # Schema-filtered parsing
    print("\n  SCHEMA-FILTERED TAPE PARSING:")

    schema = fx.Schema(["metadata", "records[0].name"])

    results = [
        bench("Tape.parse() full", lambda: fx.Tape.parse(json_str), iterations=500),
        bench("Tape.parse(schema)", lambda: fx.Tape.parse(json_str, schema=schema), iterations=500),
    ]
    print_comparison(results, "Tape.parse() full")


def bench_gron_query():
    """Benchmark gron query (selective gron output)."""
    print_header("GRON QUERY (SELECTIVE OUTPUT)")

    doc = {
        "users": [
            {"name": f"User{i}", "email": f"user{i}@example.com", "score": i * 10}
            for i in range(100)
        ]
    }
    json_str = json.dumps(doc)

    print(f"\n  Document: {len(json_str):,} bytes, 100 users")

    print("\n  QUERY: .users[*].name")
    results = [
        bench("fx.gron() full", lambda: fx.gron(json_str), iterations=1000),
        bench("fx.gron_query(.users)", lambda: fx.gron_query(json_str, ".users"), iterations=1000),
    ]
    print_comparison(results, "fx.gron() full")


# =============================================================================
# CRDT OPERATIONS
# =============================================================================


def bench_crdt_operations():
    """Benchmark CRDT merge operations."""
    print_header("CRDT MERGE OPERATIONS")

    # LWW Merge
    print("\n  LWW (LAST-WRITER-WINS) MERGE:")
    results = [
        bench(
            "fx.crdt_lww_merge()",
            lambda: fx.crdt_lww_merge("local", 100, "remote", 200),
            iterations=50000,
        ),
    ]
    print_comparison(results)

    # Max Merge
    print("\n  MAX MERGE:")
    results = [
        bench(
            "fx.crdt_max_merge()",
            lambda: fx.crdt_max_merge(10, 20),
            iterations=50000,
        ),
    ]
    print_comparison(results)

    # Min Merge
    print("\n  MIN MERGE:")
    results = [
        bench(
            "fx.crdt_min_merge()",
            lambda: fx.crdt_min_merge(10, 20),
            iterations=50000,
        ),
    ]
    print_comparison(results)

    # Additive Merge
    print("\n  ADDITIVE MERGE:")
    results = [
        bench(
            "fx.crdt_additive_merge()",
            lambda: fx.crdt_additive_merge(10, 20),
            iterations=50000,
        ),
    ]
    print_comparison(results)

    # CrdtDocument operations
    print("\n  CRDT DOCUMENT (set/merge):")

    def crdt_doc_ops():
        doc1 = fx.CrdtDocument({"counter": 0}, "node-1")
        doc1.set("counter", 10)
        doc2 = fx.CrdtDocument({"counter": 0}, "node-2")
        doc2.set("counter", 5)
        return doc1.merge(doc2)

    results = [
        bench("CrdtDocument set+merge", crdt_doc_ops, iterations=5000),
    ]
    print_comparison(results)

    # Batch merge
    print("\n  BATCH MERGE (100 fields):")
    local_state = {f"field_{i}": (i, 100) for i in range(100)}
    remote_updates = [(f"field_{i}", i + 1, 200) for i in range(100)]

    results = [
        bench(
            "fx.crdt_batch_merge()",
            lambda: fx.crdt_batch_merge(local_state, remote_updates),
            iterations=1000,
        ),
    ]
    print_comparison(results)


# =============================================================================
# DIFF + APPLY CYCLE (CRDT-like)
# =============================================================================


def bench_diff_apply_cycle():
    """Benchmark diff computation + patch application cycle (CRDT-like pattern)."""
    print_header("DIFF + APPLY CYCLE (CRDT PATTERN)")

    # Simulate concurrent edits
    base = {"counter": 0, "items": ["a", "b", "c"]}
    edit1 = {"counter": 1, "items": ["a", "b", "c"]}
    edit2 = {"counter": 0, "items": ["a", "b", "c", "d"]}

    def diff_and_apply():
        # Compute diff from base -> edit1
        patch1 = fx.diff(base, edit1)
        # Apply to edit2 (merge effect)
        return fx.patch(edit2, patch1)

    print("\n  CONCURRENT EDIT MERGE:")
    results = [
        bench("diff + apply", diff_and_apply, iterations=5000),
        bench("deep_merge (simpler)", lambda: fx.deep_merge(edit1, edit2), iterations=5000),
    ]
    print_comparison(results)


# =============================================================================
# SUMMARY
# =============================================================================


def print_summary():
    print_header("EXTENDED BENCHMARK SUMMARY")
    print(
        """
  \u250c\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2510
  \u2502              FIONN-PY EXTENDED FEATURES SUMMARY               \u2502
  \u251c\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2524
  \u2502                                                               \u2502
  \u2502  GRON OPERATIONS:                                             \u2502
  \u2502  \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500                                             \u2502
  \u2502  gron():     SIMD-accelerated JSON to gron transformation     \u2502
  \u2502  ungron():   Reconstruct JSON from gron format                 \u2502
  \u2502  gron_query(): Filter gron output with queries                 \u2502
  \u2502                                                               \u2502
  \u2502  DIFF/PATCH/MERGE (RFC 6902 & 7396):                          \u2502
  \u2502  \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500                          \u2502
  \u2502  diff():         Compute JSON Patch (RFC 6902)                 \u2502
  \u2502  patch():        Apply patch to document                       \u2502
  \u2502  merge():        RFC 7396 Merge Patch                          \u2502
  \u2502  deep_merge():   Recursive object merge                        \u2502
  \u2502  three_way_merge(): Git-style three-way merge                  \u2502
  \u2502  batch_diff():   Batch diff multiple pairs                     \u2502
  \u2502                                                               \u2502
  \u2502  SELECTIVE ACCESS (TAPE API):                                  \u2502
  \u2502  \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500                                  \u2502
  \u2502  Tape.get():       O(1) path access without re-parsing         \u2502
  \u2502  batch_resolve():  Multi-path access in single call            \u2502
  \u2502  Schema filter:    Parse only needed fields                    \u2502
  \u2502                                                               \u2502
  \u2502  USE CASES:                                                    \u2502
  \u2502  \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500                                                    \u2502
  \u2502  - CRDT-style document merging                                 \u2502
  \u2502  - Config file diffing                                         \u2502
  \u2502  - API response comparison                                     \u2502
  \u2502  - Selective field extraction from large documents             \u2502
  \u2502  - Gron for grep-able JSON exploration                         \u2502
  \u2502                                                               \u2502
  \u2514\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2518
  """
    )


def main():
    print("=" * 78)
    print("  FIONN-PY EXTENDED BENCHMARKS")
    print("  gron, diff, merge, selective access")
    print("=" * 78)

    bench_gron()
    bench_ungron()
    bench_gron_roundtrip()
    bench_diff()
    bench_patch()
    bench_merge()
    bench_batch_diff()
    bench_selective_access()
    bench_gron_query()
    bench_crdt_operations()
    bench_diff_apply_cycle()

    print_summary()

    print("\n" + "=" * 78)
    print("  EXTENDED BENCHMARK COMPLETE")
    print("=" * 78)


if __name__ == "__main__":
    main()
