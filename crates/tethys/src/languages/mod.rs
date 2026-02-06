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

use common::{ExtractedReference, ExtractedSymbol, ImportContext, ImportStatement};

use crate::types::Language;

/// Get the language support implementation for a language.
///
/// Returns `None` for languages that are declared but not yet implemented.
#[must_use]
#[allow(clippy::unnecessary_wraps)] // Option return is intentional for future language stubs
pub fn get_language_support(lang: Language) -> Option<&'static dyn LanguageSupport> {
    match lang {
        Language::Rust => Some(&rust::RustLanguage),
        Language::CSharp => Some(&csharp::CSharpLanguage),
    }
}

/// Trait for language-specific symbol extraction.
///
/// Each supported language implements this trait to define how symbols,
/// imports, and references are extracted from tree-sitter syntax trees.
#[allow(dead_code)] // Some trait methods (extensions, lsp_command, resolve_import) not yet used
pub trait LanguageSupport: Send + Sync {
    /// File extensions this language handles.
    fn extensions(&self) -> &[&str];

    /// Get the tree-sitter language for parsing.
    fn tree_sitter_language(&self) -> tree_sitter::Language;

    /// LSP server command, if available.
    ///
    /// Returns `None` if no LSP is configured for this language.
    fn lsp_command(&self) -> Option<&str>;

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

    /// Resolve an import statement to file paths within the workspace.
    ///
    /// Given an import and a context containing workspace information, returns
    /// the paths that this import resolves to. Returns an empty vec for
    /// unresolvable imports (e.g., external dependencies).
    fn resolve_import(
        &self,
        import: &ImportStatement,
        context: &ImportContext,
    ) -> Vec<std::path::PathBuf>;
}
