# SPDX-License-Identifier: MIT OR Apache-2.0
"""Type stubs for fionn.ext - Extended features.

JSONL/ISONL streaming, multi-format parsing, gron, diff, CRDT, and tape API.
"""

from collections.abc import Iterator
from enum import Enum
from typing import Any

# =============================================================================
# JSONL Streaming
# =============================================================================

class JsonlReader:
    """JSONL reader with schema filtering and batch processing.

    Performance: matches sonic-rs (fastest JSON parser).

    Examples:
        >>> for batch in fx.JsonlReader("data.jsonl", batch_size=1000):
        ...     for record in batch:
        ...         process(record)
    """

    def __init__(
        self,
        path: str,
        schema: list[str] | None = None,
        batch_size: int = 1000,
    ) -> None:
        """Create a new JSONL reader.

        Args:
            path: Path to JSONL file
            schema: Optional list of field names to extract (faster if specified)
            batch_size: Number of records per batch (default: 1000)
        """
        ...

    def __iter__(self) -> Iterator[list[dict[str, Any]]]:
        """Iterate over batches."""
        ...

    def __next__(self) -> list[dict[str, Any]]:
        """Get next batch."""
        ...

class JsonlWriter:
    """JSONL writer for streaming output.

    Examples:
        >>> with fx.JsonlWriter("output.jsonl") as writer:
        ...     writer.write({"id": 1, "name": "Alice"})
    """

    def __init__(self, path: str) -> None:
        """Create a new JSONL writer.

        Args:
            path: Path to output file
        """
        ...

    def write(self, obj: dict[str, Any]) -> None:
        """Write a record to the JSONL file."""
        ...

    def close(self) -> None:
        """Close the writer."""
        ...

    def __enter__(self) -> JsonlWriter:
        """Context manager entry."""
        ...

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: Any,
    ) -> bool:
        """Context manager exit."""
        ...

def parse_jsonl(
    data: str,
    schema: list[str] | None = None,
) -> list[dict[str, Any]]:
    """Parse JSONL string to list of Python objects.

    Args:
        data: JSONL string
        schema: Optional list of field names to extract

    Returns:
        List of parsed records
    """
    ...

def to_jsonl(data: list[dict[str, Any]]) -> str:
    """Convert list of Python objects to JSONL string.

    Args:
        data: List of dicts to serialize

    Returns:
        JSONL string
    """
    ...

# =============================================================================
# ISONL Streaming (11.9x faster than JSONL - KEY DIFFERENTIATOR)
# =============================================================================

class IsonlReader:
    """ISONL reader with SIMD-accelerated parsing.

    **11.9x faster than JSONL** - fionn's key differentiator.

    Performance (vs sonic-rs baseline):
    - Cycles: 355M vs 4,226M (11.9x fewer)
    - IPC: 5.23 vs 3.39 (54% better)
    - Cache misses: 11.3K vs 77.7K (6.9x fewer)
    - Branch misses: 32.7K vs 654K (20x fewer)

    Examples:
        >>> for batch in fx.IsonlReader("data.isonl"):
        ...     for record in batch:
        ...         process(record)
    """

    def __init__(
        self,
        path: str,
        fields: list[str] | None = None,
        batch_size: int = 1000,
    ) -> None:
        """Create a new ISONL reader.

        Args:
            path: Path to ISONL file
            fields: Optional list of field names to extract (fastest if specified)
            batch_size: Number of records per batch (default: 1000)
        """
        ...

    def __iter__(self) -> Iterator[list[dict[str, Any]]]:
        """Iterate over batches."""
        ...

    def __next__(self) -> list[dict[str, Any]]:
        """Get next batch."""
        ...

class IsonlWriter:
    """ISONL writer for streaming output.

    Examples:
        >>> with fx.IsonlWriter("output.isonl", table="users",
        ...                     schema=["id:int", "name:string"]) as writer:
        ...     writer.write({"id": 1, "name": "Alice"})
    """

    def __init__(
        self,
        path: str,
        table: str,
        schema: list[str],
    ) -> None:
        """Create a new ISONL writer.

        Args:
            path: Path to output file
            table: Table name (e.g., "users", "events")
            schema: Field definitions (e.g., ["id:int", "name:string"])
        """
        ...

    def write(self, obj: dict[str, Any]) -> None:
        """Write a record to the ISONL file."""
        ...

    def close(self) -> None:
        """Close the writer."""
        ...

    def __enter__(self) -> IsonlWriter:
        """Context manager entry."""
        ...

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: Any,
    ) -> bool:
        """Context manager exit."""
        ...

def parse_isonl(
    data: str,
    fields: list[str] | None = None,
) -> list[dict[str, Any]]:
    """Parse ISONL string to list of Python dicts.

    Args:
        data: ISONL string
        fields: Optional list of field names to extract

    Returns:
        List of parsed records
    """
    ...

def to_isonl(
    data: list[dict[str, Any]],
    table: str,
    schema: list[str],
) -> str:
    """Convert list of Python dicts to ISONL string.

    Args:
        data: List of dicts to serialize
        table: Table name
        schema: Field definitions

    Returns:
        ISONL string
    """
    ...

def jsonl_to_isonl(
    input_path: str,
    output_path: str,
    table: str,
    schema: list[str] | None = None,
    infer_schema: bool = False,
) -> int:
    """Convert JSONL file to ISONL file for 11.9x speedup on repeated reads.

    Args:
        input_path: Path to input JSONL file
        output_path: Path to output ISONL file
        table: Table name
        schema: Optional explicit schema (e.g., ["id:int", "name:string"])
        infer_schema: If True, infer schema from first line

    Returns:
        Number of records converted
    """
    ...

# =============================================================================
# Multi-Format Parsing
# =============================================================================

def parse(data: str) -> tuple[Any, str]:
    """Auto-detect format and parse.

    Returns:
        Tuple of (parsed data, detected format)
    """
    ...

def parse_yaml(data: str) -> Any:
    """Parse YAML string to Python object."""
    ...

def parse_toml(data: str) -> Any:
    """Parse TOML string to Python object."""
    ...

def parse_csv(data: str, has_header: bool = True) -> list[dict[str, Any]]:
    """Parse CSV string to list of dicts."""
    ...

def parse_ison(data: str) -> Any:
    """Parse ISON string to Python object."""
    ...

def parse_toon(data: str) -> Any:
    """Parse TOON string to Python object."""
    ...

def to_yaml(obj: Any) -> str:
    """Convert Python object to YAML string."""
    ...

def to_toml(obj: Any) -> str:
    """Convert Python object to TOML string."""
    ...

def to_csv(obj: list[dict[str, Any]]) -> str:
    """Convert list of dicts to CSV string."""
    ...

def to_ison(obj: Any) -> str:
    """Convert Python object to ISON string."""
    ...

def to_toon(obj: Any) -> str:
    """Convert Python object to TOON string."""
    ...

# =============================================================================
# Gron Operations
# =============================================================================

def gron(json: str) -> str:
    r"""Convert JSON to gron format.

    Examples:
        >>> fx.gron('{"a": {"b": 1}}')
        'json = {};\\njson.a = {};\\njson.a.b = 1;\\n'
    """
    ...

def ungron(gron_str: str) -> Any:
    """Convert gron format back to JSON."""
    ...

def gron_query(json: str, query: str) -> Any:
    """Query JSON using gron-style paths."""
    ...

# =============================================================================
# Diff/Patch/Merge
# =============================================================================

def diff(source: Any, target: Any) -> list[dict[str, Any]]:
    """Compute diff between two documents (RFC 6902 JSON Patch)."""
    ...

def patch(document: Any, patch_ops: list[dict[str, Any]]) -> Any:
    """Apply patch to document."""
    ...

def merge(base: Any, overlay: Any) -> Any:
    """Merge two documents (RFC 7396 Merge Patch)."""
    ...

def deep_merge(base: Any, overlay: Any) -> Any:
    """Deep merge two documents."""
    ...

def three_way_merge(base: Any, ours: Any, theirs: Any) -> Any:
    """Three-way merge."""
    ...

# =============================================================================
# CRDT Operations
# =============================================================================

class MergeStrategy(Enum):
    """Merge strategy for CRDT operations."""

    LastWriterWins = ...
    Additive = ...
    Max = ...
    Min = ...
    Concat = ...
    Union = ...

class CrdtDocument:
    """A CRDT-enabled document for conflict-free merging.

    Examples:
        >>> doc = fx.CrdtDocument({"counter": 0}, replica_id="node-1")
        >>> doc.set("counter", 10)
    """

    def __init__(self, initial_data: Any, replica_id: str) -> None:
        """Create a new CRDT document.

        Args:
            initial_data: Initial document data
            replica_id: Unique identifier for this replica
        """
        ...

    def set(self, path: str, value: Any) -> None:
        """Set a value at the given path."""
        ...

    def delete(self, path: str) -> None:
        """Delete a value at the given path."""
        ...

    def merge(self, other: CrdtDocument) -> list[dict[str, Any]]:
        """Merge with another document.

        Returns:
            List of conflicts (if any)
        """
        ...

    def set_strategy(self, path_pattern: str, strategy: MergeStrategy) -> None:
        """Set merge strategy for a path pattern."""
        ...

    def export_state(self) -> dict[str, Any]:
        """Export full state."""
        ...

    def export_delta(self, since_version: int) -> dict[str, Any]:
        """Export delta since a version."""
        ...

    @property
    def value(self) -> Any:
        """Get current value."""
        ...

# =============================================================================
# Pipeline Processing
# =============================================================================

class Pipeline:
    """Stream processing pipeline for JSONL/ISONL.

    Examples:
        >>> pipeline = fx.Pipeline()
        >>> pipeline.filter(lambda x: x["active"])
        >>> pipeline.map(lambda x: {"id": x["id"]})
        >>> pipeline.process_isonl("input.isonl", "output.isonl")
    """

    def __init__(self) -> None:
        """Create a new pipeline."""
        ...

    def filter(self, predicate: Any) -> None:
        """Add a filter stage."""
        ...

    def map(self, transform: Any) -> None:
        """Add a map stage."""
        ...

    def process_jsonl(self, input_path: str, output_path: str) -> int:
        """Process JSONL file through pipeline.

        Returns:
            Number of records processed
        """
        ...

    def process_isonl(self, input_path: str, output_path: str) -> int:
        """Process ISONL file through pipeline (11.9x faster than JSONL).

        Returns:
            Number of records processed
        """
        ...

    def process(
        self,
        input_path: str,
        input_format: str,
        output_path: str,
        output_format: str,
        output_schema: list[str] | None = None,
    ) -> int:
        """Process with format conversion.

        Returns:
            Number of records processed
        """
        ...

# =============================================================================
# Advanced Tape API
# =============================================================================

class Schema:
    """Schema for filtered parsing."""

    def __init__(self, fields: list[str]) -> None:
        """Create a schema.

        Args:
            fields: List of field names to include
        """
        ...

class Tape:
    """Zero-copy tape representation of JSON.

    Parse once, access fields lazily without full materialization.

    Examples:
        >>> tape = fx.Tape.parse(huge_json_bytes)
        >>> name = tape.get("users.0.name")
    """

    @staticmethod
    def parse(data: bytes, schema: Schema | None = None) -> Tape:
        """Parse JSON bytes to tape.

        Args:
            data: JSON bytes
            schema: Optional schema for filtered parsing
        """
        ...

    def get(self, path: str) -> Any:
        """Get value at path without full materialization."""
        ...

    def query(self, query: str) -> list[Any]:
        """Query using JSONPath-like syntax."""
        ...

    def to_object(self) -> Any:
        """Convert to Python object (full materialization)."""
        ...

class TapePool:
    """Pool for reusing tape allocations.

    Examples:
        >>> pool = fx.TapePool(strategy="lru", max_tapes=100)
        >>> tape = pool.parse(json_bytes)
    """

    def __init__(self, strategy: str = "lru", max_tapes: int = 100) -> None:
        """Create a tape pool.

        Args:
            strategy: Eviction strategy ("lru")
            max_tapes: Maximum number of tapes to pool
        """
        ...

    def parse(self, data: bytes) -> Tape:
        """Parse JSON bytes using pooled tape."""
        ...
