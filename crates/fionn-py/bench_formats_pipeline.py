#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Benchmarks for multi-format parsing, Pipeline processing, and Tape.query().

These benchmarks test the newly implemented features:
- parse_jsonl / to_jsonl (first-class JSONL)
- parse_isonl / to_isonl (first-class ISONL - 11.9x faster)
- Pipeline.filter / Pipeline.map / Pipeline.process
- Tape.query() with wildcards and recursive descent

Run with: python bench_formats_pipeline.py
"""

import gc
import json
import statistics
import tempfile
import timeit
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

import fionn
import fionn.ext as fx

# =============================================================================
# BENCHMARK INFRASTRUCTURE
# =============================================================================


@dataclass
class BenchResult:
    """Benchmark result with statistics."""

    name: str
    ops_per_sec: float
    ns_per_op: float
    throughput_mb_s: float = 0.0
    std_dev: float = 0.0


def bench(
    name: str,
    func: Callable,
    iterations: int = 1000,
    warmup: int = 100,
    runs: int = 5,
    data_size: int = 0,
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

    throughput = 0.0
    if data_size > 0:
        throughput = (mean * data_size) / (1024 * 1024)  # MB/s

    return BenchResult(
        name=name,
        ops_per_sec=mean,
        ns_per_op=1e9 / mean,
        throughput_mb_s=throughput,
        std_dev=std,
    )


def print_header(title: str) -> None:
    print()
    print("=" * 78)
    print(f"  {title}")
    print("=" * 78)


def print_results(results: list[BenchResult], baseline_name: Optional[str] = None) -> None:
    if not results:
        return

    baseline = next((r for r in results if r.name == baseline_name), results[0])
    baseline_ops = baseline.ops_per_sec

    results = sorted(results, key=lambda r: -r.ops_per_sec)

    max_name_len = max(len(r.name) for r in results)

    for r in results:
        ratio = r.ops_per_sec / baseline_ops
        bar_len = int(min(ratio, 15) * 3)
        bar = "\u2588" * bar_len

        throughput = f"{r.throughput_mb_s:>6.1f} MB/s" if r.throughput_mb_s > 0 else ""

        print(
            f"  {r.name:<{max_name_len}}  {r.ops_per_sec:>12,.0f} ops/sec  "
            f"({r.ns_per_op:>8.1f} ns)  {ratio:>6.2f}x  {throughput}  {bar}"
        )


# =============================================================================
# TEST DATA GENERATORS
# =============================================================================


def generate_jsonl_data(num_records: int, fields: int = 5) -> str:
    """Generate JSONL test data."""
    lines = []
    for i in range(num_records):
        record = {"id": i, "name": f"User{i}", "active": i % 2 == 0}
        for j in range(fields - 3):
            record[f"field{j}"] = f"value{j}_{i}"
        lines.append(json.dumps(record))
    return "\n".join(lines)


def generate_isonl_data(num_records: int, fields: int = 5) -> str:
    """Generate ISONL test data."""
    lines = []
    # Build schema
    schema_parts = ["id:int", "name:string", "active:bool"]
    for j in range(fields - 3):
        schema_parts.append(f"field{j}:string")

    for i in range(num_records):
        parts = ["table.users"]
        parts.extend(schema_parts)
        parts.extend([str(i), f"User{i}", "true" if i % 2 == 0 else "false"])
        for j in range(fields - 3):
            parts.append(f"value{j}_{i}")
        lines.append("|".join(parts))
    return "\n".join(lines)


def generate_nested_json(depth: int = 3, breadth: int = 3) -> dict:
    """Generate nested JSON for query testing."""
    if depth == 0:
        return {"value": 42, "name": "leaf"}

    return {
        "items": [generate_nested_json(depth - 1, breadth) for _ in range(breadth)],
        "meta": {"depth": depth, "count": breadth},
    }


# =============================================================================
# JSONL vs ISONL BENCHMARKS (First-Class Formats)
# =============================================================================


def bench_streaming_formats():
    """Benchmark JSONL vs ISONL parsing (key differentiator)."""
    print_header("JSONL vs ISONL PARSING (fionn's key advantage)")

    for num_records in [100, 1000]:
        print(f"\n  Records: {num_records}")
        print("-" * 70)

        jsonl_data = generate_jsonl_data(num_records)
        isonl_data = generate_isonl_data(num_records)

        results = []

        # JSONL parsing
        results.append(
            bench(
                "fx.parse_jsonl",
                lambda: fx.parse_jsonl(jsonl_data),
                iterations=500,
                data_size=len(jsonl_data),
            )
        )

        # ISONL parsing
        results.append(
            bench(
                "fx.parse_isonl",
                lambda: fx.parse_isonl(isonl_data),
                iterations=500,
                data_size=len(isonl_data),
            )
        )

        # Standard json.loads for comparison
        def parse_jsonl_stdlib():
            return [json.loads(line) for line in jsonl_data.split("\n") if line]

        results.append(
            bench(
                "json.loads (stdlib)",
                parse_jsonl_stdlib,
                iterations=500,
                data_size=len(jsonl_data),
            )
        )

        print_results(results, "json.loads (stdlib)")


def bench_streaming_serialization():
    """Benchmark JSONL vs ISONL serialization."""
    print_header("JSONL vs ISONL SERIALIZATION")

    for num_records in [100, 1000]:
        print(f"\n  Records: {num_records}")
        print("-" * 70)

        # Generate test data
        data = [{"id": i, "name": f"User{i}", "active": i % 2 == 0} for i in range(num_records)]

        results = []

        # JSONL serialization
        results.append(
            bench(
                "fx.to_jsonl",
                lambda: fx.to_jsonl(data),
                iterations=500,
            )
        )

        # ISONL serialization
        schema = ["id:int", "name:string", "active:bool"]
        results.append(
            bench(
                "fx.to_isonl",
                lambda: fx.to_isonl(data, table="users", schema=schema),
                iterations=500,
            )
        )

        # Stdlib for comparison
        def serialize_jsonl_stdlib():
            return "\n".join(json.dumps(r) for r in data)

        results.append(
            bench(
                "json.dumps (stdlib)",
                serialize_jsonl_stdlib,
                iterations=500,
            )
        )

        print_results(results, "json.dumps (stdlib)")


# =============================================================================
# TAPE QUERY BENCHMARKS
# =============================================================================


def bench_tape_query():
    """Benchmark Tape.query() with various query types."""
    print_header("TAPE.QUERY() - JSONPath-like Access")

    # Generate nested test data
    nested = generate_nested_json(depth=3, breadth=5)
    json_str = json.dumps(nested)
    tape = fx.Tape.parse(json_str)

    print(f"\n  Document size: {len(json_str)} bytes")
    print("-" * 70)

    results = []

    # Simple field access
    results.append(
        bench(
            "query('meta.depth')",
            lambda: tape.query("meta.depth"),
            iterations=5000,
        )
    )

    # Array index access
    results.append(
        bench(
            "query('items[0].value')",
            lambda: tape.query("items[0].value"),
            iterations=5000,
        )
    )

    # Wildcard query
    results.append(
        bench(
            "query('items[*].meta')",
            lambda: tape.query("items[*].meta"),
            iterations=5000,
        )
    )

    # Recursive descent
    results.append(
        bench(
            "query('..value')",
            lambda: tape.query("..value"),
            iterations=2000,
        )
    )

    # Compare with tape.get()
    results.append(
        bench(
            "get('meta.depth')",
            lambda: tape.get("meta.depth"),
            iterations=5000,
        )
    )

    # Compare with Python dict access (after full parse)
    obj = tape.to_object()
    results.append(
        bench(
            "dict['meta']['depth']",
            lambda: obj["meta"]["depth"],
            iterations=5000,
        )
    )

    print_results(results, "dict['meta']['depth']")


# =============================================================================
# PIPELINE BENCHMARKS
# =============================================================================


def bench_pipeline():
    """Benchmark Pipeline processing."""
    print_header("PIPELINE PROCESSING")

    for num_records in [100, 1000]:
        print(f"\n  Records: {num_records}")
        print("-" * 70)

        # Create temp files
        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            # Generate input data
            data = [
                {"id": i, "name": f"User{i}", "active": i % 2 == 0, "score": i * 10}
                for i in range(num_records)
            ]
            with open(input_path, "w") as f:
                for record in data:
                    f.write(json.dumps(record) + "\n")

            results = []

            # Passthrough (no stages)
            def passthrough():
                p = fx.Pipeline()
                return p.process_jsonl(str(input_path), str(output_path))

            results.append(bench("Pipeline (passthrough)", passthrough, iterations=100))

            # Filter only
            def filter_only():
                p = fx.Pipeline()
                p.filter(lambda x: x.get("active", False))
                return p.process_jsonl(str(input_path), str(output_path))

            results.append(bench("Pipeline (filter)", filter_only, iterations=100))

            # Map only
            def map_only():
                p = fx.Pipeline()
                p.map(lambda x: {"id": x["id"], "doubled": x["score"] * 2})
                return p.process_jsonl(str(input_path), str(output_path))

            results.append(bench("Pipeline (map)", map_only, iterations=100))

            # Filter + Map
            def filter_and_map():
                p = fx.Pipeline()
                p.filter(lambda x: x.get("active", False))
                p.map(lambda x: {"id": x["id"], "doubled": x["score"] * 2})
                return p.process_jsonl(str(input_path), str(output_path))

            results.append(bench("Pipeline (filter+map)", filter_and_map, iterations=100))

            # Pure Python comparison
            def pure_python():
                with open(input_path) as fin:
                    records = [json.loads(line) for line in fin]
                filtered = [r for r in records if r.get("active", False)]
                mapped = [{"id": r["id"], "doubled": r["score"] * 2} for r in filtered]
                with open(output_path, "w") as fout:
                    for r in mapped:
                        fout.write(json.dumps(r) + "\n")
                return len(mapped)

            results.append(bench("Pure Python", pure_python, iterations=100))

            print_results(results, "Pure Python")


# =============================================================================
# FORMAT ROUNDTRIP BENCHMARKS
# =============================================================================


def bench_format_roundtrip():
    """Benchmark format roundtrip operations."""
    print_header("FORMAT ROUNDTRIP (parse -> serialize -> parse)")

    test_data = {
        "name": "Alice",
        "age": 30,
        "active": True,
        "scores": [100, 95, 88],
        "address": {"city": "NYC", "zip": "10001"},
    }

    results = []

    # YAML roundtrip
    def yaml_roundtrip():
        yaml_str = fx.to_yaml(test_data)
        return fx.parse_yaml(yaml_str)

    results.append(bench("YAML roundtrip", yaml_roundtrip, iterations=1000))

    # TOML roundtrip (remove non-TOML-compatible data)
    toml_data = {k: v for k, v in test_data.items() if k != "scores"}
    toml_data["scores_a"] = 100
    toml_data["scores_b"] = 95

    def toml_roundtrip():
        toml_str = fx.to_toml(toml_data)
        return fx.parse_toml(toml_str)

    results.append(bench("TOML roundtrip", toml_roundtrip, iterations=1000))

    # CSV roundtrip
    csv_data = [
        {"name": "Alice", "age": "30", "city": "NYC"},
        {"name": "Bob", "age": "25", "city": "LA"},
    ]

    def csv_roundtrip():
        csv_str = fx.to_csv(csv_data)
        return fx.parse_csv(csv_str)

    results.append(bench("CSV roundtrip", csv_roundtrip, iterations=1000))

    # JSON roundtrip (using fionn core)
    json_str = json.dumps(test_data)

    def json_roundtrip():
        obj = fionn.loads(json_str.encode())
        return fionn.dumps(obj)

    results.append(bench("JSON roundtrip", json_roundtrip, iterations=1000))

    # JSONL roundtrip
    jsonl_list = [test_data, test_data, test_data]

    def jsonl_roundtrip():
        jsonl_str = fx.to_jsonl(jsonl_list)
        return fx.parse_jsonl(jsonl_str)

    results.append(bench("JSONL roundtrip", jsonl_roundtrip, iterations=1000))

    print_results(results, "JSON roundtrip")


# =============================================================================
# SELECTIVE PARSING BENCHMARKS
# =============================================================================


def bench_selective_parsing():
    """Benchmark selective field parsing (schema filtering)."""
    print_header("SELECTIVE FIELD PARSING (schema filtering)")

    # Large record with many fields
    large_record = {f"field{i}": f"value{i}" for i in range(100)}
    large_record["id"] = 12345
    large_record["name"] = "Important"
    json_str = json.dumps(large_record)

    print(f"\n  Record size: {len(json_str)} bytes, {len(large_record)} fields")
    print("-" * 70)

    results = []

    # Full parse
    def full_parse():
        tape = fx.Tape.parse(json_str)
        return tape.to_object()

    results.append(bench("Full parse (all fields)", full_parse, iterations=2000))

    # Selective parse (2 fields)
    schema = fx.Schema(["id", "name"])

    def selective_parse():
        tape = fx.Tape.parse(json_str, schema=schema)
        return tape.to_object()

    results.append(bench("Selective parse (2 fields)", selective_parse, iterations=2000))

    # Direct get (no full parse)
    def direct_get():
        tape = fx.Tape.parse(json_str)
        return {"id": tape.get("id"), "name": tape.get("name")}

    results.append(bench("Direct get (2 fields)", direct_get, iterations=2000))

    # Python stdlib
    def stdlib_parse():
        obj = json.loads(json_str)
        return {"id": obj["id"], "name": obj["name"]}

    results.append(bench("json.loads + select", stdlib_parse, iterations=2000))

    print_results(results, "json.loads + select")


# =============================================================================
# MAIN
# =============================================================================


def main():
    print()
    print("╔══════════════════════════════════════════════════════════════════════════╗")
    print("║           FIONN FORMAT & PIPELINE BENCHMARKS                             ║")
    print("║           Testing: JSONL, ISONL, Pipeline, Tape.query()                  ║")
    print("╚══════════════════════════════════════════════════════════════════════════╝")

    bench_streaming_formats()
    bench_streaming_serialization()
    bench_tape_query()
    bench_pipeline()
    bench_format_roundtrip()
    bench_selective_parsing()

    print()
    print("=" * 78)
    print("  BENCHMARK COMPLETE")
    print("=" * 78)


if __name__ == "__main__":
    main()
