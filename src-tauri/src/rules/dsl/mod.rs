//! DSL parser and serializer for the loot filter.
//!
//! Grammar (see `docs/filter_spec/loot-filter-dsl.md`):
//!
//! ```text
//! filter      := line*
//! line        := blank | comment | rule | group_open | group_close
//! comment     := '#' any*
//! rule        := [name] attr*
//! group_open  := '[' attr* ']' '{'
//! group_close := '}'
//! name        := '"' regex '"'
//! ```
//!
//! The parser is intentionally lenient: unknown tokens produce a
//! [`ValidationError::Warning`] but do not abort parsing, so an editor can
//! still render and reason about partially-typed rules.

mod attributes;
mod parsing;
mod tokens;
mod validation;

pub use parsing::parse_dsl;
pub(super) use parsing::{classify_line, ParsedLine};
pub use validation::validate_dsl;

use super::{
    FilterConfig, ItemQuality, ItemTier, NotifyColor, PlayerClass, Rule, UniqueKind, Visibility,
};
use serde::{Deserialize, Serialize};

// =====================================================================
// Error types
// =====================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}

// Keep the existing suite beneath the DSL interface, with unchanged names.
#[cfg(test)]
#[path = "../dsl_tests.rs"]
mod tests;
