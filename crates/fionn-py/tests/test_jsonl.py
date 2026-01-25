# SPDX-License-Identifier: MIT OR Apache-2.0
"""Comprehensive tests for JSONL streaming."""

from __future__ import annotations

import tempfile
from pathlib import Path

import pytest


class TestParseJsonl:
    """Test parse_jsonl() function."""

    def test_parse_single_line(self) -> None:
        """Test parsing a single JSONL line."""
        import fionn.ext as fx

        data = '{"id": 1, "name": "Alice"}'
        result = fx.parse_jsonl(data)

        assert len(result) == 1
        assert result[0]["id"] == 1
        assert result[0]["name"] == "Alice"

    def test_parse_multiple_lines(self) -> None:
        """Test parsing multiple JSONL lines."""
        import fionn.ext as fx

        data = """{"id": 1, "name": "Alice"}
{"id": 2, "name": "Bob"}
{"id": 3, "name": "Charlie"}"""

        result = fx.parse_jsonl(data)

        assert len(result) == 3
        assert result[0]["name"] == "Alice"
        assert result[1]["name"] == "Bob"
        assert result[2]["name"] == "Charlie"

    def test_parse_with_schema_filter(self) -> None:
        """Test parsing with schema filtering."""
        import fionn.ext as fx

        data = '{"id": 1, "name": "Alice", "score": 100}'
        result = fx.parse_jsonl(data, schema=["id", "score"])

        assert len(result) == 1
        assert "id" in result[0]
        assert "score" in result[0]
        # name should be filtered out
        assert "name" not in result[0]

    def test_parse_empty_lines(self) -> None:
        """Test parsing handles empty lines."""
        import fionn.ext as fx

        data = """{"id": 1}

{"id": 2}

"""
        result = fx.parse_jsonl(data)

        assert len(result) == 2

    def test_parse_whitespace_lines(self) -> None:
        """Test parsing handles whitespace-only lines."""
        import fionn.ext as fx

        data = """{"id": 1}

{"id": 2}"""
        result = fx.parse_jsonl(data)

        assert len(result) == 2

    def test_parse_different_types(self) -> None:
        """Test parsing different JSON value types."""
        import fionn.ext as fx

        data = """{"int": 42, "float": 3.14, "bool": true, "null": null, "string": "hello"}"""
        result = fx.parse_jsonl(data)

        assert len(result) == 1
        assert result[0]["int"] == 42
        assert abs(result[0]["float"] - 3.14) < 0.001
        assert result[0]["bool"] is True
        assert result[0]["null"] is None
        assert result[0]["string"] == "hello"

    def test_parse_nested_objects(self) -> None:
        """Test parsing nested objects."""
        import fionn.ext as fx

        data = '{"user": {"name": "Alice", "email": "alice@example.com"}}'
        result = fx.parse_jsonl(data)

        assert len(result) == 1
        assert result[0]["user"]["name"] == "Alice"

    def test_parse_arrays(self) -> None:
        """Test parsing arrays."""
        import fionn.ext as fx

        data = '{"tags": ["python", "rust", "json"]}'
        result = fx.parse_jsonl(data)

        assert len(result) == 1
        assert result[0]["tags"] == ["python", "rust", "json"]

    def test_parse_invalid_json(self) -> None:
        """Test error on invalid JSON."""
        import fionn
        import fionn.ext as fx

        data = '{"invalid": }'
        with pytest.raises(fionn.JSONDecodeError):
            fx.parse_jsonl(data)


class TestToJsonl:
    """Test to_jsonl() function."""

    def test_to_jsonl_single_record(self) -> None:
        """Test converting a single record."""
        import fionn.ext as fx

        data = [{"id": 1, "name": "Alice"}]
        result = fx.to_jsonl(data)

        lines = result.strip().split("\n")
        assert len(lines) == 1
        assert '"id"' in lines[0] or '"id":' in lines[0]
        assert "Alice" in lines[0]

    def test_to_jsonl_multiple_records(self) -> None:
        """Test converting multiple records."""
        import fionn.ext as fx

        data = [
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"},
            {"id": 3, "name": "Charlie"},
        ]
        result = fx.to_jsonl(data)

        lines = result.strip().split("\n")
        assert len(lines) == 3

    def test_to_jsonl_empty_list(self) -> None:
        """Test converting empty list."""
        import fionn.ext as fx

        result = fx.to_jsonl([])
        assert result == ""

    def test_to_jsonl_round_trip(self) -> None:
        """Test round-trip conversion."""
        import fionn.ext as fx

        original = [
            {"id": 1, "name": "Alice", "active": True},
            {"id": 2, "name": "Bob", "active": False},
        ]
        jsonl = fx.to_jsonl(original)
        result = fx.parse_jsonl(jsonl)

        assert len(result) == len(original)
        for i, record in enumerate(result):
            assert record["id"] == original[i]["id"]
            assert record["name"] == original[i]["name"]
            assert record["active"] == original[i]["active"]


class TestJsonlReader:
    """Test JsonlReader class."""

    def test_reader_basic(self) -> None:
        """Test basic JSONL file reading."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            f.write('{"id": 1, "name": "Alice"}\n')
            f.write('{"id": 2, "name": "Bob"}\n')
            path = f.name

        try:
            reader = fx.JsonlReader(path, batch_size=10)
            batches = list(reader)

            assert len(batches) >= 1
            all_records = [r for batch in batches for r in batch]
            assert len(all_records) == 2
            assert all_records[0]["name"] == "Alice"
            assert all_records[1]["name"] == "Bob"
        finally:
            Path(path).unlink()

    def test_reader_iterator_protocol(self) -> None:
        """Test JsonlReader follows iterator protocol."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            f.write('{"id": 1}\n')
            path = f.name

        try:
            reader = fx.JsonlReader(path)

            # Should be iterable
            assert iter(reader) is reader

            # Should return batches via __next__
            batch = next(reader)
            assert isinstance(batch, list)
        finally:
            Path(path).unlink()

    def test_reader_with_schema_filter(self) -> None:
        """Test JSONL reading with schema filtering."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            f.write('{"id": 1, "name": "Alice", "score": 100, "extra": "data"}\n')
            f.write('{"id": 2, "name": "Bob", "score": 200, "extra": "more"}\n')
            path = f.name

        try:
            reader = fx.JsonlReader(path, schema=["id", "score"], batch_size=10)
            batches = list(reader)

            all_records = [r for batch in batches for r in batch]
            assert len(all_records) == 2

            # Only id and score should be present
            for record in all_records:
                assert "id" in record
                assert "score" in record
                assert "name" not in record
                assert "extra" not in record
        finally:
            Path(path).unlink()

    def test_reader_batch_size(self) -> None:
        """Test batch size configuration."""
        import fionn.ext as fx

        # Create file with 100 records
        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            for i in range(100):
                f.write(f'{{"id": {i}}}\n')
            path = f.name

        try:
            # Batch size of 25 should give us 4 batches
            reader = fx.JsonlReader(path, batch_size=25)
            batches = list(reader)

            assert len(batches) == 4
            for batch in batches:
                assert len(batch) == 25
        finally:
            Path(path).unlink()

    def test_reader_uneven_batch(self) -> None:
        """Test handling of partial last batch."""
        import fionn.ext as fx

        # Create file with 7 records
        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            for i in range(7):
                f.write(f'{{"id": {i}}}\n')
            path = f.name

        try:
            reader = fx.JsonlReader(path, batch_size=3)
            batches = list(reader)

            # Should get 3 batches: 3, 3, 1
            assert len(batches) == 3
            assert len(batches[0]) == 3
            assert len(batches[1]) == 3
            assert len(batches[2]) == 1
        finally:
            Path(path).unlink()

    def test_reader_empty_file(self) -> None:
        """Test reading empty file."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            reader = fx.JsonlReader(path)
            batches = list(reader)
            assert len(batches) == 0
        finally:
            Path(path).unlink()

    def test_reader_file_not_found(self) -> None:
        """Test error on non-existent file."""
        import fionn.ext as fx

        with pytest.raises(IOError):
            fx.JsonlReader("/nonexistent/path/file.jsonl")

    def test_reader_invalid_json(self) -> None:
        """Test error on invalid JSON in file."""
        import fionn
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            f.write('{"valid": 1}\n')
            f.write("not valid json\n")
            path = f.name

        try:
            reader = fx.JsonlReader(path, batch_size=10)
            with pytest.raises(fionn.JSONDecodeError):
                list(reader)
        finally:
            Path(path).unlink()


class TestJsonlWriter:
    """Test JsonlWriter class."""

    def test_writer_basic(self) -> None:
        """Test basic JSONL file writing."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            with fx.JsonlWriter(path) as writer:
                writer.write({"id": 1, "name": "Alice"})
                writer.write({"id": 2, "name": "Bob"})

            # Verify file contents
            content = Path(path).read_text()
            lines = content.strip().split("\n")
            assert len(lines) == 2
            assert "Alice" in lines[0]
            assert "Bob" in lines[1]
        finally:
            Path(path).unlink()

    def test_writer_context_manager(self) -> None:
        """Test JsonlWriter as context manager."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            with fx.JsonlWriter(path) as writer:
                writer.write({"test": True})

            # File should be properly closed and flushed
            content = Path(path).read_text()
            assert "test" in content
        finally:
            Path(path).unlink()

    def test_writer_close(self) -> None:
        """Test explicit close."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            writer = fx.JsonlWriter(path)
            writer.write({"id": 1})
            writer.close()

            content = Path(path).read_text()
            assert '"id"' in content or '"id":' in content
        finally:
            Path(path).unlink()

    def test_writer_write_after_close(self) -> None:
        """Test error when writing after close."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            writer = fx.JsonlWriter(path)
            writer.close()

            with pytest.raises(IOError):
                writer.write({"id": 1})
        finally:
            Path(path).unlink()

    def test_writer_large_volume(self) -> None:
        """Test writing large number of records."""
        import fionn.ext as fx

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            with fx.JsonlWriter(path) as writer:
                for i in range(1000):
                    writer.write({"id": i, "data": f"record_{i}"})

            # Read back and verify
            reader = fx.JsonlReader(path, batch_size=100)
            all_records = [r for batch in reader for r in batch]
            assert len(all_records) == 1000
        finally:
            Path(path).unlink()


class TestJsonlReadWriteRoundTrip:
    """Test round-trip reading and writing."""

    def test_round_trip_simple(self) -> None:
        """Test simple round-trip."""
        import fionn.ext as fx

        original_data = [
            {"id": 1, "name": "Alice", "score": 100},
            {"id": 2, "name": "Bob", "score": 200},
        ]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            # Write
            with fx.JsonlWriter(path) as writer:
                for record in original_data:
                    writer.write(record)

            # Read
            reader = fx.JsonlReader(path, batch_size=10)
            read_data = [r for batch in reader for r in batch]

            # Verify
            assert len(read_data) == len(original_data)
            for i, record in enumerate(read_data):
                assert record["id"] == original_data[i]["id"]
                assert record["name"] == original_data[i]["name"]
                assert record["score"] == original_data[i]["score"]
        finally:
            Path(path).unlink()

    def test_round_trip_nested(self) -> None:
        """Test round-trip with nested data."""
        import fionn.ext as fx

        original_data = [
            {"user": {"name": "Alice", "tags": ["admin", "user"]}},
            {"user": {"name": "Bob", "tags": ["user"]}},
        ]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name

        try:
            with fx.JsonlWriter(path) as writer:
                for record in original_data:
                    writer.write(record)

            reader = fx.JsonlReader(path, batch_size=10)
            read_data = [r for batch in reader for r in batch]

            assert read_data[0]["user"]["name"] == "Alice"
            assert read_data[0]["user"]["tags"] == ["admin", "user"]
        finally:
            Path(path).unlink()
