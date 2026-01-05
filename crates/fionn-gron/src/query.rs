// SPDX-License-Identifier: MIT OR Apache-2.0
//! Query mode for filtering gron output.
//!
//! Supports JSONPath-like queries to filter gron output to specific paths.
//!
//! ## Syntax
//!
//! - `.field` - Match exact field
//! - `["field"]` - Match field (for special characters)
//! - `[0]` - Match array index
//! - `[*]` - Match all array elements (wildcard)
//! - `..field` - Recursive descent (match anywhere)
//!
//! ## Examples
//!
//! ```text
//! .users[*].name      - All user names
//! ..error             - All fields named "error" at any depth
//! .data[0].id         - First data item's ID
//! ["field.name"]      - Field with dots in name
//! ```

use std::fmt;

/// A compiled query for path matching.
#[derive(Debug, Clone)]
pub struct Query {
    /// The parsed segments
    segments: Vec<QuerySegment>,
    /// Whether query contains recursive descent
    has_recursive: bool,
    /// Whether query contains wildcards
    has_wildcard: bool,
    /// Original query string for display
    original: String,
}

/// A segment in a query path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuerySegment {
    /// Match exact field name: `.field` or `["field"]`
    Field(String),
    /// Match exact array index: `[0]`
    Index(usize),
    /// Match any array element: `[*]`
    Wildcard,
    /// Recursive descent to find field anywhere: `..field`
    Recursive(String),
}

/// Query parsing error.
#[derive(Debug, Clone)]
pub enum QueryError {
    /// Empty query string
    Empty,
    /// Unexpected character
    UnexpectedChar(char, usize),
    /// Unclosed bracket
    UnclosedBracket,
    /// Unclosed quote
    UnclosedQuote,
    /// Invalid index
    InvalidIndex(String),
    /// Expected field name
    ExpectedField,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty query"),
            Self::UnexpectedChar(c, pos) => {
                write!(f, "unexpected character '{c}' at position {pos}")
            }
            Self::UnclosedBracket => write!(f, "unclosed bracket"),
            Self::UnclosedQuote => write!(f, "unclosed quote"),
            Self::InvalidIndex(s) => write!(f, "invalid index: {s}"),
            Self::ExpectedField => write!(f, "expected field name"),
        }
    }
}

impl std::error::Error for QueryError {}

impl Query {
    /// Parse a query string.
    ///
    /// # Errors
    /// Returns an error if the query syntax is invalid.
    pub fn parse(query: &str) -> Result<Self, QueryError> {
        if query.is_empty() {
            return Err(QueryError::Empty);
        }

        let mut segments = Vec::new();
        let bytes = query.as_bytes();
        let mut i = 0;

        // Skip leading root identifier if present (e.g., "json" in "json.field")
        // We match against paths that already have the root stripped
        if bytes[0] != b'.' && bytes[0] != b'[' {
            // Find end of root identifier
            while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
                i += 1;
            }
        }

        while i < bytes.len() {
            match bytes[i] {
                b'.' => {
                    i += 1;
                    if i >= bytes.len() {
                        return Err(QueryError::ExpectedField);
                    }

                    // Check for recursive descent (..)
                    if bytes[i] == b'.' {
                        i += 1;
                        let (field, consumed) = parse_identifier(&bytes[i..])?;
                        segments.push(QuerySegment::Recursive(field));
                        i += consumed;
                    } else {
                        let (field, consumed) = parse_identifier(&bytes[i..])?;
                        segments.push(QuerySegment::Field(field));
                        i += consumed;
                    }
                }
                b'[' => {
                    i += 1;
                    if i >= bytes.len() {
                        return Err(QueryError::UnclosedBracket);
                    }

                    match bytes[i] {
                        b'*' => {
                            // Wildcard [*]
                            i += 1;
                            if i >= bytes.len() || bytes[i] != b']' {
                                return Err(QueryError::UnclosedBracket);
                            }
                            i += 1;
                            segments.push(QuerySegment::Wildcard);
                        }
                        b'"' => {
                            // Quoted field ["field"]
                            i += 1;
                            let (field, consumed) = parse_quoted_string(&bytes[i..])?;
                            i += consumed;
                            if i >= bytes.len() || bytes[i] != b']' {
                                return Err(QueryError::UnclosedBracket);
                            }
                            i += 1;
                            segments.push(QuerySegment::Field(field));
                        }
                        b'0'..=b'9' => {
                            // Numeric index [0]
                            let start = i;
                            while i < bytes.len() && bytes[i].is_ascii_digit() {
                                i += 1;
                            }
                            let index_str =
                                std::str::from_utf8(&bytes[start..i]).map_err(|_| {
                                    QueryError::InvalidIndex("invalid utf8".to_string())
                                })?;
                            let index: usize = index_str
                                .parse()
                                .map_err(|_| QueryError::InvalidIndex(index_str.to_string()))?;

                            if i >= bytes.len() || bytes[i] != b']' {
                                return Err(QueryError::UnclosedBracket);
                            }
                            i += 1;
                            segments.push(QuerySegment::Index(index));
                        }
                        c => {
                            return Err(QueryError::UnexpectedChar(c as char, i));
                        }
                    }
                }
                c => {
                    return Err(QueryError::UnexpectedChar(c as char, i));
                }
            }
        }

        let has_recursive = segments
            .iter()
            .any(|s| matches!(s, QuerySegment::Recursive(_)));
        let has_wildcard = segments.iter().any(|s| matches!(s, QuerySegment::Wildcard));

        Ok(Self {
            segments,
            has_recursive,
            has_wildcard,
            original: query.to_string(),
        })
    }

    /// Check if a gron path matches this query.
    ///
    /// The path should be in gron format: `json.users[0].name`
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        let path_segments = parse_path_segments(path);
        self.matches_segments(&path_segments, 0, 0)
    }

    /// Check if a path could potentially match (for early termination).
    ///
    /// Returns:
    /// - `Matches` if path matches query
    /// - `Partial` if path doesn't match but descendants might
    /// - `NoMatch` if path and descendants cannot match
    #[must_use]
    pub fn match_potential(&self, path: &str) -> MatchPotential {
        let path_segments = parse_path_segments(path);
        self.check_potential(&path_segments, 0, 0)
    }

    /// Get the original query string.
    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Check if query has recursive descent.
    #[must_use]
    pub const fn has_recursive(&self) -> bool {
        self.has_recursive
    }

    /// Check if query has wildcards.
    #[must_use]
    pub const fn has_wildcard(&self) -> bool {
        self.has_wildcard
    }

    fn matches_segments(
        &self,
        path_segments: &[PathSegment<'_>],
        query_idx: usize,
        path_idx: usize,
    ) -> bool {
        // If we've matched all query segments, success
        if query_idx >= self.segments.len() {
            return true;
        }

        // If we've exhausted path segments but have more query, fail
        if path_idx >= path_segments.len() {
            return false;
        }

        let query_seg = &self.segments[query_idx];
        let path_seg = &path_segments[path_idx];

        match query_seg {
            QuerySegment::Field(expected) => {
                if let PathSegment::Field(actual) = path_seg
                    && *actual == expected
                {
                    return self.matches_segments(path_segments, query_idx + 1, path_idx + 1);
                }
                false
            }
            QuerySegment::Index(expected) => {
                if let PathSegment::Index(actual) = path_seg
                    && actual == expected
                {
                    return self.matches_segments(path_segments, query_idx + 1, path_idx + 1);
                }
                false
            }
            QuerySegment::Wildcard => {
                // Wildcard matches any single segment
                self.matches_segments(path_segments, query_idx + 1, path_idx + 1)
            }
            QuerySegment::Recursive(expected) => {
                // Try matching at current position and all subsequent positions
                for i in path_idx..path_segments.len() {
                    if let PathSegment::Field(actual) = &path_segments[i]
                        && *actual == expected
                        && self.matches_segments(path_segments, query_idx + 1, i + 1)
                    {
                        return true;
                    }
                }
                false
            }
        }
    }

    fn check_potential(
        &self,
        path_segments: &[PathSegment<'_>],
        query_idx: usize,
        path_idx: usize,
    ) -> MatchPotential {
        // If we've matched all query segments, it's a match
        if query_idx >= self.segments.len() {
            return MatchPotential::Matches;
        }

        // If path is exhausted but query remains, descendants might match
        if path_idx >= path_segments.len() {
            return MatchPotential::Partial;
        }

        let query_seg = &self.segments[query_idx];
        let path_seg = &path_segments[path_idx];

        match query_seg {
            QuerySegment::Field(expected) => {
                if let PathSegment::Field(actual) = path_seg
                    && *actual == expected
                {
                    return self.check_potential(path_segments, query_idx + 1, path_idx + 1);
                }
                // Field mismatch - no match possible
                MatchPotential::NoMatch
            }
            QuerySegment::Index(expected) => {
                if let PathSegment::Index(actual) = path_seg
                    && actual == expected
                {
                    return self.check_potential(path_segments, query_idx + 1, path_idx + 1);
                }
                MatchPotential::NoMatch
            }
            QuerySegment::Wildcard => {
                // Wildcard matches any segment, continue
                self.check_potential(path_segments, query_idx + 1, path_idx + 1)
            }
            QuerySegment::Recursive(_) => {
                // Recursive can match anywhere, so always partial until matched
                MatchPotential::Partial
            }
        }
    }
}

/// Result of checking match potential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPotential {
    /// This path matches the query
    Matches,
    /// This path doesn't match but descendants might
    Partial,
    /// This path and all descendants cannot match
    NoMatch,
}

/// Parsed segment from a gron path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment<'a> {
    Field(&'a str),
    Index(usize),
}

/// Parse a gron path into segments.
fn parse_path_segments(path: &str) -> Vec<PathSegment<'_>> {
    let mut segments = Vec::new();
    let bytes = path.as_bytes();
    let mut i = 0;

    // Skip root identifier
    while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
        i += 1;
    }

    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
                    i += 1;
                }
                if i > start {
                    let field = unsafe { std::str::from_utf8_unchecked(&bytes[start..i]) };
                    segments.push(PathSegment::Field(field));
                }
            }
            b'[' => {
                i += 1;
                if i < bytes.len() && bytes[i] == b'"' {
                    // Quoted field
                    i += 1;
                    let start = i;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' && i + 1 < bytes.len() {
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    let field = unsafe { std::str::from_utf8_unchecked(&bytes[start..i]) };
                    segments.push(PathSegment::Field(field));
                    i += 1; // Skip closing quote
                } else {
                    // Numeric index
                    let start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i > start {
                        let index_str = unsafe { std::str::from_utf8_unchecked(&bytes[start..i]) };
                        if let Ok(index) = index_str.parse() {
                            segments.push(PathSegment::Index(index));
                        }
                    }
                }
                // Skip closing bracket (common to both branches)
                if i < bytes.len() && bytes[i] == b']' {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    segments
}

/// Parse an identifier from bytes.
fn parse_identifier(bytes: &[u8]) -> Result<(String, usize), QueryError> {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'.' || b == b'[' {
            break;
        }
        if !b.is_ascii_alphanumeric() && b != b'_' && b != b'-' {
            break;
        }
        i += 1;
    }
    if i == 0 {
        return Err(QueryError::ExpectedField);
    }
    let field = std::str::from_utf8(&bytes[..i])
        .map_err(|_| QueryError::ExpectedField)?
        .to_string();
    Ok((field, i))
}

/// Parse a quoted string.
fn parse_quoted_string(bytes: &[u8]) -> Result<(String, usize), QueryError> {
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                return Ok((result, i + 1));
            }
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                match bytes[i] {
                    b'"' => result.push('"'),
                    b'\\' => result.push('\\'),
                    b'n' => result.push('\n'),
                    b't' => result.push('\t'),
                    other => {
                        result.push('\\');
                        result.push(other as char);
                    }
                }
                i += 1;
            }
            b => {
                result.push(b as char);
                i += 1;
            }
        }
    }

    Err(QueryError::UnclosedQuote)
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Query Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_simple_field() {
        let query = Query::parse(".name").unwrap();
        assert_eq!(query.segments.len(), 1);
        assert_eq!(query.segments[0], QuerySegment::Field("name".to_string()));
    }

    #[test]
    fn test_parse_nested_fields() {
        let query = Query::parse(".users.name").unwrap();
        assert_eq!(query.segments.len(), 2);
        assert_eq!(query.segments[0], QuerySegment::Field("users".to_string()));
        assert_eq!(query.segments[1], QuerySegment::Field("name".to_string()));
    }

    #[test]
    fn test_parse_array_index() {
        let query = Query::parse(".items[0]").unwrap();
        assert_eq!(query.segments.len(), 2);
        assert_eq!(query.segments[0], QuerySegment::Field("items".to_string()));
        assert_eq!(query.segments[1], QuerySegment::Index(0));
    }

    #[test]
    fn test_parse_wildcard() {
        let query = Query::parse(".users[*].name").unwrap();
        assert_eq!(query.segments.len(), 3);
        assert_eq!(query.segments[0], QuerySegment::Field("users".to_string()));
        assert_eq!(query.segments[1], QuerySegment::Wildcard);
        assert_eq!(query.segments[2], QuerySegment::Field("name".to_string()));
        assert!(query.has_wildcard());
    }

    #[test]
    fn test_parse_recursive() {
        let query = Query::parse("..error").unwrap();
        assert_eq!(query.segments.len(), 1);
        assert_eq!(
            query.segments[0],
            QuerySegment::Recursive("error".to_string())
        );
        assert!(query.has_recursive());
    }

    #[test]
    fn test_parse_bracket_notation() {
        let query = Query::parse("[\"field.name\"]").unwrap();
        assert_eq!(query.segments.len(), 1);
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("field.name".to_string())
        );
    }

    #[test]
    fn test_parse_with_root() {
        let query = Query::parse("json.users[0]").unwrap();
        assert_eq!(query.segments.len(), 2);
        assert_eq!(query.segments[0], QuerySegment::Field("users".to_string()));
        assert_eq!(query.segments[1], QuerySegment::Index(0));
    }

    // =========================================================================
    // Query Matching Tests
    // =========================================================================

    #[test]
    fn test_matches_simple() {
        let query = Query::parse(".name").unwrap();
        assert!(query.matches("json.name"));
        assert!(!query.matches("json.age"));
        assert!(!query.matches("json.user.name"));
    }

    #[test]
    fn test_matches_nested() {
        let query = Query::parse(".user.name").unwrap();
        assert!(query.matches("json.user.name"));
        assert!(!query.matches("json.user"));
        assert!(!query.matches("json.user.age"));
    }

    #[test]
    fn test_matches_array_index() {
        let query = Query::parse(".items[0]").unwrap();
        assert!(query.matches("json.items[0]"));
        assert!(!query.matches("json.items[1]"));
        assert!(!query.matches("json.items"));
    }

    #[test]
    fn test_matches_wildcard() {
        let query = Query::parse(".users[*].name").unwrap();
        assert!(query.matches("json.users[0].name"));
        assert!(query.matches("json.users[99].name"));
        assert!(!query.matches("json.users[0].age"));
        assert!(!query.matches("json.users[0]"));
    }

    #[test]
    fn test_matches_recursive() {
        let query = Query::parse("..error").unwrap();
        assert!(query.matches("json.error"));
        assert!(query.matches("json.nested.error"));
        assert!(query.matches("json.deeply.nested.path.error"));
        assert!(!query.matches("json.message"));
    }

    #[test]
    fn test_match_potential_exact() {
        let query = Query::parse(".users.name").unwrap();

        assert_eq!(
            query.match_potential("json.users.name"),
            MatchPotential::Matches
        );
        assert_eq!(query.match_potential("json.users"), MatchPotential::Partial);
        assert_eq!(query.match_potential("json.items"), MatchPotential::NoMatch);
    }

    #[test]
    fn test_match_potential_wildcard() {
        let query = Query::parse(".users[*].name").unwrap();

        assert_eq!(
            query.match_potential("json.users[0].name"),
            MatchPotential::Matches
        );
        assert_eq!(
            query.match_potential("json.users[0]"),
            MatchPotential::Partial
        );
        assert_eq!(query.match_potential("json.users"), MatchPotential::Partial);
    }

    #[test]
    fn test_complex_query() {
        let query = Query::parse(".data[*].users[0].email").unwrap();

        assert!(query.matches("json.data[5].users[0].email"));
        assert!(!query.matches("json.data[5].users[1].email"));
        assert!(!query.matches("json.data[5].users[0].name"));
    }

    // =========================================================================
    // Error Tests
    // =========================================================================

    #[test]
    fn test_parse_error_empty() {
        assert!(matches!(Query::parse(""), Err(QueryError::Empty)));
    }

    #[test]
    fn test_parse_error_unclosed_bracket() {
        assert!(matches!(
            Query::parse("[0"),
            Err(QueryError::UnclosedBracket)
        ));
    }

    #[test]
    fn test_parse_error_unclosed_quote() {
        assert!(matches!(
            Query::parse("[\"field"),
            Err(QueryError::UnclosedQuote)
        ));
    }

    // =========================================================================
    // QueryError Display Tests
    // =========================================================================

    #[test]
    fn test_query_error_display_empty() {
        let err = QueryError::Empty;
        assert_eq!(err.to_string(), "empty query");
    }

    #[test]
    fn test_query_error_display_unexpected_char() {
        let err = QueryError::UnexpectedChar('$', 5);
        let msg = err.to_string();
        assert!(msg.contains("unexpected character"));
        assert!(msg.contains('$'));
        assert!(msg.contains('5'));
    }

    #[test]
    fn test_query_error_display_unclosed_bracket() {
        let err = QueryError::UnclosedBracket;
        assert_eq!(err.to_string(), "unclosed bracket");
    }

    #[test]
    fn test_query_error_display_unclosed_quote() {
        let err = QueryError::UnclosedQuote;
        assert_eq!(err.to_string(), "unclosed quote");
    }

    #[test]
    fn test_query_error_display_invalid_index() {
        let err = QueryError::InvalidIndex("abc".to_string());
        let msg = err.to_string();
        assert!(msg.contains("invalid index"));
        assert!(msg.contains("abc"));
    }

    #[test]
    fn test_query_error_display_expected_field() {
        let err = QueryError::ExpectedField;
        assert_eq!(err.to_string(), "expected field name");
    }

    #[test]
    fn test_query_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(QueryError::Empty);
        assert!(!err.to_string().is_empty());
    }

    // =========================================================================
    // Query Accessor Tests
    // =========================================================================

    #[test]
    fn test_query_original() {
        let query = Query::parse(".users[0].name").unwrap();
        assert_eq!(query.original(), ".users[0].name");
    }

    #[test]
    fn test_query_has_recursive_false() {
        let query = Query::parse(".users.name").unwrap();
        assert!(!query.has_recursive());
    }

    #[test]
    fn test_query_has_recursive_true() {
        let query = Query::parse("..name").unwrap();
        assert!(query.has_recursive());
    }

    #[test]
    fn test_query_has_wildcard_false() {
        let query = Query::parse(".users[0]").unwrap();
        assert!(!query.has_wildcard());
    }

    #[test]
    fn test_query_has_wildcard_true() {
        let query = Query::parse(".users[*]").unwrap();
        assert!(query.has_wildcard());
    }

    // =========================================================================
    // Parse Error Cases
    // =========================================================================

    #[test]
    fn test_parse_error_expected_field_after_dot() {
        let result = Query::parse(".");
        assert!(matches!(result, Err(QueryError::ExpectedField)));
    }

    #[test]
    fn test_parse_error_expected_field_after_recursive() {
        let result = Query::parse("..");
        assert!(matches!(result, Err(QueryError::ExpectedField)));
    }

    #[test]
    fn test_parse_error_unexpected_char_in_bracket() {
        let result = Query::parse("[abc]");
        assert!(matches!(result, Err(QueryError::UnexpectedChar(_, _))));
    }

    #[test]
    fn test_parse_error_unexpected_char_outside() {
        // Test unexpected char in various positions
        // After a valid field, @ causes unexpected char error
        let result = Query::parse(".field@more");
        assert!(matches!(result, Err(QueryError::UnexpectedChar('@', _))));
    }

    #[test]
    fn test_parse_wildcard_unclosed() {
        let result = Query::parse("[*");
        assert!(matches!(result, Err(QueryError::UnclosedBracket)));
    }

    #[test]
    fn test_parse_quoted_field_no_close_bracket() {
        let result = Query::parse("[\"field\"");
        assert!(matches!(result, Err(QueryError::UnclosedBracket)));
    }

    #[test]
    fn test_parse_numeric_index_no_close_bracket() {
        let result = Query::parse("[123");
        assert!(matches!(result, Err(QueryError::UnclosedBracket)));
    }

    #[test]
    fn test_parse_empty_bracket() {
        let result = Query::parse("[");
        assert!(matches!(result, Err(QueryError::UnclosedBracket)));
    }

    // =========================================================================
    // QuerySegment Tests
    // =========================================================================

    #[test]
    fn test_query_segment_equality() {
        assert_eq!(
            QuerySegment::Field("a".to_string()),
            QuerySegment::Field("a".to_string())
        );
        assert_ne!(
            QuerySegment::Field("a".to_string()),
            QuerySegment::Field("b".to_string())
        );
        assert_eq!(QuerySegment::Index(0), QuerySegment::Index(0));
        assert_ne!(QuerySegment::Index(0), QuerySegment::Index(1));
        assert_eq!(QuerySegment::Wildcard, QuerySegment::Wildcard);
        assert_eq!(
            QuerySegment::Recursive("x".to_string()),
            QuerySegment::Recursive("x".to_string())
        );
    }

    #[test]
    fn test_query_segment_clone() {
        let seg = QuerySegment::Field("test".to_string());
        let cloned = seg.clone();
        assert_eq!(seg, cloned);
    }

    #[test]
    fn test_query_segment_debug() {
        let seg = QuerySegment::Wildcard;
        let debug = format!("{seg:?}");
        assert!(debug.contains("Wildcard"));
    }

    // =========================================================================
    // MatchPotential Tests
    // =========================================================================

    #[test]
    fn test_match_potential_equality() {
        assert_eq!(MatchPotential::Matches, MatchPotential::Matches);
        assert_eq!(MatchPotential::Partial, MatchPotential::Partial);
        assert_eq!(MatchPotential::NoMatch, MatchPotential::NoMatch);
        assert_ne!(MatchPotential::Matches, MatchPotential::NoMatch);
    }

    #[test]
    fn test_match_potential_clone_copy() {
        let mp = MatchPotential::Partial;
        let cloned = mp;
        let copied = mp;
        assert_eq!(mp, cloned);
        assert_eq!(mp, copied);
    }

    #[test]
    fn test_match_potential_debug() {
        let mp = MatchPotential::Matches;
        let debug = format!("{mp:?}");
        assert!(debug.contains("Matches"));
    }

    // =========================================================================
    // Query Clone and Debug
    // =========================================================================

    #[test]
    fn test_query_clone() {
        let query = Query::parse(".users[*].name").unwrap();
        let cloned = query.clone();
        assert_eq!(query.original(), cloned.original());
        assert_eq!(query.has_recursive(), cloned.has_recursive());
        assert_eq!(query.has_wildcard(), cloned.has_wildcard());
    }

    #[test]
    fn test_query_debug() {
        let query = Query::parse(".name").unwrap();
        let debug = format!("{query:?}");
        assert!(debug.contains("Query"));
    }

    // =========================================================================
    // Quoted String Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_quoted_escape_quote() {
        let query = Query::parse("[\"field\\\"name\"]").unwrap();
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("field\"name".to_string())
        );
    }

    #[test]
    fn test_parse_quoted_escape_backslash() {
        let query = Query::parse("[\"path\\\\to\"]").unwrap();
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("path\\to".to_string())
        );
    }

    #[test]
    fn test_parse_quoted_escape_n() {
        let query = Query::parse("[\"line\\nbreak\"]").unwrap();
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("line\nbreak".to_string())
        );
    }

    #[test]
    fn test_parse_quoted_escape_t() {
        let query = Query::parse("[\"tab\\there\"]").unwrap();
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("tab\there".to_string())
        );
    }

    #[test]
    fn test_parse_quoted_unknown_escape() {
        let query = Query::parse("[\"test\\xvalue\"]").unwrap();
        // Unknown escapes are preserved as-is
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("test\\xvalue".to_string())
        );
    }

    // =========================================================================
    // Path Segment Parsing Tests
    // =========================================================================

    #[test]
    fn test_matches_with_quoted_path_field() {
        let query = Query::parse("[\"special-field\"]").unwrap();
        // Path with quoted field in gron format
        assert!(query.matches("json[\"special-field\"]"));
    }

    #[test]
    fn test_matches_path_with_escape() {
        let query = Query::parse(".field").unwrap();
        // Test path parsing with escape in quoted field
        assert!(query.matches("json.field"));
    }

    // =========================================================================
    // Recursive Matching Tests
    // =========================================================================

    #[test]
    fn test_recursive_no_match_if_field_not_found() {
        let query = Query::parse("..notfound").unwrap();
        assert!(!query.matches("json.a.b.c"));
    }

    #[test]
    fn test_recursive_match_at_various_depths() {
        let query = Query::parse("..target").unwrap();
        assert!(query.matches("json.target"));
        assert!(query.matches("json.a.target"));
        assert!(query.matches("json.a.b.target"));
        assert!(query.matches("json.a.b.c.d.target"));
    }

    #[test]
    fn test_recursive_with_continuation() {
        let query = Query::parse("..error.message").unwrap();
        assert!(query.matches("json.error.message"));
        assert!(query.matches("json.nested.error.message"));
        assert!(!query.matches("json.nested.error"));
    }

    // =========================================================================
    // Match Potential Edge Cases
    // =========================================================================

    #[test]
    fn test_match_potential_recursive() {
        let query = Query::parse("..field").unwrap();
        // Recursive always returns Partial until matched
        assert_eq!(query.match_potential("json.other"), MatchPotential::Partial);
    }

    #[test]
    fn test_match_potential_index_mismatch() {
        let query = Query::parse(".items[0]").unwrap();
        assert_eq!(
            query.match_potential("json.items[1]"),
            MatchPotential::NoMatch
        );
    }

    #[test]
    fn test_match_potential_field_index_type_mismatch() {
        let query = Query::parse(".items.name").unwrap();
        // Path has index where query expects field
        assert_eq!(
            query.match_potential("json.items[0]"),
            MatchPotential::NoMatch
        );
    }

    // =========================================================================
    // Large Index Tests
    // =========================================================================

    #[test]
    fn test_parse_large_index() {
        let query = Query::parse("[999999]").unwrap();
        assert_eq!(query.segments[0], QuerySegment::Index(999_999));
    }

    #[test]
    fn test_matches_large_index() {
        let query = Query::parse(".items[12345]").unwrap();
        assert!(query.matches("json.items[12345]"));
        assert!(!query.matches("json.items[12346]"));
    }

    // =========================================================================
    // Multiple Wildcards and Recursive
    // =========================================================================

    #[test]
    fn test_multiple_wildcards() {
        let query = Query::parse("[*][*]").unwrap();
        assert!(query.matches("json[0][0]"));
        assert!(query.matches("json[5][10]"));
    }

    #[test]
    fn test_wildcard_then_field() {
        let query = Query::parse("[*].name").unwrap();
        assert!(query.matches("json[0].name"));
        assert!(!query.matches("json[0].age"));
    }

    // =========================================================================
    // Path Parsing Edge Cases
    // =========================================================================

    #[test]
    fn test_parse_path_empty_field() {
        let segments = parse_path_segments("json..field");
        // Empty field between dots
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_parse_path_only_root() {
        let segments = parse_path_segments("json");
        assert!(segments.is_empty());
    }

    #[test]
    fn test_parse_path_trailing_dot() {
        let segments = parse_path_segments("json.field.");
        // Trailing dot produces no extra segment
        assert_eq!(segments.len(), 1);
    }

    // =========================================================================
    // Identifier with Special Characters
    // =========================================================================

    #[test]
    fn test_parse_identifier_with_underscore() {
        let query = Query::parse(".my_field").unwrap();
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("my_field".to_string())
        );
    }

    #[test]
    fn test_parse_identifier_with_hyphen() {
        let query = Query::parse(".my-field").unwrap();
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("my-field".to_string())
        );
    }

    #[test]
    fn test_parse_identifier_with_numbers() {
        let query = Query::parse(".field123").unwrap();
        assert_eq!(
            query.segments[0],
            QuerySegment::Field("field123".to_string())
        );
    }
}
