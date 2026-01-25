# SPDX-License-Identifier: MIT OR Apache-2.0
"""Tests for the Tape API (zero-copy SIMD-accelerated JSON access)."""

from __future__ import annotations

import pytest


class TestTapeParsing:
    """Test Tape.parse() and basic operations."""

    def test_parse_simple_object(self) -> None:
        """Test parsing a simple JSON object."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice", "age": 30}')
        assert tape.node_count > 0
        assert len(tape) > 0

    def test_parse_bytes(self) -> None:
        """Test parsing from bytes."""
        import fionn.ext as fx

        tape = fx.Tape.parse(b'{"name": "Bob"}')
        assert tape.node_count > 0

    def test_parse_nested(self) -> None:
        """Test parsing nested JSON."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"user": {"name": "Alice", "address": {"city": "NYC"}}}')
        assert tape.node_count > 0

    def test_parse_array(self) -> None:
        """Test parsing JSON array."""
        import fionn.ext as fx

        tape = fx.Tape.parse('[1, 2, 3, "four", true, null]')
        assert tape.node_count > 0

    def test_parse_invalid_json(self) -> None:
        """Test parsing invalid JSON raises error."""
        import fionn.ext as fx

        with pytest.raises(Exception):  # JSONDecodeError
            fx.Tape.parse('{"invalid": }')


class TestTapePathAccess:
    """Test Tape.get() and resolve_path()."""

    def test_get_simple_field(self) -> None:
        """Test getting a simple field."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice", "age": 30}')
        assert tape.get("name") == "Alice"
        assert tape.get("age") == 30

    def test_get_nested_field(self) -> None:
        """Test getting a nested field."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"user": {"name": "Alice", "address": {"city": "NYC"}}}')
        assert tape.get("user.name") == "Alice"
        assert tape.get("user.address.city") == "NYC"

    def test_get_array_element(self) -> None:
        """Test getting array elements."""
        import fionn.ext as fx

        # Simple array access at root level
        tape = fx.Tape.parse("[1, 2, 3]")
        assert tape.get("[0]") == 1
        assert tape.get("[1]") == 2
        assert tape.get("[2]") == 3
        assert tape.get("[3]") is None  # Out of bounds

        # Array inside object
        tape2 = fx.Tape.parse('{"items": [10, 20, 30]}')
        assert tape2.get("items[0]") == 10
        assert tape2.get("items[1]") == 20
        assert tape2.get("items[2]") == 30

        # Objects inside array
        tape3 = fx.Tape.parse('{"users": [{"name": "Alice"}, {"name": "Bob"}]}')
        assert tape3.get("users[0].name") == "Alice"
        assert tape3.get("users[1].name") == "Bob"

        # Deep nesting with arrays
        tape4 = fx.Tape.parse('{"data": {"items": [{"value": 42}]}}')
        assert tape4.get("data.items[0].value") == 42

        # Nested arrays
        tape5 = fx.Tape.parse("[[1, 2], [3, 4]]")
        assert tape5.get("[0][0]") == 1
        assert tape5.get("[0][1]") == 2
        assert tape5.get("[1][0]") == 3
        assert tape5.get("[1][1]") == 4

    def test_get_missing_field(self) -> None:
        """Test getting a missing field returns None."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice"}')
        assert tape.get("missing") is None
        assert tape.get("deeply.nested.missing") is None

    def test_resolve_path(self) -> None:
        """Test resolve_path returns tape index."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice"}')
        idx = tape.resolve_path("name")
        assert idx is not None
        assert tape.get_value(idx) == "Alice"

    def test_get_value_types(self) -> None:
        """Test getting different value types."""
        import fionn.ext as fx

        tape = fx.Tape.parse(
            '{"str": "hello", "int": 42, "float": 3.14, "bool": true, "null": null}'
        )
        assert tape.get("str") == "hello"
        assert tape.get("int") == 42
        assert abs(tape.get("float") - 3.14) < 0.001
        assert tape.get("bool") is True
        assert tape.get("null") is None


class TestTapeConversion:
    """Test Tape.to_json() and to_object()."""

    def test_to_json(self) -> None:
        """Test converting tape back to JSON."""
        import fionn.ext as fx

        original = '{"name":"Alice","age":30}'
        tape = fx.Tape.parse(original)
        result = tape.to_json()
        # JSON may have different spacing, so parse and compare
        import json

        assert json.loads(result) == json.loads(original)

    def test_to_object(self) -> None:
        """Test converting tape to Python object."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice", "scores": [100, 95, 88]}')
        obj = tape.to_object()
        assert obj == {"name": "Alice", "scores": [100, 95, 88]}


class TestTapeFiltering:
    """Test Tape.filter() with schema."""

    def test_filter_fields(self) -> None:
        """Test filtering to specific fields."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice", "age": 30, "email": "alice@example.com"}')
        filtered = tape.filter(["name"])
        result = filtered.to_object()
        assert "name" in result
        # Note: filter behavior may include or exclude based on implementation

    def test_parse_with_schema(self) -> None:
        """Test parsing with schema filter."""
        import fionn.ext as fx

        schema = fx.Schema(["name"])
        tape = fx.Tape.parse('{"name": "Alice", "age": 30}', schema=schema)
        # Tape is filtered during parsing
        assert tape.node_count > 0


class TestTapeUtilities:
    """Test utility functions."""

    def test_parse_tape_function(self) -> None:
        """Test parse_tape convenience function."""
        import fionn.ext as fx

        tape = fx.parse_tape('{"name": "Alice"}')
        assert tape.get("name") == "Alice"

    def test_parse_tape_with_fields(self) -> None:
        """Test parse_tape with field filter."""
        import fionn.ext as fx

        tape = fx.parse_tape('{"name": "Alice", "age": 30}', fields=["name"])
        assert tape.node_count > 0

    def test_batch_resolve(self) -> None:
        """Test batch_resolve for multiple paths."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice", "age": 30, "city": "NYC"}')
        results = fx.batch_resolve(tape, ["name", "age", "missing"])
        assert results["name"] == "Alice"
        assert results["age"] == 30
        assert results["missing"] is None

    def test_batch_query(self) -> None:
        """Test batch_query across multiple documents."""
        import fionn.ext as fx

        jsons = [
            '{"name": "Alice"}',
            '{"name": "Bob"}',
            '{"name": "Charlie"}',
        ]
        results = fx.batch_query(jsons, "name")
        assert results == ["Alice", "Bob", "Charlie"]


class TestTapePool:
    """Test TapePool for pooled parsing."""

    def test_pool_creation(self) -> None:
        """Test creating a tape pool."""
        import fionn.ext as fx

        pool = fx.TapePool(strategy="lru", max_tapes=100)
        assert "TapePool" in repr(pool)
        assert "buffers=" in repr(pool)

    def test_pool_parse(self) -> None:
        """Test parsing through pool."""
        import fionn.ext as fx

        pool = fx.TapePool()
        tape = pool.parse('{"name": "Alice"}')
        assert tape.get("name") == "Alice"


class TestTapeRepr:
    """Test Tape representation."""

    def test_repr(self) -> None:
        """Test Tape __repr__."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"a": 1}')
        assert "Tape" in repr(tape)
        assert "nodes=" in repr(tape)

    def test_schema_repr(self) -> None:
        """Test Schema __repr__."""
        import fionn.ext as fx

        schema = fx.Schema(["name", "age"])
        assert "Schema" in repr(schema)


class TestTapeQuery:
    """Test Tape.query() JSONPath-like support."""

    def test_query_root(self) -> None:
        """Test querying root element."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice"}')
        results = tape.query("$")
        assert len(results) == 1
        assert results[0] == {"name": "Alice"}

    def test_query_simple_field(self) -> None:
        """Test querying a simple field."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice", "age": 30}')
        results = tape.query("name")
        assert len(results) == 1
        assert results[0] == "Alice"

    def test_query_nested_field(self) -> None:
        """Test querying nested fields."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"user": {"name": "Alice", "address": {"city": "NYC"}}}')
        results = tape.query("user.name")
        assert results == ["Alice"]

        results = tape.query("user.address.city")
        assert results == ["NYC"]

    def test_query_array_index(self) -> None:
        """Test querying specific array index."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"items": [10, 20, 30]}')
        results = tape.query("items[1]")
        assert results == [20]

    def test_query_array_wildcard(self) -> None:
        """Test querying all array elements with [*]."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"items": [1, 2, 3]}')
        results = tape.query("items[*]")
        assert results == [1, 2, 3]

    def test_query_wildcard_nested(self) -> None:
        """Test querying nested fields through wildcard."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"users": [{"name": "Alice"}, {"name": "Bob"}]}')
        results = tape.query("users[*].name")
        assert results == ["Alice", "Bob"]

    def test_query_recursive_descent(self) -> None:
        """Test recursive descent (..) to find all occurrences."""
        import fionn.ext as fx

        tape = fx.Tape.parse(
            '{"users": [{"name": "Alice"}, {"name": "Bob"}], "admin": {"name": "Admin"}}'
        )
        results = tape.query("..name")
        # Should find all "name" fields at any depth
        assert len(results) == 3
        assert set(results) == {"Alice", "Bob", "Admin"}

    def test_query_recursive_in_nested_arrays(self) -> None:
        """Test recursive descent in nested arrays."""
        import fionn.ext as fx

        tape = fx.Tape.parse(
            '{"data": [{"items": [{"id": 1}, {"id": 2}]}, {"items": [{"id": 3}]}]}'
        )
        results = tape.query("..id")
        assert set(results) == {1, 2, 3}

    def test_query_with_dollar_prefix(self) -> None:
        """Test queries with $ prefix."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice"}')

        # Both should work the same
        results1 = tape.query("$.name")
        results2 = tape.query("name")
        assert results1 == results2 == ["Alice"]

    def test_query_empty_result(self) -> None:
        """Test query that matches nothing."""
        import fionn.ext as fx

        tape = fx.Tape.parse('{"name": "Alice"}')
        results = tape.query("missing")
        assert results == []

    def test_query_complex_path(self) -> None:
        """Test complex paths combining multiple features."""
        import fionn.ext as fx

        tape = fx.Tape.parse("""
        {
            "store": {
                "books": [
                    {"title": "Book1", "price": 10},
                    {"title": "Book2", "price": 20}
                ]
            }
        }
        """)

        results = tape.query("store.books[*].title")
        assert results == ["Book1", "Book2"]

        results = tape.query("store.books[0].price")
        assert results == [10]


@pytest.mark.benchmark
class TestTapePerformance:
    """Performance benchmarks for Tape API."""

    @pytest.mark.skip(reason="Benchmark - run explicitly")
    def test_tape_vs_loads_performance(self) -> None:
        """Benchmark Tape.get() vs fionn.loads()."""
        import timeit

        import fionn
        import fionn.ext as fx

        json_str = (
            '{"users": ['
            + ",".join([f'{{"id": {i}, "name": "User{i}"}}' for i in range(100)])
            + "]}"
        )

        # Repeated access with Tape (parse once)
        tape = fx.Tape.parse(json_str)
        tape_time = timeit.timeit(lambda: tape.get("users[50].name"), number=10000)

        # Repeated access with loads (parse every time)
        loads_time = timeit.timeit(
            lambda: fionn.loads(json_str.encode())["users"][50]["name"], number=10000
        )

        print(f"Tape: {tape_time:.4f}s, loads: {loads_time:.4f}s")
        # Tape should be faster for repeated access
