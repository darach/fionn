// SPDX-License-Identifier: MIT OR Apache-2.0
//! Schema compilation for SIMD-JSONL Skip Tape
//!
//! This module handles the compilation of schema definitions into SIMD-friendly
//! patterns that can be used for fast filtering during parsing.

use crate::skiptape::error::{Result, SkipTapeError};
use crate::skiptape::simd_ops::SimdStringOps;

/// Compiled schema for SIMD-accelerated filtering
#[derive(Debug)]
pub struct CompiledSchema {
    /// Include patterns (paths that should be kept)
    pub include_patterns: Vec<SchemaPattern>,
    /// Exclude patterns (paths that should be skipped)
    pub exclude_patterns: Vec<SchemaPattern>,
    /// Maximum parsing depth
    pub max_depth: usize,
    /// SIMD-friendly hash table for fast lookups
    pub pattern_hashes: Vec<u64>,
}

impl CompiledSchema {
    /// Compile a list of field paths into a SIMD-friendly schema
    ///
    /// # Errors
    /// Returns an error if any path pattern is invalid
    pub fn compile(paths: &[String]) -> Result<Self> {
        let mut include_patterns = Vec::new();
        let mut pattern_hashes = Vec::new();

        for path in paths {
            let pattern = SchemaPattern::compile(path)?;
            include_patterns.push(pattern);

            // Pre-compute hashes for SIMD comparison
            let hash = SimdStringOps::hash_field_name(path.as_bytes());
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
            let hash = SimdStringOps::hash_field_name(path.as_bytes());
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

        // Fast SIMD hash-based lookup for includes
        let path_hash = SimdStringOps::hash_field_name(path.as_bytes());

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
                // Also verify the children aren't all excluded
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

        let regex =
            if matches!(match_type, MatchType::Wildcard) {
                // Convert glob pattern to regex
                let regex_pattern = Self::glob_to_regex(path);
                Some(regex::Regex::new(&regex_pattern).map_err(|e| {
                    SkipTapeError::SchemaError(format!("Invalid regex pattern: {e}"))
                })?)
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

    #[test]
    fn test_compiled_schema_compile() {
        let schema = CompiledSchema::compile(&["name".to_string(), "age".to_string()]);
        assert!(schema.is_ok());
        let schema = schema.unwrap();
        assert_eq!(schema.include_patterns.len(), 2);
    }

    #[test]
    fn test_compiled_schema_compile_empty() {
        let schema = CompiledSchema::compile(&[]);
        assert!(schema.is_ok());
        let schema = schema.unwrap();
        assert!(schema.include_patterns.is_empty());
    }

    #[test]
    fn test_compiled_schema_with_excludes() {
        let schema =
            CompiledSchema::compile_with_excludes(&["name".to_string()], &["age".to_string()]);
        assert!(schema.is_ok());
        let schema = schema.unwrap();
        assert_eq!(schema.include_patterns.len(), 1);
        assert_eq!(schema.exclude_patterns.len(), 1);
    }

    #[test]
    fn test_compiled_schema_field_paths() {
        let schema = CompiledSchema::compile(&["name".to_string(), "age".to_string()]).unwrap();
        let paths = schema.field_paths();
        assert!(paths.contains(&"name".to_string()));
        assert!(paths.contains(&"age".to_string()));
    }

    #[test]
    fn test_compiled_schema_matches_path_exact() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        assert!(schema.matches_path("name"));
        assert!(!schema.matches_path("age"));
    }

    #[test]
    fn test_compiled_schema_is_excluded() {
        let schema =
            CompiledSchema::compile_with_excludes(&["*".to_string()], &["secret".to_string()])
                .unwrap();
        assert!(schema.is_excluded("secret"));
        assert!(!schema.is_excluded("name"));
    }

    #[test]
    fn test_compiled_schema_should_include_object() {
        let schema = CompiledSchema::compile(&["user.name".to_string()]).unwrap();
        assert!(schema.should_include_object("user"));
    }

    #[test]
    fn test_compiled_schema_should_not_include_excluded() {
        let schema = CompiledSchema::compile_with_excludes(
            &["user.name".to_string()],
            &["user".to_string()],
        )
        .unwrap();
        assert!(!schema.should_include_object("user"));
    }

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
        assert!(pattern.matches("user.name.first"));
    }

    #[test]
    fn test_schema_pattern_wildcard() {
        let pattern = SchemaPattern::compile("user.*").unwrap();
        assert!(matches!(pattern.match_type, MatchType::Wildcard));
        assert!(pattern.matches("user.name"));
        assert!(pattern.matches("user.age"));
        assert!(!pattern.matches("name"));
    }

    #[test]
    fn test_schema_pattern_double_wildcard() {
        let pattern = SchemaPattern::compile("user.**").unwrap();
        assert!(pattern.matches("user.profile.name"));
    }

    #[test]
    fn test_schema_pattern_could_match_children() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        assert!(pattern.could_match_children("user"));
    }

    #[test]
    fn test_schema_pattern_could_match_children_wildcard() {
        let pattern = SchemaPattern::compile("user.*").unwrap();
        assert!(pattern.could_match_children("user"));
    }

    #[test]
    fn test_glob_to_regex_special_chars() {
        // Test various special characters
        let pattern = SchemaPattern::compile("test[0]").unwrap();
        assert!(pattern.matches("test[0]"));
    }

    #[test]
    fn test_glob_to_regex_question_mark() {
        // Question mark is only used in wildcard patterns (with *)
        let pattern = SchemaPattern::compile("test?*").unwrap();
        assert!(pattern.matches("test1abc"));
        assert!(pattern.matches("testAbc"));
    }

    #[test]
    fn test_match_type_debug() {
        let mt = MatchType::Exact;
        let debug = format!("{mt:?}");
        assert!(debug.contains("Exact"));
    }

    #[test]
    fn test_match_type_clone() {
        let mt = MatchType::Wildcard;
        let cloned = mt;
        assert!(matches!(cloned, MatchType::Wildcard));
    }

    #[test]
    fn test_schema_pattern_debug() {
        let pattern = SchemaPattern::compile("name").unwrap();
        let debug = format!("{pattern:?}");
        assert!(debug.contains("name"));
    }

    #[test]
    fn test_compiled_schema_debug() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        let debug = format!("{schema:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_schema_max_depth() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        assert_eq!(schema.max_depth, 10);
    }

    #[test]
    fn test_schema_pattern_hashes() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        assert_eq!(schema.pattern_hashes.len(), 1);
    }

    #[test]
    fn test_glob_to_regex_curly_braces() {
        let pattern = SchemaPattern::compile("test{0}*").unwrap();
        assert!(pattern.matches("test{0}"));
    }

    #[test]
    fn test_glob_to_regex_parentheses() {
        let pattern = SchemaPattern::compile("test(1)*").unwrap();
        assert!(pattern.matches("test(1)"));
    }

    #[test]
    fn test_glob_to_regex_plus() {
        let pattern = SchemaPattern::compile("test+1*").unwrap();
        assert!(pattern.matches("test+1"));
    }

    #[test]
    fn test_glob_to_regex_caret() {
        let pattern = SchemaPattern::compile("test^1*").unwrap();
        assert!(pattern.matches("test^1"));
    }

    #[test]
    fn test_glob_to_regex_dollar() {
        let pattern = SchemaPattern::compile("test$1*").unwrap();
        assert!(pattern.matches("test$1"));
    }

    #[test]
    fn test_glob_to_regex_pipe() {
        let pattern = SchemaPattern::compile("test|1*").unwrap();
        assert!(pattern.matches("test|1"));
    }

    #[test]
    fn test_glob_to_regex_backslash() {
        let pattern = SchemaPattern::compile("test\\1*").unwrap();
        assert!(pattern.matches("test\\1"));
    }

    #[test]
    fn test_matches_path_hash_collision() {
        // Create a schema and test path matching with exact matches
        let schema =
            CompiledSchema::compile(&["user.name".to_string(), "user.email".to_string()]).unwrap();
        assert!(schema.matches_path("user.name"));
        assert!(schema.matches_path("user.email"));
        assert!(!schema.matches_path("user.age"));
    }

    #[test]
    fn test_matches_path_with_exclusion() {
        // Note: matches_path uses hash-based lookup, so exact matches work best
        // Wildcards should use should_include_object for object filtering
        let schema = CompiledSchema::compile_with_excludes(
            &["user.name".to_string()],
            &["user.secret".to_string()],
        )
        .unwrap();
        assert!(schema.matches_path("user.name"));
        // user.secret is excluded
        assert!(schema.is_excluded("user.secret"));
    }

    #[test]
    fn test_could_match_children_exact_no_match() {
        let pattern = SchemaPattern::compile("name").unwrap();
        // "name" cannot have children like "other.child"
        assert!(!pattern.could_match_children("other"));
    }

    #[test]
    fn test_could_match_children_prefix_reverse() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        // Test where path starts with pattern (prefix reverse)
        assert!(pattern.could_match_children("user.name.first"));
    }

    #[test]
    fn test_could_match_children_wildcard_no_regex() {
        // This is an edge case - a pattern that is Wildcard but has no regex
        // In practice, this shouldn't happen with compile(), but let's be thorough
        let pattern = SchemaPattern {
            path: "test.*".to_string(),
            components: vec!["test".to_string(), "*".to_string()],
            match_type: MatchType::Wildcard,
            regex: None,
        };
        assert!(!pattern.could_match_children("test"));
    }

    #[test]
    fn test_matches_wildcard_no_regex() {
        // Edge case: Wildcard match type but no regex
        let pattern = SchemaPattern {
            path: "test.*".to_string(),
            components: vec!["test".to_string(), "*".to_string()],
            match_type: MatchType::Wildcard,
            regex: None,
        };
        assert!(!pattern.matches("test.abc"));
    }

    #[test]
    fn test_should_include_object_no_match() {
        let schema = CompiledSchema::compile(&["other.field".to_string()]).unwrap();
        // "user" is not a prefix of "other.field"
        assert!(!schema.should_include_object("user"));
    }

    #[test]
    fn test_exclude_patterns_empty() {
        let schema = CompiledSchema::compile(&["name".to_string()]).unwrap();
        assert!(schema.exclude_patterns.is_empty());
    }

    #[test]
    fn test_match_type_copy() {
        let mt = MatchType::Prefix;
        let copied = mt;
        assert!(matches!(copied, MatchType::Prefix));
    }

    #[test]
    fn test_compiled_schema_multiple_patterns() {
        // Note: matches_path uses exact hash matching, not wildcard matching
        let schema = CompiledSchema::compile(&[
            "user.name".to_string(),
            "user.email".to_string(),
            "address.city".to_string(),
        ])
        .unwrap();

        assert!(schema.matches_path("user.name"));
        assert!(schema.matches_path("address.city"));
        assert!(!schema.matches_path("phone"));
    }

    #[test]
    fn test_compiled_schema_excludes_with_wildcards() {
        let schema =
            CompiledSchema::compile_with_excludes(&["**".to_string()], &["secret.*".to_string()])
                .unwrap();

        assert!(schema.is_excluded("secret.key"));
        assert!(!schema.is_excluded("public.key"));
    }

    #[test]
    fn test_schema_pattern_components() {
        let pattern = SchemaPattern::compile("user.profile.name").unwrap();
        assert_eq!(pattern.components.len(), 3);
        assert_eq!(pattern.components[0], "user");
        assert_eq!(pattern.components[1], "profile");
        assert_eq!(pattern.components[2], "name");
    }

    #[test]
    fn test_could_match_children_wildcard_match() {
        let pattern = SchemaPattern::compile("user.**").unwrap();
        // Double wildcard should match any depth
        assert!(pattern.could_match_children("user"));
    }

    #[test]
    fn test_could_match_children_no_match() {
        let pattern = SchemaPattern::compile("user.name").unwrap();
        // "other" is not a prefix of "user.name"
        assert!(!pattern.could_match_children("other"));
    }

    #[test]
    fn test_matches_path_no_hash_match() {
        // Path that doesn't match any hash should return false quickly
        let schema = CompiledSchema::compile(&["specific.field".to_string()]).unwrap();
        assert!(!schema.matches_path("different.field"));
    }
}
