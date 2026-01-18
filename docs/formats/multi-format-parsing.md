# Multi-Format SIMD Parsing

fionn parses JSON, YAML, TOML, CSV, ISON, and TOON with SIMD acceleration. All formats map to JSON normal form via skip tape, enabling format-agnostic diff, patch, merge, and query operations.

## Architecture

### Tape Representation

All formats emit a unified tape structure:

```rust
pub struct SkipNode<'a> {
    node_type: NodeType,
    depth: u8,
    format_kind: FormatKind,
    skip_offset: usize,        // O(1) skip to next sibling
    original_data: Option<OriginalSyntax>,  // Lossless round-trip
}

pub enum FormatKind {
    Json, Yaml, Toml, Csv, Ison, Toon
}
```

The tape stores structural tokens (object/array boundaries) and value offsets. Skip indices enable O(1) value skipping—jump directly from `[` to matching `]` without parsing contents.

### JSON Normal Form

Every format maps to equivalent JSON semantics:

| Source Concept | JSON Equivalent |
|----------------|-----------------|
| YAML anchors/aliases | Inlined values |
| TOML dotted keys | Nested objects |
| CSV rows | Array of objects |
| ISON table blocks | Array of objects |
| TOON tabular arrays | Array of objects |

Path queries use JSONPath syntax regardless of source format. `$.users[*].name` works identically on YAML, TOML, or CSV input.

### SIMD Strategies

Three strategies, selected by input size and CPU features:

| Strategy | Use Case | Technique |
|----------|----------|-----------|
| Langdale-Lemire | String-heavy | XOR prefix for quote state |
| JSONSki | Deep nesting | Bracket counting |
| Scalar | <1KB files | Byte iteration |

```rust
pub fn select_strategy(input_size: usize) -> SkipStrategy {
    if input_size < 1024 { return SkipStrategy::Scalar; }

    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") && input_size >= 4096 {
        return SkipStrategy::Avx2;
    }

    SkipStrategy::JsonSki
}
```

---

## Format Reference

### CSV

**Implementation**: `crates/fionn-simd/src/formats/csv.rs`

#### Structural Characteristics

| Element | JSON Equivalent | Delimiter |
|---------|-----------------|-----------|
| Header row | Field names | `,` (configurable) |
| Data row | Object in array | `,` |
| Quoted field | String value | `"` |

#### SIMD Opportunities

1. **Delimiter scan**: `_mm_cmpeq_epi8(chunk, comma_vec)` finds field boundaries
2. **Quote tracking**: XOR prefix detects in-string state
3. **Newline detection**: Identifies row boundaries (handle `\r\n` and `\n`)

#### JSON Normal Form

```csv
id,name,active
1,Alice,true
2,Bob,false
```

Maps to:

```json
[
  {"id": 1, "name": "Alice", "active": true},
  {"id": 2, "name": "Bob", "active": false}
]
```

**Path mapping**: `$[*].id`, `$[*].name`, `$[*].active`

#### Edge Cases

- **Quoted newlines**: Newlines inside `"..."` don't end rows
- **BOM handling**: Skip UTF-8 BOM (`EF BB BF`) at file start
- **Empty fields**: `a,,c` produces `{"a": "a", "b": null, "c": "c"}`
- **Escape sequences**: `""` inside quotes becomes single `"`

---

### YAML

**Implementation**: `crates/fionn-simd/src/formats/yaml.rs`

#### Structural Characteristics

| Element | JSON Equivalent | Indicator |
|---------|-----------------|-----------|
| Mapping | Object | Key-value pairs |
| Sequence | Array | `-` prefix |
| Anchor | Value | `&name` |
| Alias | Reference | `*name` |
| Merge key | Object spread | `<<:` |

YAML uses indentation (2 spaces default) for structure. Tabs are invalid.

#### SIMD Opportunities

1. **Indent counting**: Leading space count via `_mm_cmpeq_epi8`
2. **Anchor detection**: `&` character outside strings
3. **Alias resolution**: `*` character lookup
4. **Colon detection**: Key-value separator

```rust
fn count_indent_simd(chunk: &[u8; 64]) -> usize {
    let space_vec = _mm512_set1_epi8(b' ');
    let chunk_vec = _mm512_loadu_si512(chunk.as_ptr());
    let eq_mask = _mm512_cmpeq_epi8_mask(chunk_vec, space_vec);
    eq_mask.trailing_ones() as usize
}
```

#### JSON Normal Form

```yaml
users:
  - &alice
    name: Alice
    role: admin
  - name: Bob
    manager: *alice
```

Maps to:

```json
{
  "users": [
    {"name": "Alice", "role": "admin"},
    {"name": "Bob", "manager": {"name": "Alice", "role": "admin"}}
  ]
}
```

**Path mapping**: `$.users[*].name`, `$.users[*].role`

#### Reference Handling

Three strategies for anchors/aliases:

| Strategy | Behavior | Use Case |
|----------|----------|----------|
| Inline | Expand at parse time | Small documents |
| Lazy | Store reference, resolve on access | Large documents |
| Preserve | Keep `$ref` marker | Round-trip fidelity |

#### Edge Cases

- **Circular references**: Detect and error (or emit marker)
- **Multi-document**: `---` separator creates document array
- **Merge keys**: `<<: *base` spreads anchor's keys into current object
- **Flow style**: `{a: 1, b: 2}` inline syntax

---

### TOML

**Implementation**: `crates/fionn-simd/src/formats/toml.rs`

#### Structural Characteristics

| Element | JSON Equivalent | Syntax |
|---------|-----------------|--------|
| Table | Nested object | `[table.name]` |
| Array of tables | Array of objects | `[[table.name]]` |
| Inline table | Object | `{key = val}` |
| Dotted key | Nested path | `a.b.c = val` |

#### SIMD Opportunities

1. **Bracket detection**: `[` for table headers
2. **Equals detection**: `=` for key-value pairs
3. **Dot detection**: `.` for path components
4. **Quote detection**: Basic (`"`), literal (`'`), multi-line (`"""`)

```rust
fn skip_toml_table(input: &[u8]) -> SkipResult {
    let open_bits = (!instring) & bracket_bits(chunk, b'[');
    let close_bits = (!instring) & bracket_bits(chunk, b']');
    bracket_depth += open_count - close_count;
    // Table ends when brackets balance
}
```

#### JSON Normal Form

```toml
[database]
server = "localhost"
port = 5432

[[products]]
name = "Hammer"

[[products]]
name = "Nail"
```

Maps to:

```json
{
  "database": {"server": "localhost", "port": 5432},
  "products": [
    {"name": "Hammer"},
    {"name": "Nail"}
  ]
}
```

**Path mapping**: `$.database.server`, `$.products[*].name`

#### Edge Cases

- **Dotted key vs table conflict**: `a.b = 1` and `[a.b]` are mutually exclusive
- **Forward references**: Values may reference later-defined keys
- **Triple-quoted strings**: Multi-line strings with `"""`

---

### ISON

**Implementation**: `crates/fionn-simd/src/formats/ison.rs`
**Feature flag**: `ison`

ISON (Interchange Simple Object Notation) targets LLM workflows. Block-based, whitespace-delimited, 30-70% fewer tokens than JSON.

#### Structural Characteristics

| Element | JSON Equivalent | Syntax |
|---------|-----------------|--------|
| Table block | Array of objects | `table.name` header |
| Object block | Object | `object.name` header |
| Data row | Object fields | Space-separated values |
| Reference | Link | `:type:id` |

#### SIMD Opportunities

1. **Block header detection**: `table.` or `object.` at line start
2. **Space delimiter**: Field boundaries outside quotes
3. **Colon detection**: Reference patterns
4. **Newline detection**: Row boundaries

#### JSON Normal Form

```ison
table.users
id:int name:string email active:bool
1 Alice alice@example.com true
2 Bob bob@example.com false
```

Maps to:

```json
{
  "users": [
    {"id": 1, "name": "Alice", "email": "alice@example.com", "active": true},
    {"id": 2, "name": "Bob", "email": "bob@example.com", "active": false}
  ]
}
```

**Path mapping**: `$.users[*].id`, `$.users[*].name`

#### References

```ison
table.orders
id:int user_id:user amount:float
1 :1 99.99
```

Reference `:1` points to user with id 1. Resolution strategies:
- **Preserve**: Keep `{"$ref": "users", "id": 1}`
- **Inline**: Resolve at parse time
- **Lazy**: Resolve on access

#### ISONL Streaming

Pipe-delimited format for streaming:

```
table.events|id:int|type:string|1|click
table.events|id:int|type:string|2|view
```

Each line is self-contained—parse without buffering.

---

### TOON

**Implementation**: `crates/fionn-simd/src/formats/toon.rs`
**Feature flag**: `toon`

TOON (Token-Oriented Object Notation) uses indentation and explicit array lengths. 30-60% fewer tokens than JSON.

#### Structural Characteristics

| Element | JSON Equivalent | Syntax |
|---------|-----------------|--------|
| Object | Object | Indented key-value |
| Primitive array | Array | `[N]: a,b,c` |
| Tabular array | Array of objects | `[N]{fields}:` |
| Mixed array | Array | `- item` prefix |
| Dotted path | Nested object | `a.b.c: val` |

Delimiters: comma (default), tab, or pipe. Declared in array header.

#### SIMD Opportunities

1. **Indent counting**: Leading spaces determine depth
2. **Array header detection**: `[N]` or `[N]{fields}`
3. **Delimiter detection**: Active delimiter per scope
4. **Colon detection**: Key-value separator

```rust
struct ToonIndentTracker {
    indent_size: usize,
    depth_stack: Vec<usize>,
    strict_mode: bool,
}
```

#### JSON Normal Form

```toon
users[2]{id,name,role}:
  1,Alice,admin
  2,Bob,user
```

Maps to:

```json
{
  "users": [
    {"id": 1, "name": "Alice", "role": "admin"},
    {"id": 2, "name": "Bob", "role": "user"}
  ]
}
```

**Path mapping**: `$.users[*].id`, `$.users[*].name`

#### Key Folding

```toon
user.profile.name: Alice
user.profile.email: alice@example.com
```

Equivalent to nested structure. Preserve folding preference for round-trip.

#### Delimiter Scope

```toon
addresses[2|]{street,city}:
  123 Main St, Apt 4|Anytown
  456 Oak Ave|Othertown
```

Pipe delimiter allows commas in values. Track active delimiter per array scope.

---

## Streaming

### Challenges by Format

| Format | Challenge | Solution |
|--------|-----------|----------|
| JSON | None | Single-pass |
| YAML | Anchors before use | Two-pass or lazy |
| TOML | Forward references | Two-pass or arena buffer |
| CSV | None | Single-pass |
| ISON | Block references | Lazy resolution |
| TOON | Indent tracking | Buffer partial lines |

### Chunked Processing

Process 64-byte chunks with state carried across boundaries:

```rust
struct StreamState {
    prev_instring: u64,
    prev_escaped: u64,
    depth: usize,
    partial_line: Vec<u8>,
}
```

### Multi-Document

YAML `---` and JSONL newlines produce document arrays. Emit `DocumentStart` markers for each.

---

## Error Handling

Unified error format with format-specific context:

```rust
pub struct TapeError {
    error_type: TapeErrorType,
    source_format: FormatKind,
    source_location: ErrorLocation,
    path_context: String,
    message: String,
    original_data: Option<String>,
}
```

Format-specific errors:

| Format | Error Type | Example |
|--------|------------|---------|
| CSV | Field count mismatch | Row has 3 fields, header has 4 |
| YAML | Circular reference | Anchor references itself |
| TOML | Table redefined | `[a.b]` defined twice |
| ISON | Undefined reference | `:99` points to nonexistent id |
| TOON | Array length mismatch | `[3]` but only 2 items |

---

## Round-Trip Fidelity

Preserve original syntax for lossless conversion:

```rust
pub enum OriginalSyntax {
    YamlAnchor { name: String },
    YamlFlowStyle,
    TomlDottedKey { full_key: String },
    TomlTripleQuotedString,
    IsonReference { kind: ReferenceKind },
    ToonFoldedKey { path: String },
    ToonArrayHeader { header_text: String },
}
```

Convert YAML → JSON → YAML and preserve anchors, flow style, comments (where possible).

---

## Implementation Status

All formats implemented in `crates/fionn-simd/src/formats/`:
Feature flags: `ison` and `toon` gate experimental formats.

---

## References

### Specifications
- [YAML 1.2](https://yaml.org/spec/1.2.2/)
- [TOML v1.0.0](https://toml.io/en/v1.0.0)
- [RFC 4180 (CSV)](https://datatracker.ietf.org/doc/html/rfc4180)
- [ISON Spec](https://www.ison.dev/spec)
- [TOON Format](https://toonformat.dev/)

### Research
- Langdale & Lemire: "Parsing Gigabytes of JSON per Second"
- JSONSki: "Streaming Semi-structured Data with Bit-Parallel Fast-Forwarding"
