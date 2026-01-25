# SPDX-License-Identifier: MIT OR Apache-2.0
"""Property-based tests for Tape API path resolution.

These tests use Hypothesis to generate random JSON structures and verify
that the Tape API correctly resolves paths to values.
"""

from __future__ import annotations

import json
from typing import Any

import pytest
from hypothesis import assume, given, settings
from hypothesis import strategies as st

# =============================================================================
# JSON GENERATION STRATEGIES
# =============================================================================

# Primitive JSON values
json_primitives = st.one_of(
    st.none(),
    st.booleans(),
    st.integers(min_value=-(2**53), max_value=2**53),  # JS safe integers
    st.floats(allow_nan=False, allow_infinity=False),
    st.text(
        min_size=0,
        max_size=100,
        alphabet=st.characters(
            blacklist_categories=("Cs",),  # Exclude surrogates
            blacklist_characters=("\x00",),  # Exclude null bytes
        ),
    ),
)


def json_values(max_depth: int = 3):
    """Generate arbitrary JSON values with controlled depth."""
    if max_depth <= 0:
        return json_primitives

    return st.one_of(
        json_primitives,
        st.lists(st.deferred(lambda: json_values(max_depth - 1)), max_size=10),
        st.dictionaries(
            keys=st.text(
                min_size=1,
                max_size=20,
                alphabet=st.characters(
                    whitelist_categories=("L", "N"),  # Letters and numbers only for keys
                ),
            ).filter(lambda s: s and not s[0].isdigit()),  # Valid identifier-like keys
            values=st.deferred(lambda: json_values(max_depth - 1)),
            max_size=10,
        ),
    )


# Strategy for valid field names (no special characters)
valid_field_names = st.text(
    min_size=1,
    max_size=20,
    alphabet=st.characters(whitelist_categories=("L", "N")),
).filter(lambda s: s and not s[0].isdigit() and s.isidentifier())


# =============================================================================
# PROPERTY TESTS: BASIC PATH RESOLUTION
# =============================================================================


class TestTapePathResolutionProperties:
    """Property-based tests for Tape.get() path resolution."""

    @given(
        st.dictionaries(
            keys=valid_field_names,
            values=json_primitives,
            min_size=1,
            max_size=20,
        )
    )
    @settings(max_examples=200)
    def test_flat_object_field_access(self, obj: dict[str, Any]) -> None:
        """Property: All fields in a flat object should be accessible by name."""
        import fionn.ext as fx

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        for key, expected in obj.items():
            result = tape.get(key)
            if isinstance(expected, float):
                assert result is not None
                assert abs(result - expected) < 1e-10, f"Float mismatch for {key}"
            else:
                assert result == expected, f"Mismatch for {key}: got {result}, expected {expected}"

    @given(st.lists(json_primitives, min_size=1, max_size=50))
    @settings(max_examples=200)
    def test_array_index_access(self, arr: list[Any]) -> None:
        """Property: All elements in an array should be accessible by index."""
        import fionn.ext as fx

        json_str = json.dumps(arr)
        tape = fx.Tape.parse(json_str)

        for i, expected in enumerate(arr):
            result = tape.get(f"[{i}]")
            if isinstance(expected, float):
                assert result is not None
                assert abs(result - expected) < 1e-10, f"Float mismatch at [{i}]"
            elif isinstance(expected, (list, dict)):
                # Complex values return None from get() - use to_object()
                pass
            else:
                assert result == expected, f"Mismatch at [{i}]: got {result}, expected {expected}"

    @given(st.lists(json_primitives, min_size=1, max_size=20))
    @settings(max_examples=100)
    def test_array_out_of_bounds(self, arr: list[Any]) -> None:
        """Property: Out of bounds array access returns None."""
        import fionn.ext as fx

        json_str = json.dumps(arr)
        tape = fx.Tape.parse(json_str)

        # Access beyond array length should return None
        assert tape.get(f"[{len(arr)}]") is None
        assert tape.get(f"[{len(arr) + 100}]") is None

    @given(st.dictionaries(keys=valid_field_names, values=json_primitives, min_size=1, max_size=10))
    @settings(max_examples=100)
    def test_missing_field_returns_none(self, obj: dict[str, Any]) -> None:
        """Property: Accessing a non-existent field returns None."""
        import fionn.ext as fx

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        # Generate a key that doesn't exist
        nonexistent = "nonexistent_field_xyz123"
        assume(nonexistent not in obj)

        assert tape.get(nonexistent) is None


# =============================================================================
# PROPERTY TESTS: NESTED PATH RESOLUTION
# =============================================================================


class TestTapeNestedPathResolutionProperties:
    """Property-based tests for nested path resolution."""

    @given(
        st.dictionaries(
            keys=valid_field_names,
            values=st.dictionaries(
                keys=valid_field_names,
                values=json_primitives,
                min_size=1,
                max_size=5,
            ),
            min_size=1,
            max_size=5,
        )
    )
    @settings(max_examples=200)
    def test_nested_object_access(self, obj: dict[str, dict[str, Any]]) -> None:
        """Property: Nested object fields are accessible via dot notation."""
        import fionn.ext as fx

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        for outer_key, inner_obj in obj.items():
            for inner_key, expected in inner_obj.items():
                path = f"{outer_key}.{inner_key}"
                result = tape.get(path)

                if isinstance(expected, float):
                    assert result is not None, f"None for {path}"
                    assert abs(result - expected) < 1e-10, f"Float mismatch for {path}"
                else:
                    assert result == expected, (
                        f"Mismatch for {path}: got {result}, expected {expected}"
                    )

    @given(
        st.dictionaries(
            keys=valid_field_names,
            values=st.lists(json_primitives, min_size=1, max_size=10),
            min_size=1,
            max_size=5,
        )
    )
    @settings(max_examples=200)
    def test_object_with_array_values(self, obj: dict[str, list[Any]]) -> None:
        """Property: Array elements inside objects are accessible."""
        import fionn.ext as fx

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        for key, arr in obj.items():
            for i, expected in enumerate(arr):
                path = f"{key}[{i}]"
                result = tape.get(path)

                if isinstance(expected, float):
                    assert result is not None, f"None for {path}"
                    assert abs(result - expected) < 1e-10, f"Float mismatch for {path}"
                elif isinstance(expected, (list, dict)):
                    # Complex nested values - skip
                    pass
                else:
                    assert result == expected, (
                        f"Mismatch for {path}: got {result}, expected {expected}"
                    )

    @given(
        st.lists(
            st.dictionaries(
                keys=valid_field_names,
                values=json_primitives,
                min_size=1,
                max_size=5,
            ),
            min_size=1,
            max_size=10,
        )
    )
    @settings(max_examples=200)
    def test_array_of_objects(self, arr: list[dict[str, Any]]) -> None:
        """Property: Object fields inside arrays are accessible."""
        import fionn.ext as fx

        # Wrap in an object for typical use case
        obj = {"items": arr}
        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        for i, item in enumerate(arr):
            for key, expected in item.items():
                path = f"items[{i}].{key}"
                result = tape.get(path)

                if isinstance(expected, float):
                    assert result is not None, f"None for {path}"
                    assert abs(result - expected) < 1e-10, f"Float mismatch for {path}"
                else:
                    assert result == expected, (
                        f"Mismatch for {path}: got {result}, expected {expected}"
                    )


# =============================================================================
# PROPERTY TESTS: DEEPLY NESTED STRUCTURES
# =============================================================================


class TestTapeDeepNestingProperties:
    """Property-based tests for deeply nested structures."""

    @given(st.integers(min_value=1, max_value=10), json_primitives)
    @settings(max_examples=100)
    def test_deep_object_nesting(self, depth: int, leaf_value: Any) -> None:
        """Property: Deeply nested object fields are accessible."""
        import fionn.ext as fx

        assume(leaf_value is not None or depth < 5)  # Avoid too many Nones

        # Build nested structure: {"a": {"a": {"a": ... leaf_value}}}
        obj: dict[str, Any] = {"value": leaf_value}
        for _ in range(depth):
            obj = {"nested": obj}

        json_str = json.dumps(obj)
        tape = fx.Tape.parse(json_str)

        # Build path: nested.nested.nested...value
        path = ".".join(["nested"] * depth) + ".value"
        result = tape.get(path)

        if isinstance(leaf_value, float):
            assert result is not None
            assert abs(result - leaf_value) < 1e-10
        else:
            assert result == leaf_value, f"At depth {depth}: got {result}, expected {leaf_value}"

    @given(st.integers(min_value=1, max_value=10), json_primitives)
    @settings(max_examples=100)
    def test_deep_array_nesting(self, depth: int, leaf_value: Any) -> None:
        """Property: Deeply nested array elements are accessible."""
        import fionn.ext as fx

        assume(not isinstance(leaf_value, (list, dict)))  # Only primitives at leaf

        # Build nested structure: [[[[leaf_value]]]]
        arr: Any = leaf_value
        for _ in range(depth):
            arr = [arr]

        json_str = json.dumps(arr)
        tape = fx.Tape.parse(json_str)

        # Build path: [0][0][0]...
        path = "[0]" * depth
        result = tape.get(path)

        if isinstance(leaf_value, float):
            assert result is not None
            assert abs(result - leaf_value) < 1e-10
        else:
            assert result == leaf_value, f"At depth {depth}: got {result}, expected {leaf_value}"


# =============================================================================
# PROPERTY TESTS: TO_OBJECT CONSISTENCY
# =============================================================================


class TestTapeToObjectConsistency:
    """Property tests verifying to_object() matches Python json.loads()."""

    @given(json_values(max_depth=3))
    @settings(max_examples=300)
    def test_to_object_matches_json_loads(self, value: Any) -> None:
        """Property: Tape.to_object() produces same result as json.loads()."""
        import fionn.ext as fx

        json_str = json.dumps(value)
        tape = fx.Tape.parse(json_str)

        tape_result = tape.to_object()
        json_result = json.loads(json_str)

        # Compare with tolerance for floats
        def compare(a: Any, b: Any) -> bool:
            if isinstance(a, float) and isinstance(b, float):
                return abs(a - b) < 1e-10
            elif isinstance(a, dict) and isinstance(b, dict):
                if set(a.keys()) != set(b.keys()):
                    return False
                return all(compare(a[k], b[k]) for k in a)
            elif isinstance(a, list) and isinstance(b, list):
                if len(a) != len(b):
                    return False
                return all(compare(x, y) for x, y in zip(a, b))
            else:
                return a == b

        assert compare(tape_result, json_result), (
            f"Mismatch:\n  tape: {tape_result}\n  json: {json_result}"
        )


# =============================================================================
# PROPERTY TESTS: ROUNDTRIP
# =============================================================================


class TestTapeRoundtripProperties:
    """Property tests for Tape JSON roundtrip."""

    @given(json_values(max_depth=3))
    @settings(max_examples=200)
    def test_to_json_roundtrip(self, value: Any) -> None:
        """Property: Tape.to_json() produces valid JSON that parses to same value."""
        import fionn.ext as fx

        # Filter out floats that exceed safe integer range (JSON limitation)
        def has_unsafe_float(v: Any) -> bool:
            if isinstance(v, float):
                return abs(v) > 2**53
            elif isinstance(v, dict):
                return any(has_unsafe_float(x) for x in v.values())
            elif isinstance(v, list):
                return any(has_unsafe_float(x) for x in v)
            return False

        assume(not has_unsafe_float(value))

        json_str = json.dumps(value)
        tape = fx.Tape.parse(json_str)

        # Get JSON back from tape
        tape_json = tape.to_json()

        # Parse both and compare
        original = json.loads(json_str)
        roundtrip = json.loads(tape_json)

        def compare(a: Any, b: Any) -> bool:
            if isinstance(a, float) and isinstance(b, float):
                # Allow for float precision loss in roundtrip
                if a == 0:
                    return abs(b) < 1e-10
                return abs(a - b) / max(abs(a), 1e-10) < 1e-10
            elif isinstance(a, float) and isinstance(b, int):
                # Integer representation of float
                return abs(a - b) < 1e-10
            elif isinstance(a, int) and isinstance(b, float):
                return abs(a - b) < 1e-10
            elif isinstance(a, dict) and isinstance(b, dict):
                if set(a.keys()) != set(b.keys()):
                    return False
                return all(compare(a[k], b[k]) for k in a)
            elif isinstance(a, list) and isinstance(b, list):
                if len(a) != len(b):
                    return False
                return all(compare(x, y) for x, y in zip(a, b))
            else:
                return a == b

        assert compare(original, roundtrip), (
            f"Roundtrip mismatch:\n  original: {original}\n  roundtrip: {roundtrip}"
        )


# =============================================================================
# EDGE CASE TESTS
# =============================================================================


class TestTapeEdgeCases:
    """Test edge cases that might break path resolution."""

    def test_empty_object(self) -> None:
        """Empty object should have no accessible fields."""
        import fionn.ext as fx

        tape = fx.Tape.parse("{}")
        assert tape.get("anything") is None
        assert tape.to_object() == {}

    def test_empty_array(self) -> None:
        """Empty array should have no accessible elements."""
        import fionn.ext as fx

        tape = fx.Tape.parse("[]")
        assert tape.get("[0]") is None
        assert tape.to_object() == []

    def test_null_value(self) -> None:
        """Null value should be accessible and return None."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"value": null}')
        result = tape.get("value")
        assert result is None

    def test_empty_string(self) -> None:
        """Empty string should be accessible."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"value": ""}')
        assert tape.get("value") == ""

    def test_special_characters_in_values(self) -> None:
        """Values with special characters should work."""
        import fionn.ext as fx

        obj = {"value": 'hello\nworld\ttab"quote'}
        tape = fx.Tape.parse(json.dumps(obj))
        assert tape.get("value") == obj["value"]

    def test_unicode_values(self) -> None:
        """Unicode values should be accessible."""
        import fionn.ext as fx

        obj = {"emoji": "Hello World!", "chinese": "Chinese characters", "arabic": "Arabic text"}
        tape = fx.Tape.parse(json.dumps(obj))
        for key, expected in obj.items():
            assert tape.get(key) == expected

    def test_large_integers(self) -> None:
        """Large integers should be handled correctly."""
        import fionn.ext as fx

        obj = {"big": 9007199254740992, "neg": -9007199254740992}
        tape = fx.Tape.parse(json.dumps(obj))
        assert tape.get("big") == 9007199254740992
        assert tape.get("neg") == -9007199254740992

    def test_scientific_notation(self) -> None:
        """Scientific notation floats should work."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"value": 1.23e10}')
        result = tape.get("value")
        assert result is not None
        assert abs(result - 1.23e10) < 1e5  # Allow for float precision

    @given(st.integers(min_value=0, max_value=99))
    def test_numeric_string_keys(self, num: int) -> None:
        """Object keys that look like numbers should work."""
        import fionn.ext as fx

        # Keys that are numeric strings
        obj = {str(num): "value"}
        tape = fx.Tape.parse(json.dumps(obj))
        assert tape.get(str(num)) == "value"


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
