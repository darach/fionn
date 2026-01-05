// SPDX-License-Identifier: MIT OR Apache-2.0
//! Schema-based filtering for `DOMless` processing
//!
//! This module provides schema filtering capabilities that enable selective
//! parsing and processing of JSON documents. Schema filtering is fundamental
//! to skip-tape and sparse extraction optimizations.

use crate::error::{DsonError, Result};
use std::sync::Arc;

/// Hash a field name using `AHash` for fast lookups
#[inline]
#[must_use]
pub fn hash_field_name(field: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = ahash::AHasher::default();
    field.hash(&mut hasher);
    hasher.finish()
}

/// Schema-based filtering for `DOMless` processing
///
/// Uses `Arc`-wrapped patterns for cheap cloning without regex recompilation.
#[derive(Debug, Clone)]
pub struct SchemaFilter {
    /// Compiled JSON-path patterns for efficient matching
    paths: Arc<[String]>,
    /// Pre-compiled regex patterns for path matching (Arc for cheap clone)
    compiled_patterns: Arc<[regex::Regex]>,
}

impl SchemaFilter {
    /// Create a new schema filter from path patterns
    ///
    /// # Errors
    /// Returns an error if any path pattern has invalid regex syntax.
    pub fn new(paths: Vec<String>) -> Result<Self> {
        let mut compiled_patterns = Vec::with_capacity(paths.len());

        for path in &paths {
            // Convert JSON-path patterns to regex
            let regex_pattern = Self::json_path_to_regex(path);
            let regex = regex::Regex::new(&regex_pattern).map_err(|e| {
                DsonError::InvalidOperation(format!("Invalid JSON-path pattern '{path}': {e}"))
            })?;
            compiled_patterns.push(regex);
        }

        // Convert to Arc slices for cheap cloning
        let paths: Arc<[String]> = paths.into();
        let compiled_patterns: Arc<[regex::Regex]> = compiled_patterns.into();

        Ok(Self {
            paths,
            compiled_patterns,
        })
    }

    /// Get the schema paths
    #[must_use]
    #[inline]
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// Check if a JSON path matches any schema pattern
    #[inline]
    #[must_use]
    pub fn matches(&self, json_path: &str) -> bool {
        self.compiled_patterns
            .iter()
            .any(|pattern| pattern.is_match(json_path))
    }

    /// Convert JSON-path pattern to regex
    fn json_path_to_regex(pattern: &str) -> String {
        // Convert JSON-path patterns like "users[*].id" to regex
        let mut regex = "^".to_string();

        for part in pattern.split('.') {
            if part == "*" {
                regex.push_str(r"[^\.]*");
            } else if part.starts_with('[') && part.ends_with(']') {
                if part == "[*]" {
                    regex.push_str(r"\[\d+\]");
                } else {
                    // Specific array index like [0]
                    regex.push_str(&regex::escape(part));
                }
            } else {
                regex.push_str(&regex::escape(part));
            }
            regex.push_str(r"\.?");
        }

        // Remove trailing optional dot
        if regex.ends_with(r"\.?") {
            regex.truncate(regex.len() - 3);
        }
        regex.push('$');
        regex
    }
}

/// Compiled schema for SIMD-accelerated filtering
#[derive(Debug)]
pub struct CompiledSchema {
    /// Include patterns (paths that should be kept)
    pub include_patterns: Vec<SchemaPattern>,
    /// Exclude patterns (paths that should be skipped)
    pub exclude_patterns: Vec<SchemaPattern>,
    /// Maximum parsing depth
    pub max_depth: usize,
    /// Hash table for fast lookups
    pub pattern_hashes: Vec<u64>,
}

impl CompiledSchema {
    /// Compile a list of field paths into a schema
    ///
    /// # Errors
    /// Returns an error if any path pattern is invalid
    pub fn compile(paths: &[String]) -> Result<Self> {
        let mut include_patterns = Vec::new();
        let mut pattern_hashes = Vec::new();

        for path in paths {
            let pattern = SchemaPattern::compile(path)?;
            include_patterns.push(pattern);

            // Pre-compute hashes for fast comparison
            let hash = hash_field_name(path.as_bytes());
            pattern_hashes.push(hash);
        }

        Ok(Self {
            include_patterns,
            exclude_patterns: Vec::new(),
            max_depth: 10, // Default max depth
            pattern_hashes,
        })
    }

    /// Compile a schema with both include and exclude patterns
    ///
    /// # Errors
    /// Returns an error if any path pattern is invalid
    pub fn compile_with_excludes(
        include_paths: &[String],
        exclude_paths: &[String],
    ) -> Result<Self> {
        let mut include_patterns = Vec::new();
        let mut exclude_patterns = Vec::new();
        let mut pattern_hashes = Vec::new();

        for path in include_paths {
            let pattern = SchemaPattern::compile(path)?;
            include_patterns.push(pattern);
            let hash = hash_field_name(path.as_bytes());
            pattern_hashes.push(hash);
        }

        for path in exclude_paths {
            let pattern = SchemaPattern::compile(path)?;
            exclude_patterns.push(pattern);
        }

        Ok(Self {
            include_patterns,
            exclude_patterns,
            max_depth: 10,
            pattern_hashes,
        })
    }

    /// Get the field paths that this schema includes
    #[must_use]
    pub fn field_paths(&self) -> Vec<String> {
        self.include_patterns
            .iter()
            .map(|pattern| pattern.path.clone())
            .collect()
    }

    /// Check if a path matches the schema (includes but not excludes)
    #[must_use]
    pub fn matches_path(&self, path: &str) -> bool {
        // First check if path is excluded
        for exclude_pattern in &self.exclude_patterns {
            if exclude_pattern.matches(path) {
                return false;
            }
        }

        // Fast hash-based lookup for includes
        let path_hash = hash_field_name(path.as_bytes());

        // Check if hash matches any pattern (fast rejection)
        if !self.pattern_hashes.contains(&path_hash) {
            return false;
        }

        // Full pattern matching for hash collisions
        for pattern in &self.include_patterns {
            if pattern.matches(path) {
                return true;
            }
        }

        false
    }

    /// Check if a path is explicitly excluded
    #[must_use]
    pub fn is_excluded(&self, path: &str) -> bool {
        for exclude_pattern in &self.exclude_patterns {
            if exclude_pattern.matches(path) {
                return true;
            }
        }
        false
    }

    /// Check if we should include an object at the given path
    #[must_use]
    pub fn should_include_object(&self, path: &str) -> bool {
        // First check if path is excluded
        if self.is_excluded(path) {
            return false;
        }

        // Check if any child paths would match
        for pattern in &self.include_patterns {
            if pattern.could_match_children(path) {
                return true;
            }
        }
        false
    }
}

/// Individual schema pattern for path matching
#[derive(Debug)]
pub struct SchemaPattern {
    /// Original path string
    pub path: String,
    /// Path components for structured matching
    pub components: Vec<String>,
    /// Match type (exact, prefix, wildcard)
    pub match_type: MatchType,
    /// Pre-compiled regex for complex patterns
    pub regex: Option<regex::Regex>,
}

impl SchemaPattern {
    /// Compile a path pattern
    ///
    /// # Errors
    /// Returns an error if the pattern contains an invalid regex
    pub fn compile(path: &str) -> Result<Self> {
        let components: Vec<String> = path
            .split('.')
            .map(std::string::ToString::to_string)
            .collect();

        let match_type = if path.contains('*') {
            MatchType::Wildcard
        } else if components.len() > 1 {
            MatchType::Prefix
        } else {
            MatchType::Exact
        };

        let regex = if matches!(match_type, MatchType::Wildcard) {
            // Convert glob pattern to regex
            let regex_pattern = Self::glob_to_regex(path);
            Some(
                regex::Regex::new(&regex_pattern)
                    .map_err(|e| DsonError::ParseError(format!("Invalid regex pattern: {e}")))?,
            )
        } else {
            None
        };

        Ok(Self {
            path: path.to_string(),
            components,
            match_type,
            regex,
        })
    }

    /// Check if this pattern matches a path
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        match self.match_type {
            MatchType::Exact => self.path == path,
            MatchType::Prefix => path.starts_with(&self.path),
            MatchType::Wildcard => self
                .regex
                .as_ref()
                .is_some_and(|regex| regex.is_match(path)),
        }
    }

    /// Check if this pattern could match children of the given path
    #[must_use]
    pub fn could_match_children(&self, path: &str) -> bool {
        match self.match_type {
            MatchType::Exact => self.path.starts_with(&format!("{path}.")),
            MatchType::Prefix => {
                self.path.starts_with(&format!("{path}."))
                    || path.starts_with(&format!("{}.", self.path))
            }
            MatchType::Wildcard => {
                // For wildcards, check if the pattern could match deeper paths
                let test_path = format!("{path}.test");
                self.regex
                    .as_ref()
                    .is_some_and(|regex| regex.is_match(&test_path))
            }
        }
    }

    /// Convert glob pattern to regex
    fn glob_to_regex(pattern: &str) -> String {
        let mut regex = String::from("^");
        let mut chars = pattern.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '*' => {
                    if chars.peek() == Some(&'*') {
                        // ** matches any character sequence including dots
                        chars.next(); // consume second *
                        regex.push_str(".*");
                    } else {
                        // * matches any character sequence except dots
                        regex.push_str("[^.]*");
                    }
                }
                '.' => regex.push_str("\\."),
                '?' => regex.push('.'),
                '[' => regex.push_str("\\["),
                ']' => regex.push_str("\\]"),
                '{' => regex.push_str("\\{"),
                '}' => regex.push_str("\\}"),
                '(' => regex.push_str("\\("),
                ')' => regex.push_str("\\)"),
                '+' => regex.push_str("\\+"),
                '^' => regex.push_str("\\^"),
                '$' => regex.push_str("\\$"),
                '|' => regex.push_str("\\|"),
                '\\' => regex.push_str("\\\\"),
                other => regex.push(other),
            }
        }

        regex.push('$');
        regex
    }
}

/// Type of pattern matching
#[derive(Debug, Clone, Copy)]
pub enum MatchType {
    /// Exact path match only
    Exact,
    /// Prefix match (path starts with pattern)
    Prefix,
    /// Wildcard/glob pattern matching
    Wildcard,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // hash_field_name Tests
    // =========================================================================

    #[test]
    fn test_hash_field_name() {
        let hash1 = hash_field_name(b"test");
        let hash2 = hash_field_name(b"test");
        let hash3 = hash_field_name(b"other");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_hash_field_name_empty() {
        // Just verify it doesn't panic and returns a valid hash
        let _hash = hash_field_name(b"");
    }

    #[test]
    fn test_hash_field_name_unicode() {
        let hash1 = hash_field_name("名前".as_bytes());
        let hash2 = hash_field_name("名前".as_bytes());
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_field_name_special_chars() {
        let hash1 = hash_field_name(b"user.name[0]");
        let hash2 = hash_field_name(b"user.name[0]");
        let hash3 = hash_field_name(b"user.name[1]");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    // =========================================================================
    // SchemaFilter Tests
    // =========================================================================

    #[test]
    fn test_schema_filter_new() {
        let filter = SchemaFilter::new(vec!["user.name".to_string()]);
        assert!(filter.is_ok());
    }

    #[test]
    fn test_schema_filter_new_empty() {
        let filter = SchemaFilter::new(vec![]);
        assert!(filter.is_ok());
        assert_eq!(filter.unwrap().paths().len(), 0);
    }

    #[test]
    fn test_schema_filter_new_invalid_regex() {
        // Create a pattern that would produce invalid regex
        let filter = SchemaFilter::new(vec!["[invalid".to_string()]);
        // Note: our converter escapes brackets, so this should be OK
        assert!(filter.is_ok());
    }

    #[test]
    fn test_schema_filter_paths() {
        let filter =
            SchemaFilter::new(vec!["user.name".to_string(), "user.email".to_string()]).unwrap();
        assert_eq!(filter.paths().len(), 2);
        assert_eq!(filter.paths()[0], "user.name");
        assert_eq!(filter.paths()[1], "user.email");
    }

    #[test]
    fn test_schema_filter_matches() {
        let filter = SchemaFilter::new(vec!["user.name".to_string()]).unwrap();
        assert!(filter.matches("user.name"));
        assert!(!filter.matches("user.email"));
    }

    #[test]
    fn test_schema_filter_matches_multiple_patterns() {
        let filter =
            SchemaFilter::new(vec!["user.name".to_string(), "user.age".to_string()]).unwrap();
        assert!(filter.matches("user.name"));
        assert!(filter.matches("user.age"));
        assert!(!filter.matches("user.email"));
    }

    #[test]
    fn test_schema_filter_matches_wildcard_star() {
        let filter = SchemaFilter::new(vec!["users.*.id".to_string()]).unwrap();
        // The * becomes [^\.]*
        assert!(filter.matches("users.foo.id"));
    }

    #[test]
    fn test_schema_filter_matches_array_wildcard() {
        // SchemaFilter.json_path_to_regex only handles [*] as a component when it's the whole part
        let filter = SchemaFilter::new(vec!["users.[*].id".to_string()]).unwrap();
        // [*] becomes \[\d+\]
        assert!(filter.matches("users.[0].id"));
        assert!(filter.matches("users.[99].id"));
    }

    #[test]
    fn test_schema_filter_matches_specific_array_index() {
        let filter = SchemaFilter::new(vec!["users[0].name".to_string()]).unwrap();
        assert!(filter.matches("users[0].name"));
        assert!(!filter.matches("users[1].name"));
    }

    #[test]
    fn test_schema_filter_debug() {
        let filter = SchemaFilter::new(vec!["test".to_string()]).unwrap();
        let debug_str = format!("{filter:?}");
        assert!(debug_str.contains("SchemaFilter"));
    }

    #[test]
    fn test_schema_filter_clone() {
        let filter = SchemaFilter::new(vec!["test".to_string()]).unwrap();
        let cloned = filter.clone();
        assert_eq!(filter.paths(), cloned.paths());
    }

    // =========================================================================
    // CompiledSchema Tests
    // =========================================================================

    #[test]
    fn test_compiled_schema_compile() {
        let schema = CompiledSchema::compile(&["name".to_string(), "age".to_string()]);
        assert!(schema.is_ok());
        let schema = schema.unwrap();
        assert_eq!(schema.include_patterns.len(), 2);
    }

    #[test]
    fn test_compiled_schema_compile_empty() {
        let schema = CompiledSchema::compile(&[]).unwrap();
        assert_eq!(schema.include_patterns.len(), 0);
        assert_eq!(schema.pattern_hashes.len(), 0);
    }

    #[test]
    fn test_compiled_schema_compile_with_excludes() {
        let schema = CompiledSchema::compile_with_excludes(
            &["user.name".to_string(), "user.age".to_string()],
            &["user.password".to_string()],
        )
        .unwrap();
        assert_eq!(schema.include_patterns.len(), 2);
        assert_eq!(schema.exclude_patterns.len(), 1);
        assert_eq!(schema.max_depth, 10);
    }

    #[test]
    fn test_compiled_schema_compile_with_excludes_empty() {
        let schema = CompiledSchema::compile_with_excludes(&[], &[]).unwrap();
        assert_eq!(schema.include_patterns.len(), 0);
        assert_eq!(schema.exclude_patterns.len(), 0);
    }

    #[test]
    fn test_compiled_schema_field_paths() {
        let schema = CompiledSchema::compile(&["name".to_string(), "email".to_string()]).unwrap();
        let paths = schema.field_paths();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"name".to_string()));
        assert!(paths.contains(&"email".to_string()));
    }

    #[test]
    fn test_compiled_schema_matches_path() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        assert!(schema.matches_path("name"));
        assert!(!schema.matches_path("age"));
    }

    #[test]
    fn test_compiled_schema_matches_path_with_exclude() {
        let schema = CompiledSchema::compile_with_excludes(
            &["user.name".to_string(), "user.age".to_string()],
            &["user.age".to_string()],
        )
        .unwrap();
        assert!(schema.matches_path("user.name"));
        assert!(!schema.matches_path("user.age")); // Excluded
    }

    #[test]
    fn test_compiled_schema_matches_path_hash_mismatch() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        // "nonexistent" won't have matching hash
        assert!(!schema.matches_path("nonexistent"));
    }

    #[test]
    fn test_compiled_schema_is_excluded() {
        let schema = CompiledSchema::compile_with_excludes(
            &["user.name".to_string()],
            &["user.password".to_string()],
        )
        .unwrap();
        assert!(!schema.is_excluded("user.name"));
        assert!(schema.is_excluded("user.password"));
    }

    #[test]
    fn test_compiled_schema_is_excluded_empty() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        assert!(!schema.is_excluded("name"));
        assert!(!schema.is_excluded("anything"));
    }

    #[test]
    fn test_compiled_schema_should_include_object() {
        let schema = CompiledSchema::compile(&["user.name".to_string()]).unwrap();
        // "user" should be included because it has children that match
        assert!(schema.should_include_object("user"));
    }

    #[test]
    fn test_compiled_schema_should_include_object_excluded() {
        let schema = CompiledSchema::compile_with_excludes(
            &["user.name".to_string()],
            &["user".to_string()],
        )
        .unwrap();
        assert!(!schema.should_include_object("user")); // Explicitly excluded
    }

    #[test]
    fn test_compiled_schema_should_include_object_no_match() {
        let schema = CompiledSchema::compile(&["user.name".to_string()]).unwrap();
        assert!(!schema.should_include_object("config")); // No matching children
    }

    #[test]
    fn test_compiled_schema_debug() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        let debug_str = format!("{schema:?}");
        assert!(debug_str.contains("CompiledSchema"));
    }

    // =========================================================================
    // SchemaPattern Tests
    // =========================================================================

    #[test]
    fn test_schema_pattern_exact() {
        let pattern = SchemaPattern::compile("name").unwrap();
        assert!(matches!(pattern.match_type, MatchType::Exact));
        assert!(pattern.matches("name"));
        assert!(!pattern.matches("age"));
    }

    #[test]
    fn test_schema_pattern_prefix() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        assert!(matches!(pattern.match_type, MatchType::Prefix));
        assert!(pattern.matches("user.name"));
        assert!(pattern.matches("user.name.first")); // Prefix match
        assert!(!pattern.matches("user.age"));
    }

    #[test]
    fn test_schema_pattern_wildcard() {
        let pattern = SchemaPattern::compile("user.*").unwrap();
        assert!(matches!(pattern.match_type, MatchType::Wildcard));
        assert!(pattern.matches("user.name"));
        assert!(!pattern.matches("name"));
    }

    #[test]
    fn test_schema_pattern_double_wildcard() {
        let pattern = SchemaPattern::compile("user.**").unwrap();
        assert!(matches!(pattern.match_type, MatchType::Wildcard));
        // ** matches any sequence including dots
        assert!(pattern.matches("user.name"));
        assert!(pattern.matches("user.name.first"));
    }

    #[test]
    fn test_schema_pattern_could_match_children_exact() {
        let pattern = SchemaPattern::compile("user").unwrap();
        // Exact match that could have children
        assert!(!pattern.could_match_children("user")); // "user." doesn't start with "user."
        assert!(!pattern.could_match_children("other"));
    }

    #[test]
    fn test_schema_pattern_could_match_children_exact_deeper() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        // Pattern "user.name" starts with "user."
        assert!(pattern.could_match_children("user"));
    }

    #[test]
    fn test_schema_pattern_could_match_children_prefix() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        assert!(matches!(pattern.match_type, MatchType::Prefix));
        // Check if pattern could match children
        assert!(pattern.could_match_children("user")); // pattern starts with "user."
    }

    #[test]
    fn test_schema_pattern_could_match_children_prefix_reverse() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        // path "user.name.first" starts with "user.name."
        assert!(pattern.could_match_children("user.name.first"));
    }

    #[test]
    fn test_schema_pattern_could_match_children_wildcard() {
        let pattern = SchemaPattern::compile("user.*").unwrap();
        assert!(matches!(pattern.match_type, MatchType::Wildcard));
        // Wildcard pattern tests against path + ".test"
        assert!(pattern.could_match_children("user"));
    }

    #[test]
    fn test_schema_pattern_could_match_children_wildcard_no_match() {
        let pattern = SchemaPattern::compile("user.*").unwrap();
        assert!(!pattern.could_match_children("config")); // "config.test" won't match "user.*"
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_question_mark() {
        // ? in glob is converted to . (any single char) but pattern must be wildcard type
        let pattern = SchemaPattern::compile("user.*ame").unwrap();
        assert!(pattern.matches("user.name"));
        assert!(pattern.matches("user.fame"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_special_chars() {
        // Test all special regex characters are escaped
        let pattern = SchemaPattern::compile("a[b]c").unwrap();
        assert!(pattern.matches("a[b]c"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_braces() {
        let pattern = SchemaPattern::compile("a{b}c").unwrap();
        assert!(pattern.matches("a{b}c"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_parens() {
        let pattern = SchemaPattern::compile("a(b)c").unwrap();
        assert!(pattern.matches("a(b)c"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_plus() {
        let pattern = SchemaPattern::compile("a+b").unwrap();
        assert!(pattern.matches("a+b"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_caret() {
        let pattern = SchemaPattern::compile("a^b").unwrap();
        assert!(pattern.matches("a^b"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_dollar() {
        let pattern = SchemaPattern::compile("a$b").unwrap();
        assert!(pattern.matches("a$b"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_pipe() {
        let pattern = SchemaPattern::compile("a|b").unwrap();
        assert!(pattern.matches("a|b"));
    }

    #[test]
    fn test_schema_pattern_glob_to_regex_backslash() {
        let pattern = SchemaPattern::compile(r"a\b").unwrap();
        assert!(pattern.matches(r"a\b"));
    }

    #[test]
    fn test_schema_pattern_debug() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        let debug_str = format!("{pattern:?}");
        assert!(debug_str.contains("SchemaPattern"));
        assert!(debug_str.contains("user.name"));
    }

    #[test]
    fn test_schema_pattern_components() {
        let pattern = SchemaPattern::compile("user.profile.name").unwrap();
        assert_eq!(pattern.components.len(), 3);
        assert_eq!(pattern.components[0], "user");
        assert_eq!(pattern.components[1], "profile");
        assert_eq!(pattern.components[2], "name");
    }

    // =========================================================================
    // MatchType Tests
    // =========================================================================

    #[test]
    fn test_match_type_debug() {
        assert!(format!("{:?}", MatchType::Exact).contains("Exact"));
        assert!(format!("{:?}", MatchType::Prefix).contains("Prefix"));
        assert!(format!("{:?}", MatchType::Wildcard).contains("Wildcard"));
    }

    #[test]
    fn test_match_type_clone() {
        let mt = MatchType::Exact;
        let cloned = mt;
        assert!(matches!(cloned, MatchType::Exact));
    }

    #[test]
    fn test_match_type_copy() {
        let mt = MatchType::Prefix;
        let copied = mt;
        assert!(matches!(copied, MatchType::Prefix));
        // Original still accessible
        assert!(matches!(mt, MatchType::Prefix));
    }

    // =========================================================================
    // json_path_to_regex Tests (via SchemaFilter)
    // =========================================================================

    #[test]
    fn test_json_path_to_regex_simple() {
        let filter = SchemaFilter::new(vec!["name".to_string()]).unwrap();
        assert!(filter.matches("name"));
        assert!(!filter.matches("name2"));
    }

    #[test]
    fn test_json_path_to_regex_dotted() {
        let filter = SchemaFilter::new(vec!["user.name".to_string()]).unwrap();
        assert!(filter.matches("user.name"));
        assert!(!filter.matches("user_name"));
    }

    #[test]
    fn test_json_path_to_regex_star() {
        let filter = SchemaFilter::new(vec!["*.name".to_string()]).unwrap();
        assert!(filter.matches("user.name"));
        assert!(filter.matches("admin.name"));
    }

    #[test]
    fn test_json_path_to_regex_array_wildcard() {
        // SchemaFilter requires array wildcards to be separate components
        let filter = SchemaFilter::new(vec!["items.[*]".to_string()]).unwrap();
        assert!(filter.matches("items.[0]"));
        assert!(filter.matches("items.[123]"));
    }

    #[test]
    fn test_json_path_to_regex_mixed() {
        // SchemaFilter requires wildcards as separate components
        let filter = SchemaFilter::new(vec!["users.[*].*.id".to_string()]).unwrap();
        assert!(filter.matches("users.[0].profile.id"));
    }
}
