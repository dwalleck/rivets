//! Rust language support for Tethys.
//!
//! Implements symbol extraction for Rust source files using tree-sitter-rust.

// Tree-sitter returns usize for positions, but we store u32 for compactness.
// This is safe for practical source files (no file has 4 billion lines).
#![allow(clippy::cast_possible_truncation)]

use super::LanguageSupport;
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

    // Structure nodes
    pub const DECLARATION_LIST: &str = "declaration_list";
    pub const TYPE_IDENTIFIER: &str = "type_identifier";
    pub const GENERIC_TYPE: &str = "generic_type";
    pub const VISIBILITY_MODIFIER: &str = "visibility_modifier";
    pub const FUNCTION_MODIFIERS: &str = "function_modifiers";
    pub const TYPE_PARAMETERS: &str = "type_parameters";
    pub const PARAMETER: &str = "parameter";
    pub const SELF_PARAMETER: &str = "self_parameter";

    // Modifier keywords
    pub const ASYNC: &str = "async";
    pub const UNSAFE: &str = "unsafe";
    pub const CONST: &str = "const";
}

/// Rust language support implementation.
#[allow(dead_code)] // Used via trait object in get_language_support()
pub struct RustLanguage;

impl LanguageSupport for RustLanguage {
    fn extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn lsp_command(&self) -> Option<&str> {
        Some("rust-analyzer")
    }
}

/// An extracted symbol from Rust source code.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub span: Option<Span>,
    pub signature: Option<String>,
    #[allow(dead_code)] // Populated for future use by callers
    pub signature_details: Option<FunctionSignature>,
    pub visibility: Visibility,
    pub parent_name: Option<String>,
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
            if let Some(sym) = extract_struct(node, content) {
                symbols.push(sym);
            }
        }
        ENUM_ITEM => {
            if let Some(sym) = extract_enum(node, content) {
                symbols.push(sym);
            }
        }
        TRAIT_ITEM => {
            if let Some(sym) = extract_trait(node, content) {
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
                        if item.kind() == FUNCTION_ITEM {
                            if let Some(mut sym) =
                                extract_function(&item, content, type_name.as_deref())
                            {
                                sym.kind = SymbolKind::Method;
                                symbols.push(sym);
                            }
                        }
                    }
                }
            }
        }
        CONST_ITEM => {
            if let Some(sym) = extract_const(node, content) {
                symbols.push(sym);
            }
        }
        STATIC_ITEM => {
            if let Some(sym) = extract_static(node, content) {
                symbols.push(sym);
            }
        }
        TYPE_ITEM => {
            if let Some(sym) = extract_type_alias(node, content) {
                symbols.push(sym);
            }
        }
        MACRO_DEFINITION => {
            if let Some(sym) = extract_macro(node, content) {
                symbols.push(sym);
            }
        }
        MOD_ITEM => {
            if let Some(sym) = extract_module(node, content) {
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
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);
    let signature = extract_function_signature(node, content);
    let signature_details = extract_signature_details(node, content);

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
    })
}

fn extract_struct(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Struct,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
    })
}

fn extract_enum(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Enum,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
    })
}

fn extract_trait(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Trait,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
    })
}

fn extract_const(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Const,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
    })
}

fn extract_static(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Static,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
    })
}

fn extract_type_alias(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::TypeAlias,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
    })
}

fn extract_macro(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Macro,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility: Visibility::Public, // macros are typically public if exported
        parent_name: None,
    })
}

fn extract_module(node: &tree_sitter::Node, content: &[u8]) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, content)?;

    let visibility = extract_visibility(node, content);

    Some(ExtractedSymbol {
        name,
        kind: SymbolKind::Module,
        line: node.start_position().row as u32 + 1,
        column: node.start_position().column as u32 + 1,
        span: Some(node_span(node)),
        signature: None,
        signature_details: None,
        visibility,
        parent_name: None,
    })
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

/// Extract visibility from a node (looks for `visibility_modifier` child).
fn extract_visibility(node: &tree_sitter::Node, content: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == node_kinds::VISIBILITY_MODIFIER {
            let text = node_text(&child, content).unwrap_or_default();
            return match text.as_str() {
                "pub" => Visibility::Public,
                s if s.starts_with("pub(crate)") => Visibility::Crate,
                s if s.starts_with("pub(super)") => Visibility::Module,
                s if s.starts_with("pub(in") => Visibility::Module,
                _ => Visibility::Public,
            };
        }
    }
    Visibility::Private
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
                let text = node_text(&child, content).unwrap_or_default();
                parameters.push(Parameter {
                    name: text,
                    type_annotation: None,
                });
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
    })
}

/// Get text content of a node.
fn node_text(node: &tree_sitter::Node, content: &[u8]) -> Option<String> {
    std::str::from_utf8(&content[node.byte_range()])
        .ok()
        .map(String::from)
}

/// Convert tree-sitter positions to our Span type.
fn node_span(node: &tree_sitter::Node) -> Span {
    Span {
        start_line: node.start_position().row as u32 + 1,
        start_column: node.start_position().column as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        end_column: node.end_position().column as u32 + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rust(code: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        parser.parse(code, None).unwrap()
    }

    #[test]
    fn rust_language_extensions() {
        let lang = RustLanguage;
        assert_eq!(lang.extensions(), &["rs"]);
    }

    #[test]
    fn rust_language_has_lsp() {
        let lang = RustLanguage;
        assert_eq!(lang.lsp_command(), Some("rust-analyzer"));
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

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "User");
        assert_eq!(symbols[0].kind, SymbolKind::Struct);
    }

    #[test]
    fn extracts_enum() {
        let code = "enum Status { Active, Inactive }";
        let tree = parse_rust(code);
        let symbols = extract_symbols(&tree, code.as_bytes());

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Status");
        assert_eq!(symbols[0].kind, SymbolKind::Enum);
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
}
