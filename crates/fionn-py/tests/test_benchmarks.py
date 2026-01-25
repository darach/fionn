# SPDX-License-Identifier: MIT OR Apache-2.0
"""Performance benchmarks for fionn vs orjson baseline.

Run with: pytest tests/test_benchmarks.py -v --benchmark-enable
Or standalone: python tests/test_benchmarks.py

Benchmarks establish:
1. Core API (loads/dumps) performance vs orjson
2. ISONL streaming performance (11.9x target)
3. JSONL streaming performance
"""

from __future__ import annotations

import json
import tempfile
import time
from pathlib import Path

import pytest

# Constants for benchmark data sizes
SMALL_RECORD_COUNT = 100
MEDIUM_RECORD_COUNT = 1000
LARGE_RECORD_COUNT = 10000

# Sample records of varying complexity
SMALL_RECORD = {"id": 1, "name": "Alice"}
MEDIUM_RECORD = {
    "id": 1,
    "name": "Alice",
    "email": "alice@example.com",
    "age": 30,
    "active": True,
    "score": 95.5,
    "tags": ["admin", "user", "verified"],
}
LARGE_RECORD = {
    "id": 1,
    "name": "Alice Wonderland",
    "email": "alice@example.com",
    "phone": "+1-555-123-4567",
    "age": 30,
    "active": True,
    "verified": True,
    "premium": False,
    "score": 95.5,
    "balance": 1234.56,
    "created_at": "2024-01-15T10:30:00Z",
    "updated_at": "2024-12-01T14:22:33Z",
    "tags": ["admin", "user", "verified", "premium"],
    "preferences": {
        "theme": "dark",
        "language": "en",
        "notifications": True,
        "timezone": "America/New_York",
    },
    "address": {
        "street": "123 Main Street",
        "city": "New York",
        "state": "NY",
        "zip": "10001",
        "country": "USA",
    },
}


def generate_records(template: dict, count: int) -> list[dict]:
    """Generate a list of records based on a template."""
    return [{**template, "id": i} for i in range(count)]


def generate_jsonl_file(records: list[dict], path: str) -> None:
    """Write records to a JSONL file."""
    with open(path, "w") as f:
        for record in records:
            f.write(json.dumps(record) + "\n")


def generate_isonl_file(records: list[dict], path: str, table: str, schema: list[str]) -> None:
    """Write records to an ISONL file."""
    import fionn.ext as fx

    with fx.IsonlWriter(path, table=table, schema=schema) as writer:
        for record in records:
            writer.write(record)


# =============================================================================
# CORE API BENCHMARKS (fionn vs orjson)
# =============================================================================


class TestCoreApiBenchmarks:
    """Benchmark fionn core API against orjson."""

    @pytest.fixture
    def orjson_available(self) -> bool:
        """Check if orjson is available."""
        try:
            import orjson

            return True
        except ImportError:
            return False

    # -------------------------------------------------------------------------
    # loads() benchmarks
    # -------------------------------------------------------------------------

    def test_loads_small_dict(self) -> None:
        """Benchmark loads() with small dict."""
        import fionn

        data = b'{"id": 1, "name": "Alice"}'
        iterations = 10000

        start = time.perf_counter()
        for _ in range(iterations):
            fionn.loads(data)
        elapsed = time.perf_counter() - start

        ops_per_sec = iterations / elapsed
        print(f"\nfionn.loads (small dict): {ops_per_sec:,.0f} ops/sec")

    def test_loads_medium_dict(self) -> None:
        """Benchmark loads() with medium dict."""
        import fionn

        data = json.dumps(MEDIUM_RECORD).encode()
        iterations = 10000

        start = time.perf_counter()
        for _ in range(iterations):
            fionn.loads(data)
        elapsed = time.perf_counter() - start

        ops_per_sec = iterations / elapsed
        print(f"\nfionn.loads (medium dict): {ops_per_sec:,.0f} ops/sec")

    def test_loads_large_dict(self) -> None:
        """Benchmark loads() with large dict."""
        import fionn

        data = json.dumps(LARGE_RECORD).encode()
        iterations = 5000

        start = time.perf_counter()
        for _ in range(iterations):
            fionn.loads(data)
        elapsed = time.perf_counter() - start

        ops_per_sec = iterations / elapsed
        print(f"\nfionn.loads (large dict): {ops_per_sec:,.0f} ops/sec")

    def test_loads_array_of_dicts(self) -> None:
        """Benchmark loads() with array of dicts."""
        import fionn

        records = generate_records(MEDIUM_RECORD, 100)
        data = json.dumps(records).encode()
        iterations = 1000

        start = time.perf_counter()
        for _ in range(iterations):
            fionn.loads(data)
        elapsed = time.perf_counter() - start

        ops_per_sec = iterations / elapsed
        print(f"\nfionn.loads (100-element array): {ops_per_sec:,.0f} ops/sec")

    @pytest.mark.skipif(
        not pytest.importorskip("orjson", reason="orjson not installed"),
        reason="orjson not installed",
    )
    def test_loads_vs_orjson_small(self) -> None:
        """Compare loads() performance against orjson (small)."""
        import fionn
        import orjson

        data = b'{"id": 1, "name": "Alice"}'
        iterations = 50000

        # fionn
        start = time.perf_counter()
        for _ in range(iterations):
            fionn.loads(data)
        fionn_time = time.perf_counter() - start

        # orjson
        start = time.perf_counter()
        for _ in range(iterations):
            orjson.loads(data)
        orjson_time = time.perf_counter() - start

        fionn_ops = iterations / fionn_time
        orjson_ops = iterations / orjson_time
        ratio = fionn_ops / orjson_ops

        print("\n=== loads() Small Dict Comparison ===")
        print(f"fionn:  {fionn_ops:,.0f} ops/sec")
        print(f"orjson: {orjson_ops:,.0f} ops/sec")
        print(f"Ratio:  {ratio:.2f}x")

    @pytest.mark.skipif(
        not pytest.importorskip("orjson", reason="orjson not installed"),
        reason="orjson not installed",
    )
    def test_loads_vs_orjson_large(self) -> None:
        """Compare loads() performance against orjson (large)."""
        import fionn
        import orjson

        data = json.dumps(LARGE_RECORD).encode()
        iterations = 10000

        # fionn
        start = time.perf_counter()
        for _ in range(iterations):
            fionn.loads(data)
        fionn_time = time.perf_counter() - start

        # orjson
        start = time.perf_counter()
        for _ in range(iterations):
            orjson.loads(data)
        orjson_time = time.perf_counter() - start

        fionn_ops = iterations / fionn_time
        orjson_ops = iterations / orjson_time
        ratio = fionn_ops / orjson_ops

        print("\n=== loads() Large Dict Comparison ===")
        print(f"fionn:  {fionn_ops:,.0f} ops/sec")
        print(f"orjson: {orjson_ops:,.0f} ops/sec")
        print(f"Ratio:  {ratio:.2f}x")

    # -------------------------------------------------------------------------
    # dumps() benchmarks
    # -------------------------------------------------------------------------

    def test_dumps_small_dict(self) -> None:
        """Benchmark dumps() with small dict."""
        import fionn

        data = {"id": 1, "name": "Alice"}
        iterations = 10000

        start = time.perf_counter()
        for _ in range(iterations):
            fionn.dumps(data)
        elapsed = time.perf_counter() - start

        ops_per_sec = iterations / elapsed
        print(f"\nfionn.dumps (small dict): {ops_per_sec:,.0f} ops/sec")

    def test_dumps_medium_dict(self) -> None:
        """Benchmark dumps() with medium dict."""
        import fionn

        iterations = 10000

        start = time.perf_counter()
        for _ in range(iterations):
            fionn.dumps(MEDIUM_RECORD)
        elapsed = time.perf_counter() - start

        ops_per_sec = iterations / elapsed
        print(f"\nfionn.dumps (medium dict): {ops_per_sec:,.0f} ops/sec")

    def test_dumps_large_dict(self) -> None:
        """Benchmark dumps() with large dict."""
        import fionn

        iterations = 5000

        start = time.perf_counter()
        for _ in range(iterations):
            fionn.dumps(LARGE_RECORD)
        elapsed = time.perf_counter() - start

        ops_per_sec = iterations / elapsed
        print(f"\nfionn.dumps (large dict): {ops_per_sec:,.0f} ops/sec")

    @pytest.mark.skipif(
        not pytest.importorskip("orjson", reason="orjson not installed"),
        reason="orjson not installed",
    )
    def test_dumps_vs_orjson_small(self) -> None:
        """Compare dumps() performance against orjson (small)."""
        import fionn
        import orjson

        data = {"id": 1, "name": "Alice"}
        iterations = 50000

        # fionn
        start = time.perf_counter()
        for _ in range(iterations):
            fionn.dumps(data)
        fionn_time = time.perf_counter() - start

        # orjson
        start = time.perf_counter()
        for _ in range(iterations):
            orjson.dumps(data)
        orjson_time = time.perf_counter() - start

        fionn_ops = iterations / fionn_time
        orjson_ops = iterations / orjson_time
        ratio = fionn_ops / orjson_ops

        print("\n=== dumps() Small Dict Comparison ===")
        print(f"fionn:  {fionn_ops:,.0f} ops/sec")
        print(f"orjson: {orjson_ops:,.0f} ops/sec")
        print(f"Ratio:  {ratio:.2f}x")

    @pytest.mark.skipif(
        not pytest.importorskip("orjson", reason="orjson not installed"),
        reason="orjson not installed",
    )
    def test_dumps_vs_orjson_large(self) -> None:
        """Compare dumps() performance against orjson (large)."""
        import fionn
        import orjson

        iterations = 10000

        # fionn
        start = time.perf_counter()
        for _ in range(iterations):
            fionn.dumps(LARGE_RECORD)
        fionn_time = time.perf_counter() - start

        # orjson
        start = time.perf_counter()
        for _ in range(iterations):
            orjson.dumps(LARGE_RECORD)
        orjson_time = time.perf_counter() - start

        fionn_ops = iterations / fionn_time
        orjson_ops = iterations / orjson_time
        ratio = fionn_ops / orjson_ops

        print("\n=== dumps() Large Dict Comparison ===")
        print(f"fionn:  {fionn_ops:,.0f} ops/sec")
        print(f"orjson: {orjson_ops:,.0f} ops/sec")
        print(f"Ratio:  {ratio:.2f}x")


# =============================================================================
# JSONL STREAMING BENCHMARKS
# =============================================================================


class TestJsonlStreamingBenchmarks:
    """Benchmark JSONL streaming performance."""

    def test_jsonl_reader_throughput(self) -> None:
        """Benchmark JSONL reader throughput."""
        import fionn.ext as fx

        records = generate_records(MEDIUM_RECORD, MEDIUM_RECORD_COUNT)

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            generate_jsonl_file(records, f.name)
            path = f.name

        try:
            # Calculate file size
            file_size = Path(path).stat().st_size

            # Benchmark reading
            iterations = 10
            start = time.perf_counter()
            for _ in range(iterations):
                reader = fx.JsonlReader(path, batch_size=100)
                record_count = sum(len(batch) for batch in reader)
            elapsed = time.perf_counter() - start

            total_records = record_count * iterations
            records_per_sec = total_records / elapsed
            mb_per_sec = (file_size * iterations) / elapsed / (1024 * 1024)

            print("\n=== JSONL Reader Performance ===")
            print(f"Records: {records_per_sec:,.0f} records/sec")
            print(f"Throughput: {mb_per_sec:.2f} MB/sec")
        finally:
            Path(path).unlink()

    def test_jsonl_writer_throughput(self) -> None:
        """Benchmark JSONL writer throughput."""
        import fionn.ext as fx

        records = generate_records(MEDIUM_RECORD, MEDIUM_RECORD_COUNT)

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            # Benchmark writing
            iterations = 10
            start = time.perf_counter()
            for _ in range(iterations):
                with fx.JsonlWriter(path) as writer:
                    for record in records:
                        writer.write(record)
            elapsed = time.perf_counter() - start

            total_records = len(records) * iterations
            records_per_sec = total_records / elapsed

            # Calculate file size
            file_size = Path(path).stat().st_size
            mb_per_sec = (file_size * iterations) / elapsed / (1024 * 1024)

            print("\n=== JSONL Writer Performance ===")
            print(f"Records: {records_per_sec:,.0f} records/sec")
            print(f"Throughput: {mb_per_sec:.2f} MB/sec")
        finally:
            Path(path).unlink()


# =============================================================================
# ISONL STREAMING BENCHMARKS (Target: 11.9x faster than JSONL)
# =============================================================================


class TestIsonlStreamingBenchmarks:
    """Benchmark ISONL streaming performance.

    Target: 11.9x faster than JSONL based on hardware analysis.
    """

    def test_isonl_reader_throughput(self) -> None:
        """Benchmark ISONL reader throughput."""
        import fionn.ext as fx

        records = generate_records(MEDIUM_RECORD, MEDIUM_RECORD_COUNT)
        schema = [
            "id:int",
            "name:string",
            "email:string",
            "age:int",
            "active:bool",
            "score:float",
        ]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            path = f.name

        try:
            # Write ISONL file
            with fx.IsonlWriter(path, table="records", schema=schema) as writer:
                for record in records:
                    writer.write(record)

            file_size = Path(path).stat().st_size

            # Benchmark reading
            iterations = 10
            start = time.perf_counter()
            for _ in range(iterations):
                reader = fx.IsonlReader(path, batch_size=100)
                record_count = sum(len(batch) for batch in reader)
            elapsed = time.perf_counter() - start

            total_records = record_count * iterations
            records_per_sec = total_records / elapsed
            mb_per_sec = (file_size * iterations) / elapsed / (1024 * 1024)

            print("\n=== ISONL Reader Performance ===")
            print(f"Records: {records_per_sec:,.0f} records/sec")
            print(f"Throughput: {mb_per_sec:.2f} MB/sec")
        finally:
            Path(path).unlink()

    def test_isonl_vs_jsonl_comparison(self) -> None:
        """Compare ISONL vs JSONL reader performance.

        Target: ISONL should be 11.9x faster than JSONL.
        """
        import fionn.ext as fx

        records = generate_records(MEDIUM_RECORD, MEDIUM_RECORD_COUNT)
        schema = [
            "id:int",
            "name:string",
            "email:string",
            "age:int",
            "active:bool",
            "score:float",
        ]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as jf:
            jsonl_path = jf.name
        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as if_:
            isonl_path = if_.name

        try:
            # Write files
            generate_jsonl_file(records, jsonl_path)
            with fx.IsonlWriter(isonl_path, table="records", schema=schema) as writer:
                for record in records:
                    writer.write(record)

            iterations = 10

            # Benchmark JSONL
            start = time.perf_counter()
            for _ in range(iterations):
                reader = fx.JsonlReader(jsonl_path, batch_size=100)
                _ = sum(len(batch) for batch in reader)
            jsonl_time = time.perf_counter() - start

            # Benchmark ISONL
            start = time.perf_counter()
            for _ in range(iterations):
                reader = fx.IsonlReader(isonl_path, batch_size=100)
                _ = sum(len(batch) for batch in reader)
            isonl_time = time.perf_counter() - start

            speedup = jsonl_time / isonl_time

            print("\n=== ISONL vs JSONL Comparison ===")
            print(f"JSONL time:  {jsonl_time:.3f}s")
            print(f"ISONL time:  {isonl_time:.3f}s")
            print(f"Speedup:     {speedup:.1f}x")
            print("Target:      11.9x")

            # Note: In Python the speedup may be lower due to GIL
            # The 11.9x target is for pure Rust processing
        finally:
            Path(jsonl_path).unlink()
            Path(isonl_path).unlink()

    def test_isonl_selective_field_extraction(self) -> None:
        """Benchmark ISONL with selective field extraction.

        This is the key advantage of ISONL - extracting specific fields
        without parsing the entire record.
        """
        import fionn.ext as fx

        records = generate_records(LARGE_RECORD, MEDIUM_RECORD_COUNT)
        schema = [f"{k}:string" for k in LARGE_RECORD if isinstance(LARGE_RECORD[k], str)][
            :5
        ]  # Take first 5 string fields

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            path = f.name

        try:
            # Write ISONL file
            with fx.IsonlWriter(path, table="records", schema=schema) as writer:
                for record in records:
                    writer.write(record)

            iterations = 10

            # Read all fields
            start = time.perf_counter()
            for _ in range(iterations):
                reader = fx.IsonlReader(path, batch_size=100)
                _ = sum(len(batch) for batch in reader)
            all_fields_time = time.perf_counter() - start

            # Read single field
            start = time.perf_counter()
            for _ in range(iterations):
                reader = fx.IsonlReader(path, fields=["id"], batch_size=100)
                _ = sum(len(batch) for batch in reader)
            single_field_time = time.perf_counter() - start

            speedup = all_fields_time / single_field_time

            print("\n=== ISONL Selective Field Extraction ===")
            print(f"All fields time:    {all_fields_time:.3f}s")
            print(f"Single field time:  {single_field_time:.3f}s")
            print(f"Speedup:            {speedup:.1f}x")
        finally:
            Path(path).unlink()


# =============================================================================
# JSONL TO ISONL CONVERSION BENCHMARK
# =============================================================================


class TestConversionBenchmarks:
    """Benchmark JSONL to ISONL conversion."""

    def test_jsonl_to_isonl_conversion(self) -> None:
        """Benchmark JSONL to ISONL conversion throughput."""
        import fionn.ext as fx

        records = generate_records(MEDIUM_RECORD, MEDIUM_RECORD_COUNT)

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as jf:
            jsonl_path = jf.name
        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as if_:
            isonl_path = if_.name

        try:
            generate_jsonl_file(records, jsonl_path)
            jsonl_size = Path(jsonl_path).stat().st_size

            # Benchmark conversion
            iterations = 5
            start = time.perf_counter()
            for _ in range(iterations):
                fx.jsonl_to_isonl(
                    jsonl_path,
                    isonl_path,
                    table="records",
                    infer_schema=True,
                )
            elapsed = time.perf_counter() - start

            total_records = MEDIUM_RECORD_COUNT * iterations
            records_per_sec = total_records / elapsed
            mb_per_sec = (jsonl_size * iterations) / elapsed / (1024 * 1024)

            print("\n=== JSONL to ISONL Conversion ===")
            print(f"Records: {records_per_sec:,.0f} records/sec")
            print(f"Throughput: {mb_per_sec:.2f} MB/sec")
        finally:
            Path(jsonl_path).unlink()
            Path(isonl_path).unlink()


# =============================================================================
# SUMMARY REPORT
# =============================================================================


def run_all_benchmarks() -> None:
    """Run all benchmarks and print summary report."""
    print("=" * 70)
    print("FIONN PERFORMANCE BENCHMARKS")
    print("=" * 70)

    # Core API
    suite = TestCoreApiBenchmarks()
    suite.test_loads_small_dict()
    suite.test_loads_medium_dict()
    suite.test_loads_large_dict()
    suite.test_dumps_small_dict()
    suite.test_dumps_medium_dict()
    suite.test_dumps_large_dict()

    # JSONL streaming
    jsonl_suite = TestJsonlStreamingBenchmarks()
    jsonl_suite.test_jsonl_reader_throughput()
    jsonl_suite.test_jsonl_writer_throughput()

    # ISONL streaming
    isonl_suite = TestIsonlStreamingBenchmarks()
    isonl_suite.test_isonl_reader_throughput()
    isonl_suite.test_isonl_vs_jsonl_comparison()
    isonl_suite.test_isonl_selective_field_extraction()

    # Conversion
    conv_suite = TestConversionBenchmarks()
    conv_suite.test_jsonl_to_isonl_conversion()

    print("\n" + "=" * 70)
    print("BENCHMARK COMPLETE")
    print("=" * 70)


if __name__ == "__main__":
    run_all_benchmarks()
