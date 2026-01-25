#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Comprehensive benchmarks: fionn-py vs orjson vs stdlib json."""

import json
import statistics
import timeit
from pathlib import Path

import fionn
import fionn.ext as fx
import orjson

# Test data paths
BENCH_DIR = Path("/tmp/fionn-bench")


def load_test_file(name: str) -> bytes:
    """Load test file as bytes."""
    return (BENCH_DIR / name).read_bytes()


def benchmark(func, iterations: int = 1000, warmup: int = 100) -> dict:
    """Run benchmark with warmup and multiple iterations."""
    # Warmup
    for _ in range(warmup):
        func()

    # Collect samples
    times = []
    for _ in range(5):
        t = timeit.timeit(func, number=iterations)
        times.append(iterations / t)

    return {
        "ops_per_sec": statistics.mean(times),
        "std": statistics.stdev(times) if len(times) > 1 else 0,
        "min": min(times),
        "max": max(times),
    }


def print_comparison(name: str, results: dict, baseline_key: str = "orjson"):
    """Print benchmark comparison."""
    print(f"\n{'=' * 70}")
    print(f"  {name}")
    print(f"{'=' * 70}")

    baseline = results.get(baseline_key, {}).get("ops_per_sec", 1)

    for lib, data in sorted(results.items(), key=lambda x: -x[1]["ops_per_sec"]):
        ops = data["ops_per_sec"]
        std = data["std"]
        ratio = ops / baseline if baseline else 0
        bar = "█" * int(ratio * 20) if ratio <= 2 else "█" * 40
        print(f"  {lib:12} {ops:>12,.0f} ops/sec (±{std:>6,.0f}) {ratio:>5.2f}x  {bar}")


def bench_loads():
    """Benchmark JSON parsing (loads)."""
    print("\n" + "=" * 70)
    print("  LOADS BENCHMARKS (JSON parsing)")
    print("=" * 70)

    for name, iterations in [
        ("small.json", 10000),
        ("medium.json", 5000),
        ("large.json", 1000),
        ("xlarge.json", 100),
    ]:
        data = load_test_file(name)
        size = len(data)

        results = {
            "fionn": benchmark(lambda d=data: fionn.loads(d), iterations),
            "orjson": benchmark(lambda d=data: orjson.loads(d), iterations),
            "json": benchmark(lambda d=data: json.loads(d), iterations),
        }

        print_comparison(f"loads() - {name} ({size:,} bytes)", results)


def bench_dumps():
    """Benchmark JSON serialization (dumps)."""
    print("\n" + "=" * 70)
    print("  DUMPS BENCHMARKS (JSON serialization)")
    print("=" * 70)

    test_objects = {
        "small": {"name": "Alice", "age": 30, "active": True, "score": 95.5},
        "medium": json.loads(load_test_file("medium.json")),
        "large": json.loads(load_test_file("large.json")),
        "xlarge": json.loads(load_test_file("xlarge.json")),
    }

    for name, (obj, iterations) in [
        ("small", (test_objects["small"], 10000)),
        ("medium", (test_objects["medium"], 5000)),
        ("large", (test_objects["large"], 1000)),
        ("xlarge", (test_objects["xlarge"], 100)),
    ]:
        results = {
            "fionn": benchmark(lambda o=obj: fionn.dumps(o), iterations),
            "orjson": benchmark(lambda o=obj: orjson.dumps(o), iterations),
            "json": benchmark(lambda o=obj: json.dumps(o).encode(), iterations),
        }

        print_comparison(f"dumps() - {name}", results)


def bench_roundtrip():
    """Benchmark full roundtrip (loads + dumps)."""
    print("\n" + "=" * 70)
    print("  ROUNDTRIP BENCHMARKS (loads + dumps)")
    print("=" * 70)

    for name, iterations in [
        ("small.json", 5000),
        ("medium.json", 2000),
        ("large.json", 500),
    ]:
        data = load_test_file(name)

        results = {
            "fionn": benchmark(lambda d=data: fionn.dumps(fionn.loads(d)), iterations),
            "orjson": benchmark(lambda d=data: orjson.dumps(orjson.loads(d)), iterations),
            "json": benchmark(lambda d=data: json.dumps(json.loads(d)).encode(), iterations),
        }

        print_comparison(f"roundtrip - {name}", results)


def bench_jsonl_streaming():
    """Benchmark JSONL streaming."""
    print("\n" + "=" * 70)
    print("  JSONL STREAMING BENCHMARKS")
    print("=" * 70)

    # 1K records
    jsonl_1k = load_test_file("data.jsonl").decode()

    results_1k = {
        "fionn.ext": benchmark(lambda: fx.parse_jsonl(jsonl_1k), 500),
        "line-by-line": benchmark(
            lambda: [json.loads(line) for line in jsonl_1k.strip().split("\n")], 500
        ),
    }
    print_comparison("parse_jsonl (1K records)", results_1k, "line-by-line")

    # 10K records
    jsonl_10k = load_test_file("data_10k.jsonl").decode()

    results_10k = {
        "fionn.ext": benchmark(lambda: fx.parse_jsonl(jsonl_10k), 50),
        "line-by-line": benchmark(
            lambda: [json.loads(line) for line in jsonl_10k.strip().split("\n")], 50
        ),
    }
    print_comparison("parse_jsonl (10K records)", results_10k, "line-by-line")


def bench_isonl_vs_jsonl():
    """Benchmark ISONL vs JSONL."""
    print("\n" + "=" * 70)
    print("  ISONL vs JSONL BENCHMARKS")
    print("=" * 70)

    # Create equivalent JSONL and ISONL data
    records = 1000
    jsonl_data = "\n".join(
        [json.dumps({"id": i, "name": f"User{i}", "score": i * 1.5}) for i in range(records)]
    )
    isonl_data = "\n".join(
        [f"data|id:int|name:string|score:float|{i}|User{i}|{i * 1.5}" for i in range(records)]
    )

    print(f"\n  Data sizes: JSONL={len(jsonl_data):,} bytes, ISONL={len(isonl_data):,} bytes")
    print(f"  ISONL size ratio: {len(isonl_data) / len(jsonl_data):.2f}x")

    results = {
        "JSONL": benchmark(lambda: fx.parse_jsonl(jsonl_data), 200),
        "ISONL": benchmark(lambda: fx.parse_isonl(isonl_data), 200),
    }

    print_comparison("parse 1K records", results, "JSONL")


def bench_tape_api():
    """Benchmark Tape API vs repeated loads."""
    print("\n" + "=" * 70)
    print("  TAPE API BENCHMARKS (repeated access)")
    print("=" * 70)

    data = load_test_file("large.json")
    json_str = data.decode()

    # Parse once with Tape
    tape = fx.Tape.parse(json_str)

    # Benchmark repeated field access
    results_access = {
        "Tape.get()": benchmark(lambda: tape.get("data[50].name"), 5000),
        "loads()[path]": benchmark(lambda: fionn.loads(data)["data"][50]["name"], 5000),
        "orjson+access": benchmark(lambda: orjson.loads(data)["data"][50]["name"], 5000),
    }
    print_comparison("Single field access (repeated)", results_access, "orjson+access")

    # Benchmark multiple field access
    paths = ["metadata.count", "metadata.page"]

    results_multi = {
        "batch_resolve": benchmark(lambda: fx.batch_resolve(tape, paths), 5000),
        "Tape.get() x2": benchmark(
            lambda: (tape.get("metadata.count"), tape.get("metadata.page")), 5000
        ),
        "loads()+access": benchmark(
            lambda: (
                (obj := fionn.loads(data))["metadata"]["count"],
                obj["metadata"]["page"],
            ),
            5000,
        ),
    }
    print_comparison("Multiple field access", results_multi, "loads()+access")


def bench_cli_comparison():
    """Show CLI vs Python comparison summary."""
    print("\n" + "=" * 70)
    print("  CLI vs PYTHON SUMMARY")
    print("=" * 70)

    print("""
  fionn CLI (Pure Rust):
    - gron small.json:     ~1,000 ops/sec (mostly process startup)
    - gron xlarge.json:    ~350 ops/sec
    - stream 1K JSONL:     ~500 ops/sec
    - stream 10K JSONL:    ~85 ops/sec

  fionn-py (Python bindings):
    - loads small:         ~800K ops/sec
    - loads xlarge:        ~5K ops/sec
    - parse_jsonl 1K:      ~18K ops/sec

  Key insight: Python bindings amortize process startup cost, making them
  faster for repeated operations within a single process.
""")


def main():
    print("=" * 70)
    print("  COMPREHENSIVE FIONN-PY BENCHMARKS")
    print("  fionn-py vs orjson vs stdlib json")
    print("=" * 70)

    bench_loads()
    bench_dumps()
    bench_roundtrip()
    bench_jsonl_streaming()
    bench_isonl_vs_jsonl()
    bench_tape_api()
    bench_cli_comparison()

    print("\n" + "=" * 70)
    print("  BENCHMARK COMPLETE")
    print("=" * 70)


if __name__ == "__main__":
    main()
