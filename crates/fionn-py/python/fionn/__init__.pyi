# SPDX-License-Identifier: MIT OR Apache-2.0
"""Type stubs for fionn - Fast JSON library with SIMD acceleration.

Drop-in replacement for orjson with extended features.
"""

from typing import Any, Callable, Final

# =============================================================================
# Version
# =============================================================================

__version__: str

# =============================================================================
# Option Flags (orjson-compatible)
# =============================================================================

OPT_APPEND_NEWLINE: Final[int]
OPT_INDENT_2: Final[int]
OPT_NAIVE_UTC: Final[int]
OPT_NON_STR_KEYS: Final[int]
OPT_OMIT_MICROSECONDS: Final[int]
OPT_PASSTHROUGH_DATACLASS: Final[int]
OPT_PASSTHROUGH_DATETIME: Final[int]
OPT_PASSTHROUGH_SUBCLASS: Final[int]
OPT_SERIALIZE_DATACLASS: Final[int]
OPT_SERIALIZE_NUMPY: Final[int]
OPT_SERIALIZE_UUID: Final[int]
OPT_SORT_KEYS: Final[int]
OPT_STRICT_INTEGER: Final[int]
OPT_UTC_Z: Final[int]

# =============================================================================
# Exceptions (orjson-compatible)
# =============================================================================

class JSONEncodeError(TypeError):
    """Exception raised when JSON encoding fails.

    Subclass of TypeError for orjson compatibility.
    """

    ...

class JSONDecodeError(ValueError):
    """Exception raised when JSON decoding fails.

    Subclass of ValueError for orjson compatibility.
    """

    ...

# =============================================================================
# Core Functions (orjson-compatible)
# =============================================================================

def loads(data: bytes | bytearray | memoryview | str, /) -> Any:
    """Parse JSON bytes to Python object.

    Drop-in replacement for orjson.loads().

    Args:
        data: JSON bytes, bytearray, memoryview, or str

    Returns:
        Python object (dict, list, str, int, float, bool, or None)

    Raises:
        JSONDecodeError: If the input is not valid JSON

    Examples:
        >>> fionn.loads(b'{"a": 1}')
        {'a': 1}
        >>> fionn.loads(b'[1, 2, 3]')
        [1, 2, 3]
    """
    ...

def dumps(
    obj: Any,
    /,
    default: Callable[[Any], Any] | None = None,
    option: int | None = None,
) -> bytes:
    r"""Serialize Python object to JSON bytes.

    Drop-in replacement for orjson.dumps().

    Args:
        obj: Python object to serialize
        default: Optional callable for non-serializable objects
        option: Optional bitwise OR of OPT_* flags

    Returns:
        JSON bytes

    Raises:
        JSONEncodeError: If the object cannot be serialized

    Examples:
        >>> fionn.dumps({"a": 1})
        b'{"a":1}'
        >>> fionn.dumps({"a": 1}, option=fionn.OPT_INDENT_2)
        b'{\\n  "a": 1\\n}'
    """
    ...

# =============================================================================
# Fragment Class (orjson-compatible)
# =============================================================================

class Fragment:
    """A fragment of pre-serialized JSON.

    Use Fragment to embed already-serialized JSON within a larger structure
    without the overhead of re-parsing and re-serializing.

    Examples:
        >>> fragment = fionn.Fragment(b'{"nested": true}')
        >>> fionn.dumps({"outer": fragment})
        b'{"outer":{"nested": true}}'
    """

    def __init__(self, data: bytes | str, /) -> None:
        """Create a new Fragment from JSON bytes or string.

        Args:
            data: Pre-serialized JSON as bytes or str

        Raises:
            TypeError: If data is not bytes or str
        """
        ...

    def __bytes__(self) -> bytes:
        """Get the raw JSON bytes."""
        ...

    def __repr__(self) -> str:
        """String representation."""
        ...

    def __len__(self) -> int:
        """Length of the fragment in bytes."""
        ...
