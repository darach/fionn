// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kind predicates for query path filtering
//!
//! This module provides the predicate types and parsing logic for
//! the `::` kind predicate syntax in query paths:
//!
//! - `::string` - Universal value type filter
//! - `::yaml:anchor` - Namespaced format-specific filter
//! - `::allow-loss(comments)` - Loss waiver annotation
//! - `::require-lossless` - Strict lossless requirement

use crate::format::{FormatSpecificKind, NodeKind, ParsingContext};

/// Kind predicate for filtering nodes by type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindPredicate {
    /// Match a specific node kind
    Kind(NodeKind),

    /// Match any of several kinds (union)
    Union(Vec<NodeKind>),

    /// Match all except this kind (negation)
    Not(Box<Self>),

    /// Context-aware filter
    Context(ContextPredicate),

    /// Format-specific predicate (namespaced)
    FormatSpecific(FormatSpecificKind),
}

/// Context-aware predicates for string/comment/quote filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPredicate {
    /// Inside a string literal
    InString,
    /// Outside a string literal
    OutsideString,
    /// Inside a comment
    InComment,
    /// Outside a comment
    OutsideComment,
    /// Quoted value
    Quoted,
    /// Unquoted value
    Unquoted,
    /// Escaped sequence
    Escaped,
    /// Unescaped content
    Unescaped,
}

/// Transformation fidelity annotation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FidelityAnnotation {
    /// Allow loss of specified categories
    AllowLoss(Vec<LossCategory>),
    /// Require lossless transformation
    RequireLossless,
}

/// Categories of information that can be lost in transformation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LossCategory {
    /// Structural elements (anchors, merge keys)
    Structural,
    /// Syntactic elements (dotted keys, inline tables)
    Syntactic,
    /// Type coercion (datetime → string)
    TypeCoercion,
    /// Comments
    Comments,
    /// References (YAML aliases, ISON refs)
    References,
    /// Key ordering
    Ordering,
}

/// Parsed predicate from path suffix
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedPredicate {
    /// Kind predicates (can be multiple, combined with AND)
    pub kind_predicates: Vec<KindPredicate>,
    /// Fidelity annotation (optional)
    pub fidelity: Option<FidelityAnnotation>,
}

impl KindPredicate {
    /// Check if a node kind matches this predicate
    #[must_use]
    pub fn matches(&self, kind: NodeKind, context: ParsingContext) -> bool {
        match self {
            Self::Kind(expected) => expected.matches(kind),
            Self::Union(kinds) => kinds.iter().any(|k| k.matches(kind)),
            Self::Not(inner) => !inner.matches(kind, context),
            Self::Context(ctx_pred) => ctx_pred.matches(context),
            Self::FormatSpecific(fsk) => {
                matches!(kind, NodeKind::FormatSpecific(k) if k == *fsk)
            }
        }
    }

    /// Parse a kind predicate from string
    ///
    /// Supports:
    /// - Universal: `string`, `number`, `object`, etc.
    /// - Semantic: `reference`, `header`, `comment`
    /// - Context: `in-string`, `outside-comment`, `quoted`
    /// - Negation: `!comment`
    /// - Union: `(string|number)`
    /// - Namespaced: `yaml:anchor`, `toml:section-header`
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        // Negation
        if let Some(rest) = s.strip_prefix('!') {
            return Self::parse(rest).map(|p| Self::Not(Box::new(p)));
        }

        // Union
        if s.starts_with('(') && s.ends_with(')') {
            let inner = &s[1..s.len() - 1];
            let kinds: Vec<_> = inner
                .split('|')
                .filter_map(|part| Self::parse_single_kind(part.trim()))
                .collect();
            if kinds.is_empty() {
                return None;
            }
            return Some(Self::Union(kinds));
        }

        // Namespaced format-specific
        if let Some((format, predicate)) = s.split_once(':') {
            if let Some(fsk) = FormatSpecificKind::from_namespaced(format, predicate) {
                return Some(Self::FormatSpecific(fsk));
            }
            // Unknown namespace
            return None;
        }

        // Context predicates
        if let Some(ctx) = ContextPredicate::parse(s) {
            return Some(Self::Context(ctx));
        }

        // Universal kind
        Self::parse_single_kind(s).map(Self::Kind)
    }

    /// Parse a single kind name (no modifiers)
    fn parse_single_kind(s: &str) -> Option<NodeKind> {
        match s {
            // Value types
            "string" => Some(NodeKind::String),
            "number" => Some(NodeKind::Number),
            "boolean" | "bool" => Some(NodeKind::Boolean),
            "null" => Some(NodeKind::Null),
            "scalar" => Some(NodeKind::Scalar),

            // Structural types
            "object" => Some(NodeKind::Object),
            "array" => Some(NodeKind::Array),
            "key" => Some(NodeKind::Key),
            "value" => Some(NodeKind::Value),

            // Semantic categories
            "comment" => Some(NodeKind::Comment),
            "reference" | "ref" => Some(NodeKind::Reference),
            "definition" | "def" => Some(NodeKind::Definition),
            "header" => Some(NodeKind::Header),
            "row" => Some(NodeKind::Row),
            "section" => Some(NodeKind::Section),

            _ => None,
        }
    }
}

impl ContextPredicate {
    /// Check if a parsing context matches this predicate
    #[must_use]
    pub const fn matches(self, context: ParsingContext) -> bool {
        match self {
            Self::InString => context.is_in_string(),
            Self::OutsideString => !context.is_in_string(),
            Self::InComment => context.is_in_comment(),
            Self::OutsideComment => !context.is_in_comment(),
            Self::Quoted => context.is_quoted(),
            Self::Unquoted => !context.is_quoted(),
            Self::Escaped => context.is_escaped(),
            Self::Unescaped => !context.is_escaped(),
        }
    }

    /// Parse a context predicate from string
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "in-string" => Some(Self::InString),
            "outside-string" => Some(Self::OutsideString),
            "in-comment" => Some(Self::InComment),
            "outside-comment" => Some(Self::OutsideComment),
            "quoted" => Some(Self::Quoted),
            "unquoted" => Some(Self::Unquoted),
            "escaped" => Some(Self::Escaped),
            "unescaped" => Some(Self::Unescaped),
            _ => None,
        }
    }
}

impl LossCategory {
    /// Parse a loss category from string
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "structural" => Some(Self::Structural),
            "syntactic" => Some(Self::Syntactic),
            "type" | "type-coercion" => Some(Self::TypeCoercion),
            "comments" => Some(Self::Comments),
            "references" | "refs" => Some(Self::References),
            "ordering" => Some(Self::Ordering),
            _ => None,
        }
    }

    /// Get the category name
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Structural => "structural",
            Self::Syntactic => "syntactic",
            Self::TypeCoercion => "type-coercion",
            Self::Comments => "comments",
            Self::References => "references",
            Self::Ordering => "ordering",
        }
    }
}

impl FidelityAnnotation {
    /// Parse a fidelity annotation from string
    ///
    /// Supports:
    /// - `allow-loss(comments)` - Allow loss of comments
    /// - `allow-loss(structural,syntactic)` - Allow multiple categories
    /// - `require-lossless` - Require lossless transformation
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        if s == "require-lossless" {
            return Some(Self::RequireLossless);
        }

        if let Some(inner) = s
            .strip_prefix("allow-loss(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let categories: Vec<_> = inner
                .split(',')
                .filter_map(|cat| LossCategory::parse(cat.trim()))
                .collect();
            if categories.is_empty() {
                return None;
            }
            return Some(Self::AllowLoss(categories));
        }

        None
    }
}

impl ParsedPredicate {
    /// Parse all predicates from a `::` suffix string
    ///
    /// The suffix can contain multiple predicates separated by `::`:
    /// - `::string::outside-comment`
    /// - `::yaml:anchor::allow-loss(structural)`
    #[must_use]
    pub fn parse(suffix: &str) -> Option<Self> {
        let parts: Vec<_> = suffix.split("::").filter(|s| !s.is_empty()).collect();

        if parts.is_empty() {
            return None;
        }

        let mut kind_predicates = Vec::new();
        let mut fidelity = None;

        for part in parts {
            // Try fidelity annotation first
            if let Some(annot) = FidelityAnnotation::parse(part) {
                fidelity = Some(annot);
                continue;
            }

            // Try kind predicate
            if let Some(pred) = KindPredicate::parse(part) {
                kind_predicates.push(pred);
            }
            // Unknown predicate - ignore or error?
            // For now, ignore unknown predicates for forward compatibility
        }

        if kind_predicates.is_empty() && fidelity.is_none() {
            return None;
        }

        Some(Self {
            kind_predicates,
            fidelity,
        })
    }

    /// Check if empty (no predicates)
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::is_empty not const in stable
    pub fn is_empty(&self) -> bool {
        self.kind_predicates.is_empty() && self.fidelity.is_none()
    }

    /// Check if a node matches all kind predicates
    #[must_use]
    pub fn matches_kind(&self, kind: NodeKind, context: ParsingContext) -> bool {
        self.kind_predicates
            .iter()
            .all(|p| p.matches(kind, context))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_universal_kinds() {
        assert!(matches!(
            KindPredicate::parse("string"),
            Some(KindPredicate::Kind(NodeKind::String))
        ));
        assert!(matches!(
            KindPredicate::parse("number"),
            Some(KindPredicate::Kind(NodeKind::Number))
        ));
        assert!(matches!(
            KindPredicate::parse("object"),
            Some(KindPredicate::Kind(NodeKind::Object))
        ));
    }

    #[test]
    fn test_parse_negation() {
        let pred = KindPredicate::parse("!comment").unwrap();
        assert!(matches!(pred, KindPredicate::Not(_)));
    }

    #[test]
    fn test_parse_union() {
        let pred = KindPredicate::parse("(string|number)").unwrap();
        assert!(matches!(pred, KindPredicate::Union(v) if v.len() == 2));
    }

    #[test]
    fn test_parse_context() {
        let pred = KindPredicate::parse("in-comment").unwrap();
        assert!(matches!(
            pred,
            KindPredicate::Context(ContextPredicate::InComment)
        ));
    }

    #[test]
    fn test_parse_fidelity() {
        let annot = FidelityAnnotation::parse("allow-loss(comments)").unwrap();
        assert!(matches!(annot, FidelityAnnotation::AllowLoss(v) if v.len() == 1));

        let annot = FidelityAnnotation::parse("require-lossless").unwrap();
        assert!(matches!(annot, FidelityAnnotation::RequireLossless));
    }

    #[test]
    fn test_parse_predicate_chain() {
        let parsed = ParsedPredicate::parse("string::outside-comment").unwrap();
        assert_eq!(parsed.kind_predicates.len(), 2);
        assert!(parsed.fidelity.is_none());
    }

    #[test]
    fn test_parse_predicate_with_fidelity() {
        let parsed = ParsedPredicate::parse("string::allow-loss(comments)").unwrap();
        assert_eq!(parsed.kind_predicates.len(), 1);
        assert!(parsed.fidelity.is_some());
    }

    #[test]
    fn test_kind_matches() {
        let pred = KindPredicate::Kind(NodeKind::Scalar);
        assert!(pred.matches(NodeKind::String, ParsingContext::Normal));
        assert!(pred.matches(NodeKind::Number, ParsingContext::Normal));
        assert!(!pred.matches(NodeKind::Object, ParsingContext::Normal));
    }

    #[test]
    fn test_context_matches() {
        let pred = KindPredicate::Context(ContextPredicate::InComment);
        assert!(pred.matches(NodeKind::String, ParsingContext::InComment));
        assert!(!pred.matches(NodeKind::String, ParsingContext::Normal));
    }
}
