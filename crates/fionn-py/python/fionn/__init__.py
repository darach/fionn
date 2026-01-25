# SPDX-License-Identifier: MIT OR Apache-2.0
"""fionn - Fast JSON library with SIMD acceleration.

Drop-in replacement for orjson with extended features:
- ISONL streaming (11.9x faster than fastest JSON parser)
- JSONL streaming with schema filtering
- Multi-format support (YAML, TOML, CSV, ISON, TOON)
- Gron path-based exploration
- Diff/Patch/Merge operations
- CRDT conflict-free merging
- Zero-copy tape API

Basic usage (orjson-compatible):
    >>> import fionn
    >>> fionn.loads(b'{"a": 1}')
    {'a': 1}
    >>> fionn.dumps({"a": 1})
    b'{"a":1}'

Extended features:
    >>> import fionn.ext as fx
    >>> for batch in fx.IsonlReader("data.isonl"):  # 11.9x faster
    ...     process(batch)
"""

# Re-export everything from the native module
from fionn.fionn import (
    # Option flags
    OPT_APPEND_NEWLINE,
    OPT_INDENT_2,
    OPT_NAIVE_UTC,
    OPT_NON_STR_KEYS,
    OPT_OMIT_MICROSECONDS,
    OPT_PASSTHROUGH_DATACLASS,
    OPT_PASSTHROUGH_DATETIME,
    OPT_PASSTHROUGH_SUBCLASS,
    OPT_SERIALIZE_DATACLASS,
    OPT_SERIALIZE_NUMPY,
    OPT_SERIALIZE_UUID,
    OPT_SORT_KEYS,
    OPT_STRICT_INTEGER,
    OPT_UTC_Z,
    # Fragment class
    Fragment,
    JSONDecodeError,
    # Exceptions
    JSONEncodeError,
    # Version
    __version__,
    dumps,
    # Core functions (orjson-compatible)
    loads,
)

__all__ = [
    # Option flags
    "OPT_APPEND_NEWLINE",
    "OPT_INDENT_2",
    "OPT_NAIVE_UTC",
    "OPT_NON_STR_KEYS",
    "OPT_OMIT_MICROSECONDS",
    "OPT_PASSTHROUGH_DATACLASS",
    "OPT_PASSTHROUGH_DATETIME",
    "OPT_PASSTHROUGH_SUBCLASS",
    "OPT_SERIALIZE_DATACLASS",
    "OPT_SERIALIZE_NUMPY",
    "OPT_SERIALIZE_UUID",
    "OPT_SORT_KEYS",
    "OPT_STRICT_INTEGER",
    "OPT_UTC_Z",
    # Fragment
    "Fragment",
    "JSONDecodeError",
    # Exceptions
    "JSONEncodeError",
    # Version
    "__version__",
    "dumps",
    # Core functions
    "loads",
]
