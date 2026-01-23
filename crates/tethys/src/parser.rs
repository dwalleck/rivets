//! Tree-sitter parsing coordination.
//!
//! This module manages tree-sitter parsers for supported languages and coordinates
//! the parsing of source files into syntax trees.
//!
//! ## Responsibilities
//!
//! - Maintain parser instances per language
//! - Parse source files into tree-sitter trees
//! - Coordinate with language-specific symbol extractors
//!
//! ## Design
//!
//! The parser is stateful (tree-sitter parsers maintain internal state for
//! incremental parsing), so we keep one parser per language and reuse them.

// TODO: Phase 1 implementation
// - Parser struct holding tree-sitter parsers per language
// - parse(path, content) -> Tree method
// - Language detection from file extension
