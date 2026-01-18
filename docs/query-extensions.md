# Query Extensions: Kind and Context Filtering

fionn extends JSONPath (RFC 9535) with kind predicates and context-aware filtering. Select nodes by type, filter comments, extract format-specific structures.

## Problem

JSONPath lacks type filtering. You cannot select "all strings" or "only YAML anchors" without runtime inspection. fionn solves this with `::` predicate syntax.

## Solution

```javascript
$.users[*].name::string       // String values only
$..::comment                  // All comments (any format)
$..::yaml:anchor              // YAML anchor definitions
$.data[*]::outside-comment    // Values outside comments
```

## Syntax Reference

### Grammar (EBNF)

```ebnf
query           = path [ "::" predicate ] ;
path            = root ( '.' segment )* ;
root            = "$" | "" ;
segment         = field | index | wildcard | filter ;

predicate       = kind-filter | context-filter | format-filter ;
kind-filter     = value-kind | structural-kind ;
value-kind      = "string" | "number" | "boolean" | "null" | "scalar" ;
structural-kind = "object" | "array" | "comment" | "key" ;

context-filter  = "in-string" | "outside-string"
                | "in-comment" | "outside-comment"
                | "quoted" | "unquoted" | "escaped" ;

format-filter   = namespace ":" specific ;
namespace       = "toml" | "yaml" | "csv" | "ison" | "toon" ;
```

### Value Type Predicates

| Predicate | Matches | Example |
|-----------|---------|---------|
| `::string` | String values | `$.name::string` |
| `::number` | Numbers | `$.count::number` |
| `::boolean` | true/false | `$.active::boolean` |
| `::null` | Null values | `$.optional::null` |
| `::scalar` | All primitives | `$..::scalar` |

### Structural Predicates

| Predicate | Matches | Example |
|-----------|---------|---------|
| `::object` | Objects/mappings | `$..::object` |
| `::array` | Arrays/sequences | `$..::array` |
| `::comment` | Comments | `$..::comment` |
| `::key` | Key names | `$..::key` |

### Context Predicates

| Predicate | Matches | Use Case |
|-----------|---------|----------|
| `::in-string` | Inside strings | Debug string parsing |
| `::outside-string` | Outside strings | Skip string content |
| `::in-comment` | Inside comments | Extract documentation |
| `::outside-comment` | Active content | Filter out comments |
| `::quoted` | Quoted values | Find quoted CSV fields |
| `::escaped` | Escape sequences | Security analysis |

### Format-Specific Predicates

Use `::format:predicate` syntax for format-specific structures:

**TOML**
```javascript
$..::toml:section-header    // [table] headers
$..::toml:dotted-key        // a.b.c keys
$..::toml:inline-table      // {k=v} syntax
$..::toml:array-table       // [[array]] syntax
```

**YAML**
```javascript
$..::yaml:anchor            // &anchor definitions
$..::yaml:alias             // *alias references
$..::yaml:merge-key         // <<: merges
$..::yaml:tag               // !tag annotations
$..::yaml:document          // --- separators
```

**CSV**
```javascript
$..::csv:header             // Header row
$..::csv:data-row           // Data rows
$..::csv:quoted             // Quoted fields
```

**ISON** (feature-gated)
```javascript
$..::ison:table             // table.name blocks
$..::ison:object            // object.name blocks
$..::ison:reference         // :type:id refs
```

**TOON** (feature-gated)
```javascript
$..::toon:array-header      // [N]{fields} headers
$..::toon:tabular-row       // Tabular data rows
$..::toon:folded-key        // a.b.c folded paths
```

### Combining Predicates

```javascript
// Path + kind
$.users[*].name::string

// Path + context
$.config.*::outside-comment

// Multiple kinds (union)
$..::(string|number)

// Negation
$..::!comment

// Multiple predicates (AND)
$.data.*::string::outside-comment
```

## Examples

### Extract All String Values

```javascript
$.users[*].name::string
```

Input:
```json
{"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}
```

Output:
```json
["Alice", "Bob"]
```

### Extract Comments from Config

```javascript
$..::comment
```

Input (TOML):
```toml
# Database config
host = "localhost"  # Local dev
port = 5432
```

Output:
```json
["Database config", "Local dev"]
```

### Filter Active Config (Skip Comments)

```javascript
$..::outside-comment
```

Input (TOML):
```toml
name = "production"
# name = "development"
host = "example.com"
```

Output:
```json
{"name": "production", "host": "example.com"}
```

### Find YAML Anchors

```javascript
$..::yaml:anchor
```

Input:
```yaml
defaults: &base
  timeout: 30
config:
  <<: *base
  name: prod
```

Output:
```json
{"defaults": {"timeout": 30}}
```

### Find Quoted CSV Fields

```javascript
$..::csv:quoted
```

Input:
```csv
name,description
item1,"Has, comma"
item2,No comma
```

Output:
```json
["Has, comma"]
```

### Select Numbers Outside Comments

```javascript
$.config.*::number::outside-comment
```

Input (TOML):
```toml
[config]
timeout = 30  # seconds
# retries = 5
port = 8080
```

Output:
```json
[30, 8080]
```

## Architecture

### NodeKind Enum

```rust
pub enum NodeKind {
    // Value types
    String, Number, Boolean, Null, Scalar,

    // Structural types
    Object, Array, Key, Comment,

    // Semantic categories (cross-format)
    Reference,   // YAML alias, ISON reference
    Definition,  // YAML anchor, ISON table
    Header,      // CSV header, TOON array header

    // Format-specific (feature-gated)
    #[cfg(feature = "toml")]
    Toml(TomlKind),
    #[cfg(feature = "yaml")]
    Yaml(YamlKind),
    #[cfg(feature = "csv")]
    Csv(CsvKind),
    #[cfg(feature = "ison")]
    Ison(IsonKind),
    #[cfg(feature = "toon")]
    Toon(ToonKind),
}
```

### SkipNode Extension

```rust
pub struct SkipNode {
    pub node_type: NodeType,
    pub depth: u8,
    pub format_kind: FormatKind,

    // Kind and context tracking
    pub node_kind: NodeKind,
    pub parsing_context: Option<ParsingContext>,
}
```

### SIMD Detection

Context-aware filtering uses SIMD for O(1) boundary detection:

```rust
pub struct KindMask {
    pub string_mask: u64,   // 1 = in string
    pub comment_mask: u64,  // 1 = in comment
    pub quote_mask: u64,    // 1 = quoted
    pub escape_mask: u64,   // 1 = escaped
}
```

Process 64-byte chunks with vectorized comparison:

```rust
fn detect_context_simd(chunk: &[u8; 64]) -> KindMask {
    let quote_vec = _mm512_set1_epi8(b'"');
    let chunk_vec = _mm512_loadu_si512(chunk.as_ptr());
    let quote_bits = _mm512_cmpeq_epi8_mask(chunk_vec, quote_vec);
    // XOR prefix for quote state tracking
    // ...
}
```

## Predicate Namespacing

### Universal Predicates (Always Available)

```javascript
::string, ::number, ::boolean, ::null, ::scalar
::object, ::array, ::key, ::comment
::in-string, ::outside-string, ::in-comment, ::outside-comment
```

### Semantic Categories (Cross-Format)

```javascript
::reference    // Any reference (YAML alias, ISON :id)
::definition   // Any definition (YAML anchor, ISON table)
::header       // Any header (CSV, TOON array)
::row          // Any data row
```

### Namespaced Specifics (Feature-Gated)

```javascript
::yaml:anchor  // YAML-specific
::toml:section-header  // TOML-specific
::csv:quoted   // CSV-specific
```

### Inheritance

Generic predicates match all specifics:

```javascript
$..::reference    // Matches YAML aliases AND ISON references
$..::yaml:alias   // Matches only YAML aliases
```

## Performance

### Fast Kind Rejection

Kind predicates enable O(1) rejection before path matching:

```javascript
$..::string    // Rejected at O(1) if node isn't string
$.data.*       // Requires path parsing first (slower)
```

### SIMD Acceleration

Context predicates use 64-byte SIMD chunk processing:

```javascript
$..::outside-comment  // SIMD boundary detection
```

### Zero-Allocation

Kind filters at parse time skip node allocation:

```javascript
$..::!comment  // Comments excluded from tape
```

## Error Handling

### Unknown Predicate

```
Error: Unknown predicate '::foobar'
  at query: $.data[*]::foobar
                      ^^^^^^^
Hint: Did you mean '::string', '::number', or '::boolean'?
```

### Feature Not Enabled

```
Error: Predicate '::yaml:anchor' requires feature 'yaml'
Hint: Add 'yaml' to features in Cargo.toml
```

### Incompatible Filters

```
Error: Cannot combine scalar and container kinds
  at query: $.data.*::(object|string)
```

## Migration from Wildcards

**Before** (regex-based):
```javascript
$.data[*][?(@.type == "number")]
```

**After** (kind predicate):
```javascript
$.data[*]::number
```

Performance: O(1) kind rejection vs O(n) filter evaluation.

## References

- [RFC 9535 JSONPath](https://www.rfc-editor.org/rfc/rfc9535)
- [jq Manual](https://jqlang.org/manual/)
- [yq Kind Operator](https://mikefarah.gitbook.io/yq/operators/kind)
- [XPath Node Tests](https://www.w3.org/TR/xpath-31/#node-tests)
