//! Rust language support for Tethys.
//!
//! Implements symbol extraction for Rust source files using tree-sitter-rust.

// Tree-sitter returns usize for positions, but we store u32 for compactness.
// This is safe for practical source files (no file has 4 billion lines).
#![allow(clippy::cast_possible_truncation)]

use super::LanguageSupport;
use super::common::{
    ExtractedAttribute, ExtractedReference, ExtractedReferenceKind, ExtractedSymbol,
    ImportStatement,
};
use super::tree_sitter_utils::{node_span, node_text};
use crate::types::{FunctionSignature, Parameter, Span, SymbolKind, Visibility};

/// Tree-sitter node kind constants for Rust grammar.
///
/// These match the node types defined in tree-sitter-rust. Using constants
/// prevents typos and makes supported node types explicit.
mod node_kinds {
    // Item declarations
    pub const FUNCTION_ITEM: &str = "function_item";
    pub const STRUCT_ITEM: &str = "struct_item";
    pub const ENUM_ITEM: &str = "enum_item";
    pub const TRAIT_ITEM: &str = "trait_item";
    pub const IMPL_ITEM: &str = "impl_item";
    pub const CONST_ITEM: &str = "const_item";
    pub const STATIC_ITEM: &str = "static_item";
    pub const TYPE_ITEM: &str = "type_item";
    pub const MACRO_DEFINITION: &str = "macro_definition";
    pub const MOD_ITEM: &str = "mod_item";
    pub const USE_DECLARATION: &str = "use_declaration";

    // Structure nodes
    pub const DECLARATION_LIST: &str = "declaration_list";
    pub const TYPE_IDENTIFIER: &str = "type_identifier";
    pub const GENERIC_TYPE: &str = "generic_type";
    pub const VISIBILITY_MODIFIER: &str = "visibility_modifier";
    pub const FUNCTION_MODIFIERS: &str = "function_modifiers";
    pub const TYPE_PARAMETERS: &str = "type_parameters";
    pub const PARAMETER: &str = "parameter";
    pub const SELF_PARAMETER: &str = "self_parameter";

    // Use statement nodes
    pub const USE_LIST: &str = "use_list";
    pub const SCOPED_USE_LIST: &str = "scoped_use_list";
    pub const USE_WILDCARD: &str = "use_wildcard";
    pub const USE_AS_CLAUSE: &str = "use_as_clause";
    pub const SCOPED_IDENTIFIER: &str = "scoped_identifier";
    pub const IDENTIFIER: &str = "identifier";
    pub const CRATE: &str = "crate";
    pub const SELF: &str = "self";
    pub const SUPER: &str = "super";

    // Modifier keywords
    pub const ASYNC: &str = "async";
    pub const UNSAFE: &str = "unsafe";
    pub const CONST: &str = "const";

    // Attribute nodes
    pub const ATTRIBUTE_ITEM: &str = "attribute_item";
    pub const ATTRIBUTE: &str = "attribute";
    pub const TOKEN_TREE: &str = "token_tree";

    // Comment nodes
    pub const LINE_COMMENT: &str = "line_comment";
    pub const BLOCK_COMMENT: &str = "block_comment";

    // Sub-symbol nodes (variants, fields)
    pub const ENUM_VARIANT_LIST: &str = "enum_variant_list";
    pub const ENUM_VARIANT: &str = "enum_variant";
    pub const FIELD_DECLARATION_LIST: &str = "field_declaration_list";
    pub const FIELD_DECLARATION: &str = "field_declaration";
    pub const ORDERED_FIELD_DECLARATION_LIST: &str = "ordered_field_declaration_list";

    // Expression nodes (for reference extraction)
    pub const CALL_EXPRESSION: &str = "call_expression";
    pub const STRUCT_EXPRESSION: &str = "struct_expression";
    pub const FIELD_EXPRESSION: &str = "field_expression";
    pub const SCOPED_TYPE_IDENTIFIER: &str = "scoped_type_identifier";
}

/// Rust language support implementation.
pub struct RustLanguage;

impl LanguageSupport for RustLanguage {
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn extract_symbols(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedSymbol> {
        extract_symbols(tree, content)
    }

    fn extract_references(
        &self,
        tree: &tree_sitter::Tree,
        content: &[u8],
    ) -> Vec<ExtractedReference> {
        extract_references(tree, content)
    }

    fn extract_imports(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ImportStatement> {
        extract_use_statements(tree, content)
            .into_iter()
            .map(|u| u.to_import_statement())
            .collect()
    }
}

/// An extracted use statement from Rust source code.
///
/// Note: This is a transient parsing type, not stored in the database.
/// Column is intentionally omitted as tree-sitter column info is inconsistent across languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseStatement {
    /// Path segments (e.g., `["crate", "auth"]` for `use crate::auth::...`)
    pub path: Vec<String>,
    /// Names being imported (e.g., `["HashMap", "HashSet"]` for `use std::collections::{HashMap, HashSet}`)
    pub imported_names: Vec<String>,
    /// Whether this is a glob import (`use foo::*`)
    pub is_glob: bool,
    /// Alias if present (e.g., "Map" for `use HashMap as Map`)
    pub alias: Option<String>,
    /// Line number where the use statement appears (1-indexed)
    pub line: u32,
}

impl UseStatement {
    /// Convert to the common `ImportStatement` representation.
    #[must_use]
    pub fn to_import_statement(&self) -> ImportStatement {
        ImportStatement {
            path: self.path.clone(),
            imported_names: self.imported_names.clone(),
            is_glob: self.is_glob,
            alias: self.alias.clone(),
            line: self.line,
        }
    }
}

/// Extract references (usages) from a Rust syntax tree.
pub fn extract_references(tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedReference> {
    let mut refs = Vec::new();
    let root = tree.root_node();

    extract_references_recursive(&root, content, &mut refs, None);

    refs
}

fn extract_references_recursive(
    node: &tree_sitter::Node,
    content: &[u8],
    refs: &mut Vec<ExtractedReference>,
    containing_span: Option<Span>,
) {
    use node_kinds::{
        CALL_EXPRESSION, DECLARATION_LIST, FUNCTION_ITEM, IMPL_ITEM, STRUCT_EXPRESSION,
        STRUCT_ITEM, TRAIT_ITEM, TYPE_IDENTIFIER, USE_DECLARATION,
    };

    match node.kind() {
        // Skip use declarations - they're handled separately
        USE_DECLARATION => return,

        CALL_EXPRESSION => {
            // Function/method call
            if let Some(ref_data) = extract_call_reference(node, content, containing_span) {
                refs.push(ref_data);
            }
        }

        STRUCT_EXPRESSION => {
            // Struct constructor: `User { name: ... }`
            if let Some(ref_data) = extract_struct_constructor(node, content, containing_span) {
                refs.push(ref_data);
            }
        }

        TYPE_IDENTIFIER => {
            // Type annotation - but only if not part of a definition
            if !is_type_definition_context(node)
                && let Some(name) = node_text(node, content)
            {
                refs.push(ExtractedReference {
                    name,
                    kind: ExtractedReferenceKind::Type,
                    line: node.start_position().row as u32 + 1,
                    column: node.start_position().column as u32 + 1,
                    path: None,
                    containing_symbol_span: containing_span,
                });
            }
        }

        // Function definitions: capture span and recurse with it
        FUNCTION_ITEM => {
            let fn_span = node_span(node);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_references_recursive(&child, content, refs, Some(fn_span));
            }
            return;
        }

        // Impl blocks: recurse into methods with their own spans
        IMPL_ITEM => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == DECLARATION_LIST {
                    // Methods inside impl get their own containing spans
                    let mut inner_cursor = child.walk();
                    for item in child.children(&mut inner_cursor) {
                        if item.kind() == FUNCTION_ITEM {
                            let method_span = node_span(&item);
                            let mut method_cursor = item.walk();
                            for method_child in item.children(&mut method_cursor) {
                                extract_references_recursive(
                                    &method_child,
                                    content,
                                    refs,
                                    Some(method_span),
                                );
                            }
                        } else {
                            extract_references_recursive(&item, content, refs, containing_span);
                        }
                    }
                } else {
                    // Type references in impl header (e.g., `impl Foo for Bar`)
                    extract_references_recursive(&child, content, refs, containing_span);
                }
            }
            return;
        }

        // Struct/trait definitions: recurse but don't set containing symbol
        STRUCT_ITEM | TRAIT_ITEM => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_references_recursive(&child, content, refs, containing_span);
            }
            return;
        }

        _ => {}
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_references_recursive(&child, content, refs, containing_span);
    }
}

/// Extract a call reference from a `call_expression` node.
fn extract_call_reference(
    node: &tree_sitter::Node,
    content: &[u8],
    containing_span: Option<Span>,
) -> Option<ExtractedReference> {
    use node_kinds::{FIELD_EXPRESSION, IDENTIFIER, SCOPED_IDENTIFIER};

    // Get the function being called
    let function = node.child_by_field_name("function")?;

    match function.kind() {
        IDENTIFIER => {
            // Simple call: `foo()`
            let Some(name) = node_text(&function, content) else {
                tracing::trace!(
                    kind = function.kind(),
                    line = function.start_position().row + 1,
                    "Failed to extract identifier text from call expression, skipping"
                );
                return None;
            };
            Some(ExtractedReference {
                name,
                kind: ExtractedReferenceKind::Call,
                line: function.start_position().row as u32 + 1,
                column: function.start_position().column as u32 + 1,
                path: None,
                containing_symbol_span: containing_span,
            })
        }
        SCOPED_IDENTIFIER => {
            // Scoped call: `crate::auth::authenticate()` or `User::new()`
            let (path, name) = parse_scoped_identifier(&function, content);
            Some(ExtractedReference {
                name,
                kind: ExtractedReferenceKind::Call,
                line: function.start_position().row as u32 + 1,
                column: function.start_position().column as u32 + 1,
                path: if path.is_empty() { None } else { Some(path) },
                containing_symbol_span: containing_span,
            })
        }
        FIELD_EXPRESSION => {
            // Method call: `user.greet()` - the method name is the "field"
            let field = function.child_by_field_name("field")?;
            let Some(name) = node_text(&field, content) else {
                tracing::trace!(
                    kind = field.kind(),
                    line = field.start_position().row + 1,
                    "Failed to extract field text from method call, skipping"
                );
                return None;
            };
            Some(ExtractedReference {
                name,
                kind: ExtractedReferenceKind::Call,
                line: field.start_position().row as u32 + 1,
                column: field.start_position().column as u32 + 1,
                path: None,
                containing_symbol_span: containing_span,
            })
        }
        _ => None,
    }
}

/// Extract a struct constructor reference from a `struct_expression` node.
fn extract_struct_constructor(
    node: &tree_sitter::Node,
    content: &[u8],
    containing_span: Option<Span>,
) -> Option<ExtractedReference> {
    use node_kinds::{SCOPED_IDENTIFIER, SCOPED_TYPE_IDENTIFIER, TYPE_IDENTIFIER};

    // The name is the "name" field which can be type_identifier or scoped_type_identifier
    let name_node = node.child_by_field_name("name")?;

    match name_node.kind() {
        TYPE_IDENTIFIER => {
            let Some(name) = node_text(&name_node, content) else {
                tracing::trace!(
                    kind = name_node.kind(),
                    line = name_node.start_position().row + 1,
                    "Failed to extract type identifier text from struct constructor, skipping"
                );
                return None;
            };
            Some(ExtractedReference {
                name,
                kind: ExtractedReferenceKind::Constructor,
                line: name_node.start_position().row as u32 + 1,
                column: name_node.start_position().column as u32 + 1,
                path: None,
                containing_symbol_span: containing_span,
            })
        }
        SCOPED_IDENTIFIER | SCOPED_TYPE_IDENTIFIER => {
            let (path, name) = parse_scoped_identifier(&name_node, content);
            Some(ExtractedReference {
                name,
                kind: ExtractedReferenceKind::Constructor,
                line: name_node.start_position().row as u32 + 1,
                column: name_node.start_position().column as u32 + 1,
                path: if path.is_empty() { None } else { Some(path) },
                containing_symbol_span: containing_span,
            })
        }
        _ => None,
    }
}

/// Check if a `type_identifier` is in a definition context (not a reference).
fn is_type_definition_context(node: &tree_sitter::Node) -> bool {
    // Walk up parents to see if we're in a definition
    let mut current = *node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            // These define the type, not reference it
            "struct_item" | "enum_item" | "trait_item" | "type_item" => {
                // Check if the type_identifier is the "name" field of the definition
                if let Some(name_node) = parent.child_by_field_name("name")
                    && name_node.id() == node.id()
                {
                    return true;
                }
            }
            // impl blocks can be definitions or references
            "impl_item" => {
                // The type after `impl` is a reference unless it's `impl Trait for Type`
                // For simplicity, treat impl types as references
                return false;
            }
            _ => {}
        }
        current = parent;
    }
    false
}

/// Extract use statements from a Rust syntax tree.
pub fn extract_use_statements(tree: &tree_sitter::Tree, content: &[u8]) -> Vec<UseStatement> {
    let mut uses = Vec::new();
    let root = tree.root_node();

    extract_use_statements_recursive(&root, content, &mut uses);

    uses
}

fn extract_use_statements_recursive(
    node: &tree_sitter::Node,
    content: &[u8],
    uses: &mut Vec<UseStatement>,
) {
    use node_kinds::USE_DECLARATION;

    if node.kind() == USE_DECLARATION {
        if let Some(use_stmt) = parse_use_declaration(node, content) {
            uses.push(use_stmt);
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            extract_use_statements_recursive(&child, content, uses);
        }
    }
}

fn parse_use_declaration(node: &tree_sitter::Node, content: &[u8]) -> Option<UseStatement> {
    use node_kinds::{
        CRATE, IDENTIFIER, SCOPED_IDENTIFIER, SCOPED_USE_LIST, SELF, SUPER, USE_AS_CLAUSE,
        USE_LIST, USE_WILDCARD,
    };

    let line = node.start_position().row as u32 + 1;

    // The use declaration has an argument child that contains the actual path/imports
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // Skip visibility modifiers, `use` keyword, etc.
        match child.kind() {
            SCOPED_IDENTIFIER => {
                // Simple use: `use std::collections::HashMap;`
                let (path, name) = parse_scoped_identifier(&child, content);
                return Some(UseStatement {
                    path,
                    imported_names: vec![name],
                    is_glob: false,
                    alias: None,
                    line,
                });
            }
            SCOPED_USE_LIST => {
                // List use: `use std::collections::{HashMap, HashSet};`
                return Some(parse_scoped_use_list(&child, content, line));
            }
            USE_AS_CLAUSE => {
                // Alias use: `use std::collections::HashMap as Map;`
                return parse_use_as_clause(&child, content, line);
            }
            IDENTIFIER | CRATE | SELF | SUPER => {
                // Simple single-segment use (rare but possible)
                let name = node_text(&child, content)?;
                return Some(UseStatement {
                    path: vec![],
                    imported_names: vec![name],
                    is_glob: false,
                    alias: None,
                    line,
                });
            }
            USE_WILDCARD => {
                // Glob use: `use std::collections::*;` - the wildcard node contains the path
                return Some(parse_use_wildcard(&child, content, line));
            }
            USE_LIST => {
                // Bare use list without path (rare)
                let names = collect_use_list_names(&child, content);
                return Some(UseStatement {
                    path: vec![],
                    imported_names: names,
                    is_glob: false,
                    alias: None,
                    line,
                });
            }
            _ => {}
        }
    }

    None
}

/// Parse a `use_wildcard` node like `std::collections::*`.
fn parse_use_wildcard(node: &tree_sitter::Node, content: &[u8], line: u32) -> UseStatement {
    use node_kinds::SCOPED_IDENTIFIER;

    let mut path = Vec::new();

    // The use_wildcard contains a scoped_identifier as its path
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == SCOPED_IDENTIFIER {
            collect_scoped_path(&child, content, &mut path);
            break;
        }
    }

    UseStatement {
        path,
        imported_names: vec![],
        is_glob: true,
        alias: None,
        line,
    }
}

/// Parse a scoped identifier like `std::collections::HashMap`.
/// Returns (`path_segments`, `final_name`).
fn parse_scoped_identifier(node: &tree_sitter::Node, content: &[u8]) -> (Vec<String>, String) {
    let mut segments = Vec::new();
    collect_scoped_path(node, content, &mut segments);

    // The last segment is the imported name, the rest is the path
    if let Some(name) = segments.pop() {
        (segments, name)
    } else {
        (vec![], String::new())
    }
}

/// Recursively collect path segments from a scoped identifier.
fn collect_scoped_path(node: &tree_sitter::Node, content: &[u8], segments: &mut Vec<String>) {
    use node_kinds::{CRATE, IDENTIFIER, SCOPED_IDENTIFIER, SELF, SUPER};

    match node.kind() {
        SCOPED_IDENTIFIER => {
            // Has "path" and "name" fields
            if let Some(path_node) = node.child_by_field_name("path") {
                collect_scoped_path(&path_node, content, segments);
            }
            if let Some(name_node) = node.child_by_field_name("name")
                && let Some(text) = node_text(&name_node, content)
            {
                segments.push(text);
            }
        }
        IDENTIFIER | CRATE | SELF | SUPER => {
            if let Some(text) = node_text(node, content) {
                segments.push(text);
            }
        }
        _ => {}
    }
}

/// Parse a scoped use list like `std::collections::{HashMap, HashSet}` or `std::collections::*`.
fn parse_scoped_use_list(node: &tree_sitter::Node, content: &[u8], line: u32) -> UseStatement {
    use node_kinds::{USE_LIST, USE_WILDCARD};

    let mut path = Vec::new();
    let mut names = Vec::new();
    let mut is_glob = false;

    // The scoped_use_list has a "path" child and a "list" child
    if let Some(path_node) = node.child_by_field_name("path") {
        collect_scoped_path(&path_node, content, &mut path);
    }

    if let Some(list_node) = node.child_by_field_name("list") {
        if list_node.kind() == USE_LIST {
            names = collect_use_list_names(&list_node, content);
        } else if list_node.kind() == USE_WILDCARD {
            is_glob = true;
        }
    }

    // Also check for wildcard directly in children (tree-sitter sometimes structures it this way)
    if !is_glob && names.is_empty() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == USE_WILDCARD {
                is_glob = true;
                break;
            }
        }
    }

    UseStatement {
        path,
        imported_names: names,
        is_glob,
        alias: None,
        line,
    }
}

/// Collect names from a use list `{A, B, C}`.
fn collect_use_list_names(node: &tree_sitter::Node, content: &[u8]) -> Vec<String> {
    use node_kinds::{CRATE, IDENTIFIER, SCOPED_IDENTIFIER, SELF, SUPER};

    let mut names = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            IDENTIFIER | CRATE | SELF | SUPER => {
                if let Some(text) = node_text(&child, content) {
                    names.push(text);
                }
            }
            SCOPED_IDENTIFIER => {
                // Nested scoped identifier - get the final name
                let (_, name) = parse_scoped_identifier(&child, content);
                if !name.is_empty() {
                    names.push(name);
                }
            }
            _ => {}
        }
    }

    names
}

/// Parse a use-as clause like `HashMap as Map`.
fn parse_use_as_clause(
    node: &tree_sitter::Node,
    content: &[u8],
    line: u32,
) -> Option<UseStatement> {
    use node_kinds::SCOPED_IDENTIFIER;

    // The use_as_clause has "path" and "alias" children
    let path_node = node.child_by_field_name("path")?;
    let alias_node = node.child_by_field_name("alias")?;

    let alias = node_text(&alias_node, content)?;

    // Parse the path - it's usually a scoped_identifier
    let (path, name) = if path_node.kind() == SCOPED_IDENTIFIER {
        parse_scoped_identifier(&path_node, content)
    } else {
        (vec![], node_text(&path_node, content)?)
    };

    Some(UseStatement {
        path,
        imported_names: vec![name],
        is_glob: false,
        alias: Some(alias),
        line,
    })
}

/// Extract symbols from a Rust syntax tree.
pub fn extract_symbols(tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let root = tree.root_node();

    extract_symbols_recursive(&root, content, &mut symbols, None);

    symbols
}

fn extract_symbols_recursive(
    node: &tree_sitter::Node,
    content: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    parent_name: Option<&str>,
) {
    use node_kinds::{
        CONST_ITEM, DECLARATION_LIST, ENUM_ITEM, FUNCTION_ITEM, IMPL_ITEM, MACRO_DEFINITION,
        MOD_ITEM, STATIC_ITEM, STRUCT_ITEM, TRAIT_ITEM, TYPE_ITEM,
    };

    match node.kind() {
        FUNCTION_ITEM => {
            if let Some(sym) = extract_function(node, content, parent_name) {
                symbols.push(sym);
            }
        }
        STRUCT_ITEM => {
            if let Some(sym) = extract_simple_definition(node, content, SymbolKind::Struct) {
                let parent = sym.name.clone();
                symbols.push(sym);
                symbols.extend(extract_struct_fields(node, &parent, content));
            }
        }
        ENUM_ITEM => {
            if let Some(sym) = extract_simple_definition(node, content, SymbolKind::Enum) {
                let parent = sym.name.clone();
                symbols.push(sym);
                symbols.extend(extract_enum_variants(node, &parent, content));
            }
        }
        TRAIT_ITEM => {
            if let Some(sym) = extract_simple_definition(node, content, SymbolKind::Trait) {
                symbols.push(sym);
            }
        }
        IMPL_ITEM => {
            // Extract the type being implemented
            let type_name = find_impl_type(node, content);

            // Recurse into impl block to find methods
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == DECLARATION_LIST {
                    let mut inner_cursor = child.walk();
                    for item in child.children(&mut inner_cursor) {
                        if item.kind() == FUNCTION_ITEM
                            && let Some(mut sym) =
                                extract_function(&item, content, type_name.as_deref())
                        {
                            sym.kind = SymbolKind::Method;
                            symbols.push(sym);
                        }
                    }
                }
            }
        }
        CONST_ITEM => {
            if let Some(sym) = extract_simple_definition(node, content, SymbolKind::Const) {
                symbols.push(sym);
            }
        }
        STATIC_ITEM => {
            if let Some(sym) = extract_simple_definition(node, content, SymbolKind::Static) {
                symbols.push(sym);
            }
        }
        TYPE_ITEM => {
            if let Some(sym) = extract_simple_definition(node, content, SymbolKind::TypeAlias) {
                symbols.push(sym);
            }
        }
        MACRO_DEFINITION => {
            if let Some(sym) = extract_macro(node, content) {
                symbols.push(sym);
            }
        }
        MOD_ITEM => {
            if let Some(sym) = extract_simple_definition(node, content, SymbolKind::Module) {
                symbols.push(sym);
            }
        }
        _ => {
            // Recurse into children for containers we don't explicitly handle
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_symbols_recursive(&child, content, symbols, parent_name);
            }
        }
    }
}

fn extract_function(
    node: &tree_sitter::Node,
    content: &[u8],
    parent_name: Option<&str>,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let Some(name) = node_text(&name_node, content) else {
        tracing::trace!(
            kind = node.kind(),
            line = node.start_position().row + 1,
            "Failed to extract function name, skipping"
        );
        return None;
    };

    let visibility = extract_visibility(node, content);
    let signature = extract_function_signature(node, content);
    let signature_details = extract_signature_details(node, content);
    let attributes = extract_preceding_attributes(node, content);
    let is_test = has_test_attribute(&attributes);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Function,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature,
        signature_details,
        visibility,
        parent_name: parent_name.map(String::from),
        is_test,
        attributes,
    })
}

/// Extract a simple symbol definition (struct, enum, trait, const, static, type alias, module).
///
/// These all follow the same pattern: name field, visibility, no signature details.
fn extract_simple_definition(
    node: &tree_sitter::Node,
    content: &[u8],
    kind: SymbolKind,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let Some(name) = node_text(&name_node, content) else {
        tracing::trace!(
            kind = ?kind,
            node_kind = node.kind(),
            line = node.start_position().row + 1,
            "Failed to extract name from simple definition, skipping"
        );
        return None;
    };
    let visibility = extract_visibility(node, content);
    let attributes = extract_preceding_attributes(node, content);

    Some(ExtractedSymbol {
        name,
        kind,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
        is_test: false, // Simple definitions (struct, enum, etc.) are never tests
        attributes,
    })
}

fn extract_macro(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let Some(name) = node_text(&name_node, content) else {
        tracing::trace!(
            kind = node.kind(),
            line = node.start_position().row + 1,
            "Failed to extract macro name, skipping"
        );
        return None;
    };

    let attributes = extract_preceding_attributes(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Macro,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility: Visibility::Public, // Default to Public; tree-sitter doesn't expose macro_export
        parent_name: None,
        is_test: false, // Macros are never tests
        attributes,
    })
}

/// Walk a struct's body and emit a `struct_field` row per field.
///
/// Returns an empty Vec for unit structs (no body field). Dispatches between
/// named-field structs (`field_declaration_list`) and tuple structs
/// (`ordered_field_declaration_list`).
fn extract_struct_fields(
    struct_item: &tree_sitter::Node,
    parent_name: &str,
    content: &[u8],
) -> Vec<ExtractedSymbol> {
    let Some(body) = struct_item.child_by_field_name("body") else {
        return Vec::new();
    };
    extract_fields_from_body(&body, parent_name, content)
}

/// Dispatches a body list to the appropriate extractor.
///
/// Used for both struct bodies and struct-shaped enum variant bodies.
fn extract_fields_from_body(
    body: &tree_sitter::Node,
    parent_name: &str,
    content: &[u8],
) -> Vec<ExtractedSymbol> {
    use node_kinds::{FIELD_DECLARATION_LIST, ORDERED_FIELD_DECLARATION_LIST};
    match body.kind() {
        FIELD_DECLARATION_LIST => extract_named_fields(body, parent_name, content),
        ORDERED_FIELD_DECLARATION_LIST => extract_tuple_fields(body, parent_name, content),
        _ => Vec::new(),
    }
}

/// Extract named fields (`pub name: T,`) into `struct_field` rows.
///
/// The field's type text lands in `signature`. Attributes preceding the field
/// (e.g. `#[source]`) are attached via the standard preceding-sibling walk.
fn extract_named_fields(
    list: &tree_sitter::Node,
    parent_name: &str,
    content: &[u8],
) -> Vec<ExtractedSymbol> {
    let mut out = Vec::new();
    let mut cursor = list.walk();
    for child in list.children(&mut cursor) {
        if child.kind() != node_kinds::FIELD_DECLARATION {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let Some(type_node) = child.child_by_field_name("type") else {
            continue;
        };
        let Some(name) = node_text(&name_node, content) else {
            continue;
        };
        let Some(type_text) = node_text(&type_node, content) else {
            continue;
        };

        out.push(ExtractedSymbol {
            name,
            kind: SymbolKind::StructField,
            line: child.start_position().row as u32 + 1,
            column: child.start_position().column as u32 + 1,
            span: Some(node_span(&child)),
            signature: Some(type_text),
            signature_details: None,
            visibility: extract_visibility(&child, content),
            parent_name: Some(parent_name.to_string()),
            is_test: false,
            attributes: extract_preceding_attributes(&child, content),
        });
    }
    out
}

/// Extract tuple fields (`pub T,`) into `struct_field` rows with positional names.
///
/// Tree-sitter-rust emits the type nodes directly as children of
/// `ordered_field_declaration_list` — there is no `ordered_field_declaration`
/// wrapper. We walk in source order and treat each named non-modifier child
/// as a type node, carrying the most-recent `visibility_modifier` forward.
fn extract_tuple_fields(
    list: &tree_sitter::Node,
    parent_name: &str,
    content: &[u8],
) -> Vec<ExtractedSymbol> {
    let mut out = Vec::new();
    let mut cursor = list.walk();
    let mut index: u32 = 0;
    let mut pending_visibility = Visibility::Private;
    for child in list.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            node_kinds::VISIBILITY_MODIFIER => {
                pending_visibility = parse_visibility_modifier_node(&child, content);
            }
            // Attributes are picked up by extract_preceding_attributes against
            // the next type node. Comments aren't fields; skipping them here
            // also preserves pending_visibility so a `pub` modifier on the
            // line above a doc comment still attaches to the type that
            // follows.
            node_kinds::ATTRIBUTE_ITEM | node_kinds::LINE_COMMENT | node_kinds::BLOCK_COMMENT => {}
            _ => {
                let Some(type_text) = node_text(&child, content) else {
                    tracing::debug!(
                        node_kind = child.kind(),
                        line = child.start_position().row + 1,
                        "Failed to extract tuple-field type text; skipping field but preserving pending visibility"
                    );
                    continue;
                };
                out.push(ExtractedSymbol {
                    name: index.to_string(),
                    kind: SymbolKind::StructField,
                    line: child.start_position().row as u32 + 1,
                    column: child.start_position().column as u32 + 1,
                    span: Some(node_span(&child)),
                    signature: Some(type_text),
                    signature_details: None,
                    visibility: pending_visibility,
                    parent_name: Some(parent_name.to_string()),
                    is_test: false,
                    attributes: extract_preceding_attributes(&child, content),
                });
                index += 1;
                pending_visibility = Visibility::Private;
            }
        }
    }
    out
}

/// Parse a `visibility_modifier` node into our `Visibility` enum.
///
/// This is the single source of truth for visibility-text matching;
/// `extract_visibility` finds the modifier child and delegates here.
///
/// Unrecognized forms (e.g. `pub(self)`, hypothetical future syntax) fall
/// through to `Private`. Defaulting to `Public` would silently over-expose
/// symbols whose visibility we can't read — exactly the silent-wrong-data
/// failure mode the schema-population contract tests in this crate guard
/// against.
fn parse_visibility_modifier_node(node: &tree_sitter::Node, content: &[u8]) -> Visibility {
    let Some(text) = node_text(node, content) else {
        return Visibility::Private;
    };
    match text.as_str() {
        "pub" => Visibility::Public,
        s if s.starts_with("pub(crate)") => Visibility::Crate,
        s if s.starts_with("pub(super)") || s.starts_with("pub(in") => Visibility::Module,
        _ => Visibility::Private,
    }
}

/// Walk an enum's body and emit one `enum_variant` row per variant, plus
/// `struct_field` rows for each variant's inner fields.
///
/// Variant fields use a qualified `parent_name` (`Enum::Variant`) so the
/// `#[source]` attribute on a variant field can be located via SQL by
/// joining `attributes` to `symbols` and filtering on the parent.
fn extract_enum_variants(
    enum_item: &tree_sitter::Node,
    parent_name: &str,
    content: &[u8],
) -> Vec<ExtractedSymbol> {
    let Some(body) = enum_item.child_by_field_name("body") else {
        return Vec::new();
    };
    if body.kind() != node_kinds::ENUM_VARIANT_LIST {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() != node_kinds::ENUM_VARIANT {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let Some(name) = node_text(&name_node, content) else {
            continue;
        };

        let signature = child
            .child_by_field_name("body")
            .and_then(|b| node_text(&b, content));

        out.push(ExtractedSymbol {
            name: name.clone(),
            kind: SymbolKind::EnumVariant,
            line: child.start_position().row as u32 + 1,
            column: child.start_position().column as u32 + 1,
            span: Some(node_span(&child)),
            signature,
            signature_details: None,
            visibility: Visibility::Public, // variants inherit the enum's visibility
            parent_name: Some(parent_name.to_string()),
            is_test: false,
            attributes: extract_preceding_attributes(&child, content),
        });

        if let Some(variant_body) = child.child_by_field_name("body") {
            let qualified = format!("{parent_name}::{name}");
            out.extend(extract_fields_from_body(&variant_body, &qualified, content));
        }
    }
    out
}

/// Find the type name being implemented in an impl block.
fn find_impl_type(node: &tree_sitter::Node, content: &[u8]) -> Option<String> {
    use node_kinds::{GENERIC_TYPE, TYPE_IDENTIFIER};

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == TYPE_IDENTIFIER {
            return node_text(&child, content);
        }
        // Handle generic types like `impl<T> Foo<T>`
        if child.kind() == GENERIC_TYPE {
            let type_node = child.child_by_field_name("type")?;
            return node_text(&type_node, content);
        }
    }
    None
}

/// Extract visibility from an item-level node (struct, enum, fn, ...).
///
/// Locates the `visibility_modifier` child and delegates the actual text
/// matching to [`parse_visibility_modifier_node`], which is the single source
/// of truth for visibility-string interpretation. Items with no modifier
/// fall through to `Private`.
fn extract_visibility(node: &tree_sitter::Node, content: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == node_kinds::VISIBILITY_MODIFIER {
            return parse_visibility_modifier_node(&child, content);
        }
    }
    Visibility::Private
}

/// Test attribute patterns to detect.
///
/// These are the common test framework attributes in Rust:
/// - `#[test]` - Standard library test
/// - `#[tokio::test]` - Tokio async test
/// - `#[async_std::test]` - async-std async test
/// - `#[rstest]` - rstest parameterized test
/// - `#[quickcheck]` - quickcheck property test
/// - `#[proptest]` - proptest property test
const TEST_ATTRIBUTES: &[&str] = &[
    "test",
    "tokio::test",
    "async_std::test",
    "rstest",
    "quickcheck",
    "proptest",
];

/// Walk preceding sibling `attribute_item` nodes and return them in source order.
///
/// In tree-sitter-rust, attributes are SIBLING nodes that precede the item they
/// annotate, not children:
///
/// ```text
/// source_file
///   attribute_item   ← #[derive(Clone)]
///   attribute_item   ← #[non_exhaustive]
///   struct_item      ← node we're attaching attributes to
/// ```
///
/// The walk runs backwards through `prev_sibling()`, collecting every
/// `attribute_item` it encounters. It transparently steps over:
///
/// - **Comments** (`line_comment`, `block_comment`) — including doc comments
///   like `///`, which sit between attributes and items in idiomatic Rust.
/// - **Visibility modifiers** (`visibility_modifier`) — emitted as a separate
///   sibling between `#[source]` and the type node in tuple fields like
///   `Foo(#[source] pub SomeError)`.
/// - **Anonymous tokens** (commas, parens, braces) — interior punctuation in
///   `field_declaration_list` and `ordered_field_declaration_list`.
///
/// The walk terminates at any other named node (a previous item, a previous
/// field, etc.), so attributes from the *previous* sibling never bleed into
/// this node. Results are reversed before returning so the caller sees them
/// in source order — earliest first.
fn extract_preceding_attributes(
    node: &tree_sitter::Node,
    content: &[u8],
) -> Vec<ExtractedAttribute> {
    let mut attrs = Vec::new();
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        match sibling.kind() {
            node_kinds::ATTRIBUTE_ITEM => {
                if let Some(attr) = extract_attribute(&sibling, content) {
                    attrs.push(attr);
                }
            }
            node_kinds::LINE_COMMENT
            | node_kinds::BLOCK_COMMENT
            | node_kinds::VISIBILITY_MODIFIER => {}
            _ if !sibling.is_named() => {}
            _ => break,
        }
        prev = sibling.prev_sibling();
    }
    attrs.reverse();
    attrs
}

/// Parse a single `attribute_item` node into its name and (optional) raw args text.
///
/// Tree shape (verified against tree-sitter-rust 0.23):
/// ```text
/// attribute_item
///   #
///   [
///   attribute
///     identifier | scoped_identifier   ← attribute name (e.g. "derive", "tauri::command")
///     token_tree [field=arguments]?    ← raw text including outer parens
///   ]
/// ```
///
/// Outer parens are stripped from `args` so `LIKE` queries can match the
/// content directly without anchoring around `(...)`. Marker attributes like
/// `#[source]` have no `token_tree` child and return `args == None`.
fn extract_attribute(attr_item: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedAttribute> {
    let mut item_cursor = attr_item.walk();
    let attribute = attr_item
        .children(&mut item_cursor)
        .find(|c| c.kind() == node_kinds::ATTRIBUTE)?;

    let mut name: Option<String> = None;
    let mut args: Option<String> = None;
    let mut attr_cursor = attribute.walk();
    for child in attribute.children(&mut attr_cursor) {
        match child.kind() {
            node_kinds::IDENTIFIER | node_kinds::SCOPED_IDENTIFIER if name.is_none() => {
                name = node_text(&child, content);
            }
            node_kinds::TOKEN_TREE => {
                let raw = node_text(&child, content)?;
                args = Some(strip_outer_parens(&raw).to_string());
            }
            _ => {}
        }
    }

    Some(ExtractedAttribute {
        name: name?,
        args,
        line: attr_item.start_position().row as u32 + 1,
    })
}

fn strip_outer_parens(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed)
}

/// Returns true if any of the supplied attributes is a recognized test marker.
fn has_test_attribute(attrs: &[ExtractedAttribute]) -> bool {
    attrs
        .iter()
        .any(|a| TEST_ATTRIBUTES.contains(&a.name.as_str()))
}

/// Extract function signature (just the declaration line without body).
fn extract_function_signature(node: &tree_sitter::Node, content: &[u8]) -> Option<String> {
    // Find the parameters and return type
    let params = node.child_by_field_name("parameters")?;
    let name = node.child_by_field_name("name")?;

    let name_text = node_text(&name, content)?;
    let params_text = node_text(&params, content)?;

    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|rt| node_text(&rt, content));

    let sig = if let Some(rt) = return_type {
        format!("fn {name_text}{params_text} {rt}")
    } else {
        format!("fn {name_text}{params_text}")
    };

    Some(sig)
}

/// Extract structured function signature details.
fn extract_signature_details(
    node: &tree_sitter::Node,
    content: &[u8],
) -> Option<FunctionSignature> {
    use node_kinds::{ASYNC, CONST, FUNCTION_MODIFIERS, TYPE_PARAMETERS, UNSAFE};

    let params_node = node.child_by_field_name("parameters")?;
    let parameters = extract_parameters(&params_node, content);

    // Extract return type - the return_type field is the type node directly
    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|rt| node_text(&rt, content));

    // Check for async, unsafe, const modifiers
    let mut is_async = false;
    let mut is_unsafe = false;
    let mut is_const = false;
    let mut generics = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            FUNCTION_MODIFIERS => {
                // Modifiers are nested inside function_modifiers
                let mut mod_cursor = child.walk();
                for mod_child in child.children(&mut mod_cursor) {
                    match mod_child.kind() {
                        ASYNC => is_async = true,
                        UNSAFE => is_unsafe = true,
                        CONST => is_const = true,
                        _ => {}
                    }
                }
            }
            TYPE_PARAMETERS => generics = node_text(&child, content),
            _ => {}
        }
    }

    Some(FunctionSignature {
        parameters,
        return_type,
        is_async,
        is_unsafe,
        is_const,
        generics,
    })
}

/// Extract parameters from a parameters node.
fn extract_parameters(params_node: &tree_sitter::Node, content: &[u8]) -> Vec<Parameter> {
    use node_kinds::{PARAMETER, SELF_PARAMETER};

    let mut parameters = Vec::new();
    let mut cursor = params_node.walk();

    for child in params_node.children(&mut cursor) {
        match child.kind() {
            PARAMETER => {
                if let Some(param) = extract_parameter(&child, content) {
                    parameters.push(param);
                }
            }
            SELF_PARAMETER => {
                // Handle &self, &mut self, self
                // Skip if we can't extract the text rather than inserting empty string
                if let Some(text) = node_text(&child, content) {
                    let kind = match text.as_str() {
                        "&mut self" => crate::types::ParameterKind::SelfMutRef,
                        "&self" => crate::types::ParameterKind::SelfRef,
                        _ => crate::types::ParameterKind::SelfValue,
                    };
                    parameters.push(Parameter {
                        name: text,
                        type_annotation: None,
                        kind,
                    });
                }
            }
            _ => {}
        }
    }

    parameters
}

/// Extract a single parameter.
fn extract_parameter(param_node: &tree_sitter::Node, content: &[u8]) -> Option<Parameter> {
    let pattern = param_node.child_by_field_name("pattern")?;
    let name = node_text(&pattern, content)?;

    let type_annotation = param_node
        .child_by_field_name("type")
        .and_then(|t| node_text(&t, content));

    Some(Parameter {
        name,
        type_annotation,
        kind: crate::types::ParameterKind::Regular,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rust(code: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("tree-sitter-rust language should be valid");
        parser
            .parse(code, None)
            .expect("parsing test code should succeed")
    }

    #[test]
    fn extracts_simple_function() {
        let code = "fn hello() {}";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[0].visibility, Visibility::Private);
    }

    #[test]
    fn extracts_public_function() {
        let code = "pub fn hello() {}";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].visibility, Visibility::Public);
    }

    #[test]
    fn extracts_struct() {
        let code = "pub struct User { name: String }";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let struct_sym = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Struct)
            .expect("struct should be extracted");
        assert_eq!(struct_sym.name, "User");
    }

    #[test]
    fn extracts_enum() {
        let code = "enum Status { Active, Inactive }";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let enum_sym = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Enum)
            .expect("enum should be extracted");
        assert_eq!(enum_sym.name, "Status");
    }

    #[test]
    fn extracts_trait() {
        let code = "pub trait Display { fn display(&self); }";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Display");
        assert_eq!(symbols[0].kind, SymbolKind::Trait);
    }

    #[test]
    fn extracts_impl_methods() {
        let code = r"
struct User {}

impl User {
    pub fn new() -> Self { User {} }
    fn private_method(&self) {}
}
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        // Should find: User (struct), new (method), private_method (method)
        assert_eq!(symbols.len(), 3);

        let struct_sym = symbols.iter().find(|s| s.name == "User").unwrap();
        assert_eq!(struct_sym.kind, SymbolKind::Struct);

        let new_sym = symbols.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(new_sym.kind, SymbolKind::Method);
        assert_eq!(new_sym.parent_name, Some("User".to_string()));
    }

    #[test]
    fn extracts_multiple_items() {
        let code = r"
pub fn foo() {}
pub fn bar() {}
struct Baz {}
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 3);

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));
        assert!(names.contains(&"Baz"));
    }

    #[test]
    fn extracts_function_signature() {
        let code = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        let sig = symbols[0].signature.as_ref().unwrap();
        assert!(sig.contains("fn add"));
        assert!(sig.contains("i32"));
    }

    #[test]
    fn extracts_structured_signature_details() {
        let code = "pub async fn fetch_user(id: i64, name: &str) -> Result<User, Error> {}";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        let details = symbols[0]
            .signature_details
            .as_ref()
            .expect("should have signature_details");

        assert!(details.is_async);
        assert!(!details.is_unsafe);
        assert!(!details.is_const);
        assert_eq!(details.parameters.len(), 2);
        assert_eq!(details.parameters[0].name, "id");
        assert_eq!(
            details.parameters[0].type_annotation,
            Some("i64".to_string())
        );
        assert_eq!(details.parameters[1].name, "name");
        assert_eq!(
            details.parameters[1].type_annotation,
            Some("&str".to_string())
        );
        assert_eq!(details.return_type, Some("Result<User, Error>".to_string()));
    }

    #[test]
    fn extracts_method_signature_with_self() {
        let code = r"
impl User {
    pub fn greet(&self, message: &str) -> String {}
}
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let method = symbols
            .iter()
            .find(|s| s.name == "greet")
            .expect("should find greet method");
        let details = method
            .signature_details
            .as_ref()
            .expect("should have signature_details");

        assert!(details.is_method());
        assert_eq!(details.param_count(), 1); // excludes &self
        assert_eq!(details.parameters[0].name, "&self");
        assert_eq!(details.parameters[1].name, "message");
    }

    #[test]
    fn signature_returns_result_helper() {
        let code = "fn save() -> Result<(), Error> {}";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let details = symbols[0].signature_details.as_ref().unwrap();
        assert!(details.returns_result());
        assert!(!details.returns_option());
    }

    #[test]
    fn signature_returns_option_helper() {
        let code = "fn find() -> Option<User> {}";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let details = symbols[0].signature_details.as_ref().unwrap();
        assert!(!details.returns_result());
        assert!(details.returns_option());
    }

    // ========================================================================
    // Use Statement Extraction Tests (Phase 2: Step 1)
    // ========================================================================

    #[test]
    fn extracts_simple_use_statement() {
        let code = "use std::collections::HashMap;";
        let tree = parse_rust(code);
        let uses = extract_use_statements(&tree, code.as_bytes());

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, vec!["std", "collections"]);
        assert_eq!(uses[0].imported_names, vec!["HashMap"]);
        assert!(!uses[0].is_glob);
        assert!(uses[0].alias.is_none());
    }

    #[test]
    fn extracts_use_with_list() {
        let code = "use std::collections::{HashMap, HashSet};";
        let tree = parse_rust(code);
        let uses = extract_use_statements(&tree, code.as_bytes());

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, vec!["std", "collections"]);
        assert!(uses[0].imported_names.contains(&"HashMap".to_string()));
        assert!(uses[0].imported_names.contains(&"HashSet".to_string()));
        assert!(!uses[0].is_glob);
    }

    #[test]
    fn extracts_crate_use() {
        let code = "use crate::auth::Authenticator;";
        let tree = parse_rust(code);
        let uses = extract_use_statements(&tree, code.as_bytes());

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, vec!["crate", "auth"]);
        assert_eq!(uses[0].imported_names, vec!["Authenticator"]);
    }

    #[test]
    fn extracts_self_use() {
        let code = "use self::inner::Helper;";
        let tree = parse_rust(code);
        let uses = extract_use_statements(&tree, code.as_bytes());

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, vec!["self", "inner"]);
        assert_eq!(uses[0].imported_names, vec!["Helper"]);
    }

    #[test]
    fn extracts_super_use() {
        let code = "use super::Config;";
        let tree = parse_rust(code);
        let uses = extract_use_statements(&tree, code.as_bytes());

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, vec!["super"]);
        assert_eq!(uses[0].imported_names, vec!["Config"]);
    }

    #[test]
    fn extracts_glob_use() {
        let code = "use std::collections::*;";
        let tree = parse_rust(code);
        let uses = extract_use_statements(&tree, code.as_bytes());

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, vec!["std", "collections"]);
        assert!(uses[0].is_glob);
        assert!(uses[0].imported_names.is_empty());
    }

    #[test]
    fn extracts_use_as_alias() {
        let code = "use std::collections::HashMap as Map;";
        let tree = parse_rust(code);
        let uses = extract_use_statements(&tree, code.as_bytes());

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].path, vec!["std", "collections"]);
        assert_eq!(uses[0].imported_names, vec!["HashMap"]);
        assert_eq!(uses[0].alias, Some("Map".to_string()));
    }

    // ========================================================================
    // Reference Extraction Tests (Phase 2: Step 3)
    // ========================================================================

    #[test]
    fn extracts_function_call() {
        let code = "fn main() { foo(); }";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let foo_ref = refs.iter().find(|r| r.name == "foo");
        assert!(foo_ref.is_some(), "should find function call to foo");
        assert_eq!(foo_ref.unwrap().kind, ExtractedReferenceKind::Call);
    }

    #[test]
    fn extracts_type_annotation() {
        let code = "fn greet(user: User) {}";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let user_ref = refs.iter().find(|r| r.name == "User");
        assert!(user_ref.is_some(), "should find type annotation User");
        assert_eq!(user_ref.unwrap().kind, ExtractedReferenceKind::Type);
    }

    #[test]
    fn extracts_struct_constructor() {
        let code = "fn new() -> User { User { name: \"test\".into() } }";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let constructor_ref = refs
            .iter()
            .find(|r| r.name == "User" && r.kind == ExtractedReferenceKind::Constructor);
        assert!(constructor_ref.is_some(), "should find struct constructor");
    }

    #[test]
    fn extracts_scoped_call() {
        let code = "fn main() { crate::auth::authenticate(); }";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let auth_ref = refs.iter().find(|r| r.name == "authenticate");
        assert!(auth_ref.is_some(), "should find scoped function call");
        assert_eq!(auth_ref.unwrap().kind, ExtractedReferenceKind::Call);
        assert!(auth_ref.unwrap().path.is_some());
    }

    #[test]
    fn extracts_associated_function_call() {
        let code = "fn create() -> User { User::new() }";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let new_ref = refs
            .iter()
            .find(|r| r.name == "new" && r.kind == ExtractedReferenceKind::Call);
        assert!(new_ref.is_some(), "should find associated function call");
    }

    #[test]
    fn tracks_containing_symbol_for_references() {
        let code = r"
fn outer() {
    foo();
}

fn another() {
    bar();
}
";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        // foo() is called from outer(), which spans lines 2-4
        let foo_ref = refs.iter().find(|r| r.name == "foo").unwrap();
        assert!(
            foo_ref.containing_symbol_span.is_some(),
            "should track containing symbol"
        );
        let outer_span = foo_ref.containing_symbol_span.as_ref().unwrap();
        assert_eq!(outer_span.start_line(), 2, "outer() starts at line 2");

        // bar() is called from another(), which starts at line 6
        let bar_ref = refs.iter().find(|r| r.name == "bar").unwrap();
        let another_span = bar_ref.containing_symbol_span.as_ref().unwrap();
        assert_eq!(another_span.start_line(), 6, "another() starts at line 6");
    }

    #[test]
    fn top_level_reference_has_no_containing_symbol() {
        // Static/const initializers at module level have no containing function
        let code = "static FOO: User = User { name: \"\" };";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let user_ref = refs
            .iter()
            .find(|r| r.name == "User" && r.kind == ExtractedReferenceKind::Constructor);
        assert!(user_ref.is_some(), "should find User constructor");
        // Top-level references have no containing symbol
        assert!(
            user_ref.unwrap().containing_symbol_span.is_none(),
            "top-level reference should not have containing symbol"
        );
    }

    #[test]
    fn references_in_closures_track_containing_function() {
        // References inside closures should point to the enclosing function
        let code = r"
fn outer() {
    let closure = || {
        helper();
    };
}
";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let helper_ref = refs
            .iter()
            .find(|r| r.name == "helper" && r.kind == ExtractedReferenceKind::Call);
        assert!(helper_ref.is_some(), "should find helper() call");

        // The reference inside the closure should have containing_symbol_span
        // pointing to the outer function
        let containing = helper_ref.unwrap().containing_symbol_span;
        assert!(
            containing.is_some(),
            "closure reference should have containing symbol span"
        );

        // The containing span should be the outer function (line 2)
        let span = containing.unwrap();
        assert_eq!(
            span.start_line(),
            2,
            "containing span should point to outer() function"
        );
    }

    #[test]
    fn references_in_nested_functions_track_inner_function() {
        // Rust allows nested function definitions - references should track the innermost
        let code = r"
fn outer() {
    fn inner() {
        helper();
    }
}
";
        let tree = parse_rust(code);
        let refs = extract_references(&tree, code.as_bytes());

        let helper_ref = refs
            .iter()
            .find(|r| r.name == "helper" && r.kind == ExtractedReferenceKind::Call);
        assert!(helper_ref.is_some(), "should find helper() call");

        // The reference should point to inner(), not outer()
        let containing = helper_ref.unwrap().containing_symbol_span;
        assert!(
            containing.is_some(),
            "nested function reference should have containing symbol span"
        );

        // The containing span should be the inner function (line 3)
        let span = containing.unwrap();
        assert_eq!(
            span.start_line(),
            3,
            "containing span should point to inner() function, not outer()"
        );
    }

    #[test]
    fn extracts_attributes_on_struct() {
        let code = r#"
#[derive(Clone, Debug)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Foo { x: i32 }
"#;
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let foo = symbols
            .iter()
            .find(|s| s.name == "Foo")
            .expect("struct Foo should be extracted");

        let names: Vec<&str> = foo.attributes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["derive", "cfg_attr"]);

        assert_eq!(
            foo.attributes[0].args.as_deref(),
            Some("Clone, Debug"),
            "derive args should be the comma list",
        );
        assert!(
            foo.attributes[1]
                .args
                .as_deref()
                .is_some_and(|a| a.contains("specta::Type")),
            "cfg_attr args should contain specta::Type; got {:?}",
            foo.attributes[1].args,
        );
    }

    #[test]
    fn marker_attribute_has_no_args() {
        let code = r"
#[non_exhaustive]
pub enum E { A, B }
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let e = symbols
            .iter()
            .find(|s| s.name == "E")
            .expect("enum E should be extracted");

        let non_exhaustive = e
            .attributes
            .iter()
            .find(|a| a.name == "non_exhaustive")
            .expect("non_exhaustive attribute should be extracted");
        assert!(
            non_exhaustive.args.is_none(),
            "marker attributes have no args; got {:?}",
            non_exhaustive.args,
        );
    }

    #[test]
    fn function_without_attributes_has_empty_vec() {
        let code = "fn plain() {}";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert!(
            symbols[0].attributes.is_empty(),
            "function without attributes should have empty Vec, got {:?}",
            symbols[0].attributes,
        );
    }

    #[test]
    fn extracts_enum_variants_with_parent_name() {
        let code = r"
pub enum Status {
    Active,
    Pending(String),
    Failed { reason: String },
}
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let variant_names: Vec<&str> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::EnumVariant)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(variant_names, vec!["Active", "Pending", "Failed"]);

        for variant in symbols.iter().filter(|s| s.kind == SymbolKind::EnumVariant) {
            assert_eq!(
                variant.parent_name.as_deref(),
                Some("Status"),
                "variant {} should have parent_name=Status",
                variant.name,
            );
        }
    }

    #[test]
    fn enum_variant_signature_captures_payload() {
        let code = r"
pub enum E {
    Unit,
    Tuple(String),
    Record { x: i32 },
}
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let unit = symbols
            .iter()
            .find(|s| s.name == "Unit" && s.kind == SymbolKind::EnumVariant)
            .expect("Unit variant should be extracted");
        assert!(
            unit.signature.is_none(),
            "unit variant has no payload, got {:?}",
            unit.signature,
        );

        let tuple = symbols
            .iter()
            .find(|s| s.name == "Tuple" && s.kind == SymbolKind::EnumVariant)
            .expect("Tuple variant should be extracted");
        assert!(
            tuple
                .signature
                .as_deref()
                .is_some_and(|s| s.contains("String")),
            "tuple variant signature should contain payload type; got {:?}",
            tuple.signature,
        );

        let record = symbols
            .iter()
            .find(|s| s.name == "Record" && s.kind == SymbolKind::EnumVariant)
            .expect("Record variant should be extracted");
        assert!(
            record
                .signature
                .as_deref()
                .is_some_and(|s| s.contains("x: i32")),
            "record variant signature should contain field; got {:?}",
            record.signature,
        );
    }

    #[test]
    fn extracts_struct_fields_with_parent_name_and_type() {
        let code = r"
pub struct User {
    pub id: u64,
    name: String,
}
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let fields: Vec<&ExtractedSymbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::StructField)
            .collect();
        assert_eq!(fields.len(), 2);

        let id = fields
            .iter()
            .find(|s| s.name == "id")
            .expect("field id should exist");
        assert_eq!(id.parent_name.as_deref(), Some("User"));
        assert_eq!(id.signature.as_deref(), Some("u64"));
        assert_eq!(id.visibility, Visibility::Public);

        let name = fields
            .iter()
            .find(|s| s.name == "name")
            .expect("field name should exist");
        assert_eq!(name.signature.as_deref(), Some("String"));
        assert_eq!(name.visibility, Visibility::Private);
    }

    #[test]
    fn tuple_struct_fields_use_position_as_name() {
        let code = "pub struct GitRef(String);";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let fields: Vec<&ExtractedSymbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::StructField)
            .collect();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "0");
        assert_eq!(fields[0].signature.as_deref(), Some("String"));
        assert_eq!(fields[0].parent_name.as_deref(), Some("GitRef"));
    }

    #[test]
    fn variant_field_source_attribute_attaches_to_field() {
        let code = r"
pub enum E {
    Bad {
        path: String,
        #[source]
        cause: std::io::Error,
    },
}
";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let cause = symbols
            .iter()
            .find(|s| s.name == "cause" && s.kind == SymbolKind::StructField)
            .expect("variant field 'cause' should be extracted");

        assert_eq!(cause.parent_name.as_deref(), Some("E::Bad"));
        assert!(
            cause
                .signature
                .as_deref()
                .is_some_and(|s| s.contains("io::Error")),
            "field signature should contain io::Error; got {:?}",
            cause.signature,
        );
        let attr_names: Vec<&str> = cause.attributes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(attr_names, vec!["source"]);
    }
}
