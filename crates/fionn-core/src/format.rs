// SPDX-License-Identifier: MIT OR Apache-2.0
//! Format types and node kind classification for multi-format support
//!
//! This module provides the foundational types for fionn's multi-format
//! tape architecture, including format identification and node kind
//! classification for schema filtering.

use std::fmt;

/// Data format kind for tape identification
///
/// Each tape carries a format marker indicating its source format,
/// enabling format-specific parsing and transformation logic.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FormatKind {
    /// JSON (JavaScript Object Notation) - base format
    #[default]
    Json = 0,

    /// TOML (Tom's Obvious Minimal Language)
    #[cfg(feature = "toml")]
    Toml = 1,

    /// YAML (YAML Ain't Markup Language)
    #[cfg(feature = "yaml")]
    Yaml = 2,

    /// CSV (Comma-Separated Values)
    #[cfg(feature = "csv")]
    Csv = 3,

    /// ISON (Interchange Simple Object Notation) - LLM-optimized
    #[cfg(feature = "ison")]
    Ison = 4,

    /// TOON (Token-Oriented Object Notation) - LLM-optimized
    #[cfg(feature = "toon")]
    Toon = 5,
}

impl FormatKind {
    /// Get the format name as a string
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            #[cfg(feature = "toml")]
            Self::Toml => "toml",
            #[cfg(feature = "yaml")]
            Self::Yaml => "yaml",
            #[cfg(feature = "csv")]
            Self::Csv => "csv",
            #[cfg(feature = "ison")]
            Self::Ison => "ison",
            #[cfg(feature = "toon")]
            Self::Toon => "toon",
        }
    }

    /// Parse format kind from string
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "json" | "jsonl" => Some(Self::Json),
            #[cfg(feature = "toml")]
            "toml" => Some(Self::Toml),
            #[cfg(feature = "yaml")]
            "yaml" | "yml" => Some(Self::Yaml),
            #[cfg(feature = "csv")]
            "csv" => Some(Self::Csv),
            #[cfg(feature = "ison")]
            "ison" | "isonl" => Some(Self::Ison),
            #[cfg(feature = "toon")]
            "toon" => Some(Self::Toon),
            _ => None,
        }
    }

    /// Check if this format supports comments
    #[must_use]
    pub const fn supports_comments(self) -> bool {
        match self {
            Self::Json => false,
            #[cfg(feature = "toml")]
            Self::Toml => true,
            #[cfg(feature = "yaml")]
            Self::Yaml => true,
            #[cfg(feature = "csv")]
            Self::Csv => false, // CSV comment support is parser-specific
            #[cfg(feature = "ison")]
            Self::Ison => true,
            #[cfg(feature = "toon")]
            Self::Toon => false,
        }
    }

    /// Check if this format supports references/anchors
    #[must_use]
    pub const fn supports_references(self) -> bool {
        match self {
            Self::Json => false,
            #[cfg(feature = "toml")]
            Self::Toml => false,
            #[cfg(feature = "yaml")]
            Self::Yaml => true, // anchors/aliases
            #[cfg(feature = "csv")]
            Self::Csv => false,
            #[cfg(feature = "ison")]
            Self::Ison => true, // :type:id references
            #[cfg(feature = "toon")]
            Self::Toon => false,
        }
    }
}

impl fmt::Display for FormatKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// Format Auto-Detection
// ============================================================================

/// Result of format detection with confidence level
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectionResult {
    /// Detected format
    pub format: FormatKind,
    /// Confidence level (0.0 to 1.0)
    pub confidence: Confidence,
}

/// Confidence level for format detection
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// Low confidence - might be wrong
    Low = 0,
    /// Medium confidence - likely correct
    Medium = 1,
    /// High confidence - almost certainly correct
    High = 2,
    /// Exact match - file extension or MIME type
    Exact = 3,
}

impl FormatKind {
    /// Detect format from file extension
    ///
    /// Returns `Some(result)` with `Confidence::Exact` for known extensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use fionn_core::FormatKind;
    ///
    /// let result = FormatKind::detect_from_extension("json").unwrap();
    /// assert_eq!(result.format, FormatKind::Json);
    /// ```
    #[must_use]
    pub fn detect_from_extension(ext: &str) -> Option<DetectionResult> {
        let ext_lower = ext.to_lowercase();
        let format = match ext_lower.as_str() {
            "json" | "jsonl" | "ndjson" | "geojson" => Some(Self::Json),
            #[cfg(feature = "yaml")]
            "yaml" | "yml" => Some(Self::Yaml),
            #[cfg(feature = "toml")]
            "toml" => Some(Self::Toml),
            #[cfg(feature = "csv")]
            "csv" | "tsv" | "psv" => Some(Self::Csv),
            #[cfg(feature = "ison")]
            "ison" | "isonl" => Some(Self::Ison),
            #[cfg(feature = "toon")]
            "toon" => Some(Self::Toon),
            _ => None,
        };
        format.map(|f| DetectionResult {
            format: f,
            confidence: Confidence::Exact,
        })
    }

    /// Detect format from MIME type
    ///
    /// Returns `Some(result)` with `Confidence::Exact` for known MIME types.
    ///
    /// # Examples
    ///
    /// ```
    /// use fionn_core::FormatKind;
    ///
    /// let result = FormatKind::detect_from_mime_type("application/json").unwrap();
    /// assert_eq!(result.format, FormatKind::Json);
    /// ```
    #[must_use]
    pub fn detect_from_mime_type(mime: &str) -> Option<DetectionResult> {
        let format = match mime.to_lowercase().as_str() {
            "application/json" | "text/json" | "application/x-ndjson" => Some(Self::Json),
            #[cfg(feature = "yaml")]
            "application/x-yaml" | "text/yaml" | "text/x-yaml" | "application/yaml" => {
                Some(Self::Yaml)
            }
            #[cfg(feature = "toml")]
            "application/toml" | "text/x-toml" => Some(Self::Toml),
            #[cfg(feature = "csv")]
            "text/csv" | "text/tab-separated-values" => Some(Self::Csv),
            _ => None,
        };
        format.map(|f| DetectionResult {
            format: f,
            confidence: Confidence::Exact,
        })
    }

    /// Detect format from content by examining the first bytes
    ///
    /// Uses heuristics to determine the most likely format. Returns the
    /// format with highest confidence, or JSON as fallback.
    ///
    /// # Detection Heuristics
    ///
    /// | Format | Detection Method |
    /// |--------|------------------|
    /// | JSON   | Starts with `{` or `[` (after whitespace) |
    /// | YAML   | Contains `---` document marker or `:` with indentation |
    /// | TOML   | Contains `[section]` or `key = value` patterns |
    /// | CSV    | Contains `,` with consistent structure per line |
    /// | ISON   | Contains `:type:` reference patterns |
    /// | TOON   | Contains indented tabular data with array headers |
    ///
    /// # Examples
    ///
    /// ```
    /// use fionn_core::FormatKind;
    ///
    /// let json = r#"{"hello": "world"}"#;
    /// let result = FormatKind::detect_from_content(json.as_bytes());
    /// assert_eq!(result.format, FormatKind::Json);
    /// ```
    #[must_use]
    pub fn detect_from_content(content: &[u8]) -> DetectionResult {
        // Empty content defaults to JSON
        if content.is_empty() {
            return DetectionResult {
                format: Self::Json,
                confidence: Confidence::Low,
            };
        }

        // Try to interpret as UTF-8 for text-based detection
        let Ok(text) = std::str::from_utf8(content) else {
            return DetectionResult {
                format: Self::Json,
                confidence: Confidence::Low,
            };
        };

        // Skip leading whitespace
        let trimmed = text.trim_start();

        // Check for JSON object - starts with {
        if trimmed.starts_with('{') {
            return DetectionResult {
                format: Self::Json,
                confidence: Confidence::High,
            };
        }

        // Check for JSON array - starts with [ but not a TOML section header
        // JSON arrays contain data like [1,2,3] or ["a","b"]
        // TOML section headers look like [section] or [[table]]
        if trimmed.starts_with('[') {
            // Look for closing bracket to determine if it's TOML section or JSON array
            if let Some(bracket_end) = trimmed.find(']') {
                let inside = &trimmed[1..bracket_end];
                // TOML sections contain simple identifiers, not JSON values
                let looks_like_toml_section = inside
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '[');
                if looks_like_toml_section && !inside.contains(',') && !inside.contains(':') {
                    // This is likely a TOML section header, check TOML first
                    #[cfg(feature = "toml")]
                    if Self::looks_like_toml(trimmed) {
                        return DetectionResult {
                            format: Self::Toml,
                            confidence: Confidence::Medium,
                        };
                    }
                }
            }
            // Otherwise it's likely JSON array
            return DetectionResult {
                format: Self::Json,
                confidence: Confidence::High,
            };
        }

        // Check for YAML - document marker or characteristic patterns
        #[cfg(feature = "yaml")]
        if Self::looks_like_yaml(trimmed) {
            return DetectionResult {
                format: Self::Yaml,
                confidence: Confidence::Medium,
            };
        }

        // Check for TOML - section headers or key=value
        #[cfg(feature = "toml")]
        if Self::looks_like_toml(trimmed) {
            return DetectionResult {
                format: Self::Toml,
                confidence: Confidence::Medium,
            };
        }

        // Check for ISON - :type:id references
        #[cfg(feature = "ison")]
        if Self::looks_like_ison(trimmed) {
            return DetectionResult {
                format: Self::Ison,
                confidence: Confidence::Medium,
            };
        }

        // Check for TOON - indented tabular or array headers
        #[cfg(feature = "toon")]
        if Self::looks_like_toon(trimmed) {
            return DetectionResult {
                format: Self::Toon,
                confidence: Confidence::Medium,
            };
        }

        // Check for CSV - comma/tab separated with consistent columns
        #[cfg(feature = "csv")]
        if Self::looks_like_csv(trimmed) {
            return DetectionResult {
                format: Self::Csv,
                confidence: Confidence::Medium,
            };
        }

        // Default to JSON with low confidence
        DetectionResult {
            format: Self::Json,
            confidence: Confidence::Low,
        }
    }

    /// Check if content looks like YAML
    #[cfg(feature = "yaml")]
    fn looks_like_yaml(content: &str) -> bool {
        // Document marker is a strong indicator
        if content.starts_with("---") {
            return true;
        }

        // Check first few lines for YAML patterns
        for line in content.lines().take(10) {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Check for key: value pattern (with space after colon)
            if let Some(colon_pos) = line.find(':') {
                // Colon should be followed by space, newline, or end of line
                let after_colon = &line[colon_pos + 1..];
                if after_colon.is_empty() || after_colon.starts_with(' ') {
                    // Must not look like a URL (http: or https:)
                    let before_colon = &line[..colon_pos];
                    if !before_colon.ends_with("http") && !before_colon.ends_with("https") {
                        return true;
                    }
                }
            }

            // Check for list item
            if line.starts_with("- ") {
                return true;
            }
        }

        false
    }

    /// Check if content looks like TOML
    #[cfg(feature = "toml")]
    fn looks_like_toml(content: &str) -> bool {
        for line in content.lines().take(20) {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section header [name] or [[name]]
            if line.starts_with('[') && line.ends_with(']') {
                return true;
            }

            // Key = value pattern (with spaces around =)
            if let Some(eq_pos) = line.find(" = ") {
                let key = &line[..eq_pos];
                // Key should be a valid identifier (or quoted string)
                if key
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
                {
                    return true;
                }
                // Or quoted key
                if key.starts_with('"') && key.ends_with('"') {
                    return true;
                }
            }
        }

        false
    }

    /// Check if content looks like CSV
    #[cfg(feature = "csv")]
    fn looks_like_csv(content: &str) -> bool {
        let lines: Vec<&str> = content.lines().take(5).collect();
        if lines.len() < 2 {
            return false;
        }

        // Detect delimiter (comma, tab, or pipe)
        let delimiter = Self::detect_csv_delimiter(lines[0]);

        // Count fields in first few lines
        let field_counts: Vec<usize> = lines
            .iter()
            .map(|line| line.split(delimiter).count())
            .collect();

        // Consistent field count is a strong indicator
        if field_counts.len() >= 2 && field_counts[0] > 1 {
            // All lines should have same field count
            let first_count = field_counts[0];
            if field_counts.iter().all(|&c| c == first_count) {
                return true;
            }
        }

        false
    }

    /// Detect likely CSV delimiter
    #[cfg(feature = "csv")]
    fn detect_csv_delimiter(line: &str) -> char {
        let comma_count = line.chars().filter(|&c| c == ',').count();
        let tab_count = line.chars().filter(|&c| c == '\t').count();
        let pipe_count = line.chars().filter(|&c| c == '|').count();

        if tab_count >= comma_count && tab_count >= pipe_count {
            '\t'
        } else if pipe_count > comma_count {
            '|'
        } else {
            ','
        }
    }

    /// Check if content looks like ISON
    #[cfg(feature = "ison")]
    fn looks_like_ison(content: &str) -> bool {
        // ISON has :type:id reference patterns
        for line in content.lines().take(20) {
            // Check for :type:id pattern (at least 3 parts when split by :)
            if line.contains(':') && line.split(':').count() >= 3 {
                return true;
            }

            // Check for block headers like "table users {"
            if line.starts_with("table ") || line.starts_with("object ") {
                return true;
            }

            // Check for field declarations like "fields: id name email"
            if line.starts_with("fields:") || line.starts_with("  fields:") {
                return true;
            }
        }

        false
    }

    /// Check if content looks like TOON
    #[cfg(feature = "toon")]
    fn looks_like_toon(content: &str) -> bool {
        for line in content.lines().take(20) {
            // Check for array header pattern [N]{...}
            if line.contains('[') && line.contains("]{") {
                return true;
            }

            // Check for folded key pattern (key.subkey.subsubkey:)
            if let Some(colon_pos) = line.find(':') {
                let key_part = &line[..colon_pos];
                if key_part.contains('.')
                    && key_part
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '.' || c == '_')
                {
                    return true;
                }
            }

            // Check for tabular data with consistent indentation
            if line.starts_with("  ") && !line.starts_with("    ") {
                // 2-space indent (TOON typical)
                if line.contains(',') || line.contains('\t') {
                    return true;
                }
            }
        }

        false
    }

    /// Detect format using all available methods
    ///
    /// Tries detection in order of reliability:
    /// 1. File extension (if provided)
    /// 2. MIME type (if provided)
    /// 3. Content sniffing
    ///
    /// Returns the result with highest confidence.
    #[must_use]
    pub fn detect(
        content: &[u8],
        extension: Option<&str>,
        mime_type: Option<&str>,
    ) -> DetectionResult {
        // Try extension first (most reliable)
        if let Some(ext) = extension
            && let Some(result) = Self::detect_from_extension(ext)
        {
            return result;
        }

        // Try MIME type next
        if let Some(mime) = mime_type
            && let Some(result) = Self::detect_from_mime_type(mime)
        {
            return result;
        }

        // Fall back to content sniffing
        Self::detect_from_content(content)
    }
}

// ============================================================================
// Node Kind Classification
// ============================================================================

/// Universal node kinds for filtering (cross-format)
///
/// These kinds form a semantic hierarchy that works across all formats,
/// enabling queries like `$..::string` or `$..::reference`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    // === Value Types (0-15) ===
    /// String value
    String = 0,
    /// Numeric value (integer or float)
    Number = 1,
    /// Boolean value (true/false)
    Boolean = 2,
    /// Null value
    Null = 3,
    /// Any scalar (string, number, boolean, null)
    Scalar = 4,

    // === Structural Types (16-31) ===
    /// Object/mapping container
    Object = 16,
    /// Array/sequence container
    Array = 17,
    /// Key name (in object)
    Key = 18,
    /// Value (in key-value pair)
    Value = 19,

    // === Semantic Categories (32-47) ===
    /// Comment (any format)
    Comment = 32,
    /// Reference (YAML alias, ISON ref)
    Reference = 33,
    /// Definition (YAML anchor, ISON table)
    Definition = 34,
    /// Header row/declaration
    Header = 35,
    /// Data row
    Row = 36,
    /// Section/block boundary
    Section = 37,

    // === Format-Specific (48+) ===
    /// Format-specific node kind
    FormatSpecific(FormatSpecificKind) = 48,
}

/// Format-specific node kinds (namespaced)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatSpecificKind {
    // === TOML (feature = "toml") ===
    #[cfg(feature = "toml")]
    /// TOML dotted key (a.b.c)
    TomlDottedKey,
    #[cfg(feature = "toml")]
    /// TOML inline table {k=v}
    TomlInlineTable,
    #[cfg(feature = "toml")]
    /// TOML array table `[[name]]`
    TomlArrayTable,
    #[cfg(feature = "toml")]
    /// TOML section header `[name]`
    TomlSectionHeader,

    // === YAML (feature = "yaml") ===
    #[cfg(feature = "yaml")]
    /// YAML anchor &name
    YamlAnchor,
    #[cfg(feature = "yaml")]
    /// YAML alias *name
    YamlAlias,
    #[cfg(feature = "yaml")]
    /// YAML merge key <<:
    YamlMergeKey,
    #[cfg(feature = "yaml")]
    /// YAML type tag !tag
    YamlTag,
    #[cfg(feature = "yaml")]
    /// YAML document separator ---
    YamlDocument,

    // === CSV (feature = "csv") ===
    #[cfg(feature = "csv")]
    /// CSV quoted field
    CsvQuoted,
    #[cfg(feature = "csv")]
    /// CSV sparse field (missing)
    CsvSparse,

    // === ISON (feature = "ison") ===
    #[cfg(feature = "ison")]
    /// ISON table block
    IsonTable,
    #[cfg(feature = "ison")]
    /// ISON object block
    IsonObject,
    #[cfg(feature = "ison")]
    /// ISON reference :type:id
    IsonRef,
    #[cfg(feature = "ison")]
    /// ISON field declaration
    IsonFieldDecl,

    // === TOON (feature = "toon") ===
    #[cfg(feature = "toon")]
    /// TOON tabular array `[N]{fields}`
    ToonTabular,
    #[cfg(feature = "toon")]
    /// TOON folded key (dotted path)
    ToonFolded,
    #[cfg(feature = "toon")]
    /// TOON list item (- prefix)
    ToonListItem,
}

impl NodeKind {
    /// Check if this kind is a value type
    #[must_use]
    pub const fn is_value_type(self) -> bool {
        matches!(
            self,
            Self::String | Self::Number | Self::Boolean | Self::Null | Self::Scalar
        )
    }

    /// Check if this kind is a container type
    #[must_use]
    pub const fn is_container(self) -> bool {
        matches!(self, Self::Object | Self::Array)
    }

    /// Check if this kind is a semantic category
    #[must_use]
    pub const fn is_semantic_category(self) -> bool {
        matches!(
            self,
            Self::Comment
                | Self::Reference
                | Self::Definition
                | Self::Header
                | Self::Row
                | Self::Section
        )
    }

    /// Check if this kind is format-specific
    #[must_use]
    pub const fn is_format_specific(self) -> bool {
        matches!(self, Self::FormatSpecific(_))
    }

    /// Check if a kind matches this kind (with inheritance)
    ///
    /// Semantic categories match their specific variants:
    /// - `Reference` matches `YamlAlias`, `IsonRef`
    /// - `Definition` matches `YamlAnchor`, `IsonTable`
    /// - etc.
    #[must_use]
    pub fn matches(self, other: Self) -> bool {
        if self == other {
            return true;
        }

        // Scalar matches any value type
        if self == Self::Scalar && other.is_value_type() {
            return true;
        }

        // Semantic category inheritance
        match self {
            Self::Reference => other.is_reference_kind(),
            Self::Definition => other.is_definition_kind(),
            Self::Header => other.is_header_kind(),
            Self::Comment => other.is_comment_kind(),
            _ => false,
        }
    }

    /// Check if this is a reference kind
    #[must_use]
    const fn is_reference_kind(self) -> bool {
        match self {
            Self::Reference => true,
            Self::FormatSpecific(fsk) => fsk.is_reference(),
            _ => false,
        }
    }

    /// Check if this is a definition kind
    #[must_use]
    const fn is_definition_kind(self) -> bool {
        match self {
            Self::Definition => true,
            Self::FormatSpecific(fsk) => fsk.is_definition(),
            _ => false,
        }
    }

    /// Check if this is a header kind
    #[must_use]
    const fn is_header_kind(self) -> bool {
        match self {
            Self::Header => true,
            Self::FormatSpecific(fsk) => fsk.is_header(),
            _ => false,
        }
    }

    /// Check if this is a comment kind
    #[must_use]
    const fn is_comment_kind(self) -> bool {
        matches!(self, Self::Comment)
    }
}

impl FormatSpecificKind {
    /// Get the format this kind belongs to
    #[must_use]
    pub const fn format(self) -> FormatKind {
        match self {
            #[cfg(feature = "toml")]
            Self::TomlDottedKey
            | Self::TomlInlineTable
            | Self::TomlArrayTable
            | Self::TomlSectionHeader => FormatKind::Toml,

            #[cfg(feature = "yaml")]
            Self::YamlAnchor
            | Self::YamlAlias
            | Self::YamlMergeKey
            | Self::YamlTag
            | Self::YamlDocument => FormatKind::Yaml,

            #[cfg(feature = "csv")]
            Self::CsvQuoted | Self::CsvSparse => FormatKind::Csv,

            #[cfg(feature = "ison")]
            Self::IsonTable | Self::IsonObject | Self::IsonRef | Self::IsonFieldDecl => {
                FormatKind::Ison
            }

            #[cfg(feature = "toon")]
            Self::ToonTabular | Self::ToonFolded | Self::ToonListItem => FormatKind::Toon,
        }
    }

    /// Get the predicate name (for query syntax)
    #[must_use]
    pub const fn predicate_name(self) -> &'static str {
        match self {
            #[cfg(feature = "toml")]
            Self::TomlDottedKey => "dotted-key",
            #[cfg(feature = "toml")]
            Self::TomlInlineTable => "inline-table",
            #[cfg(feature = "toml")]
            Self::TomlArrayTable => "array-table",
            #[cfg(feature = "toml")]
            Self::TomlSectionHeader => "section-header",

            #[cfg(feature = "yaml")]
            Self::YamlAnchor => "anchor",
            #[cfg(feature = "yaml")]
            Self::YamlAlias => "alias",
            #[cfg(feature = "yaml")]
            Self::YamlMergeKey => "merge-key",
            #[cfg(feature = "yaml")]
            Self::YamlTag => "tag",
            #[cfg(feature = "yaml")]
            Self::YamlDocument => "document",

            #[cfg(feature = "csv")]
            Self::CsvQuoted => "quoted",
            #[cfg(feature = "csv")]
            Self::CsvSparse => "sparse",

            #[cfg(feature = "ison")]
            Self::IsonTable => "table",
            #[cfg(feature = "ison")]
            Self::IsonObject => "object",
            #[cfg(feature = "ison")]
            Self::IsonRef => "ref",
            #[cfg(feature = "ison")]
            Self::IsonFieldDecl => "field-decl",

            #[cfg(feature = "toon")]
            Self::ToonTabular => "tabular",
            #[cfg(feature = "toon")]
            Self::ToonFolded => "folded",
            #[cfg(feature = "toon")]
            Self::ToonListItem => "list-item",
        }
    }

    /// Check if this is a reference kind
    #[must_use]
    pub const fn is_reference(self) -> bool {
        match self {
            #[cfg(feature = "yaml")]
            Self::YamlAlias => true,
            #[cfg(feature = "ison")]
            Self::IsonRef => true,
            #[allow(unreachable_patterns)] // Matches feature-gated variants
            _ => false,
        }
    }

    /// Check if this is a definition kind
    #[must_use]
    pub const fn is_definition(self) -> bool {
        match self {
            #[cfg(feature = "yaml")]
            Self::YamlAnchor => true,
            #[cfg(feature = "ison")]
            Self::IsonTable | Self::IsonObject => true,
            #[allow(unreachable_patterns)] // Matches feature-gated variants
            _ => false,
        }
    }

    /// Check if this is a header kind
    #[must_use]
    pub const fn is_header(self) -> bool {
        match self {
            #[cfg(feature = "toml")]
            Self::TomlSectionHeader => true,
            #[cfg(feature = "ison")]
            Self::IsonFieldDecl => true,
            #[cfg(feature = "toon")]
            Self::ToonTabular => true,
            #[allow(unreachable_patterns)] // Matches feature-gated variants
            _ => false,
        }
    }

    /// Parse from format:predicate string
    #[must_use]
    #[allow(unused_variables)] // Variables used conditionally per feature
    #[allow(clippy::missing_const_for_fn)] // str matching prevents const
    pub fn from_namespaced(format: &str, predicate: &str) -> Option<Self> {
        match format {
            #[cfg(feature = "toml")]
            "toml" => match predicate {
                "dotted-key" => Some(Self::TomlDottedKey),
                "inline-table" => Some(Self::TomlInlineTable),
                "array-table" => Some(Self::TomlArrayTable),
                "section-header" => Some(Self::TomlSectionHeader),
                _ => None,
            },
            #[cfg(feature = "yaml")]
            "yaml" => match predicate {
                "anchor" => Some(Self::YamlAnchor),
                "alias" => Some(Self::YamlAlias),
                "merge-key" => Some(Self::YamlMergeKey),
                "tag" => Some(Self::YamlTag),
                "document" => Some(Self::YamlDocument),
                _ => None,
            },
            #[cfg(feature = "csv")]
            "csv" => match predicate {
                "quoted" => Some(Self::CsvQuoted),
                "sparse" => Some(Self::CsvSparse),
                _ => None,
            },
            #[cfg(feature = "ison")]
            "ison" => match predicate {
                "table" => Some(Self::IsonTable),
                "object" => Some(Self::IsonObject),
                "ref" => Some(Self::IsonRef),
                "field-decl" => Some(Self::IsonFieldDecl),
                _ => None,
            },
            #[cfg(feature = "toon")]
            "toon" => match predicate {
                "tabular" => Some(Self::ToonTabular),
                "folded" => Some(Self::ToonFolded),
                "list-item" => Some(Self::ToonListItem),
                _ => None,
            },
            _ => None,
        }
    }
}

// ============================================================================
// Parsing Context
// ============================================================================

/// Parsing context for context-aware filtering
///
/// Tracks whether a node is inside strings, comments, or quoted regions.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ParsingContext {
    /// Normal context (not in string/comment/quote)
    #[default]
    Normal = 0,
    /// Inside a string literal
    InString = 1,
    /// Inside a comment
    InComment = 2,
    /// Inside a quoted value
    Quoted = 3,
    /// Escaped character/sequence
    Escaped = 4,
}

impl ParsingContext {
    /// Check if this context is inside a string
    #[must_use]
    pub const fn is_in_string(self) -> bool {
        matches!(self, Self::InString)
    }

    /// Check if this context is inside a comment
    #[must_use]
    pub const fn is_in_comment(self) -> bool {
        matches!(self, Self::InComment)
    }

    /// Check if this context is quoted
    #[must_use]
    pub const fn is_quoted(self) -> bool {
        matches!(self, Self::Quoted)
    }

    /// Check if this context is escaped
    #[must_use]
    pub const fn is_escaped(self) -> bool {
        matches!(self, Self::Escaped)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_kind_default() {
        assert_eq!(FormatKind::default(), FormatKind::Json);
    }

    #[test]
    fn test_format_kind_name() {
        assert_eq!(FormatKind::Json.name(), "json");
    }

    #[test]
    fn test_format_kind_from_name() {
        assert_eq!(FormatKind::from_name("json"), Some(FormatKind::Json));
        assert_eq!(FormatKind::from_name("JSON"), Some(FormatKind::Json));
        assert_eq!(FormatKind::from_name("unknown"), None);
    }

    #[test]
    fn test_node_kind_is_value_type() {
        assert!(NodeKind::String.is_value_type());
        assert!(NodeKind::Number.is_value_type());
        assert!(NodeKind::Boolean.is_value_type());
        assert!(NodeKind::Null.is_value_type());
        assert!(NodeKind::Scalar.is_value_type());
        assert!(!NodeKind::Object.is_value_type());
    }

    #[test]
    fn test_node_kind_is_container() {
        assert!(NodeKind::Object.is_container());
        assert!(NodeKind::Array.is_container());
        assert!(!NodeKind::String.is_container());
    }

    #[test]
    fn test_node_kind_scalar_matches_value_types() {
        assert!(NodeKind::Scalar.matches(NodeKind::String));
        assert!(NodeKind::Scalar.matches(NodeKind::Number));
        assert!(NodeKind::Scalar.matches(NodeKind::Boolean));
        assert!(NodeKind::Scalar.matches(NodeKind::Null));
        assert!(!NodeKind::Scalar.matches(NodeKind::Object));
    }

    #[test]
    fn test_parsing_context_default() {
        assert_eq!(ParsingContext::default(), ParsingContext::Normal);
    }

    // ========================================================================
    // Format Detection Tests
    // ========================================================================

    #[test]
    fn test_detect_from_extension_json() {
        let result = FormatKind::detect_from_extension("json").unwrap();
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::Exact);

        // Also test variants
        assert!(FormatKind::detect_from_extension("jsonl").is_some());
        assert!(FormatKind::detect_from_extension("ndjson").is_some());
        assert!(FormatKind::detect_from_extension("geojson").is_some());
    }

    #[test]
    fn test_detect_from_extension_unknown() {
        assert!(FormatKind::detect_from_extension("xyz").is_none());
        assert!(FormatKind::detect_from_extension("txt").is_none());
    }

    #[test]
    fn test_detect_from_mime_type_json() {
        let result = FormatKind::detect_from_mime_type("application/json").unwrap();
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::Exact);
    }

    #[test]
    fn test_detect_from_content_json() {
        let json_obj = r#"{"hello": "world"}"#;
        let result = FormatKind::detect_from_content(json_obj.as_bytes());
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::High);

        let json_arr = r"[1, 2, 3]";
        let result = FormatKind::detect_from_content(json_arr.as_bytes());
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::High);

        // With leading whitespace
        let json_ws = r#"   {"key": "value"}"#;
        let result = FormatKind::detect_from_content(json_ws.as_bytes());
        assert_eq!(result.format, FormatKind::Json);
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn test_detect_from_content_yaml() {
        let yaml_doc = "---\nname: Alice\nage: 30";
        let result = FormatKind::detect_from_content(yaml_doc.as_bytes());
        assert_eq!(result.format, FormatKind::Yaml);

        let yaml_kv = "name: Alice\nage: 30";
        let result = FormatKind::detect_from_content(yaml_kv.as_bytes());
        assert_eq!(result.format, FormatKind::Yaml);

        let yaml_list = "- item1\n- item2\n- item3";
        let result = FormatKind::detect_from_content(yaml_list.as_bytes());
        assert_eq!(result.format, FormatKind::Yaml);
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_detect_from_content_toml() {
        let toml_section = "[package]\nname = \"myapp\"";
        let result = FormatKind::detect_from_content(toml_section.as_bytes());
        assert_eq!(result.format, FormatKind::Toml);

        let toml_kv = "name = \"myapp\"\nversion = \"1.0\"";
        let result = FormatKind::detect_from_content(toml_kv.as_bytes());
        assert_eq!(result.format, FormatKind::Toml);
    }

    #[cfg(feature = "csv")]
    #[test]
    fn test_detect_from_content_csv() {
        let csv = "id,name,age\n1,Alice,30\n2,Bob,25";
        let result = FormatKind::detect_from_content(csv.as_bytes());
        assert_eq!(result.format, FormatKind::Csv);

        let tsv = "id\tname\tage\n1\tAlice\t30\n2\tBob\t25";
        let result = FormatKind::detect_from_content(tsv.as_bytes());
        assert_eq!(result.format, FormatKind::Csv);
    }

    #[test]
    fn test_detect_from_content_empty() {
        let result = FormatKind::detect_from_content(&[]);
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn test_detect_combined() {
        // Extension takes precedence
        let json_content = r#"{"test": 1}"#;
        let result = FormatKind::detect(json_content.as_bytes(), Some("json"), None);
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::Exact);

        // MIME type takes precedence over content
        let result = FormatKind::detect(json_content.as_bytes(), None, Some("application/json"));
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::Exact);

        // Falls back to content
        let result = FormatKind::detect(json_content.as_bytes(), None, None);
        assert_eq!(result.format, FormatKind::Json);
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn test_confidence_ordering() {
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
        assert!(Confidence::High < Confidence::Exact);
    }
}
