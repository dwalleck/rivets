//! Shared tree-sitter utilities for language support modules.
//!
//! Provides common functions for extracting text and positions from tree-sitter nodes.
//! Used by all language-specific extraction implementations.

// Tree-sitter returns usize for positions, but we store u32 for compactness.
// This is safe for practical source files (no file has 4 billion lines).
#![allow(clippy::cast_possible_truncation)]

use crate::types::Span;

/// Get text content of a tree-sitter node.
///
/// Returns `None` if the node's byte range contains invalid UTF-8.
pub fn node_text(node: &tree_sitter::Node, content: &[u8]) -> Option<String> {
    match std::str::from_utf8(&content[node.byte_range()]) {
        Ok(s) => Some(s.to_string()),
        Err(e) => {
            tracing::trace!(
                byte_range = ?node.byte_range(),
                error = %e,
                node_kind = %node.kind(),
                "Failed to decode node text as UTF-8"
            );
            None
        }
    }
}

/// Convert tree-sitter positions to our Span type.
///
/// Tree-sitter uses 0-indexed positions; Span uses 1-indexed.
/// Falls back to a single-character span if the node produces invalid positions.
pub fn node_span(node: &tree_sitter::Node) -> Span {
    let start_line = node.start_position().row as u32 + 1;
    let start_col = node.start_position().column as u32 + 1;
    let end_line = node.end_position().row as u32 + 1;
    let end_col = node.end_position().column as u32 + 1;

    Span::new(start_line, start_col, end_line, end_col).unwrap_or_else(|| {
        tracing::warn!(
            start_line,
            start_col,
            end_line,
            end_col,
            node_kind = %node.kind(),
            "Tree-sitter produced invalid span, using fallback"
        );
        // Fallback: single-character span at start position
        Span::new(start_line, start_col, start_line, start_col + 1)
            .expect("fallback span is always valid")
    })
}
