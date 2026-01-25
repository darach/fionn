# SPDX-License-Identifier: MIT OR Apache-2.0
"""Tests for ISONL streaming (11.9x faster than JSONL)."""

from __future__ import annotations

import tempfile
from pathlib import Path

import pytest


class TestParseIsonl:
    """Test parse_isonl() function."""

    def test_parse_single_line(self) -> None:
        """Test parsing a single ISONL line."""
        import fionn.ext as fx

        # Format: table|field1:type|field2:type|...|value1|value2|...
        data = "users|id:int|name:string|1|Alice"
        result = fx.parse_isonl(data)

        assert len(result) == 1
        assert result[0]["id"] == 1
        assert result[0]["name"] == "Alice"

    def test_parse_multiple_lines(self) -> None:
        """Test parsing multiple ISONL lines."""
        import fionn.ext as fx

        data = """users|id:int|name:string|1|Alice
users|id:int|name:string|2|Bob
users|id:int|name:string|3|Charlie"""

        result = fx.parse_isonl(data)

        assert len(result) == 3
        assert result[0]["name"] == "Alice"
        assert result[1]["name"] == "Bob"
        assert result[2]["name"] == "Charlie"

    def test_parse_with_field_filter(self) -> None:
        """Test parsing with selective field extraction."""
        import fionn.ext as fx

        data = "data|id:int|name:string|score:int|1|Alice|100"
        result = fx.parse_isonl(data, fields=["score"])

        assert len(result) == 1
        assert "score" in result[0]
        assert result[0]["score"] == 100

    def test_parse_different_types(self) -> None:
        """Test parsing different field types."""
        import fionn.ext as fx

        data = "data|i:int|f:float|s:string|b:bool|42|3.14|hello|true"
        result = fx.parse_isonl(data)

        assert len(result) == 1
        assert result[0]["i"] == 42
        assert abs(result[0]["f"] - 3.14) < 0.001
        assert result[0]["s"] == "hello"
        assert result[0]["b"] is True


class TestToIsonl:
    """Test to_isonl() function."""

    def test_to_isonl_single_record(self) -> None:
        """Test converting a single record to ISONL."""
        import fionn.ext as fx

        data = [{"id": 1, "name": "Alice"}]
        result = fx.to_isonl(data, table="users", schema=["id:int", "name:string"])

        assert "users" in result
        assert "id:int" in result
        assert "name:string" in result
        assert "1" in result
        assert "Alice" in result

    def test_to_isonl_multiple_records(self) -> None:
        """Test converting multiple records to ISONL."""
        import fionn.ext as fx

        data = [
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"},
        ]
        result = fx.to_isonl(data, table="users", schema=["id:int", "name:string"])

        lines = result.strip().split("\n")
        assert len(lines) == 2


class TestIsonlReader:
    """Test IsonlReader class."""

    def test_reader_basic(self) -> None:
        """Test basic ISONL file reading."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            f.write("users|id:int|name:string|1|Alice\n")
            f.write("users|id:int|name:string|2|Bob\n")
            path = f.name

        try:
            reader = fx.IsonlReader(path, batch_size=10)
            batches = list(reader)

            assert len(batches) >= 1
            all_records = [r for batch in batches for r in batch]
            assert len(all_records) == 2
        finally:
            Path(path).unlink()

    def test_reader_with_field_filter(self) -> None:
        """Test ISONL reading with selective field extraction."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            f.write("data|id:int|name:string|score:int|1|Alice|100\n")
            f.write("data|id:int|name:string|score:int|2|Bob|200\n")
            path = f.name

        try:
            reader = fx.IsonlReader(path, fields=["score"], batch_size=10)
            batches = list(reader)

            all_records = [r for batch in batches for r in batch]
            assert all("score" in r for r in all_records)
        finally:
            Path(path).unlink()


class TestIsonlWriter:
    """Test IsonlWriter class."""

    def test_writer_basic(self) -> None:
        """Test basic ISONL file writing."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            path = f.name

        try:
            with fx.IsonlWriter(path, table="users", schema=["id:int", "name:string"]) as writer:
                writer.write({"id": 1, "name": "Alice"})
                writer.write({"id": 2, "name": "Bob"})

            # Verify file contents
            content = Path(path).read_text()
            assert "users" in content
            assert "Alice" in content
            assert "Bob" in content
        finally:
            Path(path).unlink()


class TestJsonlToIsonl:
    """Test jsonl_to_isonl() conversion."""

    def test_convert_with_inferred_schema(self) -> None:
        """Test JSONL to ISONL conversion with schema inference."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            f.write('{"id": 1, "name": "Alice"}\n')
            f.write('{"id": 2, "name": "Bob"}\n')
            input_path = f.name

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            output_path = f.name

        try:
            count = fx.jsonl_to_isonl(input_path, output_path, table="users", infer_schema=True)

            assert count == 2

            # Verify output is valid ISONL
            content = Path(output_path).read_text()
            assert "users" in content
        finally:
            Path(input_path).unlink()
            Path(output_path).unlink()

    def test_convert_with_explicit_schema(self) -> None:
        """Test JSONL to ISONL conversion with explicit schema."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            f.write('{"id": 1, "name": "Alice", "score": 100}\n')
            input_path = f.name

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            output_path = f.name

        try:
            count = fx.jsonl_to_isonl(
                input_path,
                output_path,
                table="users",
                schema=["id:int", "name:string", "score:int"],
            )

            assert count == 1
        finally:
            Path(input_path).unlink()
            Path(output_path).unlink()


class TestIsonlRoundTrip:
    """Test round-trip ISONL serialization."""

    def test_write_read_roundtrip(self) -> None:
        """Test data survives write/read round-trip."""
        import fionn.ext as fx

        original = [
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"},
        ]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".isonl", delete=False) as f:
            path = f.name

        try:
            # Write
            with fx.IsonlWriter(path, table="users", schema=["id:int", "name:string"]) as writer:
                for record in original:
                    writer.write(record)

            # Read back
            reader = fx.IsonlReader(path, batch_size=10)
            read_back = [r for batch in reader for r in batch]

            assert len(read_back) == 2
            assert read_back[0]["id"] == 1
            assert read_back[0]["name"] == "Alice"
            assert read_back[1]["id"] == 2
            assert read_back[1]["name"] == "Bob"
        finally:
            Path(path).unlink()


@pytest.mark.benchmark
class TestIsonlPerformance:
    """Performance benchmarks for ISONL (target: 11.9x faster than JSONL)."""

    @pytest.mark.skip(reason="Benchmark - run explicitly")
    def test_isonl_vs_jsonl_throughput(self) -> None:
        """Benchmark ISONL vs JSONL throughput.

        Target: ISONL should be 11.9x faster than JSONL.
        """
        # This would be implemented with actual benchmark fixtures
        pass
