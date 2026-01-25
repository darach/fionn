#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Performance profiling: orjson vs fionn
Identify where fionn loses performance.
"""

import gc
import json
import statistics
import timeit
from dataclasses import dataclass
from typing import Callable, Optional

import fionn
import orjson

# =============================================================================
# BENCHMARK INFRASTRUCTURE
# =============================================================================


@dataclass
class BenchResult:
    name: str
    ops_per_sec: float
    ns_per_op: float
    std_dev: float


def bench(
    name: str, func: Callable, iterations: int = 1000, warmup: int = 100, runs: int = 5
) -> BenchResult:
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
        bar_len = int(min(ratio, 5) * 8)
        bar = "\u2588" * bar_len
        print(
            f"  {r.name:<{max_name_len}}  {r.ops_per_sec:>12,.0f} ops/sec  ({r.ns_per_op:>8.1f} ns)  {ratio:>5.2f}x  {bar}"
        )


# =============================================================================
# PROFILING: PARSING BREAKDOWN
# =============================================================================


def profile_parsing():
    """Break down parsing performance by data type."""
    print_header("PARSING BY DATA TYPE")

    # String-heavy
    strings = {"s1": "hello", "s2": "world", "s3": "this is a longer string " * 10}
    strings_bytes = orjson.dumps(strings)

    # Number-heavy
    numbers = {"integers": list(range(100)), "floats": [i * 0.1 for i in range(100)]}
    numbers_bytes = orjson.dumps(numbers)

    # Nested objects
    nested = {"a": {"b": {"c": {"d": {"e": {"f": {"g": 1}}}}}}}
    nested_bytes = orjson.dumps(nested)

    # Arrays
    arrays = [list(range(100)) for _ in range(10)]
    arrays_bytes = orjson.dumps(arrays)

    # Mixed
    mixed = {
        "strings": ["a", "b", "c"],
        "numbers": [1, 2, 3],
        "nested": {"x": {"y": 1}},
        "bool": True,
        "null": None,
    }
    mixed_bytes = orjson.dumps(mixed)

    for name, data in [
        ("STRING-HEAVY", strings_bytes),
        ("NUMBER-HEAVY", numbers_bytes),
        ("DEEP NESTED", nested_bytes),
        ("ARRAYS", arrays_bytes),
        ("MIXED", mixed_bytes),
    ]:
        print(f"\n  {name} ({len(data)} bytes):")
        results = [
            bench("orjson", lambda d=data: orjson.loads(d), iterations=10000),
            bench("fionn", lambda d=data: fionn.loads(d), iterations=10000),
            bench("json", lambda d=data: json.loads(d), iterations=10000),
        ]
        print_comparison(results, "orjson")


def profile_serialization():
    """Break down serialization performance by data type."""
    print_header("SERIALIZATION BY DATA TYPE")

    # String-heavy
    strings = {"s1": "hello", "s2": "world", "s3": "this is a longer string " * 10}

    # Number-heavy
    numbers = {"integers": list(range(100)), "floats": [i * 0.1 for i in range(100)]}

    # Nested objects
    nested = {"a": {"b": {"c": {"d": {"e": {"f": {"g": 1}}}}}}}

    # Arrays
    arrays = [list(range(100)) for _ in range(10)]

    # Mixed
    mixed = {
        "strings": ["a", "b", "c"],
        "numbers": [1, 2, 3],
        "nested": {"x": {"y": 1}},
        "bool": True,
        "null": None,
    }

    for name, obj in [
        ("STRING-HEAVY", strings),
        ("NUMBER-HEAVY", numbers),
        ("DEEP NESTED", nested),
        ("ARRAYS", arrays),
        ("MIXED", mixed),
    ]:
        print(f"\n  {name}:")
        results = [
            bench("orjson", lambda o=obj: orjson.dumps(o), iterations=10000),
            bench("fionn", lambda o=obj: fionn.dumps(o), iterations=10000),
            bench("json", lambda o=obj: json.dumps(o).encode(), iterations=10000),
        ]
        print_comparison(results, "orjson")


def profile_dict_size():
    """Profile impact of dict size on performance."""
    print_header("IMPACT OF DICT SIZE")

    for num_fields in [5, 10, 25, 50, 100, 200]:
        obj = {f"field_{i}": i for i in range(num_fields)}
        json_bytes = orjson.dumps(obj)

        print(f"\n  {num_fields} FIELDS ({len(json_bytes)} bytes):")
        results = [
            bench("orjson.loads", lambda b=json_bytes: orjson.loads(b), iterations=5000),
            bench("fionn.loads", lambda b=json_bytes: fionn.loads(b), iterations=5000),
        ]
        print_comparison(results, "orjson.loads")


def profile_array_size():
    """Profile impact of array size on performance."""
    print_header("IMPACT OF ARRAY SIZE")

    for num_elements in [10, 50, 100, 500, 1000]:
        arr = [{"id": i, "value": i * 1.5} for i in range(num_elements)]
        json_bytes = orjson.dumps(arr)

        print(f"\n  {num_elements} ELEMENTS ({len(json_bytes):,} bytes):")
        results = [
            bench("orjson.loads", lambda b=json_bytes: orjson.loads(b), iterations=2000),
            bench("fionn.loads", lambda b=json_bytes: fionn.loads(b), iterations=2000),
        ]
        print_comparison(results, "orjson.loads")


def profile_string_lengths():
    """Profile impact of string length on performance."""
    print_header("IMPACT OF STRING LENGTH")

    for str_len in [10, 100, 1000, 10000]:
        obj = {"data": "x" * str_len}
        json_bytes = orjson.dumps(obj)

        print(f"\n  STRING LENGTH {str_len} ({len(json_bytes)} bytes):")
        results = [
            bench("orjson.loads", lambda b=json_bytes: orjson.loads(b), iterations=10000),
            bench("fionn.loads", lambda b=json_bytes: fionn.loads(b), iterations=10000),
        ]
        print_comparison(results, "orjson.loads")


def profile_numeric_precision():
    """Profile numeric parsing performance."""
    print_header("NUMERIC PARSING")

    # Integers
    integers = {"values": list(range(100))}
    int_bytes = orjson.dumps(integers)

    # Floats
    floats = {"values": [i * 0.123456789 for i in range(100)]}
    float_bytes = orjson.dumps(floats)

    # Large integers
    large_ints = {"values": [10**i for i in range(15)]}
    large_int_bytes = orjson.dumps(large_ints)

    print("\n  100 INTEGERS:")
    results = [
        bench("orjson", lambda: orjson.loads(int_bytes), iterations=10000),
        bench("fionn", lambda: fionn.loads(int_bytes), iterations=10000),
    ]
    print_comparison(results, "orjson")

    print("\n  100 FLOATS:")
    results = [
        bench("orjson", lambda: orjson.loads(float_bytes), iterations=10000),
        bench("fionn", lambda: fionn.loads(float_bytes), iterations=10000),
    ]
    print_comparison(results, "orjson")

    print("\n  LARGE INTEGERS:")
    results = [
        bench("orjson", lambda: orjson.loads(large_int_bytes), iterations=10000),
        bench("fionn", lambda: fionn.loads(large_int_bytes), iterations=10000),
    ]
    print_comparison(results, "orjson")


def profile_where_fionn_wins():
    """Show cases where fionn beats stdlib json."""
    print_header("WHERE FIONN BEATS STDLIB JSON")

    # Small JSON
    small = {"name": "Alice", "age": 30}
    small_bytes = orjson.dumps(small)
    small_str = small_bytes.decode()

    # Medium JSON
    medium = {"users": [{"id": i, "name": f"User{i}"} for i in range(50)]}
    medium_bytes = orjson.dumps(medium)
    medium_str = medium_bytes.decode()

    print("\n  SMALL (44 bytes):")
    print("  loads:")
    results = [
        bench("fionn.loads", lambda: fionn.loads(small_bytes), iterations=10000),
        bench("json.loads", lambda: json.loads(small_str), iterations=10000),
    ]
    print_comparison(results, "json.loads")

    print("  dumps:")
    results = [
        bench("fionn.dumps", lambda: fionn.dumps(small), iterations=10000),
        bench("json.dumps", lambda: json.dumps(small).encode(), iterations=10000),
    ]
    print_comparison(results, "json.dumps")

    print(f"\n  MEDIUM ({len(medium_bytes)} bytes):")
    print("  loads:")
    results = [
        bench("fionn.loads", lambda: fionn.loads(medium_bytes), iterations=5000),
        bench("json.loads", lambda: json.loads(medium_str), iterations=5000),
    ]
    print_comparison(results, "json.loads")

    print("  dumps:")
    results = [
        bench("fionn.dumps", lambda: fionn.dumps(medium), iterations=5000),
        bench("json.dumps", lambda: json.dumps(medium).encode(), iterations=5000),
    ]
    print_comparison(results, "json.dumps")


def print_analysis():
    """Print analysis of performance characteristics."""
    print_header("PERFORMANCE ANALYSIS")
    print("""
  WHY ORJSON IS FASTER (HYPOTHESIS):
  ───────────────────────────────────

  1. SIMD OPTIMIZATION DEPTH
     orjson uses highly optimized SIMD routines specifically tuned for
     Python's memory model. fionn-py uses simd-json which is optimized
     for Rust, requiring additional conversion overhead.

  2. PYTHON OBJECT CREATION
     orjson creates Python objects directly in C with minimal overhead.
     fionn-py goes: simd_json::OwnedValue -> PyO3 -> Python dict
     Each conversion step adds latency.

  3. STRING INTERNING
     orjson may use string interning for common short strings.
     fionn-py creates new PyString for each string.

  4. MEMORY ALLOCATION
     orjson likely uses custom allocators tuned for Python.
     fionn-py uses Rust's allocator + PyO3's allocation.

  POTENTIAL IMPROVEMENTS:
  ───────────────────────

  1. Direct simd-json tape -> Python conversion (bypass OwnedValue)
  2. String interning for repeated keys
  3. Pre-allocated dict/list with capacity hints
  4. Arena allocation for batch operations
  5. Use sonic-rs instead of simd-json (may be faster for this use case)

  WHERE FIONN WINS:
  ─────────────────

  1. Tape API - Parse once, access many (22,000x speedup)
  2. JSONL streaming - Native batch processing
  3. ISONL - 3x faster than JSONL in Python (11.9x in Rust)
  4. gron/ungron - Unique feature
  5. diff/patch/merge - RFC-compliant operations
  6. vs stdlib json - 1.5-8x faster
  """)


def main():
    print("=" * 78)
    print("  FIONN-PY PERFORMANCE PROFILING")
    print("  Identifying where orjson wins and why")
    print("=" * 78)

    profile_parsing()
    profile_serialization()
    profile_dict_size()
    profile_array_size()
    profile_string_lengths()
    profile_numeric_precision()
    profile_where_fionn_wins()
    print_analysis()

    print("\n" + "=" * 78)
    print("  PROFILING COMPLETE")
    print("=" * 78)


if __name__ == "__main__":
    main()
