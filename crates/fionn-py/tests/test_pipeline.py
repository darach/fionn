# SPDX-License-Identifier: MIT OR Apache-2.0
"""Tests for Pipeline class (filter, map, process)."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

import pytest


class TestPipelineCreation:
    """Test Pipeline creation and basic operations."""

    def test_create_pipeline(self) -> None:
        """Test creating an empty pipeline."""
        import fionn.ext as fx

        pipeline = fx.Pipeline()
        assert len(pipeline) == 0
        assert "Pipeline" in repr(pipeline)

    def test_pipeline_filter_chaining(self) -> None:
        """Test filter method returns self for chaining."""
        import fionn.ext as fx

        pipeline = fx.Pipeline()
        pipeline.filter(lambda x: x.get("active", False))
        # Should be the same pipeline (method chaining)
        assert len(pipeline) == 1

    def test_pipeline_map_chaining(self) -> None:
        """Test map method returns self for chaining."""
        import fionn.ext as fx

        pipeline = fx.Pipeline()
        pipeline.map(lambda x: {"id": x.get("id")})
        assert len(pipeline) == 1

    def test_pipeline_combined_stages(self) -> None:
        """Test combining filter and map stages."""
        import fionn.ext as fx

        pipeline = fx.Pipeline()
        pipeline.filter(lambda x: x.get("active", False))
        pipeline.map(lambda x: {"id": x["id"], "score": x["score"] * 2})
        assert len(pipeline) == 2
        assert "filter" in repr(pipeline)
        assert "map" in repr(pipeline)


class TestPipelineProcessJsonl:
    """Test Pipeline.process_jsonl()."""

    def test_process_jsonl_no_stages(self) -> None:
        """Test processing JSONL with no stages (passthrough)."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            # Create input file
            input_data = [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"},
            ]
            with open(input_path, "w") as f:
                for record in input_data:
                    f.write(json.dumps(record) + "\n")

            # Process
            pipeline = fx.Pipeline()
            count = pipeline.process_jsonl(str(input_path), str(output_path))
            assert count == 2

            # Verify output
            with open(output_path) as f:
                output_data = [json.loads(line) for line in f]
            assert len(output_data) == 2

    def test_process_jsonl_with_filter(self) -> None:
        """Test processing JSONL with filter stage."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            # Create input file
            input_data = [
                {"id": 1, "active": True},
                {"id": 2, "active": False},
                {"id": 3, "active": True},
            ]
            with open(input_path, "w") as f:
                for record in input_data:
                    f.write(json.dumps(record) + "\n")

            # Process with filter
            pipeline = fx.Pipeline()
            pipeline.filter(lambda x: x.get("active", False))
            count = pipeline.process_jsonl(str(input_path), str(output_path))
            assert count == 2  # Only active records

            # Verify output
            with open(output_path) as f:
                output_data = [json.loads(line) for line in f]
            assert len(output_data) == 2
            assert all(r["active"] for r in output_data)

    def test_process_jsonl_with_map(self) -> None:
        """Test processing JSONL with map stage."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            # Create input file
            input_data = [
                {"id": 1, "score": 10},
                {"id": 2, "score": 20},
            ]
            with open(input_path, "w") as f:
                for record in input_data:
                    f.write(json.dumps(record) + "\n")

            # Process with map
            pipeline = fx.Pipeline()
            pipeline.map(lambda x: {"id": x["id"], "doubled": x["score"] * 2})
            count = pipeline.process_jsonl(str(input_path), str(output_path))
            assert count == 2

            # Verify output
            with open(output_path) as f:
                output_data = [json.loads(line) for line in f]
            assert output_data[0]["doubled"] == 20
            assert output_data[1]["doubled"] == 40

    def test_process_jsonl_filter_then_map(self) -> None:
        """Test processing JSONL with filter then map."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            input_data = [
                {"id": 1, "score": 10, "active": True},
                {"id": 2, "score": 20, "active": False},
                {"id": 3, "score": 30, "active": True},
            ]
            with open(input_path, "w") as f:
                for record in input_data:
                    f.write(json.dumps(record) + "\n")

            pipeline = fx.Pipeline()
            pipeline.filter(lambda x: x.get("active", False))
            pipeline.map(lambda x: {"id": x["id"], "score": x["score"] * 2})
            count = pipeline.process_jsonl(str(input_path), str(output_path))
            assert count == 2

            with open(output_path) as f:
                output_data = [json.loads(line) for line in f]
            assert len(output_data) == 2
            assert output_data[0] == {"id": 1, "score": 20}
            assert output_data[1] == {"id": 3, "score": 60}


class TestPipelineProcessIsonl:
    """Test Pipeline.process_isonl()."""

    def test_process_isonl_passthrough(self) -> None:
        """Test processing ISONL with no stages."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.isonl"
            output_path = Path(tmpdir) / "output.isonl"

            # Create ISONL input
            isonl_data = [
                "table.users|id:int|name:string|1|Alice",
                "table.users|id:int|name:string|2|Bob",
            ]
            with open(input_path, "w") as f:
                f.write("\n".join(isonl_data) + "\n")

            pipeline = fx.Pipeline()
            count = pipeline.process_isonl(str(input_path), str(output_path))
            assert count == 2

    def test_process_isonl_with_filter(self) -> None:
        """Test processing ISONL with filter stage."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.isonl"
            output_path = Path(tmpdir) / "output.isonl"

            # Create ISONL input with varying ids
            isonl_data = [
                "table.users|id:int|name:string|1|Alice",
                "table.users|id:int|name:string|2|Bob",
                "table.users|id:int|name:string|3|Charlie",
            ]
            with open(input_path, "w") as f:
                f.write("\n".join(isonl_data) + "\n")

            # Filter for id > 1
            pipeline = fx.Pipeline()
            pipeline.filter(lambda x: x.get("id", 0) > 1)
            count = pipeline.process_isonl(str(input_path), str(output_path))
            assert count == 2  # Bob and Charlie


class TestPipelineProcess:
    """Test Pipeline.process() with format conversion."""

    def test_process_jsonl_to_jsonl(self) -> None:
        """Test processing JSONL to JSONL."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            input_data = [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"},
            ]
            with open(input_path, "w") as f:
                for record in input_data:
                    f.write(json.dumps(record) + "\n")

            pipeline = fx.Pipeline()
            count = pipeline.process(str(input_path), "jsonl", str(output_path), "jsonl")
            assert count == 2

    def test_process_with_output_schema(self) -> None:
        """Test processing with output schema filter."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            input_data = [
                {"id": 1, "name": "Alice", "email": "alice@example.com"},
                {"id": 2, "name": "Bob", "email": "bob@example.com"},
            ]
            with open(input_path, "w") as f:
                for record in input_data:
                    f.write(json.dumps(record) + "\n")

            pipeline = fx.Pipeline()
            count = pipeline.process(
                str(input_path),
                "jsonl",
                str(output_path),
                "jsonl",
                output_schema=["id", "name"],  # Exclude email
            )
            assert count == 2

            with open(output_path) as f:
                output_data = [json.loads(line) for line in f]
            assert "email" not in output_data[0]
            assert output_data[0] == {"id": 1, "name": "Alice"}

    def test_process_json_array_input(self) -> None:
        """Test processing JSON array as input."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.json"
            output_path = Path(tmpdir) / "output.jsonl"

            input_data = [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"},
            ]
            with open(input_path, "w") as f:
                json.dump(input_data, f)

            pipeline = fx.Pipeline()
            count = pipeline.process(str(input_path), "json", str(output_path), "jsonl")
            assert count == 2

    def test_process_jsonl_to_isonl(self) -> None:
        """Test converting JSONL to ISONL format."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.isonl"

            input_data = [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"},
            ]
            with open(input_path, "w") as f:
                for record in input_data:
                    f.write(json.dumps(record) + "\n")

            pipeline = fx.Pipeline()
            count = pipeline.process(str(input_path), "jsonl", str(output_path), "isonl")
            assert count == 2

            # Verify ISONL output format
            with open(output_path) as f:
                lines = f.readlines()
            assert len(lines) == 2
            # ISONL lines should contain pipe separators
            assert "|" in lines[0]


class TestPipelineErrorHandling:
    """Test Pipeline error handling."""

    def test_process_invalid_input_format(self) -> None:
        """Test error on invalid input format."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.txt"
            output_path = Path(tmpdir) / "output.jsonl"

            with open(input_path, "w") as f:
                f.write("test")

            pipeline = fx.Pipeline()
            with pytest.raises(ValueError):
                pipeline.process(str(input_path), "invalid_format", str(output_path), "jsonl")

    def test_process_invalid_output_format(self) -> None:
        """Test error on invalid output format."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.txt"

            with open(input_path, "w") as f:
                f.write('{"id": 1}\n')

            pipeline = fx.Pipeline()
            with pytest.raises(ValueError):
                pipeline.process(str(input_path), "jsonl", str(output_path), "invalid_format")

    def test_process_missing_input_file(self) -> None:
        """Test error on missing input file."""
        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            output_path = Path(tmpdir) / "output.jsonl"

            pipeline = fx.Pipeline()
            with pytest.raises(IOError):
                pipeline.process_jsonl("/nonexistent/path/input.jsonl", str(output_path))
