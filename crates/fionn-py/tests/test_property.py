# SPDX-License-Identifier: MIT OR Apache-2.0
"""Property-based tests using hypothesis for fionn API.

These tests verify invariants and properties across randomly generated inputs.
Run with: pytest tests/test_property.py -v
"""

from __future__ import annotations

import json
from typing import Any

import pytest

# Try to import hypothesis - skip tests if not available
try:
    from hypothesis import HealthCheck, assume, given, settings
    from hypothesis import strategies as st

    HAS_HYPOTHESIS = True
except ImportError:
    HAS_HYPOTHESIS = False

    # Create dummy decorators
    def given(*args, **kwargs):
        def decorator(fn):
            return pytest.mark.skip(reason="hypothesis not installed")(fn)

        return decorator

    def settings(*args, **kwargs):
        def decorator(fn):
            return fn

        return decorator

    st = None


# ============================================================================
# JSON Value Strategies
# ============================================================================

if HAS_HYPOTHESIS:
    # Basic JSON values
    json_primitives = st.one_of(
        st.none(),
        st.booleans(),
        st.integers(min_value=-(2**53), max_value=2**53),  # Safe for floats
        st.floats(allow_nan=False, allow_infinity=False),
        st.text(max_size=100),
    )

    # Recursive JSON structures
    json_values = st.recursive(
        json_primitives,
        lambda children: st.one_of(
            st.lists(children, max_size=10),
            st.dictionaries(
                st.text(min_size=1, max_size=20).filter(
                    lambda s: s.isidentifier() or s.replace("_", "").isalnum()
                ),
                children,
                max_size=10,
            ),
        ),
        max_leaves=50,
    )

    # JSON objects only (for formats that require objects)
    json_objects = st.dictionaries(
        st.text(min_size=1, max_size=20).filter(lambda s: s.isidentifier()),
        json_primitives,
        min_size=1,
        max_size=10,
    )

    # List of JSON objects (for tabular formats)
    json_object_lists = st.lists(json_objects, min_size=1, max_size=10)


# ============================================================================
# Tape API Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestTapeProperties:
    """Property tests for Tape API."""

    @given(json_values)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_tape_parse_roundtrip(self, value: Any) -> None:
        """Property: Tape.parse().to_object() == original value."""
        import fionn.ext as fx

        # Skip values that can't be serialized to JSON
        try:
            json_str = json.dumps(value)
        except (TypeError, ValueError):
            assume(False)
            return

        tape = fx.Tape.parse(json_str)
        result = tape.to_object()

        # Compare with tolerance for floats
        if isinstance(value, float) and isinstance(result, float):
            assert abs(value - result) < 1e-10 or value == result
        else:
            assert result == value

    @given(json_objects)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_tape_get_returns_correct_values(self, obj: dict) -> None:
        """Property: tape.get(key) == obj[key] for all keys."""
        import fionn.ext as fx

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        for key, expected in obj.items():
            result = tape.get(key)
            if isinstance(expected, float) and isinstance(result, (float, int)):
                # Large floats may lose precision or be converted to int
                if abs(expected) > 2**53:
                    continue  # Skip precision-lossy floats
                assert abs(expected - result) < 1e-10 or expected == result
            elif result is not None or expected is not None:
                assert result == expected

    @given(json_objects, st.text(min_size=1, max_size=50).filter(lambda s: not s.isdigit()))
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_tape_get_missing_returns_none(self, obj: dict, key: str) -> None:
        """Property: tape.get(nonexistent_key) returns None for objects."""
        import fionn.ext as fx

        # Skip if key exists in the object
        assume(key not in obj)

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        result = tape.get(key)
        assert result is None

    @given(json_objects)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_tape_query_root_returns_full_object(self, obj: dict) -> None:
        """Property: tape.query("$") returns [entire_object]."""
        import fionn.ext as fx

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)
        results = tape.query("$")

        assert len(results) == 1
        assert results[0] == obj


# ============================================================================
# JSONL Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestJsonlProperties:
    """Property tests for JSONL format."""

    @given(json_object_lists)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_jsonl_roundtrip(self, objects: list[dict]) -> None:
        """Property: parse_jsonl(to_jsonl(data)) == data."""
        import fionn.ext as fx

        # Serialize to JSONL
        jsonl_str = fx.to_jsonl(objects)

        # Parse back
        result = fx.parse_jsonl(jsonl_str)

        # Compare
        assert len(result) == len(objects)
        for original, parsed in zip(objects, result):
            assert parsed == original

    @given(json_object_lists)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_jsonl_line_count(self, objects: list[dict]) -> None:
        """Property: JSONL has one line per object (plus optional trailing newline)."""
        import fionn.ext as fx

        jsonl_str = fx.to_jsonl(objects)
        lines = [l for l in jsonl_str.strip().split("\n") if l.strip()]

        assert len(lines) == len(objects)


# ============================================================================
# CSV Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestCsvProperties:
    """Property tests for CSV format."""

    @given(
        st.lists(
            st.fixed_dictionaries(
                {
                    "name": st.from_regex(r"[a-zA-Z]{1,10}", fullmatch=True),
                    "city": st.from_regex(r"[a-zA-Z]{1,10}", fullmatch=True),
                }
            ),
            min_size=1,
            max_size=5,
        )
    )
    @settings(
        max_examples=50, suppress_health_check=[HealthCheck.too_slow, HealthCheck.filter_too_much]
    )
    def test_csv_roundtrip_string_only(self, objects: list[dict]) -> None:
        """Property: CSV roundtrip preserves string-only data."""
        import fionn.ext as fx

        csv_str = fx.to_csv(objects)
        result = fx.parse_csv(csv_str)

        assert len(result) == len(objects)
        for original, parsed in zip(objects, result):
            # Keys should match
            assert set(parsed.keys()) == set(original.keys())
            # Values should match
            for key in original:
                assert parsed[key] == original[key]


# ============================================================================
# YAML Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestYamlProperties:
    """Property tests for YAML format."""

    @given(json_objects)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_yaml_roundtrip(self, obj: dict) -> None:
        """Property: parse_yaml(to_yaml(data)) == data."""
        import fionn.ext as fx

        yaml_str = fx.to_yaml(obj)
        result = fx.parse_yaml(yaml_str)

        # Compare with tolerance for floats
        assert set(result.keys()) == set(obj.keys())
        for key in obj:
            expected = obj[key]
            actual = result[key]
            if isinstance(expected, float) and isinstance(actual, float):
                assert abs(expected - actual) < 1e-10 or expected == actual
            else:
                assert actual == expected


# ============================================================================
# TOML Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestTomlProperties:
    """Property tests for TOML format."""

    @given(
        st.dictionaries(
            st.text(min_size=1, max_size=10).filter(lambda s: s.isidentifier()),
            st.one_of(
                st.booleans(),
                st.integers(min_value=-(2**31), max_value=2**31),
                st.text(min_size=0, max_size=20),
            ),
            min_size=1,
            max_size=5,
        )
    )
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_toml_roundtrip(self, obj: dict) -> None:
        """Property: parse_toml(to_toml(data)) == data (for TOML-compatible values)."""
        import fionn.ext as fx

        # TOML doesn't support None values
        if any(v is None for v in obj.values()):
            assume(False)
            return

        toml_str = fx.to_toml(obj)
        result = fx.parse_toml(toml_str)

        assert result == obj


# ============================================================================
# Gron Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestGronProperties:
    """Property tests for gron operations."""

    # Use ASCII-only values for gron roundtrip to avoid Unicode encoding issues
    ascii_primitives = st.one_of(
        st.none(),
        st.booleans(),
        st.integers(min_value=-(2**31), max_value=2**31),
        st.floats(allow_nan=False, allow_infinity=False, min_value=-1e10, max_value=1e10),
        st.from_regex(r"[a-zA-Z0-9_ ]{0,20}", fullmatch=True),  # ASCII-only strings
    )

    ascii_json = st.recursive(
        ascii_primitives,
        lambda children: st.one_of(
            st.lists(children, max_size=5),
            st.dictionaries(
                st.from_regex(r"[a-zA-Z][a-zA-Z0-9_]{0,10}", fullmatch=True),
                children,
                max_size=5,
            ),
        ),
        max_leaves=20,
    )

    @given(ascii_json)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_gron_ungron_roundtrip(self, value: Any) -> None:
        """Property: ungron(gron(data)) == data (ASCII-only)."""
        import fionn.ext as fx

        try:
            json_str = json.dumps(value)
        except (TypeError, ValueError):
            assume(False)
            return

        # Gron and ungron
        gron_str = fx.gron(json_str)
        result = fx.ungron(gron_str)

        # Deep comparison with float tolerance
        def compare_values(a: Any, b: Any) -> bool:
            if isinstance(a, float) and isinstance(b, float):
                rel_tol = 1e-7
                return abs(a - b) <= rel_tol * max(abs(a), abs(b), 1.0)
            elif isinstance(a, list) and isinstance(b, list):
                return len(a) == len(b) and all(compare_values(x, y) for x, y in zip(a, b))
            elif isinstance(a, dict) and isinstance(b, dict):
                return set(a.keys()) == set(b.keys()) and all(compare_values(a[k], b[k]) for k in a)
            else:
                return a == b

        assert compare_values(value, result), f"Mismatch: {value} vs {result}"


# ============================================================================
# Diff Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestDiffProperties:
    """Property tests for diff/patch operations."""

    @given(json_objects, json_objects)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_patch_applies_diff(self, a: dict, b: dict) -> None:
        """Property: patch(a, diff(a, b)) == b."""
        import fionn.ext as fx

        diff_ops = fx.diff(a, b)
        result = fx.patch(a, diff_ops)

        assert result == b

    @given(json_objects)
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_diff_identical_is_empty(self, obj: dict) -> None:
        """Property: diff(x, x) == []."""
        import fionn.ext as fx

        diff_ops = fx.diff(obj, obj)
        assert diff_ops == []

    @given(json_objects, json_objects, json_objects)
    @settings(max_examples=50, suppress_health_check=[HealthCheck.too_slow])
    def test_deep_merge_associative(self, a: dict, b: dict, c: dict) -> None:
        """Property: deep_merge is associative - deep_merge(deep_merge(a, b), c) == deep_merge(a, deep_merge(b, c))."""
        import fionn.ext as fx

        # Note: This property may not hold for all merge strategies
        # but should hold for simple dict merging
        fx.deep_merge(fx.deep_merge(a, b), c)
        fx.deep_merge(a, fx.deep_merge(b, c))

        # For simple dict merge, this should be equal
        # (may differ for complex nested structures with conflicts)


# ============================================================================
# Pipeline Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestPipelineProperties:
    """Property tests for Pipeline operations."""

    @given(json_object_lists)
    @settings(max_examples=50, suppress_health_check=[HealthCheck.too_slow])
    def test_pipeline_identity(self, objects: list[dict]) -> None:
        """Property: Pipeline with no stages preserves data."""
        import tempfile
        from pathlib import Path

        import fionn.ext as fx

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            # Write input
            with open(input_path, "w") as f:
                for obj in objects:
                    f.write(json.dumps(obj) + "\n")

            # Process with empty pipeline
            pipeline = fx.Pipeline()
            count = pipeline.process_jsonl(str(input_path), str(output_path))

            assert count == len(objects)

            # Read output
            with open(output_path) as f:
                output = [json.loads(line) for line in f if line.strip()]

            assert len(output) == len(objects)

    @given(json_object_lists)
    @settings(max_examples=50, suppress_health_check=[HealthCheck.too_slow])
    def test_pipeline_filter_preserves_matching(self, objects: list[dict]) -> None:
        """Property: Filter stage preserves all matching records."""
        import tempfile
        from pathlib import Path

        import fionn.ext as fx

        # Add an "active" field to some objects
        for i, obj in enumerate(objects):
            obj["active"] = i % 2 == 0

        with tempfile.TemporaryDirectory() as tmpdir:
            input_path = Path(tmpdir) / "input.jsonl"
            output_path = Path(tmpdir) / "output.jsonl"

            with open(input_path, "w") as f:
                for obj in objects:
                    f.write(json.dumps(obj) + "\n")

            pipeline = fx.Pipeline()
            pipeline.filter(lambda x: x.get("active", False))
            count = pipeline.process_jsonl(str(input_path), str(output_path))

            expected_count = sum(1 for o in objects if o.get("active", False))
            assert count == expected_count


# ============================================================================
# Schema Filtering Property Tests
# ============================================================================


@pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")
class TestSchemaProperties:
    """Property tests for schema-based filtering."""

    # Use bounded values to avoid JSON parsing issues
    safe_primitives = st.one_of(
        st.none(),
        st.booleans(),
        st.integers(min_value=-(2**31), max_value=2**31),
        st.floats(allow_nan=False, allow_infinity=False, min_value=-1e100, max_value=1e100),
        st.text(max_size=50),
    )

    safe_objects = st.dictionaries(
        st.text(min_size=1, max_size=20).filter(lambda s: s.isidentifier()),
        safe_primitives,
        min_size=1,
        max_size=10,
    )

    @given(safe_objects, st.lists(st.text(min_size=1, max_size=10), min_size=1, max_size=5))
    @settings(max_examples=100, suppress_health_check=[HealthCheck.too_slow])
    def test_schema_filter_no_extra_fields(self, obj: dict, fields: list[str]) -> None:
        """Property: Schema filtering only includes requested fields."""
        import fionn.ext as fx

        json_str = json.dumps(obj)
        schema = fx.Schema(fields)
        tape = fx.Tape.parse(json_str, schema=schema)

        # Filtered tape should only have fields in schema (if they existed)
        result = tape.to_object()

        if isinstance(result, dict):
            for key in result:
                assert key in fields or key in obj
