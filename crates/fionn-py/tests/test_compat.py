# SPDX-License-Identifier: MIT OR Apache-2.0
"""Comprehensive tests for orjson compatibility."""

from __future__ import annotations

from typing import Any

import pytest


class TestLoadsBasicTypes:
    """Test loads() with basic JSON types."""

    def test_loads_dict(self) -> None:
        """Test parsing a dict."""
        import fionn

        result = fionn.loads(b'{"a": 1, "b": 2}')
        assert result == {"a": 1, "b": 2}

    def test_loads_empty_dict(self) -> None:
        """Test parsing an empty dict."""
        import fionn

        result = fionn.loads(b"{}")
        assert result == {}

    def test_loads_list(self) -> None:
        """Test parsing a list."""
        import fionn

        result = fionn.loads(b"[1, 2, 3]")
        assert result == [1, 2, 3]

    def test_loads_empty_list(self) -> None:
        """Test parsing an empty list."""
        import fionn

        result = fionn.loads(b"[]")
        assert result == []

    def test_loads_string(self) -> None:
        """Test parsing a string."""
        import fionn

        result = fionn.loads(b'"hello"')
        assert result == "hello"

    def test_loads_empty_string(self) -> None:
        """Test parsing an empty string."""
        import fionn

        result = fionn.loads(b'""')
        assert result == ""

    def test_loads_unicode_string(self) -> None:
        """Test parsing unicode strings."""
        import fionn

        result = fionn.loads(b'"\xe4\xb8\xad\xe6\x96\x87"')
        assert result == "中文"

    def test_loads_escaped_string(self) -> None:
        """Test parsing escaped characters."""
        import fionn

        result = fionn.loads(b'"hello\\nworld"')
        assert result == "hello\nworld"

        result = fionn.loads(b'"hello\\tworld"')
        assert result == "hello\tworld"

        result = fionn.loads(b'"hello\\"world"')
        assert result == 'hello"world'

    def test_loads_integer(self) -> None:
        """Test parsing integers."""
        import fionn

        assert fionn.loads(b"0") == 0
        assert fionn.loads(b"123") == 123
        assert fionn.loads(b"-42") == -42
        assert fionn.loads(b"9007199254740991") == 9007199254740991  # Max safe int

    def test_loads_large_integer(self) -> None:
        """Test parsing large integers."""
        import fionn

        # i64 max
        result = fionn.loads(b"9223372036854775807")
        assert result == 9223372036854775807

    def test_loads_float(self) -> None:
        """Test parsing floats."""
        import fionn

        assert fionn.loads(b"0.0") == 0.0
        assert fionn.loads(b"123.456") == 123.456
        assert fionn.loads(b"-3.14") == -3.14
        assert fionn.loads(b"1e10") == 1e10
        assert fionn.loads(b"1.5e-5") == 1.5e-5

    def test_loads_boolean(self) -> None:
        """Test parsing booleans."""
        import fionn

        assert fionn.loads(b"true") is True
        assert fionn.loads(b"false") is False

    def test_loads_null(self) -> None:
        """Test parsing null."""
        import fionn

        assert fionn.loads(b"null") is None


class TestLoadsInputTypes:
    """Test loads() with different input types."""

    def test_loads_bytes(self) -> None:
        """Test parsing from bytes."""
        import fionn

        result = fionn.loads(b'{"a": 1}')
        assert result == {"a": 1}

    def test_loads_str(self) -> None:
        """Test parsing from str."""
        import fionn

        result = fionn.loads('{"a": 1}')
        assert result == {"a": 1}

    def test_loads_bytearray(self) -> None:
        """Test parsing from bytearray."""
        import fionn

        result = fionn.loads(bytearray(b'{"a": 1}'))
        assert result == {"a": 1}

    def test_loads_memoryview(self) -> None:
        """Test parsing from memoryview."""
        import fionn

        result = fionn.loads(memoryview(b'{"a": 1}'))
        assert result == {"a": 1}

    def test_loads_invalid_type(self) -> None:
        """Test error on invalid input type."""
        import fionn

        with pytest.raises(TypeError):
            fionn.loads(123)  # type: ignore[arg-type]


class TestLoadsNested:
    """Test loads() with nested structures."""

    def test_loads_nested_dict(self) -> None:
        """Test parsing nested dicts."""
        import fionn

        result = fionn.loads(b'{"a": {"b": {"c": 1}}}')
        assert result == {"a": {"b": {"c": 1}}}

    def test_loads_nested_list(self) -> None:
        """Test parsing nested lists."""
        import fionn

        result = fionn.loads(b"[[1, 2], [3, 4]]")
        assert result == [[1, 2], [3, 4]]

    def test_loads_mixed_nested(self) -> None:
        """Test parsing mixed nested structures."""
        import fionn

        result = fionn.loads(b'{"a": [1, {"b": 2}], "c": {"d": [3, 4]}}')
        assert result == {"a": [1, {"b": 2}], "c": {"d": [3, 4]}}

    def test_loads_deeply_nested(self) -> None:
        """Test parsing deeply nested structures."""
        import fionn

        # 50 levels of nesting
        json_str = b'{"a":' * 50 + b"1" + b"}" * 50
        result = fionn.loads(json_str)

        # Navigate to the value
        current = result
        for _ in range(50):
            current = current["a"]
        assert current == 1


class TestLoadsErrors:
    """Test loads() error handling."""

    def test_loads_invalid_json(self) -> None:
        """Test error on invalid JSON."""
        import fionn

        with pytest.raises(fionn.JSONDecodeError):
            fionn.loads(b"invalid")

    def test_loads_truncated(self) -> None:
        """Test error on truncated JSON."""
        import fionn

        with pytest.raises(fionn.JSONDecodeError):
            fionn.loads(b'{"a":')

    def test_loads_trailing_comma(self) -> None:
        """Test error on trailing comma."""
        import fionn

        with pytest.raises(fionn.JSONDecodeError):
            fionn.loads(b'{"a": 1,}')

    def test_loads_single_quotes(self) -> None:
        """Test error on single quotes."""
        import fionn

        with pytest.raises(fionn.JSONDecodeError):
            fionn.loads(b"{'a': 1}")


class TestDumpsBasicTypes:
    """Test dumps() with basic Python types."""

    def test_dumps_dict(self) -> None:
        """Test serializing a dict."""
        import fionn

        result = fionn.dumps({"a": 1})
        assert result == b'{"a":1}'

    def test_dumps_empty_dict(self) -> None:
        """Test serializing an empty dict."""
        import fionn

        result = fionn.dumps({})
        assert result == b"{}"

    def test_dumps_list(self) -> None:
        """Test serializing a list."""
        import fionn

        result = fionn.dumps([1, 2, 3])
        assert result == b"[1,2,3]"

    def test_dumps_empty_list(self) -> None:
        """Test serializing an empty list."""
        import fionn

        result = fionn.dumps([])
        assert result == b"[]"

    def test_dumps_tuple(self) -> None:
        """Test serializing a tuple (as array)."""
        import fionn

        result = fionn.dumps((1, 2, 3))
        assert result == b"[1,2,3]"

    def test_dumps_string(self) -> None:
        """Test serializing a string."""
        import fionn

        result = fionn.dumps("hello")
        assert result == b'"hello"'

    def test_dumps_unicode_string(self) -> None:
        """Test serializing unicode strings."""
        import fionn

        result = fionn.dumps("中文")
        # Should produce valid UTF-8
        assert fionn.loads(result) == "中文"

    def test_dumps_integer(self) -> None:
        """Test serializing integers."""
        import fionn

        assert fionn.dumps(0) == b"0"
        assert fionn.dumps(123) == b"123"
        assert fionn.dumps(-42) == b"-42"

    def test_dumps_float(self) -> None:
        """Test serializing floats."""
        import fionn

        result = fionn.dumps(3.14)
        assert abs(float(result) - 3.14) < 0.001

    def test_dumps_boolean(self) -> None:
        """Test serializing booleans."""
        import fionn

        assert fionn.dumps(True) == b"true"
        assert fionn.dumps(False) == b"false"

    def test_dumps_none(self) -> None:
        """Test serializing None."""
        import fionn

        assert fionn.dumps(None) == b"null"


class TestDumpsOptions:
    """Test dumps() with various options."""

    def test_dumps_indent(self) -> None:
        """Test OPT_INDENT_2."""
        import fionn

        result = fionn.dumps({"a": 1}, option=fionn.OPT_INDENT_2)
        assert b"\n" in result
        assert b"  " in result

    def test_dumps_sort_keys(self) -> None:
        """Test OPT_SORT_KEYS."""
        import fionn

        result = fionn.dumps({"z": 1, "a": 2, "m": 3}, option=fionn.OPT_SORT_KEYS)
        assert result == b'{"a":2,"m":3,"z":1}'

    def test_dumps_append_newline(self) -> None:
        """Test OPT_APPEND_NEWLINE."""
        import fionn

        result = fionn.dumps({"a": 1}, option=fionn.OPT_APPEND_NEWLINE)
        assert result.endswith(b"\n")

    def test_dumps_combined_options(self) -> None:
        """Test combining multiple options."""
        import fionn

        result = fionn.dumps(
            {"b": 1, "a": 2},
            option=fionn.OPT_SORT_KEYS | fionn.OPT_APPEND_NEWLINE,
        )
        assert result == b'{"a":2,"b":1}\n'

    def test_dumps_non_str_keys(self) -> None:
        """Test OPT_NON_STR_KEYS."""
        import fionn

        result = fionn.dumps({1: "one", 2: "two"}, option=fionn.OPT_NON_STR_KEYS)
        # Keys should be converted to strings
        parsed = fionn.loads(result)
        assert "1" in parsed or 1 in parsed

    def test_dumps_strict_integer(self) -> None:
        """Test OPT_STRICT_INTEGER."""
        import fionn

        # 2^53 is too large
        with pytest.raises(fionn.JSONEncodeError):
            fionn.dumps(2**54, option=fionn.OPT_STRICT_INTEGER)


class TestDumpsDefault:
    """Test dumps() with default function."""

    def test_dumps_default_function(self) -> None:
        """Test using default function for custom types."""
        import fionn

        class Custom:
            def __init__(self, value: int) -> None:
                self.value = value

        def default(obj: Any) -> Any:
            if isinstance(obj, Custom):
                return {"custom": obj.value}
            raise TypeError(f"Cannot serialize {type(obj)}")

        result = fionn.dumps(Custom(42), default=default)
        assert fionn.loads(result) == {"custom": 42}

    def test_dumps_default_with_list(self) -> None:
        """Test default function with list containing custom types."""
        import fionn

        class Point:
            def __init__(self, x: int, y: int) -> None:
                self.x = x
                self.y = y

        def default(obj: Any) -> Any:
            if isinstance(obj, Point):
                return [obj.x, obj.y]
            raise TypeError

        result = fionn.dumps([Point(1, 2), Point(3, 4)], default=default)
        assert fionn.loads(result) == [[1, 2], [3, 4]]


class TestDumpsErrors:
    """Test dumps() error handling."""

    def test_dumps_non_serializable(self) -> None:
        """Test error on non-serializable type."""
        import fionn

        with pytest.raises(fionn.JSONEncodeError):
            fionn.dumps(object())

    def test_dumps_nan(self) -> None:
        """Test error on NaN."""
        import fionn

        with pytest.raises(fionn.JSONEncodeError):
            fionn.dumps(float("nan"))

    def test_dumps_infinity(self) -> None:
        """Test error on infinity."""
        import fionn

        with pytest.raises(fionn.JSONEncodeError):
            fionn.dumps(float("inf"))

    def test_dumps_non_str_keys_error(self) -> None:
        """Test error on non-string keys without OPT_NON_STR_KEYS."""
        import fionn

        with pytest.raises(fionn.JSONEncodeError):
            fionn.dumps({1: "one"})


class TestFragment:
    """Test Fragment class."""

    def test_fragment_bytes(self) -> None:
        """Test Fragment with bytes input."""
        import fionn

        fragment = fionn.Fragment(b'{"a": 1}')
        assert len(fragment) == 8
        assert bytes(fragment) == b'{"a": 1}'

    def test_fragment_str(self) -> None:
        """Test Fragment with str input."""
        import fionn

        fragment = fionn.Fragment('{"a": 1}')
        assert len(fragment) == 8

    def test_fragment_repr(self) -> None:
        """Test Fragment string representation."""
        import fionn

        fragment = fionn.Fragment(b'{"a": 1}')
        repr_str = repr(fragment)
        assert "Fragment" in repr_str

    def test_fragment_in_dumps(self) -> None:
        """Test Fragment embedded in dumps."""
        import fionn

        fragment = fionn.Fragment(b'{"nested": true}')
        result = fionn.dumps({"outer": fragment})
        parsed = fionn.loads(result)
        assert parsed["outer"]["nested"] is True

    def test_fragment_invalid_type(self) -> None:
        """Test Fragment with invalid input type."""
        import fionn

        with pytest.raises(TypeError):
            fionn.Fragment(123)  # type: ignore[arg-type]


class TestOptionFlags:
    """Test option flag values match orjson."""

    def test_flag_values(self) -> None:
        """Test flag values are powers of 2."""
        import fionn

        assert fionn.OPT_APPEND_NEWLINE == 1
        assert fionn.OPT_INDENT_2 == 2
        assert fionn.OPT_NAIVE_UTC == 4
        assert fionn.OPT_NON_STR_KEYS == 8
        assert fionn.OPT_OMIT_MICROSECONDS == 16
        assert fionn.OPT_PASSTHROUGH_DATACLASS == 32
        assert fionn.OPT_PASSTHROUGH_DATETIME == 64
        assert fionn.OPT_PASSTHROUGH_SUBCLASS == 128
        assert fionn.OPT_SERIALIZE_DATACLASS == 256
        assert fionn.OPT_SERIALIZE_NUMPY == 512
        assert fionn.OPT_SERIALIZE_UUID == 1024
        assert fionn.OPT_SORT_KEYS == 2048
        assert fionn.OPT_STRICT_INTEGER == 4096
        assert fionn.OPT_UTC_Z == 8192

    def test_flags_combinable(self) -> None:
        """Test flags can be combined with bitwise OR."""
        import fionn

        combined = fionn.OPT_INDENT_2 | fionn.OPT_SORT_KEYS
        assert combined == 2050

    def test_all_flags_unique(self) -> None:
        """Test all flags have unique values."""
        import fionn

        flags = [
            fionn.OPT_APPEND_NEWLINE,
            fionn.OPT_INDENT_2,
            fionn.OPT_NAIVE_UTC,
            fionn.OPT_NON_STR_KEYS,
            fionn.OPT_OMIT_MICROSECONDS,
            fionn.OPT_PASSTHROUGH_DATACLASS,
            fionn.OPT_PASSTHROUGH_DATETIME,
            fionn.OPT_PASSTHROUGH_SUBCLASS,
            fionn.OPT_SERIALIZE_DATACLASS,
            fionn.OPT_SERIALIZE_NUMPY,
            fionn.OPT_SERIALIZE_UUID,
            fionn.OPT_SORT_KEYS,
            fionn.OPT_STRICT_INTEGER,
            fionn.OPT_UTC_Z,
        ]
        assert len(flags) == len(set(flags))


class TestExceptions:
    """Test exception types."""

    def test_json_decode_error_is_value_error(self) -> None:
        """JSONDecodeError should be subclass of ValueError."""
        import fionn

        assert issubclass(fionn.JSONDecodeError, ValueError)

    def test_json_encode_error_is_type_error(self) -> None:
        """JSONEncodeError should be subclass of TypeError."""
        import fionn

        assert issubclass(fionn.JSONEncodeError, TypeError)

    def test_decode_error_message(self) -> None:
        """Test decode error has useful message."""
        import fionn

        try:
            fionn.loads(b"invalid")
            pytest.fail("Should have raised JSONDecodeError")
        except fionn.JSONDecodeError as e:
            assert "Invalid JSON" in str(e) or "invalid" in str(e).lower()

    def test_encode_error_message(self) -> None:
        """Test encode error has useful message."""
        import fionn

        try:
            fionn.dumps(object())
            pytest.fail("Should have raised JSONEncodeError")
        except fionn.JSONEncodeError as e:
            assert "serializable" in str(e).lower() or "type" in str(e).lower()


class TestRoundTrip:
    """Test round-trip serialization/deserialization."""

    @pytest.mark.parametrize(
        "value",
        [
            None,
            True,
            False,
            0,
            1,
            -1,
            123456789,
            0.0,
            3.14159,
            -2.71828,
            "",
            "hello",
            "hello\nworld",
            [],
            [1, 2, 3],
            {},
            {"a": 1},
            {"a": {"b": {"c": [1, 2, 3]}}},
            [{"a": 1}, {"b": 2}],
        ],
    )
    def test_round_trip(self, value: Any) -> None:
        """Test value survives round-trip."""
        import fionn

        serialized = fionn.dumps(value)
        deserialized = fionn.loads(serialized)
        assert deserialized == value

    def test_round_trip_float_precision(self) -> None:
        """Test float precision in round-trip."""
        import fionn

        value = 1.1234567890123456
        serialized = fionn.dumps(value)
        deserialized = fionn.loads(serialized)
        assert abs(deserialized - value) < 1e-10


class TestEdgeCases:
    """Test edge cases and special values."""

    def test_whitespace_handling(self) -> None:
        """Test handling of whitespace in input."""
        import fionn

        result = fionn.loads(b'  { "a" : 1 }  ')
        assert result == {"a": 1}

    def test_empty_input(self) -> None:
        """Test error on empty input."""
        import fionn

        with pytest.raises(fionn.JSONDecodeError):
            fionn.loads(b"")

    def test_whitespace_only(self) -> None:
        """Test error on whitespace-only input."""
        import fionn

        with pytest.raises(fionn.JSONDecodeError):
            fionn.loads(b"   ")

    def test_multiple_values(self) -> None:
        """Test error on multiple JSON values."""
        import fionn

        with pytest.raises(fionn.JSONDecodeError):
            fionn.loads(b'{"a": 1}{"b": 2}')

    def test_special_chars_in_string(self) -> None:
        """Test special characters in strings."""
        import fionn

        special = "tab\there\nnewline\rcarriage"
        result = fionn.loads(fionn.dumps(special))
        assert result == special
