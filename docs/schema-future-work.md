# Schema Validation Future Work

## Executive Summary

fionn currently has **path-based schema filtering** (`SchemaFilter`, `CompiledSchema`) for selective parsing, but lacks **type-based schema validation**. This document analyzes options for adding fast schema validation support that aligns with fionn's SIMD-accelerated, multi-format architecture.

**Key Insight**: fionn's `TapeSource` abstraction provides a unique opportunity for schema validation that works across all supported formats (JSON, YAML, TOML, CSV, ISON, TOON) with a single implementation.

---

## 1. Schema Validation Landscape

### 1.1 Schema Standards

| Standard | Primary Use | Rust Support | fionn Relevance |
|----------|-------------|--------------|-----------------|
| **JSON Schema** | JSON/YAML/TOML validation | Excellent | High - universal |
| **Avro** | Binary serialization, schema evolution | Good | Medium - streaming |
| **Protocol Buffers** | RPC, compact binary | Excellent | Low - different domain |
| **OpenAPI/Swagger** | API specification | Good | Medium - subset of JSON Schema |
| **JSON Type Definition (JTD)** | Simpler alternative to JSON Schema | Limited | High - performance focus |
| **TypeSchema** | Code generation focus | None | Low |
| **CSV Schema (CSVW)** | Tabular data | None | Medium - CSV support |

### 1.2 JSON Schema Draft Versions

| Draft | Year | Key Features | Adoption |
|-------|------|--------------|----------|
| Draft-04 | 2013 | Core vocabulary | Legacy |
| Draft-06 | 2017 | `const`, `contains`, `propertyNames` | Common |
| Draft-07 | 2018 | `if/then/else`, `readOnly/writeOnly` | Most common |
| 2019-09 | 2019 | Vocabularies, `unevaluatedProperties` | Growing |
| 2020-12 | 2020 | Dynamic references, output formats | Latest |

**Recommendation**: Support Draft-07 as baseline, with 2020-12 as stretch goal.

---

## 2. Rust Ecosystem Analysis

### 2.1 JSON Schema Crates

#### `jsonschema` (Most Mature)
```
- Stars: ~500 | Downloads: ~2M
- Drafts: 4, 6, 7, 2019-09, 2020-12
- Performance: Good (~10-50μs for simple schemas)
- Dependencies: Heavy (serde_json, regex, url, etc.)
```

**Pros:**
- Most complete JSON Schema support
- Well-maintained, active development
- Good error messages
- Supports custom formats and keywords

**Cons:**
- Requires `serde_json::Value` (no tape integration)
- Heavy dependency tree
- Not SIMD-optimized

**Integration effort**: Low (wrapper) to High (native)

#### `boon` (Performance Focus)
```
- Stars: ~50 | Downloads: ~10K
- Drafts: 2020-12 only
- Performance: Claims 2-3x faster than jsonschema
- Dependencies: Lighter
```

**Pros:**
- Performance-focused design
- Modern draft support
- Cleaner codebase

**Cons:**
- Only 2020-12 draft
- Less mature
- Still requires Value conversion

#### `valico` (Legacy)
```
- Stars: ~100 | Downloads: ~50K
- Drafts: 4 only
- Status: Maintenance mode
```

**Not recommended** - outdated, limited features.

### 2.2 JSON Type Definition (JTD)

#### `jtd` crate
```
- Stars: ~20 | Downloads: ~5K
- Spec: RFC 8927
- Performance: Very fast (~1-5μs)
- Dependencies: Minimal
```

**Pros:**
- Much simpler than JSON Schema
- Designed for validation performance
- Predictable validation time (no regexes)
- Easy to implement custom

**Cons:**
- Less expressive than JSON Schema
- Limited adoption
- No ecosystem tooling

**Key insight**: JTD's simplicity makes it ideal for a tape-native implementation.

### 2.3 Avro

#### `apache-avro` (Official)
```
- Stars: Part of Apache project
- Performance: Good for binary format
- Dependencies: Medium
```

**Pros:**
- Schema evolution support
- Compact binary format
- Strong typing

**Cons:**
- Different paradigm (binary serialization)
- Schema must exist upfront
- Not suitable for JSON/YAML validation

### 2.4 Performance Comparison (Estimated)

| Crate | Simple Schema | Complex Schema | Notes |
|-------|---------------|----------------|-------|
| `jsonschema` | ~20μs | ~200μs | Includes Value conversion |
| `boon` | ~8μs | ~80μs | Claims 2-3x improvement |
| `jtd` | ~2μs | ~20μs | Simpler schema model |
| **Tape-native** | ~0.5μs | ~10μs | Theoretical, no conversion |

---

## 3. Architecture Options

### 3.1 Option A: Wrapper Approach (Low Effort)

```rust
// Convert TapeSource to Value, validate with existing crate
pub fn validate<T: TapeSource>(
    tape: &T,
    schema: &CompiledJsonSchema,
) -> Result<(), ValidationError> {
    let value = tape_to_value(tape)?;  // Conversion overhead
    jsonschema::validate(&schema.inner, &value)
}
```

**Pros:**
- Quick to implement (~1 week)
- Full JSON Schema support via `jsonschema`
- Battle-tested validation logic

**Cons:**
- Loses tape performance benefits
- Memory allocation for Value conversion
- No streaming validation
- ~10-20x slower than tape-native

**Effort**: ~500 LOC | **Performance**: Baseline

### 3.2 Option B: Tape-Native JSON Schema (High Effort)

```rust
/// Compiled schema optimized for tape validation
pub struct TapeSchema {
    root: SchemaNode,
    definitions: HashMap<String, SchemaNode>,
    // Pre-computed for fast validation
    required_paths: HashSet<u64>,  // Hashed paths
    type_checks: Vec<(PathMatcher, TypeConstraint)>,
    pattern_cache: Vec<CompiledRegex>,
}

/// Validate tape directly without Value conversion
pub fn validate_tape<T: TapeSource>(
    tape: &T,
    schema: &TapeSchema,
) -> Result<ValidationReport, ValidationError> {
    let mut validator = TapeValidator::new(schema);
    for (idx, node) in tape.iter().enumerate() {
        validator.check_node(idx, node)?;
    }
    validator.finalize()
}
```

**Pros:**
- Maximum performance (zero-copy)
- Streaming validation possible
- Works uniformly across all formats
- SIMD opportunities for type checking

**Cons:**
- Major implementation effort
- Must reimplement JSON Schema semantics
- Complex edge cases (references, conditionals)

**Effort**: ~3000-5000 LOC | **Performance**: 10-20x baseline

### 3.3 Option C: JTD-Native Implementation (Medium Effort)

```rust
/// JTD schema compiled for tape validation
pub struct JtdSchema {
    form: JtdForm,
    definitions: HashMap<String, JtdSchema>,
}

pub enum JtdForm {
    Empty,
    Type(JtdType),  // boolean, string, timestamp, etc.
    Enum(HashSet<String>),
    Elements(Box<JtdSchema>),  // Array items
    Properties {
        required: HashMap<String, JtdSchema>,
        optional: HashMap<String, JtdSchema>,
        additional: bool,
    },
    Values(Box<JtdSchema>),  // Map values
    Discriminator {
        tag: String,
        mapping: HashMap<String, JtdSchema>,
    },
    Ref(String),
}

// Much simpler validation logic
pub fn validate_jtd<T: TapeSource>(
    tape: &T,
    schema: &JtdSchema,
) -> Result<(), JtdError> {
    validate_node(tape, 0, schema)
}
```

**Pros:**
- Simpler than JSON Schema (RFC 8927)
- Predictable O(n) validation
- No regex, no references complexity
- Easy to implement tape-native
- Good enough for most use cases

**Cons:**
- Less expressive (no patterns, no conditionals)
- Limited ecosystem adoption
- Users may need JSON Schema compatibility

**Effort**: ~1500 LOC | **Performance**: 5-10x baseline

### 3.4 Option D: Hybrid Approach (Recommended)

```rust
pub enum SchemaKind {
    /// Fast path - JTD-based tape validation
    Jtd(JtdSchema),
    /// Full path - JSON Schema via jsonschema crate
    JsonSchema(CompiledJsonSchema),
    /// Inferred - from sample data
    Inferred(InferredSchema),
}

pub fn validate<T: TapeSource>(
    tape: &T,
    schema: &SchemaKind,
) -> Result<ValidationReport, ValidationError> {
    match schema {
        SchemaKind::Jtd(jtd) => validate_jtd_native(tape, jtd),
        SchemaKind::JsonSchema(js) => validate_via_conversion(tape, js),
        SchemaKind::Inferred(inf) => validate_inferred(tape, inf),
    }
}

/// Auto-select fastest validation path
pub fn compile_schema(schema_json: &str) -> Result<SchemaKind, Error> {
    if let Ok(jtd) = try_parse_as_jtd(schema_json) {
        // Simple schema - use fast path
        Ok(SchemaKind::Jtd(compile_jtd(jtd)?))
    } else {
        // Complex schema - use full JSON Schema
        Ok(SchemaKind::JsonSchema(compile_json_schema(schema_json)?))
    }
}
```

**Pros:**
- Fast path for common cases (80%+ of real schemas)
- Full compatibility when needed
- Progressive enhancement opportunity
- Schema inference as bonus feature

**Cons:**
- Two code paths to maintain
- Users may be confused about which path is used

**Effort**: ~2000 LOC | **Performance**: 5-15x baseline (varies)

---

## 4. Integration with fionn Architecture

### 4.1 TapeSource Integration

```rust
/// Extension trait for schema validation
pub trait ValidatableTape: TapeSource {
    fn validate(&self, schema: &SchemaKind) -> ValidationResult;
    fn validate_streaming<F>(&self, schema: &SchemaKind, on_error: F)
        -> Result<(), ValidationError>
    where F: FnMut(ValidationError);
}

// Blanket implementation for all TapeSource
impl<T: TapeSource> ValidatableTape for T {
    fn validate(&self, schema: &SchemaKind) -> ValidationResult {
        schema::validate(self, schema)
    }
}
```

### 4.2 Multi-Format Schema Validation

```rust
/// Schema that works across formats
pub struct UniversalSchema {
    inner: SchemaKind,
    /// Format-specific type mappings
    type_mappings: FormatTypeMappings,
}

impl UniversalSchema {
    /// Validate with format-aware type coercion
    pub fn validate<T: TapeSource>(&self, tape: &T) -> ValidationResult {
        let format = tape.format();
        let mapper = self.type_mappings.get(format);
        validate_with_mapping(tape, &self.inner, mapper)
    }
}

/// How types map between formats
pub struct FormatTypeMappings {
    // JSON number -> TOML integer/float distinction
    // CSV string -> type inference
    // YAML timestamp -> string in JSON
}
```

### 4.3 CLI Integration

```bash
# Validate JSON against schema
fionn validate --schema schema.json data.json

# Validate YAML against same schema
fionn validate --schema schema.json data.yaml

# Infer schema from data
fionn schema --infer data.json > schema.json

# Validate with auto-detected format
fionn validate -f auto --schema schema.json data.toml
```

### 4.4 Streaming Validation

```rust
/// Validate JSONL records against schema
pub fn validate_stream<R: BufRead>(
    reader: R,
    schema: &SchemaKind,
) -> impl Iterator<Item = Result<(), (usize, ValidationError)>> {
    reader.lines().enumerate().map(|(line_num, line)| {
        let tape = DsonTape::parse(line?.as_bytes())?;
        tape.validate(schema)
            .map_err(|e| (line_num, e))
    })
}
```

---

## 5. SIMD Optimization Opportunities

### 5.1 Type Classification (High Impact)

```rust
/// SIMD-accelerated type checking
pub fn classify_types_simd(tape: &[TapeNode]) -> TypeBitmap {
    // Use SIMD to classify 16/32 nodes at once
    // Returns bitmap: is_string, is_number, is_bool, is_null, is_object, is_array
}

/// Fast type validation
pub fn validate_types_simd(
    tape: &[TapeNode],
    expected: &TypeBitmap,
) -> Option<usize> {
    // SIMD compare expected vs actual, return first mismatch index
}
```

### 5.2 Required Field Checking (Medium Impact)

```rust
/// Pre-compute required field hashes
pub struct RequiredFields {
    hashes: Vec<u64>,  // FNV-1a hashes of required field names
}

/// SIMD check for required fields in object
pub fn check_required_simd(
    keys: &[&str],
    required: &RequiredFields,
) -> MissingFields {
    // Hash all keys with SIMD
    // Compare against required hashes
}
```

### 5.3 Enum/Const Validation (Medium Impact)

```rust
/// Bloom filter for large enum sets
pub struct EnumValidator {
    bloom: BloomFilter,      // Fast negative check
    exact: HashSet<String>,  // Exact match for positives
}

/// SIMD string comparison for small enum sets
pub fn check_enum_simd(value: &str, variants: &[&str]) -> bool {
    // Use SIMD memcmp for small sets
}
```

### 5.4 Pattern Matching (Lower Impact)

```rust
/// Pre-compiled regex with SIMD where possible
pub struct PatternValidator {
    // Use aho-corasick for literal patterns
    // Fall back to regex for complex patterns
    literals: AhoCorasick,
    complex: Vec<Regex>,
}
```

---

## 6. Schema Inference Enhancement

### 6.1 Current State

The CLI has basic `infer_schema` in `main.rs` that produces JSON Schema from a single value. It's simplistic and doesn't handle:
- Multiple samples
- Type unions
- Confidence scoring
- Format-specific types

### 6.2 Proposed Enhancement

```rust
pub struct SchemaInferrer {
    samples: usize,
    config: InferenceConfig,
}

pub struct InferenceConfig {
    /// Minimum samples before inferring optional
    min_samples_for_optional: usize,
    /// Detect string patterns (email, date, uuid)
    detect_formats: bool,
    /// Merge similar object shapes
    merge_similar: bool,
    /// Maximum enum values before switching to string
    max_enum_values: usize,
}

pub struct InferredField {
    pub path: String,
    pub types: Vec<InferredType>,  // Union types
    pub required: Confidence,       // 0.0-1.0
    pub format: Option<String>,     // Detected format
    pub examples: Vec<String>,
    pub null_count: usize,
    pub total_count: usize,
}

impl SchemaInferrer {
    /// Infer from multiple tape sources
    pub fn infer<T: TapeSource>(&mut self, tape: &T) -> &mut Self;

    /// Finalize to schema
    pub fn finalize(self) -> InferredSchema;

    /// Export as JSON Schema
    pub fn to_json_schema(&self) -> Value;

    /// Export as JTD
    pub fn to_jtd(&self) -> Value;

    /// Export as TypeScript
    pub fn to_typescript(&self) -> String;

    /// Export as Rust structs
    pub fn to_rust(&self) -> String;
}
```

---

## 7. Error Reporting

### 7.1 Rich Validation Errors

```rust
pub struct ValidationError {
    pub path: String,           // JSON Pointer path
    pub message: String,        // Human-readable message
    pub keyword: String,        // Schema keyword that failed
    pub schema_path: String,    // Path in schema
    pub instance: Option<Value>,// Failing value (if small)
}

pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub annotations: Vec<Annotation>,  // 2020-12 annotations
    pub stats: ValidationStats,
}

pub struct ValidationStats {
    pub nodes_checked: usize,
    pub time_taken: Duration,
    pub schema_keywords_evaluated: usize,
}
```

### 7.2 Error Output Formats

```rust
pub enum ErrorFormat {
    /// Basic list of errors
    Basic,
    /// JSON Schema 2020-12 output format
    Detailed,
    /// Verbose with context
    Verbose,
    /// GitHub Actions compatible
    GithubActions,
    /// Machine-readable JSON
    Json,
}
```

---

## 8. Recommendation

### Phase 1: Foundation (2-3 weeks)
1. Add `jsonschema` crate as optional dependency
2. Implement wrapper validation via Value conversion
3. Add `--schema` flag to `fionn validate` command
4. Basic error reporting

**Deliverables:**
- `fionn validate --schema schema.json data.json`
- Works for all formats (via Value conversion)
- ~500 LOC

### Phase 2: JTD Fast Path (2-3 weeks)
1. Implement JTD parser (RFC 8927)
2. Tape-native JTD validation
3. Auto-detection of JTD-compatible schemas
4. SIMD type classification

**Deliverables:**
- 5-10x faster validation for simple schemas
- `SchemaKind::Jtd` variant
- ~1500 LOC

### Phase 3: Enhanced Inference (1-2 weeks)
1. Multi-sample schema inference
2. Confidence scoring
3. Format detection (email, date, uuid)
4. Export to JSON Schema, JTD, TypeScript, Rust

**Deliverables:**
- `fionn schema --infer --samples 100 data.jsonl`
- Multiple output formats
- ~800 LOC

### Phase 4: Streaming & SIMD (2-3 weeks)
1. Streaming validation for JSONL
2. SIMD required field checking
3. Bloom filter for large enums
4. Parallel validation for arrays

**Deliverables:**
- Sub-microsecond simple validations
- `fionn validate --stream --schema schema.json data.jsonl`
- ~1200 LOC

### Total Estimate
- **LOC**: ~4000
- **Time**: 8-12 weeks
- **Dependencies**: +jsonschema (optional)

---

## 9. Alternative: Minimal Viable Schema

If full schema validation is overkill, consider a minimal type-checking system:

```rust
/// Simple type schema (no JSON Schema complexity)
pub enum SimpleType {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array(Box<SimpleType>),
    Object(HashMap<String, (SimpleType, bool)>),  // (type, required)
    Union(Vec<SimpleType>),
    Any,
}

/// ~200 LOC to implement
pub fn validate_simple<T: TapeSource>(
    tape: &T,
    schema: &SimpleType,
) -> Result<(), SimpleTypeError>;
```

This covers 90% of use cases with 5% of the complexity.

---

## 10. Open Questions

1. **Draft compatibility**: Should we prioritize Draft-07 (most common) or 2020-12 (latest)?

2. **Avro integration**: Is schema evolution important for fionn's use cases?

3. **Custom formats**: Should we support user-defined format validators?

4. **Performance target**: What validation throughput do we need? (GB/s? records/s?)

5. **Error verbosity**: How much context in error messages? (Memory vs. usefulness)

6. **CSV schemas**: Support CSVW or custom CSV schema format?

---

## 11. References

- [JSON Schema Specification](https://json-schema.org/specification)
- [RFC 8927 - JSON Type Definition](https://datatracker.ietf.org/doc/html/rfc8927)
- [jsonschema crate](https://github.com/Stranger6667/jsonschema-rs)
- [boon crate](https://github.com/santhosh-tekuri/boon)
- [Apache Avro](https://avro.apache.org/docs/current/spec.html)
- [CSV on the Web](https://www.w3.org/TR/tabular-data-primer/)

---

*Document created: 2025-01-11*
*Status: Proposal - awaiting review*
