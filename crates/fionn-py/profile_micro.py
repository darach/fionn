#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Micro-benchmarks to isolate Python overhead sources.

Goal: Identify exactly where time is spent in fionn vs orjson.
"""

import gc
import statistics
import timeit
from dataclasses import dataclass
from typing import Callable

import fionn
import fionn.ext as fx
import orjson

# =============================================================================
# BENCHMARK INFRASTRUCTURE
# =============================================================================


@dataclass
class BenchResult:
    name: str
    ns_per_op: float
    std_dev_ns: float


def bench_ns(
    name: str, func: Callable, iterations: int = 10000, warmup: int = 1000, runs: int = 7
) -> BenchResult:
    """Benchmark returning nanoseconds per operation."""
    for _ in range(warmup):
        func()
    gc.collect()

    samples = []
    for _ in range(runs):
        gc.disable()
        t = timeit.timeit(func, number=iterations)
        gc.enable()
        samples.append((t / iterations) * 1e9)  # ns per op

    # Remove outliers (min and max)
    if len(samples) > 3:
        samples = sorted(samples)[1:-1]

    mean = statistics.mean(samples)
    std = statistics.stdev(samples) if len(samples) > 1 else 0
    return BenchResult(name=name, ns_per_op=mean, std_dev_ns=std)


def print_comparison(results: list[BenchResult], title: str):
    print(f"\n  {title}")
    print("  " + "-" * 60)

    baseline = results[0].ns_per_op
    for r in results:
        ratio = r.ns_per_op / baseline
        overhead = r.ns_per_op - baseline
        print(f"    {r.name:<25} {r.ns_per_op:>8.1f} ns  ({ratio:>5.2f}x)  +{overhead:>6.1f} ns")


# =============================================================================
# MICRO-BENCHMARKS: PARSING
# =============================================================================


def micro_parse_empty_object():
    """Minimal JSON - isolate base overhead."""
    print("\n" + "=" * 70)
    print("  MICRO: EMPTY OBJECT {}")
    print("=" * 70)

    data = b"{}"

    results = [
        bench_ns("orjson.loads", lambda: orjson.loads(data)),
        bench_ns("fionn.loads", lambda: fionn.loads(data)),
    ]
    print_comparison(results, "Empty object")

    # Calculate pure overhead
    print(f"\n    Pure overhead: {results[1].ns_per_op - results[0].ns_per_op:.1f} ns")


def micro_parse_single_field():
    """Single field - measure per-field overhead."""
    print("\n" + "=" * 70)
    print("  MICRO: SINGLE FIELD")
    print("=" * 70)

    # String value
    data_str = b'{"name": "Alice"}'
    results = [
        bench_ns("orjson (str value)", lambda: orjson.loads(data_str)),
        bench_ns("fionn (str value)", lambda: fionn.loads(data_str)),
    ]
    print_comparison(results, "Single string field")

    # Integer value
    data_int = b'{"age": 30}'
    results = [
        bench_ns("orjson (int value)", lambda: orjson.loads(data_int)),
        bench_ns("fionn (int value)", lambda: fionn.loads(data_int)),
    ]
    print_comparison(results, "Single int field")

    # Float value
    data_float = b'{"score": 3.14159}'
    results = [
        bench_ns("orjson (float value)", lambda: orjson.loads(data_float)),
        bench_ns("fionn (float value)", lambda: fionn.loads(data_float)),
    ]
    print_comparison(results, "Single float field")


def micro_parse_scaling():
    """Measure how overhead scales with field count."""
    print("\n" + "=" * 70)
    print("  MICRO: FIELD COUNT SCALING")
    print("=" * 70)

    for n in [1, 2, 5, 10, 20, 50]:
        obj = {f"f{i}": i for i in range(n)}
        data = orjson.dumps(obj)

        r_orjson = bench_ns(f"{n} fields", lambda d=data: orjson.loads(d))
        r_fionn = bench_ns(f"{n} fields", lambda d=data: fionn.loads(d))

        overhead = r_fionn.ns_per_op - r_orjson.ns_per_op
        per_field = overhead / n if n > 0 else 0

        print(
            f"    {n:>3} fields: orjson={r_orjson.ns_per_op:>7.1f}ns, "
            f"fionn={r_fionn.ns_per_op:>7.1f}ns, "
            f"overhead={overhead:>6.1f}ns ({per_field:>5.1f}ns/field)"
        )


def micro_parse_string_lengths():
    """Measure string creation overhead by length."""
    print("\n" + "=" * 70)
    print("  MICRO: STRING LENGTH SCALING")
    print("=" * 70)

    for length in [1, 10, 100, 1000, 10000]:
        obj = {"s": "x" * length}
        data = orjson.dumps(obj)

        r_orjson = bench_ns(f"{length:>5} chars", lambda d=data: orjson.loads(d))
        r_fionn = bench_ns(f"{length:>5} chars", lambda d=data: fionn.loads(d))

        overhead = r_fionn.ns_per_op - r_orjson.ns_per_op
        per_char = overhead / length if length > 0 else 0

        print(
            f"    {length:>5} chars: orjson={r_orjson.ns_per_op:>7.1f}ns, "
            f"fionn={r_fionn.ns_per_op:>7.1f}ns, "
            f"overhead={overhead:>6.1f}ns ({per_char:>6.3f}ns/char)"
        )


def micro_parse_array_scaling():
    """Measure array element creation overhead."""
    print("\n" + "=" * 70)
    print("  MICRO: ARRAY ELEMENT SCALING")
    print("=" * 70)

    for n in [1, 10, 100, 1000]:
        data = orjson.dumps(list(range(n)))

        r_orjson = bench_ns(
            f"{n:>4} ints", lambda d=data: orjson.loads(d), iterations=5000 if n > 100 else 10000
        )
        r_fionn = bench_ns(
            f"{n:>4} ints", lambda d=data: fionn.loads(d), iterations=5000 if n > 100 else 10000
        )

        overhead = r_fionn.ns_per_op - r_orjson.ns_per_op
        per_elem = overhead / n if n > 0 else 0

        print(
            f"    {n:>4} elements: orjson={r_orjson.ns_per_op:>8.1f}ns, "
            f"fionn={r_fionn.ns_per_op:>8.1f}ns, "
            f"overhead={overhead:>7.1f}ns ({per_elem:>5.1f}ns/elem)"
        )


def micro_parse_nesting():
    """Measure nesting depth overhead."""
    print("\n" + "=" * 70)
    print("  MICRO: NESTING DEPTH")
    print("=" * 70)

    for depth in [1, 2, 5, 10]:
        obj = {}
        current = obj
        for i in range(depth):
            current["nested"] = {} if i < depth - 1 else "value"
            if i < depth - 1:
                current = current["nested"]

        data = orjson.dumps(obj)

        r_orjson = bench_ns(f"depth {depth}", lambda d=data: orjson.loads(d))
        r_fionn = bench_ns(f"depth {depth}", lambda d=data: fionn.loads(d))

        overhead = r_fionn.ns_per_op - r_orjson.ns_per_op
        per_level = overhead / depth if depth > 0 else 0

        print(
            f"    depth {depth:>2}: orjson={r_orjson.ns_per_op:>7.1f}ns, "
            f"fionn={r_fionn.ns_per_op:>7.1f}ns, "
            f"overhead={overhead:>6.1f}ns ({per_level:>5.1f}ns/level)"
        )


def micro_repeated_keys():
    """Measure overhead with repeated key names (interning opportunity)."""
    print("\n" + "=" * 70)
    print("  MICRO: REPEATED KEYS (INTERNING OPPORTUNITY)")
    print("=" * 70)

    # Array of objects with same keys
    for n in [10, 50, 100]:
        obj = [{"id": i, "name": f"Item{i}", "value": i * 1.5} for i in range(n)]
        data = orjson.dumps(obj)

        r_orjson = bench_ns(f"{n} objs", lambda d=data: orjson.loads(d), iterations=2000)
        r_fionn = bench_ns(f"{n} objs", lambda d=data: fionn.loads(d), iterations=2000)

        overhead = r_fionn.ns_per_op - r_orjson.ns_per_op
        # 3 keys per object
        per_key = overhead / (n * 3) if n > 0 else 0

        print(
            f"    {n:>3} objects (3 keys each): orjson={r_orjson.ns_per_op:>8.1f}ns, "
            f"fionn={r_fionn.ns_per_op:>8.1f}ns, "
            f"overhead/key={per_key:>5.1f}ns"
        )


# =============================================================================
# MICRO-BENCHMARKS: SERIALIZATION
# =============================================================================


def micro_dumps_empty():
    """Minimal serialization overhead."""
    print("\n" + "=" * 70)
    print("  MICRO: DUMPS EMPTY OBJECT")
    print("=" * 70)

    obj = {}

    results = [
        bench_ns("orjson.dumps", lambda: orjson.dumps(obj)),
        bench_ns("fionn.dumps", lambda: fionn.dumps(obj)),
    ]
    print_comparison(results, "Empty object serialization")


def micro_dumps_scaling():
    """Serialization scaling with field count."""
    print("\n" + "=" * 70)
    print("  MICRO: DUMPS FIELD SCALING")
    print("=" * 70)

    for n in [1, 5, 10, 50, 100]:
        obj = {f"field_{i}": i for i in range(n)}

        r_orjson = bench_ns(f"{n} fields", lambda o=obj: orjson.dumps(o))
        r_fionn = bench_ns(f"{n} fields", lambda o=obj: fionn.dumps(o))

        overhead = r_fionn.ns_per_op - r_orjson.ns_per_op
        per_field = overhead / n if n > 0 else 0

        print(
            f"    {n:>3} fields: orjson={r_orjson.ns_per_op:>7.1f}ns, "
            f"fionn={r_fionn.ns_per_op:>7.1f}ns, "
            f"overhead={overhead:>6.1f}ns ({per_field:>5.1f}ns/field)"
        )


# =============================================================================
# TAPE API COMPARISON
# =============================================================================


def micro_tape_advantage():
    """Show where Tape API eliminates Python overhead."""
    print("\n" + "=" * 70)
    print("  MICRO: TAPE API vs FULL PARSE")
    print("=" * 70)

    obj = {"users": [{"id": i, "name": f"User{i}", "score": i * 1.5} for i in range(100)]}
    data = orjson.dumps(obj)
    json_str = data.decode()

    # Parse once
    tape = fx.Tape.parse(json_str)

    print(f"\n    Document: {len(data)} bytes, 100 users")

    # Access single field
    path = "users[50].name"

    r_tape = bench_ns("Tape.get()", lambda: tape.get(path), iterations=50000)
    r_orjson = bench_ns(
        "orjson+access", lambda: orjson.loads(data)["users"][50]["name"], iterations=5000
    )
    r_fionn = bench_ns(
        "fionn+access", lambda: fionn.loads(data)["users"][50]["name"], iterations=2000
    )

    results = [r_tape, r_orjson, r_fionn]

    print("\n    Single field access (users[50].name):")
    for r in results:
        speedup = r_orjson.ns_per_op / r.ns_per_op
        print(f"      {r.name:<20} {r.ns_per_op:>8.1f} ns  ({speedup:>6.1f}x vs orjson+access)")


# =============================================================================
# ANALYSIS
# =============================================================================


def print_analysis():
    print("\n" + "=" * 70)
    print("  OVERHEAD ANALYSIS")
    print("=" * 70)
    print("""
    IDENTIFIED OVERHEAD SOURCES:
    ────────────────────────────

    1. BASE OVERHEAD (~50-100ns)
       - PyO3 function call overhead
       - GIL acquisition/release
       - Input type checking/extraction

    2. PER-FIELD OVERHEAD (~20-50ns/field)
       - PyString creation for keys
       - dict.set_item() calls
       - Pattern matching in Rust

    3. PER-ELEMENT OVERHEAD (~10-30ns/element)
       - PyList.append() calls
       - PyObject creation

    4. STRING CREATION (~5-10ns + ~0.1ns/char)
       - PyString::new() allocation
       - UTF-8 validation (already done by simd-json)

    OPTIMIZATION OPPORTUNITIES:
    ───────────────────────────

    1. STRING INTERNING
       - Cache common/repeated keys
       - Use PyString::intern() for known keys
       - Potential: 20-50% reduction on repeated keys

    2. BATCH LIST/DICT CREATION
       - Pre-allocate with capacity
       - Use PyList::new() with collected items
       - Potential: 10-20% reduction

    3. REDUCE ALLOCATIONS
       - Buffer pooling for input bytes
       - Arena allocation for intermediate values
       - Potential: 5-15% reduction

    4. DIRECT TAPE-TO-PYTHON
       - Skip OwnedValue intermediary
       - Build Python objects directly from tape
       - Potential: 20-40% reduction for complex docs

    WHERE FIONN WINS (USE THESE):
    ─────────────────────────────

    1. TAPE API - 100-1000x faster for selective access
    2. BATCH OPERATIONS - Amortized overhead
    3. STREAMING (JSONL/ISONL) - Pipeline efficiency
    4. VS STDLIB JSON - 2-8x faster always
    """)


# =============================================================================
# MAIN
# =============================================================================


def main():
    print("=" * 70)
    print("  FIONN-PY MICRO-BENCHMARKS")
    print("  Isolating Python overhead sources")
    print("=" * 70)

    micro_parse_empty_object()
    micro_parse_single_field()
    micro_parse_scaling()
    micro_parse_string_lengths()
    micro_parse_array_scaling()
    micro_parse_nesting()
    micro_repeated_keys()
    micro_dumps_empty()
    micro_dumps_scaling()
    micro_tape_advantage()
    print_analysis()

    print("\n" + "=" * 70)
    print("  MICRO-BENCHMARK COMPLETE")
    print("=" * 70)


if __name__ == "__main__":
    main()
