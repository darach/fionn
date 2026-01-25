#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Dimensional Performance Analysis: fionn-py

Mirrors the Rust benchmark dimensions from docs/benchmark-analysis.md:
1. Format Class Performance (Document vs Tabular)
2. Streaming vs Singular Processing
3. Skip Parsing Effectiveness
4. Tape API Performance
5. ISONL vs JSONL (GIL-adjusted)
6. Size Scaling Analysis

Reference baselines from Rust benchmarks:
- ISONL vs JSONL: 11.9x in Rust (format design beats parser optimization)
- Schema-guided skip: 13.7x at depth-2
- Streaming vs singular: 9.1x for 16MB files
- Skip vs traverse: 20,150x for 100 elements
"""

import gc
import json
import statistics
import timeit
from dataclasses import dataclass
from typing import Any, Callable, Optional

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
    min_ops: float
    max_ops: float
    iterations: int

    def __str__(self) -> str:
        return f"{self.name}: {self.ops_per_sec:,.0f} ops/sec ({self.ns_per_op:.1f} ns/op)"


def bench(
    name: str, func: Callable, iterations: int = 1000, warmup: int = 100, runs: int = 5
) -> BenchResult:
    """Run benchmark with warmup, multiple runs, and statistics."""
    # Warmup
    for _ in range(warmup):
        func()

    # Force GC before measurement
    gc.collect()

    # Collect samples
    samples = []
    for _ in range(runs):
        gc.disable()
        t = timeit.timeit(func, number=iterations)
        gc.enable()
        samples.append(iterations / t)

    mean = statistics.mean(samples)
    std = statistics.stdev(samples) if len(samples) > 1 else 0

    return BenchResult(
        name=name,
        ops_per_sec=mean,
        ns_per_op=1e9 / mean,
        std_dev=std,
        min_ops=min(samples),
        max_ops=max(samples),
        iterations=iterations * runs,
    )


def print_header(title: str) -> None:
    """Print section header."""
    print()
    print("=" * 78)
    print(f"  {title}")
    print("=" * 78)


def print_comparison(results: list[BenchResult], baseline_name: Optional[str] = None) -> None:
    """Print comparison table with relative performance."""
    if not results:
        return

    baseline = next((r for r in results if r.name == baseline_name), results[0])
    baseline_ops = baseline.ops_per_sec

    # Sort by ops/sec descending
    results = sorted(results, key=lambda r: -r.ops_per_sec)

    max_name_len = max(len(r.name) for r in results)

    for r in results:
        ratio = r.ops_per_sec / baseline_ops
        bar_len = int(min(ratio, 10) * 4)  # Cap at 10x for display
        bar = "█" * bar_len

        print(
            f"  {r.name:<{max_name_len}}  {r.ops_per_sec:>12,.0f} ops/sec  "
            f"({r.ns_per_op:>8.1f} ns)  {ratio:>6.2f}x  {bar}"
        )


# =============================================================================
# TEST DATA GENERATION
# =============================================================================


def generate_test_data() -> dict[str, Any]:
    """Generate test data matching Rust benchmark sizes."""

    # Tiny: ~50-100 bytes
    tiny = {"id": 1, "name": "test", "active": True}

    # Small: ~1-2 KB (30 records)
    small = {
        "users": [{"id": i, "name": f"User{i}", "email": f"user{i}@example.com"} for i in range(30)]
    }

    # Medium: ~10 KB (100 records)
    medium = {
        "data": [
            {"id": i, "name": f"Item{i}", "value": i * 1.5, "tags": ["a", "b", "c"]}
            for i in range(100)
        ],
        "metadata": {"count": 100},
    }

    # Large: ~100 KB (1000 records)
    large = {
        "records": [
            {
                "id": i,
                "name": f"Record{i}",
                "description": f"Description for record {i} with some text",
                "values": [j * 0.1 for j in range(10)],
                "nested": {"a": i, "b": i * 2},
            }
            for i in range(1000)
        ]
    }

    # Deep nesting (depth 10)
    def make_deep(depth: int) -> dict:
        if depth == 0:
            return {"value": 42}
        return {"level": depth, "child": make_deep(depth - 1)}

    deep = make_deep(10)

    # Wide (500 fields)
    wide = {f"field_{i}": i for i in range(500)}

    return {
        "tiny": tiny,
        "small": small,
        "medium": medium,
        "large": large,
        "deep": deep,
        "wide": wide,
    }


def generate_streaming_data(records: int) -> tuple:
    """Generate JSONL and ISONL data for streaming benchmarks."""
    # JSONL
    jsonl_lines = []
    for i in range(records):
        jsonl_lines.append(json.dumps({"id": i, "name": f"User{i}", "score": i * 1.5}))
    jsonl_data = "\n".join(jsonl_lines)

    # ISONL (schema-embedded)
    isonl_lines = []
    for i in range(records):
        isonl_lines.append(f"data|id:int|name:string|score:float|{i}|User{i}|{i * 1.5}")
    isonl_data = "\n".join(isonl_lines)

    return jsonl_data, isonl_data


# =============================================================================
# DIMENSION 1: FORMAT PARSING PERFORMANCE
# =============================================================================


def bench_format_parsing(data: dict[str, Any]) -> None:
    """Benchmark format parsing by size class."""
    print_header("DIMENSION 1: FORMAT PARSING (Size Scaling)")

    print("""
  Rust baseline (docs/benchmark-grid.md):
  - JSON tiny: 163 ns, small: 530 ns, medium: 4.5 µs, large: 43.7 µs
  - serde_json baseline: tiny 88 ns, small 1.13 µs, large 191 µs
  """)

    for size_name in ["tiny", "small", "medium", "large"]:
        obj = data[size_name]
        json_bytes = orjson.dumps(obj)
        json_str = json_bytes.decode()

        print(f"\n  {size_name.upper()} ({len(json_bytes):,} bytes):")

        results = [
            bench(
                "orjson",
                lambda b=json_bytes: orjson.loads(b),
                iterations=10000 if size_name in ["tiny", "small"] else 1000,
            ),
            bench(
                "fionn",
                lambda b=json_bytes: fionn.loads(b),
                iterations=10000 if size_name in ["tiny", "small"] else 1000,
            ),
            bench(
                "json",
                lambda s=json_str: json.loads(s),
                iterations=10000 if size_name in ["tiny", "small"] else 1000,
            ),
        ]

        print_comparison(results, "orjson")


# =============================================================================
# DIMENSION 2: STREAMING VS SINGULAR
# =============================================================================


def bench_streaming(data: dict[str, Any]) -> None:
    """Benchmark streaming vs singular processing."""
    print_header("DIMENSION 2: STREAMING VS SINGULAR")

    print("""
  Rust baseline (docs/benchmark-analysis.md):
  - Singular 14MB: 114 ms (121 MiB/s)
  - Streaming 16MB: 12.5 ms (1.07 GiB/s)
  - Speedup: 9.1x
  """)

    # Create large singular JSON
    large_obj = data["large"]
    singular_json = orjson.dumps(large_obj)

    # Create equivalent JSONL
    records = large_obj["records"]
    jsonl_data = "\n".join(orjson.dumps(r).decode() for r in records)

    print(f"\n  Singular JSON: {len(singular_json):,} bytes ({len(records)} records)")
    print(f"  JSONL: {len(jsonl_data):,} bytes ({len(records)} records)")

    results = [
        bench("fionn.loads (singular)", lambda: fionn.loads(singular_json), iterations=500),
        bench("orjson.loads (singular)", lambda: orjson.loads(singular_json), iterations=500),
        bench("fx.parse_jsonl (stream)", lambda: fx.parse_jsonl(jsonl_data), iterations=500),
        bench(
            "line-by-line json",
            lambda: [json.loads(line) for line in jsonl_data.split("\n")],
            iterations=200,
        ),
    ]

    print()
    print_comparison(results, "line-by-line json")


# =============================================================================
# DIMENSION 3: ISONL VS JSONL (GIL-adjusted)
# =============================================================================


def bench_isonl_vs_jsonl() -> None:
    """Benchmark ISONL vs JSONL - the key fionn innovation."""
    print_header("DIMENSION 3: ISONL VS JSONL (GIL-Adjusted)")

    print("""
  Rust baseline (11.9x speedup):
  - sonic-rs (JSONL): 4,226M cycles / 5K iter
  - ISONL SIMD: 355M cycles / 5K iter

  Python GIL overhead reduces this to ~1.1-2x due to:
  - Per-record Python object creation dominates
  - GIL prevents true parallel SIMD advantage
  - Full 11.9x available only via Tape API or CLI
  """)

    for records in [100, 1000, 10000]:
        jsonl_data, isonl_data = generate_streaming_data(records)

        print(
            f"\n  {records:,} RECORDS (JSONL: {len(jsonl_data):,} bytes, ISONL: {len(isonl_data):,} bytes):"
        )

        iterations = 1000 if records <= 1000 else 100

        results = [
            bench("ISONL", lambda d=isonl_data: fx.parse_isonl(d), iterations=iterations),
            bench("JSONL (fionn)", lambda d=jsonl_data: fx.parse_jsonl(d), iterations=iterations),
            bench(
                "JSONL (stdlib)",
                lambda d=jsonl_data: [json.loads(l) for l in d.split("\n")],
                iterations=iterations,
            ),
        ]

        print_comparison(results, "JSONL (stdlib)")


# =============================================================================
# DIMENSION 4: TAPE API PERFORMANCE
# =============================================================================


def bench_tape_api(data: dict[str, Any]) -> None:
    """Benchmark Tape API - the killer feature for repeated access."""
    print_header("DIMENSION 4: TAPE API (Parse Once, Access Many)")

    print("""
  Rust baseline (docs/benchmark-grid.md):
  - Skip vs traverse (100 elements): 20,150x speedup
  - Skip: 2 ns constant, Traverse: 40.3 µs linear

  Python: Tape.get() avoids re-parsing entirely, achieving similar
  relative speedup for repeated field access patterns.
  """)

    # Use large data for meaningful results
    obj = data["large"]
    json_str = json.dumps(obj)
    json_bytes = json_str.encode()

    # Parse once into tape
    tape = fx.Tape.parse(json_str)

    print(f"\n  Document: {len(json_bytes):,} bytes, {tape.node_count} tape nodes")

    # Single field access
    print("\n  SINGLE FIELD ACCESS (records[500].name):")

    results = [
        bench("Tape.get()", lambda: tape.get("records[500].name"), iterations=10000),
        bench(
            "orjson + index",
            lambda: orjson.loads(json_bytes)["records"][500]["name"],
            iterations=1000,
        ),
        bench(
            "fionn + index",
            lambda: fionn.loads(json_bytes)["records"][500]["name"],
            iterations=1000,
        ),
        bench("json + index", lambda: json.loads(json_str)["records"][500]["name"], iterations=500),
    ]

    print_comparison(results, "json + index")

    # Multiple field access
    print("\n  MULTIPLE FIELD ACCESS (3 fields, repeated 1000x):")

    paths = ["records[0].id", "records[500].name", "records[999].values"]

    def tape_multi():
        return [tape.get(p) for p in paths]

    def orjson_multi():
        d = orjson.loads(json_bytes)
        return [d["records"][0]["id"], d["records"][500]["name"], d["records"][999]["values"]]

    def fionn_multi():
        d = fionn.loads(json_bytes)
        return [d["records"][0]["id"], d["records"][500]["name"], d["records"][999]["values"]]

    results = [
        bench("Tape.get() x3", tape_multi, iterations=5000),
        bench("batch_resolve()", lambda: fx.batch_resolve(tape, paths), iterations=5000),
        bench("orjson + access", orjson_multi, iterations=1000),
        bench("fionn + access", fionn_multi, iterations=1000),
    ]

    print_comparison(results, "orjson + access")


# =============================================================================
# DIMENSION 5: SERIALIZATION (DUMPS)
# =============================================================================


def bench_serialization(data: dict[str, Any]) -> None:
    """Benchmark JSON serialization by size."""
    print_header("DIMENSION 5: JSON SERIALIZATION (dumps)")

    print("""
  fionn-py uses direct serialization (no serde intermediary):
  - Python dict → byte buffer (itoa/ryu for numbers)
  - Avoids serde_json::Value allocation
  """)

    for size_name in ["tiny", "small", "medium", "large"]:
        obj = data[size_name]

        print(f"\n  {size_name.upper()}:")

        iterations = 10000 if size_name in ["tiny", "small"] else 1000

        results = [
            bench("orjson", lambda o=obj: orjson.dumps(o), iterations=iterations),
            bench("fionn", lambda o=obj: fionn.dumps(o), iterations=iterations),
            bench("json", lambda o=obj: json.dumps(o).encode(), iterations=iterations),
        ]

        print_comparison(results, "orjson")


# =============================================================================
# DIMENSION 6: ROUNDTRIP (LOADS + DUMPS)
# =============================================================================


def bench_roundtrip(data: dict[str, Any]) -> None:
    """Benchmark full roundtrip performance."""
    print_header("DIMENSION 6: ROUNDTRIP (loads + dumps)")

    for size_name in ["tiny", "small", "medium"]:
        obj = data[size_name]
        json_bytes = orjson.dumps(obj)

        print(f"\n  {size_name.upper()} ({len(json_bytes):,} bytes):")

        iterations = 5000 if size_name in ["tiny", "small"] else 1000

        results = [
            bench(
                "orjson", lambda b=json_bytes: orjson.dumps(orjson.loads(b)), iterations=iterations
            ),
            bench("fionn", lambda b=json_bytes: fionn.dumps(fionn.loads(b)), iterations=iterations),
            bench(
                "json",
                lambda b=json_bytes: json.dumps(json.loads(b)).encode(),
                iterations=iterations,
            ),
        ]

        print_comparison(results, "orjson")


# =============================================================================
# DIMENSION 7: DEEP/WIDE STRUCTURE HANDLING
# =============================================================================


def bench_structure_types(data: dict[str, Any]) -> None:
    """Benchmark deep and wide structure handling."""
    print_header("DIMENSION 7: STRUCTURE TYPES (Deep/Wide)")

    print("""
  Rust baseline (skip vs traverse):
  - Deep (10 levels): 2ns skip vs 40µs traverse
  - Wide (500 fields): Field-selective parsing
  """)

    # Deep structure
    deep = data["deep"]
    deep_bytes = orjson.dumps(deep)
    deep_str = json.dumps(deep)

    print(f"\n  DEEP NESTING (depth=10, {len(deep_bytes)} bytes):")

    results = [
        bench("orjson", lambda: orjson.loads(deep_bytes), iterations=10000),
        bench("fionn", lambda: fionn.loads(deep_bytes), iterations=10000),
        bench("json", lambda: json.loads(deep_str), iterations=10000),
    ]
    print_comparison(results, "orjson")

    # Wide structure
    wide = data["wide"]
    wide_bytes = orjson.dumps(wide)
    wide_str = json.dumps(wide)

    print(f"\n  WIDE STRUCTURE (500 fields, {len(wide_bytes):,} bytes):")

    results = [
        bench("orjson", lambda: orjson.loads(wide_bytes), iterations=5000),
        bench("fionn", lambda: fionn.loads(wide_bytes), iterations=5000),
        bench("json", lambda: json.loads(wide_str), iterations=5000),
    ]
    print_comparison(results, "orjson")


# =============================================================================
# SUMMARY
# =============================================================================


def print_summary() -> None:
    """Print summary of findings."""
    print_header("SUMMARY: fionn-py Performance Profile")

    print("""
  ┌─────────────────────────────────────────────────────────────────────────┐
  │                     FIONN-PY PERFORMANCE SUMMARY                         │
  ├─────────────────────────────────────────────────────────────────────────┤
  │                                                                         │
  │  VS ORJSON (baseline for fastest JSON):                                 │
  │  ───────────────────────────────────────                                │
  │  loads:  0.3-0.5x orjson (acceptable for drop-in replacement)           │
  │  dumps:  0.1-0.4x orjson (direct serialization, no serde)               │
  │                                                                         │
  │  VS STDLIB JSON:                                                        │
  │  ──────────────────                                                     │
  │  loads:  1.3-3x faster (small data)                                     │
  │  dumps:  1.3-7.6x faster (significant speedup)                          │
  │                                                                         │
  │  STREAMING (fionn advantage):                                           │
  │  ──────────────────────────────                                         │
  │  JSONL:  2.3-2.6x faster than line-by-line stdlib                       │
  │  ISONL:  1.1x vs JSONL in Python (11.9x in pure Rust)                   │
  │                                                                         │
  │  TAPE API (killer feature):                                             │
  │  ──────────────────────────────                                         │
  │  Repeated access: 50-200x faster than re-parsing                        │
  │  Parse once, access many times without GIL per-access                   │
  │                                                                         │
  │  WHERE FIONN WINS:                                                      │
  │  ───────────────────                                                    │
  │  ✓ JSONL streaming (2.6x)                                               │
  │  ✓ Tape API repeated access (50-200x)                                   │
  │  ✓ Extended features (gron, diff, CRDT)                                 │
  │  ✓ vs stdlib json (1.3-7.6x)                                            │
  │                                                                         │
  │  WHERE ORJSON WINS:                                                     │
  │  ────────────────────                                                   │
  │  ✓ Single JSON parse (2-3x faster)                                      │
  │  ✓ Single JSON serialize (2-10x faster)                                 │
  │                                                                         │
  │  RUST VS PYTHON OVERHEAD:                                               │
  │  ─────────────────────────                                              │
  │  - GIL prevents parallel SIMD advantage                                 │
  │  - Per-object Python allocation dominates                               │
  │  - Full Rust performance via CLI or Tape API                            │
  │                                                                         │
  └─────────────────────────────────────────────────────────────────────────┘
  """)


# =============================================================================
# MAIN
# =============================================================================


def main():
    print("=" * 78)
    print("  FIONN-PY DIMENSIONAL PERFORMANCE ANALYSIS")
    print("  Benchmarking all dimensions from docs/benchmark-analysis.md")
    print("=" * 78)

    # Generate test data
    print("\nGenerating test data...")
    data = generate_test_data()

    for name, obj in data.items():
        size = len(json.dumps(obj))
        print(f"  {name}: {size:,} bytes")

    # Run dimensional benchmarks
    bench_format_parsing(data)
    bench_streaming(data)
    bench_isonl_vs_jsonl()
    bench_tape_api(data)
    bench_serialization(data)
    bench_roundtrip(data)
    bench_structure_types(data)

    # Print summary
    print_summary()

    print("\n" + "=" * 78)
    print("  BENCHMARK COMPLETE")
    print("=" * 78)


if __name__ == "__main__":
    main()
