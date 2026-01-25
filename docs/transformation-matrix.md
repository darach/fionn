# Cross-Format Transformation Matrix

Complete reference for format transformations, loss categories, and validation requirements.

## Full Transformation Matrix

```
                                         TARGET FORMAT
                 ┌────────┬────────┬────────┬────────┬────────┬────────┐
                 │  JSON  │  YAML  │  TOML  │  CSV   │  ISON  │  TOON  │
    ┌────────────┼────────┼────────┼────────┼────────┼────────┼────────┤
    │    JSON    │   ✅   │   ✅   │   ✅   │  ⚠️NH  │   ✅   │   ✅   │
    ├────────────┼────────┼────────┼────────┼────────┼────────┼────────┤
    │    YAML    │  ⚠️ARC │   ✅   │  ⚠️ARC │⚠️ARCNH │  ⚠️AR  │  ⚠️ARC │
S   ├────────────┼────────┼────────┼────────┼────────┼────────┼────────┤
O   │    TOML    │  ⚠️CD  │   ✅   │   ✅   │ ⚠️CDNH │  ⚠️C   │  ⚠️C   │
U   ├────────────┼────────┼────────┼────────┼────────┼────────┼────────┤
R   │    CSV     │  ⚠️QHT │ ⚠️QHT  │ ⚠️QHT  │   ✅   │  ⚠️QH  │  ⚠️QH  │
C   ├────────────┼────────┼────────┼────────┼────────┼────────┼────────┤
E   │    ISON    │  ⚠️R   │   ✅   │   ✅   │  ⚠️RNH │   ✅   │   ✅   │
    ├────────────┼────────┼────────┼────────┼────────┼────────┼────────┤
    │    TOON    │  ⚠️CF  │   ✅   │   ✅   │ ⚠️CFNH │   ✅   │   ✅   │
    └────────────┴────────┴────────┴────────┴────────┴────────┴────────┘
```

## Loss Codes

| Code | Loss Category | Description | Recoverable |
|------|---------------|-------------|-------------|
| A | Anchors | YAML `&anchor`/`*alias` → inlined | No |
| R | References | ISON `:type:id`, YAML `*alias` → inlined or `$ref` | Partial |
| C | Comments | YAML `#`, TOML `#`, TOON `#` → dropped | No |
| D | Datetime | TOML native datetime → ISO string | Partial |
| F | Folded keys | TOON `a.b.c:` → nested structure | Yes |
| N | Nesting | Nested objects → flattened or rejected | No |
| H | Header | CSV header row ↔ field names mapping | Yes |
| Q | Quoting | CSV `"quoted,field"` → unquoted string | Yes |
| T | Type inference | CSV all-strings → must infer int/bool/null | Partial |

## Format Capability Matrix

| Feature | JSON | YAML | TOML | CSV | ISON | TOON |
|---------|------|------|------|-----|------|------|
| Objects | ✅ | ✅ | ✅ | ⚠️* | ✅ | ✅ |
| Arrays | ✅ | ✅ | ✅ | ✅† | ✅ | ✅ |
| Nesting | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ |
| Anchors/Refs | ❌ | ✅ | ❌ | ❌ | ✅ | ❌ |
| Comments | ❌ | ✅ | ✅ | ❌ | ❌ | ✅ |
| Multi-doc | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| Typed schema | ❌ | ❌ | ❌ | ✅‡ | ✅ | ✅ |
| Native datetime | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ |
| Streaming | JSONL | ❌ | ❌ | ✅ | ISONL | ✅ |
| Header row | ❌ | ❌ | ❌ | ✅ | ✅ | ✅ |
| Delimiter cfg | ❌ | ❌ | ❌ | `,;|␉` | `|` | `,|␉` |

Notes:
- `*` CSV "objects" = rows with header-defined fields (flat only)
- `†` CSV "array" = the entire document IS an array of row-objects
- `‡` CSV schema via header + type inference

## CSV Transformation Rules

### To CSV (from any format)

| Input | Output | Notes |
|-------|--------|-------|
| Array of objects | Rows | Each object = 1 row |
| Object keys | Header row | First row |
| Nested objects | FLATTEN or ERROR | `user.name` → `"user.name"` column |
| Arrays in values | SERIALIZE or ERROR | As string |
| Null | Empty field `""` | |
| Values with `,` | Quoted | `"value,with,commas"` |

### From CSV (to any format)

| Input | Output | Notes |
|-------|--------|-------|
| Header row | Field names | |
| Data row | Object in array | |
| Empty field | Null | Configurable |
| `"true"`/`"false"` | Boolean | Type inference |
| `"123"`, `"3.14"` | Number | Type inference |
| Quoted values | String | Quotes stripped |

Result is ALWAYS `[{...}, {...}, ...]` - array of flat objects.

## Fidelity Modes

| Mode | Behavior | Use Case |
|------|----------|----------|
| `Strict` | Error on ANY loss | Lossless pipelines |
| `Warning` | Warn + continue | Development |
| `Lossy` | Silent drop | Best-effort conversion |

### Fidelity with CSV

| Mode | Nesting Behavior |
|------|------------------|
| `Strict` | ERROR if source has nesting |
| `Warning` | FLATTEN: `{user:{name:"x"}}` → `{"user.name":"x"}` |
| `Lossy` | FLATTEN + drop arrays: `{tags:[a,b]}` → `{"tags":"[a,b]"}` |

## Round-Trip Guarantees

### Same-Format Round-Trip

All formats: `A → A → A` is lossless.

### Cross-Format Round-Trip with OriginalSyntax

| Transformation | Preserved via OriginalSyntax |
|----------------|------------------------------|
| YAML → JSON → YAML | Anchors, flow style |
| TOML → JSON → TOML | Comments, dotted keys |
| TOON → JSON → TOON | Folded keys, array headers |
| CSV → JSON → CSV | Quoting, delimiter, newlines |

### Cross-Format Round-Trip (Different Target)

| Transformation | Result |
|----------------|--------|
| YAML → TOML → YAML | Anchors LOST (TOML has no equivalent) |
| YAML → JSON → YAML | Anchors PRESERVED (via OriginalSyntax) |
| Any → CSV → Any | Nesting LOST (CSV is flat) |

## Validation Requirements

### Lossless Translation Tests

For each `✅` cell in the matrix:

```rust
proptest! {
    #[test]
    fn lossless_roundtrip(doc in arb_format_doc()) {
        let transformed = transform(doc, target_format);
        let back = transform(transformed, source_format);
        assert!(semantically_equal(doc, back));
    }
}
```

### Asymmetric Translation Tests

For each `⚠️` cell in the matrix:

```rust
proptest! {
    #[test]
    fn asymmetric_transform(doc in arb_format_doc()) {
        let result = transform(doc, target_format);

        // Verify expected losses
        for loss_code in expected_losses(source, target) {
            match loss_code {
                'A' => assert!(anchors_inlined(&result)),
                'C' => assert!(comments_removed(&result)),
                'N' => assert!(nesting_flattened(&result)),
                // ...
            }
        }

        // Verify preserved content
        assert!(values_preserved(doc, result));
    }
}
```

### Fuzz Targets

```rust
// Fuzz each transformation pair
fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = parse_format(data, source_format) {
        let _ = transform(doc, target_format);
        // Should not panic, may return error
    }
});

// Fuzz round-trip
fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = parse_format(data, source_format) {
        if let Ok(transformed) = transform(doc.clone(), target_format) {
            if let Ok(back) = transform(transformed, source_format) {
                // Verify no data corruption
                assert!(no_data_corruption(doc, back));
            }
        }
    }
});
```

### Benchmark Matrix

Benchmark each transformation:

| Metric | Description |
|--------|-------------|
| Throughput | bytes/sec, docs/sec |
| Latency | p50, p95, p99 |
| Memory | Peak allocation |
| Loss ratio | % of features lost |

## CRDT Cross-Format Merge

### Merge Scenarios

| Scenario | Resolution |
|----------|------------|
| YAML anchor + JSON field | Both applied; anchor inlined in JSON view |
| TOML table + YAML section | LWW on path; preserve format syntax |
| CSV row + ISON row | Position-aware merge; maintain order |
| Comment (YAML) + Comment (TOML) | Separate namespaces |

### Delta Structure

```rust
pub struct FormatDelta {
    /// Operations since last sync
    operations: Vec<CrdtOperation>,
    /// Vector clock at delta generation
    clock: VectorClock,
    /// Source format
    source_format: FormatKind,
    /// Format-specific metadata
    original_syntax: Vec<OriginalSyntax>,
}
```

## Implementation Checklist

### Per-Format Parser

- [ ] Emits unified tape (not JSON strings)
- [ ] Preserves OriginalSyntax
- [ ] Handles all edge cases
- [ ] Fuzz tested
- [ ] Benchmarked

### Per-Transformation Pair

- [ ] Lossless if marked ✅
- [ ] Expected losses if marked ⚠️
- [ ] Property tested
- [ ] Fuzz tested
- [ ] Benchmarked

### CRDT Integration

- [ ] Delta generation works
- [ ] Delta application works
- [ ] Cross-format merge works
- [ ] Conflicts resolved correctly
- [ ] Property tested
