//! Language-specific symbol extraction.
//!
//! Each supported language implements the `LanguageSupport` trait, which defines
//! how to extract symbols, imports, and references from tree-sitter syntax trees.
//!
//! ## Adding a New Language
//!
//! 1. Add the variant to `Language` enum in `types.rs`
//! 2. Create a new module (e.g., `python.rs`)
//! 3. Implement `LanguageSupport` trait
//! 4. Register in `get_language_support()`
//!
//! ## Design
//!
//! The trait-based design allows language-specific logic while maintaining
//! a uniform interface for the indexer.

pub mod common;
pub mod csharp;
pub mod rust;
mod tree_sitter_utils;

use common::{ExtractedReference, ExtractedSymbol, ImportStatement};

use crate::types::Language;

/// Get the language support implementation for a language.
#[must_use]
pub fn get_language_support(lang: Language) -> &'static dyn LanguageSupport {
    match lang {
        Language::Rust => &rust::RustLanguage,
        Language::CSharp => &csharp::CSharpLanguage,
    }
}

/// Trait for language-specific symbol extraction.
///
/// Each supported language implements this trait to define how symbols,
/// imports, and references are extracted from tree-sitter syntax trees.
pub trait LanguageSupport: Send + Sync {
    /// Get the tree-sitter language for parsing.
    fn tree_sitter_language(&self) -> tree_sitter::Language;

    /// Extract symbols from a parsed syntax tree.
    fn extract_symbols(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedSymbol>;

    /// Extract references (usages) from a parsed syntax tree.
    fn extract_references(
        &self,
        tree: &tree_sitter::Tree,
        content: &[u8],
    ) -> Vec<ExtractedReference>;

    /// Extract import statements from a parsed syntax tree.
    fn extract_imports(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ImportStatement>;
}
