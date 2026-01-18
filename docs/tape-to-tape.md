# Tape-to-Tape Transformation

fionn transforms data between formats (JSON, YAML, TOML, CSV, ISON, TOON) via a unified tape representation. The tape stores structural tokens and enables format-agnostic operations without intermediate DOM construction.

## Unified Tape Format

All formats emit the same tape structure with format-specific markers:

```rust
pub enum FormatKind {
    Json, Toml, Yaml, Csv, Ison, Toon,
}

pub enum NodeType {
    // Universal (shared across formats)
    ObjectStart, ObjectEnd,
    ArrayStart, ArrayEnd,
    String(u16), Number(f64), Bool(bool), Null,
    SkipMarker,

    // Format-specific
    TomlTableStart, TomlInlineTableStart, TomlArrayTableStart,
    YamlDocumentStart, YamlAnchor(String), YamlAlias(String),
    CsvRowStart, CsvRowEnd, CsvHeaderRow,
    // ... ISON/TOON markers
}
```

The tape header identifies source format:

```rust
pub struct TapeHeader {
    format_kind: FormatKind,
    version: u8,
    encoding: TextEncoding,
}
```

## Original Syntax Preservation

Preserve original syntax for lossless round-trips:

```rust
pub enum OriginalSyntax {
    TomlDottedKey { full_path: String },
    TomlTripleQuotedString,
    YamlAnchor { name: String },
    YamlAlias { target: String },
    CsvQuotedValue { has_quotes: bool },
}
```

Convert YAML → JSON → YAML and anchors survive (stored in `OriginalSyntax`).

## Transformation Pipeline

### Same-Format (Direct Copy)

```rust
fn transform_same_format(&mut self) -> Result<()> {
    for node in self.input_tape.nodes() {
        if self.schema.matches_node(node) {
            self.output_tape.add_node(node.clone());
        } else {
            self.output_tape.add_node(SkipNode::skip_marker(node.depth));
        }
    }
}
```

Cost: O(n) nodes, no allocations for node copying.

### Cross-Format (With Translation)

```rust
fn transform_cross_format(&mut self) -> Result<()> {
    for input_node in self.input_tape.nodes() {
        let output_path = self.translate_path(&input_node)?;
        if self.schema.matches_path(&output_path) {
            let output_node = self.translate_node(input_node)?;
            self.output_tape.add_node(output_node);
        }
    }
}
```

Path translation:
- TOML `database.port` → JSON `$.database.port`
- CSV column `email` → JSON `$.email`
- YAML already uses JSONPath syntax

## Streaming Transformations

Process chunks with state carried across boundaries:

```rust
pub struct StreamingTransformer<R: BufRead> {
    reader: R,
    input_arena: Bump,
    output_arena: Bump,
    chunk_size: usize,
    schema: CompiledSchema,
}
```

| Format | Challenge | Strategy |
|--------|-----------|----------|
| TOML | Forward references | Two-pass or buffer |
| YAML | Multi-document | Emit document markers |
| CSV | Variable rows | Buffer until complete |

## Error Handling

```rust
pub struct TapeError {
    error_type: TapeErrorType,
    source_format: FormatKind,
    source_location: ErrorLocation,
    target_format: Option<FormatKind>,
    path_context: String,
    message: String,
}

pub enum TapeErrorType {
    ParseError,
    TransformationError,
    SchemaViolation,
    IncompatibleConversion,
}
```

Example error:
```
IncompatibleConversion at $.config.defaults
YAML anchor 'base' cannot be represented in JSON
Source: line 10, offset 5
```

## Strict Mode and Lossless Transforms

### Fidelity Levels

```rust
pub enum TransformFidelity {
    Strict,   // Error on any loss
    Warning,  // Warn and continue
    Lossy,    // Silent drop
}
```

### Information Loss Categories

| Category | Example | Recoverable |
|----------|---------|-------------|
| Structural | YAML anchor → JSON | No |
| Syntactic | TOML dotted key → nested | Yes |
| Type | YAML datetime → string | Partial |
| Comments | Any → JSON | No |

### Explicit Waivers

```rust
pub struct LossWaiver {
    path_pattern: String,
    category: LossCategory,
    reason: String,
}

let options = TransformOptions::new()
    .fidelity(TransformFidelity::Strict)
    .waive(LossWaiver {
        path_pattern: "$.metadata.*".into(),
        category: LossCategory::Comments,
        reason: "Metadata comments not needed".into(),
    });
```

Query syntax with waivers:
```javascript
$.config.*                      // Standard
$.metadata::allow-loss(comments) // Waived
$.critical::require-lossless    // Strict
```

### Compile-Time Verification

```rust
// Marker trait for lossless pairs
pub trait LosslessTransform<Target: Format>: Format {}

impl LosslessTransform<Json> for Json {}
impl LosslessTransform<Json> for Toml {}
impl LosslessTransform<Yaml> for Json {}
// YAML → JSON NOT implemented (anchors lost)

// Only compiles for lossless pairs
pub fn transform_lossless<S, T>(
    source: &SkipTape<S>,
    schema: &TypedSchema<S, T>,
) -> Result<SkipTape<T>>
where
    S: Format + LosslessTransform<T>,
    T: Format,
```

## Performance

| Operation | Strategy |
|-----------|----------|
| Path translation | Pre-compiled hash lookup |
| Node iteration | Sequential (cache-friendly) |
| String copying | Arena-to-arena bulk |
| Format detection | SIMD vector compare |

Reuse buffers between transformations:

```rust
pub struct TransformBuffer {
    path_buffer: Vec<ParsedPath>,
    value_buffer: Vec<u8>,
}

impl TransformBuffer {
    pub fn clear(&mut self) {
        self.path_buffer.clear();
        self.value_buffer.clear();
    }
}
```

## Implementation Status

Tape-to-tape transformation implemented in:
- `crates/fionn-simd/src/transform/`
- `crates/fionn-core/src/tape_source.rs`

All format parsers emit unified tape. Cross-format diff, patch, merge operate on tape directly without DOM construction.
