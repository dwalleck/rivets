//! C# language support for Tethys.
//!
//! Implements symbol extraction for C# source files using tree-sitter-c-sharp.

// Tree-sitter returns usize for positions, but we store u32 for compactness.
// This is safe for practical source files (no file has 4 billion lines).
#![allow(clippy::cast_possible_truncation)]

use std::path::PathBuf;

use super::common::{
    ExtractedReference, ExtractedReferenceKind, ExtractedSymbol, ImportContext, ImportStatement,
};
use super::tree_sitter_utils::{node_span, node_text};
use super::LanguageSupport;
use crate::types::{FunctionSignature, Parameter, Span, SymbolKind, Visibility};

/// Tree-sitter node kind constants for C# grammar.
///
/// These match the node types defined in tree-sitter-c-sharp. Using constants
/// prevents typos and makes supported node types explicit.
mod node_kinds {
    // Type declarations
    pub const CLASS_DECLARATION: &str = "class_declaration";
    pub const STRUCT_DECLARATION: &str = "struct_declaration";
    pub const INTERFACE_DECLARATION: &str = "interface_declaration";
    pub const ENUM_DECLARATION: &str = "enum_declaration";
    pub const RECORD_DECLARATION: &str = "record_declaration";

    // Members
    pub const METHOD_DECLARATION: &str = "method_declaration";
    pub const CONSTRUCTOR_DECLARATION: &str = "constructor_declaration";

    // Namespaces & imports
    pub const NAMESPACE_DECLARATION: &str = "namespace_declaration";
    pub const FILE_SCOPED_NAMESPACE_DECLARATION: &str = "file_scoped_namespace_declaration";
    pub const USING_DIRECTIVE: &str = "using_directive";

    // Expressions
    pub const INVOCATION_EXPRESSION: &str = "invocation_expression";
    pub const OBJECT_CREATION_EXPRESSION: &str = "object_creation_expression";
    pub const MEMBER_ACCESS_EXPRESSION: &str = "member_access_expression";

    // Types & identifiers
    pub const IDENTIFIER: &str = "identifier";
    pub const QUALIFIED_NAME: &str = "qualified_name";
    pub const GENERIC_NAME: &str = "generic_name";

    // Structure
    pub const DECLARATION_LIST: &str = "declaration_list";
    pub const PARAMETER_LIST: &str = "parameter_list";
    pub const PARAMETER: &str = "parameter";
    pub const MODIFIER: &str = "modifier";

    // Keywords used as modifiers
    pub const PUBLIC: &str = "public";
    pub const PRIVATE: &str = "private";
    pub const PROTECTED: &str = "protected";
    pub const INTERNAL: &str = "internal";

    // Attribute nodes
    pub const ATTRIBUTE_LIST: &str = "attribute_list";
    pub const ATTRIBUTE: &str = "attribute";
}

/// C# language support implementation.
pub struct CSharpLanguage;

impl LanguageSupport for CSharpLanguage {
    fn extensions(&self) -> &[&str] {
        &["cs"]
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_c_sharp::LANGUAGE.into()
    }

    fn lsp_command(&self) -> Option<&str> {
        Some("csharp-ls")
    }

    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        content: &[u8],
    ) -> Vec<super::common::ExtractedSymbol> {
        extract_symbols(tree, content)
    }

    fn extract_references(
        &self,
        tree: &tree_sitter::Tree,
        content: &[u8],
    ) -> Vec<super::common::ExtractedReference> {
        extract_references(tree, content)
    }

    fn extract_imports(
        &self,
        tree: &tree_sitter::Tree,
        content: &[u8],
    ) -> Vec<super::common::ImportStatement> {
        extract_using_directives(tree, content)
            .into_iter()
            .map(|u| u.to_import_statement())
            .collect()
    }

    fn resolve_import(&self, _import: &ImportStatement, _context: &ImportContext) -> Vec<PathBuf> {
        // TODO: Implement namespace resolution (Task 6)
        vec![]
    }
}

/// An extracted using directive from C# source code.
///
/// Note: This is a transient parsing type, not stored in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsingDirective {
    /// Namespace segments (e.g., `["System", "Collections", "Generic"]`)
    pub namespace: Vec<String>,
    /// Alias if present (e.g., "Map" for `using Map = System.Collections.Generic.Dictionary;`)
    pub alias: Option<String>,
    /// Whether this is a static using (`using static System.Math;`)
    pub is_static: bool,
    /// Line number where the using directive appears (1-indexed)
    pub line: u32,
}

impl UsingDirective {
    /// Convert to the common `ImportStatement` representation.
    #[must_use]
    pub fn to_import_statement(&self) -> ImportStatement {
        ImportStatement {
            path: self.namespace.clone(),
            imported_names: Vec::new(),
            is_glob: false,
            alias: self.alias.clone(),
            line: self.line,
        }
    }
}

/// Extract references (usages) from a C# syntax tree.
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
        CLASS_DECLARATION, CONSTRUCTOR_DECLARATION, DECLARATION_LIST, INTERFACE_DECLARATION,
        INVOCATION_EXPRESSION, METHOD_DECLARATION, OBJECT_CREATION_EXPRESSION, STRUCT_DECLARATION,
        USING_DIRECTIVE,
    };

    match node.kind() {
        // Skip using directives - they're handled separately
        USING_DIRECTIVE => return,

        INVOCATION_EXPRESSION => {
            // Method call
            if let Some(mut ref_data) = extract_invocation_reference(node, content) {
                ref_data.containing_symbol_span = containing_span;
                refs.push(ref_data);
            }
        }

        OBJECT_CREATION_EXPRESSION => {
            // Constructor call: `new User()`
            if let Some(mut ref_data) = extract_object_creation(node, content) {
                ref_data.containing_symbol_span = containing_span;
                refs.push(ref_data);
            }
        }

        // Method definitions: capture span and recurse with it
        METHOD_DECLARATION | CONSTRUCTOR_DECLARATION => {
            let method_span = node_span(node);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_references_recursive(&child, content, refs, Some(method_span));
            }
            return;
        }

        // Class/struct/interface definitions: recurse into methods with their own spans
        CLASS_DECLARATION | STRUCT_DECLARATION | INTERFACE_DECLARATION => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == DECLARATION_LIST {
                    let mut inner_cursor = child.walk();
                    for item in child.children(&mut inner_cursor) {
                        match item.kind() {
                            METHOD_DECLARATION | CONSTRUCTOR_DECLARATION => {
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
                            }
                            _ => {
                                extract_references_recursive(&item, content, refs, containing_span);
                            }
                        }
                    }
                } else {
                    // Type references in class header (e.g., base class, interfaces)
                    extract_references_recursive(&child, content, refs, containing_span);
                }
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

/// Extract an invocation reference from an `invocation_expression` node.
fn extract_invocation_reference(
    node: &tree_sitter::Node,
    content: &[u8],
) -> Option<ExtractedReference> {
    use node_kinds::{IDENTIFIER, MEMBER_ACCESS_EXPRESSION};

    // The function being called is typically the first child
    let function = node.child(0)?;

    match function.kind() {
        IDENTIFIER => {
            // Simple call: `Foo()`
            let Some(name) = node_text(&function, content) else {
                tracing::trace!(
                    kind = function.kind(),
                    line = function.start_position().row + 1,
                    "Failed to extract identifier text from invocation expression, skipping"
                );
                return None;
            };
            Some(ExtractedReference {
                name,
                kind: ExtractedReferenceKind::Call,
                line: function.start_position().row as u32 + 1,
                column: function.start_position().column as u32 + 1,
                path: None,
                containing_symbol_span: None,
            })
        }
        MEMBER_ACCESS_EXPRESSION => {
            // Member access call: `obj.Method()` or `System.Console.WriteLine()`
            let (path, name) = parse_member_access(&function, content)?;
            Some(ExtractedReference {
                name,
                kind: ExtractedReferenceKind::Call,
                line: function.start_position().row as u32 + 1,
                column: function.start_position().column as u32 + 1,
                path: if path.is_empty() { None } else { Some(path) },
                containing_symbol_span: None,
            })
        }
        _ => None,
    }
}

/// Extract an object creation reference from an `object_creation_expression` node.
fn extract_object_creation(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedReference> {
    use node_kinds::{GENERIC_NAME, IDENTIFIER, QUALIFIED_NAME};

    // Find the type being constructed - it's after the "new" keyword
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            IDENTIFIER => {
                let Some(name) = node_text(&child, content) else {
                    tracing::trace!(
                        kind = child.kind(),
                        line = child.start_position().row + 1,
                        "Failed to extract identifier from object creation, skipping"
                    );
                    continue;
                };
                return Some(ExtractedReference {
                    name,
                    kind: ExtractedReferenceKind::Constructor,
                    line: child.start_position().row as u32 + 1,
                    column: child.start_position().column as u32 + 1,
                    path: None,
                    containing_symbol_span: None,
                });
            }
            QUALIFIED_NAME => {
                let (path, name) = parse_qualified_name(&child, content)?;
                return Some(ExtractedReference {
                    name,
                    kind: ExtractedReferenceKind::Constructor,
                    line: child.start_position().row as u32 + 1,
                    column: child.start_position().column as u32 + 1,
                    path: if path.is_empty() { None } else { Some(path) },
                    containing_symbol_span: None,
                });
            }
            GENERIC_NAME => {
                // Generic type like `List<string>`
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, content) {
                        return Some(ExtractedReference {
                            name,
                            kind: ExtractedReferenceKind::Constructor,
                            line: child.start_position().row as u32 + 1,
                            column: child.start_position().column as u32 + 1,
                            path: None,
                            containing_symbol_span: None,
                        });
                    }
                    tracing::trace!(
                        kind = name_node.kind(),
                        line = name_node.start_position().row + 1,
                        "Failed to extract generic type name from object creation, trying fallback"
                    );
                }
                // Fallback: get the identifier child
                let mut inner_cursor = child.walk();
                for inner_child in child.children(&mut inner_cursor) {
                    if inner_child.kind() == IDENTIFIER {
                        let Some(name) = node_text(&inner_child, content) else {
                            tracing::trace!(
                                kind = inner_child.kind(),
                                line = inner_child.start_position().row + 1,
                                "Failed to extract identifier from generic type fallback, skipping"
                            );
                            continue;
                        };
                        return Some(ExtractedReference {
                            name,
                            kind: ExtractedReferenceKind::Constructor,
                            line: child.start_position().row as u32 + 1,
                            column: child.start_position().column as u32 + 1,
                            path: None,
                            containing_symbol_span: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    None
}

/// Parse a member access expression like `obj.Method` or `System.Console.WriteLine`.
/// Returns `None` if the expression cannot be parsed.
fn parse_member_access(node: &tree_sitter::Node, content: &[u8]) -> Option<(Vec<String>, String)> {
    let mut segments = Vec::new();
    collect_member_access_path(node, content, &mut segments);

    // The last segment is the member name, the rest is the path
    let name = segments.pop()?;
    if name.is_empty() {
        return None;
    }
    Some((segments, name))
}

/// Recursively collect path segments from a member access expression.
fn collect_member_access_path(
    node: &tree_sitter::Node,
    content: &[u8],
    segments: &mut Vec<String>,
) {
    use node_kinds::{IDENTIFIER, MEMBER_ACCESS_EXPRESSION};

    match node.kind() {
        MEMBER_ACCESS_EXPRESSION => {
            // Has "expression" and "name" parts
            if let Some(expr_node) = node.child_by_field_name("expression") {
                collect_member_access_path(&expr_node, content, segments);
            }
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Some(text) = node_text(&name_node, content) {
                    segments.push(text);
                }
            }
        }
        IDENTIFIER => {
            if let Some(text) = node_text(node, content) {
                segments.push(text);
            }
        }
        _ => {}
    }
}

/// Parse a qualified name like `System.Collections.Generic.List`.
/// Returns `None` if the name cannot be parsed.
fn parse_qualified_name(node: &tree_sitter::Node, content: &[u8]) -> Option<(Vec<String>, String)> {
    let mut segments = Vec::new();
    collect_qualified_name_path(node, content, &mut segments);

    // The last segment is the type name, the rest is the path
    let name = segments.pop()?;
    if name.is_empty() {
        return None;
    }
    Some((segments, name))
}

/// Recursively collect path segments from a qualified name.
fn collect_qualified_name_path(
    node: &tree_sitter::Node,
    content: &[u8],
    segments: &mut Vec<String>,
) {
    use node_kinds::{IDENTIFIER, QUALIFIED_NAME};

    match node.kind() {
        QUALIFIED_NAME => {
            // Has "qualifier" and "name" parts
            if let Some(qualifier) = node.child_by_field_name("qualifier") {
                collect_qualified_name_path(&qualifier, content, segments);
            }
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Some(text) = node_text(&name_node, content) {
                    segments.push(text);
                }
            }
        }
        IDENTIFIER => {
            if let Some(text) = node_text(node, content) {
                segments.push(text);
            }
        }
        _ => {}
    }
}

/// Extract using directives from a C# syntax tree.
pub fn extract_using_directives(tree: &tree_sitter::Tree, content: &[u8]) -> Vec<UsingDirective> {
    let mut directives = Vec::new();
    let root = tree.root_node();

    extract_using_directives_recursive(&root, content, &mut directives);

    directives
}

fn extract_using_directives_recursive(
    node: &tree_sitter::Node,
    content: &[u8],
    directives: &mut Vec<UsingDirective>,
) {
    use node_kinds::USING_DIRECTIVE;

    if node.kind() == USING_DIRECTIVE {
        if let Some(directive) = parse_using_directive(node, content) {
            directives.push(directive);
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            extract_using_directives_recursive(&child, content, directives);
        }
    }
}

fn parse_using_directive(node: &tree_sitter::Node, content: &[u8]) -> Option<UsingDirective> {
    use node_kinds::{IDENTIFIER, QUALIFIED_NAME};

    let line = node.start_position().row as u32 + 1;
    let full_text = node_text(node, content).unwrap_or_default();

    // Check for `using static`
    let is_static = full_text.contains("static");

    // Check for alias: `using Alias = Namespace.Type;`
    let alias = if full_text.contains('=') {
        // Find the alias name (before the '=')
        let mut cursor = node.walk();
        let mut found_alias = None;
        for child in node.children(&mut cursor) {
            if child.kind() == IDENTIFIER || child.kind() == "name_equals" {
                if child.kind() == "name_equals" {
                    // The alias is inside the name_equals node
                    if let Some(name_node) = child.child_by_field_name("name") {
                        found_alias = node_text(&name_node, content);
                        break;
                    }
                } else {
                    // Check if there's a '=' after this identifier
                    let next = child.next_sibling();
                    if next.and_then(|n| node_text(&n, content)) == Some("=".to_string()) {
                        found_alias = node_text(&child, content);
                        break;
                    }
                }
            }
        }
        if found_alias.is_none() {
            tracing::debug!(
                line = line,
                directive_text = %full_text,
                "Failed to extract alias from using directive with '=' character"
            );
        }
        found_alias
    } else {
        None
    };

    // Extract the namespace
    let mut namespace = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            IDENTIFIER => {
                // Could be a simple using or the identifier after an alias
                if alias.is_none() && !is_static {
                    if let Some(text) = node_text(&child, content) {
                        namespace.push(text);
                    }
                }
            }
            QUALIFIED_NAME => {
                collect_qualified_name_path(&child, content, &mut namespace);
            }
            _ => {}
        }
    }

    // If we couldn't parse the namespace, skip this directive
    if namespace.is_empty() && alias.is_none() {
        return None;
    }

    Some(UsingDirective {
        namespace,
        alias,
        is_static,
        line,
    })
}

/// Extract symbols from a C# syntax tree.
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
        CLASS_DECLARATION, CONSTRUCTOR_DECLARATION, DECLARATION_LIST, ENUM_DECLARATION,
        FILE_SCOPED_NAMESPACE_DECLARATION, INTERFACE_DECLARATION, METHOD_DECLARATION,
        NAMESPACE_DECLARATION, RECORD_DECLARATION, STRUCT_DECLARATION,
    };

    match node.kind() {
        CLASS_DECLARATION => {
            if let Some(sym) = extract_type_declaration(node, content, SymbolKind::Class, None) {
                let class_name = sym.name.clone();
                symbols.push(sym);
                // Recurse into class body for methods
                extract_class_members(node, content, symbols, &class_name);
            }
        }
        STRUCT_DECLARATION => {
            if let Some(sym) = extract_type_declaration(node, content, SymbolKind::Struct, None) {
                let struct_name = sym.name.clone();
                symbols.push(sym);
                extract_class_members(node, content, symbols, &struct_name);
            }
        }
        INTERFACE_DECLARATION => {
            if let Some(sym) = extract_type_declaration(node, content, SymbolKind::Interface, None)
            {
                let interface_name = sym.name.clone();
                symbols.push(sym);
                extract_class_members(node, content, symbols, &interface_name);
            }
        }
        ENUM_DECLARATION => {
            if let Some(sym) = extract_type_declaration(node, content, SymbolKind::Enum, None) {
                symbols.push(sym);
            }
        }
        RECORD_DECLARATION => {
            // Map to SymbolKind::Class since our type system lacks a Record kind
            if let Some(sym) = extract_type_declaration(node, content, SymbolKind::Class, None) {
                let record_name = sym.name.clone();
                symbols.push(sym);
                extract_class_members(node, content, symbols, &record_name);
            }
        }
        NAMESPACE_DECLARATION | FILE_SCOPED_NAMESPACE_DECLARATION => {
            if let Some(sym) = extract_namespace(node, content) {
                let ns_name = sym.name.clone();
                symbols.push(sym);
                // Recurse into namespace body
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == DECLARATION_LIST {
                        let mut inner_cursor = child.walk();
                        for item in child.children(&mut inner_cursor) {
                            extract_symbols_recursive(&item, content, symbols, Some(&ns_name));
                        }
                    }
                }
            }
            // Don't recurse again - we already handled it above
        }
        METHOD_DECLARATION => {
            if let Some(mut sym) = extract_method(node, content, parent_name) {
                // Check if static - if so, it's a function, not a method
                if !has_modifier(node, content, "static") {
                    sym.kind = SymbolKind::Method;
                }
                symbols.push(sym);
            }
        }
        CONSTRUCTOR_DECLARATION => {
            if let Some(sym) = extract_constructor(node, content, parent_name) {
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

/// Extract members (methods, constructors, nested types) from a class/struct/interface body.
fn extract_class_members(
    node: &tree_sitter::Node,
    content: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    parent_name: &str,
) {
    use node_kinds::{
        CLASS_DECLARATION, CONSTRUCTOR_DECLARATION, DECLARATION_LIST, ENUM_DECLARATION,
        INTERFACE_DECLARATION, METHOD_DECLARATION, RECORD_DECLARATION, STRUCT_DECLARATION,
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == DECLARATION_LIST {
            let mut inner_cursor = child.walk();
            for item in child.children(&mut inner_cursor) {
                match item.kind() {
                    METHOD_DECLARATION => {
                        if let Some(mut sym) = extract_method(&item, content, Some(parent_name)) {
                            // Check if static
                            if !has_modifier(&item, content, "static") {
                                sym.kind = SymbolKind::Method;
                            }
                            symbols.push(sym);
                        }
                    }
                    CONSTRUCTOR_DECLARATION => {
                        if let Some(sym) = extract_constructor(&item, content, Some(parent_name)) {
                            symbols.push(sym);
                        }
                    }
                    // Nested type declarations - pass parent_name for qualified names
                    // Records map to Class since our type system lacks a Record kind
                    CLASS_DECLARATION | RECORD_DECLARATION => {
                        if let Some(sym) = extract_type_declaration(
                            &item,
                            content,
                            SymbolKind::Class,
                            Some(parent_name),
                        ) {
                            let nested_name = sym.name.clone();
                            symbols.push(sym);
                            extract_class_members(&item, content, symbols, &nested_name);
                        }
                    }
                    STRUCT_DECLARATION => {
                        if let Some(sym) = extract_type_declaration(
                            &item,
                            content,
                            SymbolKind::Struct,
                            Some(parent_name),
                        ) {
                            let nested_name = sym.name.clone();
                            symbols.push(sym);
                            extract_class_members(&item, content, symbols, &nested_name);
                        }
                    }
                    INTERFACE_DECLARATION => {
                        if let Some(sym) = extract_type_declaration(
                            &item,
                            content,
                            SymbolKind::Interface,
                            Some(parent_name),
                        ) {
                            let nested_name = sym.name.clone();
                            symbols.push(sym);
                            extract_class_members(&item, content, symbols, &nested_name);
                        }
                    }
                    ENUM_DECLARATION => {
                        if let Some(sym) = extract_type_declaration(
                            &item,
                            content,
                            SymbolKind::Enum,
                            Some(parent_name),
                        ) {
                            symbols.push(sym);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Extract a type declaration (class, struct, interface, enum).
///
/// The `parent_name` is used for nested types to build qualified names like `Outer::Inner`.
fn extract_type_declaration(
    node: &tree_sitter::Node,
    content: &[u8],
    kind: SymbolKind,
    parent_name: Option<&str>,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let Some(name) = node_text(&name_node, content) else {
        tracing::trace!(
            kind = ?kind,
            node_kind = node.kind(),
            line = node.start_position().row + 1,
            "Failed to extract type declaration name, skipping"
        );
        return None;
    };
    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: parent_name.map(String::from),
        is_test: false, // Type declarations (class, struct, etc.) are never tests
    })
}

/// Extract a namespace declaration.
fn extract_namespace(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    // The name can be an identifier or a qualified_name
    let name_node = node.child_by_field_name("name")?;
    let Some(name) = node_text(&name_node, content) else {
        tracing::trace!(
            kind = node.kind(),
            line = node.start_position().row + 1,
            "Failed to extract namespace name, skipping"
        );
        return None;
    };

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Module,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility: Visibility::Public, // Namespaces are implicitly public
        parent_name: None,
        is_test: false, // Namespaces are never tests
    })
}

/// Extract a method declaration.
fn extract_method(
    node: &tree_sitter::Node,
    content: &[u8],
    parent_name: Option<&str>,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let Some(name) = node_text(&name_node, content) else {
        tracing::trace!(
            kind = node.kind(),
            line = node.start_position().row + 1,
            "Failed to extract method name, skipping"
        );
        return None;
    };

    let visibility = extract_visibility(node, content);
    let signature = extract_method_signature(node, content);
    let signature_details = extract_signature_details(node, content);
    let is_test = has_test_attribute(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Function, // Will be changed to Method by caller if not static
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature,
        signature_details,
        visibility,
        parent_name: parent_name.map(String::from),
        is_test,
    })
}

/// Extract a constructor declaration.
fn extract_constructor(
    node: &tree_sitter::Node,
    content: &[u8],
    parent_name: Option<&str>,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let Some(name) = node_text(&name_node, content) else {
        tracing::trace!(
            kind = node.kind(),
            line = node.start_position().row + 1,
            "Failed to extract constructor name, skipping"
        );
        return None;
    };

    let visibility = extract_visibility(node, content);
    let signature = extract_constructor_signature(node, content);
    let signature_details = extract_signature_details(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Method, // Using Method since our type system lacks a Constructor kind
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature,
        signature_details,
        visibility,
        parent_name: parent_name.map(String::from),
        is_test: false, // Constructors are never tests
    })
}

/// Extract visibility from modifier children.
/// Handles compound visibility modifiers like `protected internal` and `private protected`.
fn extract_visibility(node: &tree_sitter::Node, content: &[u8]) -> Visibility {
    use node_kinds::{INTERNAL, MODIFIER, PRIVATE, PROTECTED, PUBLIC};

    let mut has_public = false;
    let mut has_internal = false;
    let mut has_protected = false;
    let mut has_private = false;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == MODIFIER {
            let text = node_text(&child, content).unwrap_or_default();
            match text.as_str() {
                PUBLIC => has_public = true,
                INTERNAL => has_internal = true,
                PROTECTED => has_protected = true,
                PRIVATE => has_private = true,
                _ => {}
            }
        }
    }

    // Match order: public > compound modifiers > single modifiers > default (private)
    match (has_public, has_protected, has_internal, has_private) {
        (true, _, _, _) => Visibility::Public,
        (_, true, true, _) => Visibility::Crate, // protected internal
        (_, true, _, true) => Visibility::Module, // private protected
        (_, _, true, _) => Visibility::Crate,
        (_, true, _, _) => Visibility::Module,
        _ => Visibility::Private,
    }
}

/// Check if a node has a specific modifier.
fn has_modifier(node: &tree_sitter::Node, content: &[u8], modifier: &str) -> bool {
    use node_kinds::MODIFIER;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == MODIFIER {
            if let Some(text) = node_text(&child, content) {
                if text == modifier {
                    return true;
                }
            }
        }
    }
    false
}

/// C# test attribute names to detect.
///
/// These are common test framework attributes:
/// - `[Test]` - `NUnit`
/// - `[TestMethod]` - `MSTest`
/// - `[Fact]` - `xUnit`
/// - `[Theory]` - `xUnit` parameterized test
/// - `[TestCase]` - `NUnit` parameterized test
const CSHARP_TEST_ATTRIBUTES: &[&str] = &["Test", "TestMethod", "Fact", "Theory", "TestCase"];

/// Check if a node has a test attribute.
///
/// In tree-sitter-c-sharp, attributes appear as children of the method/class.
/// We look for `attribute_list` nodes containing test-related attributes.
fn has_test_attribute(node: &tree_sitter::Node, content: &[u8]) -> bool {
    use node_kinds::{ATTRIBUTE, ATTRIBUTE_LIST};

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == ATTRIBUTE_LIST {
            let mut inner_cursor = child.walk();
            for attr_child in child.children(&mut inner_cursor) {
                if attr_child.kind() == ATTRIBUTE {
                    // Get the attribute name (first identifier child)
                    if let Some(name_node) = attr_child.child_by_field_name("name") {
                        if let Some(attr_name) = node_text(&name_node, content) {
                            // Handle fully qualified names like "NUnit.Framework.Test"
                            // by checking the last segment
                            let simple_name = attr_name.rsplit('.').next().unwrap_or(&attr_name);
                            // Also strip "Attribute" suffix if present
                            let name_without_suffix =
                                simple_name.strip_suffix("Attribute").unwrap_or(simple_name);
                            if CSHARP_TEST_ATTRIBUTES.contains(&name_without_suffix) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Extract method signature (declaration without body).
fn extract_method_signature(node: &tree_sitter::Node, content: &[u8]) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    let params = node.child_by_field_name("parameters")?;

    let name_text = node_text(&name, content)?;
    let params_text = node_text(&params, content)?;

    // In tree-sitter-c-sharp, return type is accessed via "returns" field
    let return_type = node
        .child_by_field_name("returns")
        .and_then(|rt| node_text(&rt, content));

    let sig = if let Some(rt) = return_type {
        format!("{rt} {name_text}{params_text}")
    } else {
        format!("void {name_text}{params_text}")
    };

    Some(sig)
}

/// Extract constructor signature.
fn extract_constructor_signature(node: &tree_sitter::Node, content: &[u8]) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    let params = node.child_by_field_name("parameters")?;

    let name_text = node_text(&name, content)?;
    let params_text = node_text(&params, content)?;

    Some(format!("{name_text}{params_text}"))
}

/// Extract structured function signature details.
fn extract_signature_details(
    node: &tree_sitter::Node,
    content: &[u8],
) -> Option<FunctionSignature> {
    use node_kinds::PARAMETER_LIST;

    let params_node = node.child_by_field_name("parameters")?;
    let parameters = if params_node.kind() == PARAMETER_LIST {
        extract_parameters(&params_node, content)
    } else {
        Vec::new()
    };

    // Extract return type - in tree-sitter-c-sharp, it's via "returns" field
    let return_type = node
        .child_by_field_name("returns")
        .and_then(|rt| node_text(&rt, content));

    // Check for async modifier
    let is_async = has_modifier(node, content, "async");

    Some(FunctionSignature {
        parameters,
        return_type,
        is_async,
        is_unsafe: false, // TODO: detect unsafe modifier
        is_const: false,  // C# doesn't have const functions
        generics: None,   // TODO: Extract type parameters
    })
}

/// Extract parameters from a `parameter_list` node.
fn extract_parameters(params_node: &tree_sitter::Node, content: &[u8]) -> Vec<Parameter> {
    use node_kinds::PARAMETER;

    let mut parameters = Vec::new();
    let mut cursor = params_node.walk();

    for child in params_node.children(&mut cursor) {
        if child.kind() == PARAMETER {
            if let Some(param) = extract_parameter(&child, content) {
                parameters.push(param);
            }
        }
    }

    parameters
}

/// Extract a single parameter.
fn extract_parameter(param_node: &tree_sitter::Node, content: &[u8]) -> Option<Parameter> {
    let name_node = param_node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

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

    fn parse_csharp(code: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .expect("tree-sitter-c-sharp language should be valid");
        parser
            .parse(code, None)
            .expect("parsing test code should succeed")
    }

    #[test]
    fn csharp_language_extensions() {
        let lang = CSharpLanguage;
        assert_eq!(lang.extensions(), &["cs"]);
    }

    #[test]
    fn csharp_language_has_lsp() {
        let lang = CSharpLanguage;
        assert_eq!(lang.lsp_command(), Some("csharp-ls"));
    }

    #[test]
    fn extracts_class() {
        let code = "public class User { }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "User");
        assert_eq!(symbols[0].kind, SymbolKind::Class);
        assert_eq!(symbols[0].visibility, Visibility::Public);
    }

    #[test]
    fn extracts_private_class() {
        let code = "class User { }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "User");
        assert_eq!(symbols[0].visibility, Visibility::Private);
    }

    #[test]
    fn extracts_internal_class() {
        let code = "internal class Helper { }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Helper");
        assert_eq!(symbols[0].visibility, Visibility::Crate);
    }

    #[test]
    fn extracts_struct() {
        let code = "public struct Point { }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Point");
        assert_eq!(symbols[0].kind, SymbolKind::Struct);
    }

    #[test]
    fn extracts_interface() {
        let code = "public interface IService { }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "IService");
        assert_eq!(symbols[0].kind, SymbolKind::Interface);
    }

    #[test]
    fn extracts_enum() {
        let code = "public enum Status { Active, Inactive }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Status");
        assert_eq!(symbols[0].kind, SymbolKind::Enum);
    }

    #[test]
    fn extracts_method() {
        let code = r"
public class UserService {
    public void Save(User user) { }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 2);

        let class_sym = symbols
            .iter()
            .find(|s| s.name == "UserService")
            .expect("should find UserService");
        assert_eq!(class_sym.kind, SymbolKind::Class);

        let method_sym = symbols
            .iter()
            .find(|s| s.name == "Save")
            .expect("should find Save method");
        assert_eq!(method_sym.kind, SymbolKind::Method);
        assert_eq!(method_sym.parent_name, Some("UserService".to_string()));
        assert_eq!(method_sym.visibility, Visibility::Public);
    }

    #[test]
    fn extracts_static_method_as_function() {
        let code = r"
public class Utils {
    public static int Add(int a, int b) { return a + b; }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let method_sym = symbols
            .iter()
            .find(|s| s.name == "Add")
            .expect("should find Add method");
        assert_eq!(method_sym.kind, SymbolKind::Function);
    }

    #[test]
    fn extracts_constructor() {
        let code = r"
public class User {
    public User(string name) { }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let ctor_sym = symbols
            .iter()
            .find(|s| s.name == "User" && s.kind == SymbolKind::Method)
            .expect("should find User constructor");
        assert_eq!(ctor_sym.parent_name, Some("User".to_string()));
    }

    #[test]
    fn extracts_namespace() {
        let code = r"
namespace MyApp.Services {
    public class UserService { }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let ns_sym = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Module)
            .expect("should find namespace module");
        assert_eq!(ns_sym.name, "MyApp.Services");
    }

    #[test]
    fn extracts_file_scoped_namespace() {
        let code = r"
namespace MyApp.Services;

public class UserService { }
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let ns_sym = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Module)
            .expect("should find file-scoped namespace module");
        assert_eq!(ns_sym.name, "MyApp.Services");
    }

    #[test]
    fn extracts_record() {
        let code = "public record Person(string Name, int Age);";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Person");
        // Records are mapped to Class since SymbolKind lacks a Record variant
        assert_eq!(symbols[0].kind, SymbolKind::Class);
        assert_eq!(symbols[0].visibility, Visibility::Public);
    }

    #[test]
    fn extracts_nested_types() {
        let code = r"
public abstract record Distribution
{
    public sealed record Percentage : Distribution
    {
        public decimal Value { get; init; }
    }

    public sealed record FixedAmount : Distribution
    {
        public decimal Amount { get; init; }
    }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        // Should extract: Distribution, Percentage, FixedAmount
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Distribution"),
            "should extract outer type, got: {names:?}"
        );
        assert!(
            names.contains(&"Percentage"),
            "should extract nested Percentage type, got: {names:?}"
        );
        assert!(
            names.contains(&"FixedAmount"),
            "should extract nested FixedAmount type, got: {names:?}"
        );
    }

    #[test]
    fn extracts_nested_types_with_file_scoped_namespace() {
        // Matches the actual Distribution.cs file structure
        let code = r"
namespace MyApp.ValueObjects;

public abstract record Distribution
{
    public sealed record Percentage : Distribution
    {
        public decimal Value { get; init; }

        public Percentage(decimal value)
        {
            Value = value;
        }
    }

    public sealed record FixedAmount : Distribution
    {
        public decimal Amount { get; init; }
    }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"MyApp.ValueObjects"),
            "should extract namespace, got: {names:?}"
        );
        assert!(
            names.contains(&"Distribution"),
            "should extract outer type, got: {names:?}"
        );
        assert!(
            names.contains(&"Percentage"),
            "should extract nested Percentage type, got: {names:?}"
        );
        assert!(
            names.contains(&"FixedAmount"),
            "should extract nested FixedAmount type, got: {names:?}"
        );
    }

    #[test]
    fn extracts_protected_visibility() {
        let code = "public class Base { protected virtual void OnInit() { } }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let method = symbols
            .iter()
            .find(|s| s.name == "OnInit")
            .expect("should find OnInit method");
        assert_eq!(method.visibility, Visibility::Module); // protected maps to Module
    }

    #[test]
    fn extracts_protected_internal_visibility() {
        let code = "public class Base { protected internal void SharedMethod() { } }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let method = symbols
            .iter()
            .find(|s| s.name == "SharedMethod")
            .expect("should find SharedMethod");
        // protected internal = accessible from same assembly OR derived classes
        assert_eq!(method.visibility, Visibility::Crate);
    }

    #[test]
    fn extracts_private_protected_visibility() {
        let code = "public class Base { private protected void RestrictedMethod() { } }";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let method = symbols
            .iter()
            .find(|s| s.name == "RestrictedMethod")
            .expect("should find RestrictedMethod");
        // private protected = accessible only from derived classes in same assembly
        assert_eq!(method.visibility, Visibility::Module);
    }

    #[test]
    fn extracts_method_signature() {
        let code = r"
public class Calculator {
    public int Add(int a, int b) { return a + b; }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let method_sym = symbols
            .iter()
            .find(|s| s.name == "Add")
            .expect("should find Add method");
        let sig = method_sym
            .signature
            .as_ref()
            .expect("Add method should have signature");
        assert!(sig.contains("int Add"));
        assert!(sig.contains("int a"));
    }

    #[test]
    fn extracts_signature_details() {
        let code = r"
public class UserService {
    public async Task<User> GetUser(int id, string name) { return null; }
}
";
        let tree = parse_csharp(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        let method_sym = symbols
            .iter()
            .find(|s| s.name == "GetUser")
            .expect("should find GetUser method");
        let details = method_sym
            .signature_details
            .as_ref()
            .expect("should have signature_details");

        assert!(details.is_async);
        assert_eq!(details.parameters.len(), 2);
        assert_eq!(details.parameters[0].name, "id");
        assert_eq!(
            details.parameters[0].type_annotation,
            Some("int".to_string())
        );
        assert_eq!(details.parameters[1].name, "name");
    }

    // ========================================================================
    // Using Directive Extraction Tests
    // ========================================================================

    #[test]
    fn extracts_simple_using_directive() {
        let code = "using System;";
        let tree = parse_csharp(code);
        let directives = extract_using_directives(&tree, code.as_bytes());

        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].namespace, vec!["System"]);
        assert!(!directives[0].is_static);
        assert!(directives[0].alias.is_none());
    }

    #[test]
    fn extracts_qualified_using_directive() {
        let code = "using System.Collections.Generic;";
        let tree = parse_csharp(code);
        let directives = extract_using_directives(&tree, code.as_bytes());

        assert_eq!(directives.len(), 1);
        assert_eq!(
            directives[0].namespace,
            vec!["System", "Collections", "Generic"]
        );
    }

    #[test]
    fn extracts_multiple_using_directives() {
        let code = r"
using System;
using System.Collections.Generic;
using System.Linq;
";
        let tree = parse_csharp(code);
        let directives = extract_using_directives(&tree, code.as_bytes());

        assert_eq!(directives.len(), 3);
    }

    #[test]
    fn extracts_static_using_directive() {
        let code = "using static System.Math;";
        let tree = parse_csharp(code);
        let directives = extract_using_directives(&tree, code.as_bytes());

        assert_eq!(directives.len(), 1);
        assert!(directives[0].is_static, "should detect static using");
        assert_eq!(directives[0].namespace, vec!["System", "Math"]);
    }

    #[test]
    fn extracts_alias_using_directive() {
        let code = "using Dict = System.Collections.Generic.Dictionary<string, int>;";
        let tree = parse_csharp(code);
        let directives = extract_using_directives(&tree, code.as_bytes());

        assert_eq!(directives.len(), 1);
        assert!(directives[0].alias.is_some(), "should detect alias");
        assert_eq!(directives[0].alias.as_ref().unwrap(), "Dict");
    }

    // ========================================================================
    // Reference Extraction Tests
    // ========================================================================

    #[test]
    fn extracts_method_call() {
        let code = r"
public class Test {
    public void Run() {
        DoSomething();
    }
}
";
        let tree = parse_csharp(code);
        let refs = extract_references(&tree, code.as_bytes());

        let call_ref = refs.iter().find(|r| r.name == "DoSomething");
        assert!(call_ref.is_some(), "should find method call");
        assert_eq!(call_ref.unwrap().kind, ExtractedReferenceKind::Call);
    }

    #[test]
    fn extracts_member_method_call() {
        let code = r#"
public class Test {
    public void Run() {
        Console.WriteLine("test");
    }
}
"#;
        let tree = parse_csharp(code);
        let refs = extract_references(&tree, code.as_bytes());

        let call_ref = refs.iter().find(|r| r.name == "WriteLine");
        assert!(call_ref.is_some(), "should find member method call");
        assert_eq!(call_ref.unwrap().kind, ExtractedReferenceKind::Call);
        assert!(call_ref.unwrap().path.is_some());
    }

    #[test]
    fn extracts_object_creation() {
        let code = r"
public class Test {
    public void Run() {
        var user = new User();
    }
}
";
        let tree = parse_csharp(code);
        let refs = extract_references(&tree, code.as_bytes());

        let ctor_ref = refs
            .iter()
            .find(|r| r.name == "User" && r.kind == ExtractedReferenceKind::Constructor);
        assert!(ctor_ref.is_some(), "should find object creation");
    }

    #[test]
    fn extracts_generic_object_creation() {
        let code = r"
public class Test {
    public void Run() {
        var list = new List<string>();
    }
}
";
        let tree = parse_csharp(code);
        let refs = extract_references(&tree, code.as_bytes());

        let ctor_ref = refs
            .iter()
            .find(|r| r.name == "List" && r.kind == ExtractedReferenceKind::Constructor);
        assert!(ctor_ref.is_some(), "should find generic object creation");
    }

    #[test]
    fn tracks_containing_symbol_for_references() {
        let code = r"
public class Test {
    public void Method1() {
        Foo();
    }

    public void Method2() {
        Bar();
    }
}
";
        let tree = parse_csharp(code);
        let refs = extract_references(&tree, code.as_bytes());

        // Foo() is called from Method1()
        let foo_ref = refs
            .iter()
            .find(|r| r.name == "Foo")
            .expect("should find Foo reference");
        assert!(
            foo_ref.containing_symbol_span.is_some(),
            "should track containing symbol"
        );

        // Bar() is called from Method2()
        let bar_ref = refs
            .iter()
            .find(|r| r.name == "Bar")
            .expect("should find Bar reference");
        assert!(bar_ref.containing_symbol_span.is_some());

        // They should have different containing spans
        assert_ne!(
            foo_ref.containing_symbol_span.unwrap().start_line(),
            bar_ref.containing_symbol_span.unwrap().start_line()
        );
    }

    // ========================================================================
    // resolve_import Tests (Task 5)
    // ========================================================================

    #[test]
    fn resolve_import_returns_empty_for_csharp() {
        use crate::languages::common::ImportContext;

        let dir = tempfile::tempdir().expect("should create temp directory");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("should create src directory");

        let import = ImportStatement {
            path: vec![
                "System".to_string(),
                "Collections".to_string(),
                "Generic".to_string(),
            ],
            imported_names: vec![],
            is_glob: false,
            alias: None,
            line: 1,
        };

        let file_path = src.join("Program.cs");
        let context = ImportContext {
            file_path: &file_path,
            workspace_root: &src,
            known_files: &[],
        };

        let lang = CSharpLanguage;
        let resolved = lang.resolve_import(&import, &context);

        assert!(
            resolved.is_empty(),
            "C# resolve_import should return empty vec (stub)"
        );
    }
}
