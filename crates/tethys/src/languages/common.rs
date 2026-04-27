//! Common extraction types shared across language implementations.
//!
//! These types represent the intermediate output of tree-sitter extraction,
//! before conversion to the database domain model in `crate::types`.

use crate::types::{FunctionSignature, Span, SymbolKind, Visibility};

/// An extracted symbol from source code.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub span: Option<Span>,
    pub signature: Option<String>,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "populated by parsers, will be read when signature queries are added"
        )
    )]
    pub signature_details: Option<FunctionSignature>,
    pub visibility: Visibility,
    pub parent_name: Option<String>,
    /// Whether this symbol is a test function.
    ///
    /// Detected by language-specific test attributes:
    /// - Rust: `#[test]`, `#[tokio::test]`, `#[rstest]`
    /// - C#: `[Test]`, `[Fact]`, `[Theory]`, `[TestMethod]`
    pub is_test: bool,
    /// Attributes attached to this symbol (e.g. `#[derive(Clone)]`, `#[source]`).
    ///
    /// Empty when no attributes precede the symbol. Currently populated by the
    /// Rust extractor only; C# extraction will follow.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "populated by parsers, will be persisted by indexing.rs in a follow-up commit"
        )
    )]
    pub attributes: Vec<ExtractedAttribute>,
}

/// An attribute attached to a symbol.
///
/// Stores the attribute path's leading identifier as `name` (e.g. `derive`,
/// `source`, `cfg_attr`, `tauri::command`) and the raw text inside the
/// outermost parens as `args`. Marker attributes like `#[source]` have
/// `args == None`.
///
/// `args` is intentionally kept as raw text rather than a structured nested
/// representation: the rules that consume attributes (e.g. "does any
/// `cfg_attr` mention `specta::Type`") match by substring on the args, and
/// tree-sitter does not surface nested attributes as structured children
/// inside `cfg_attr(...)` anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedAttribute {
    pub name: String,
    pub args: Option<String>,
    pub line: u32,
}

/// An extracted reference (usage of a symbol) from source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedReference {
    /// Name of the referenced symbol
    pub name: String,
    /// Kind of reference
    pub kind: ExtractedReferenceKind,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
    /// The scoped path if this is a qualified reference
    pub path: Option<Vec<String>>,
    /// Span of the containing symbol for "who calls X?" queries.
    /// `None` for top-level references.
    /// Resolved to `in_symbol_id` during indexing.
    pub containing_symbol_span: Option<Span>,
}

/// Kind of reference extracted from source code.
///
/// Note: This is distinct from `types::ReferenceKind` which is the domain model
/// stored in the database. This enum represents what we extract from the AST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractedReferenceKind {
    /// Function or method call
    Call,
    /// Type annotation
    Type,
    /// Constructor invocation
    Constructor,
}

impl ExtractedReferenceKind {
    /// Convert to database reference kind.
    #[must_use]
    pub fn to_db_kind(self) -> crate::types::ReferenceKind {
        match self {
            Self::Call => crate::types::ReferenceKind::Call,
            Self::Type => crate::types::ReferenceKind::Type,
            Self::Constructor => crate::types::ReferenceKind::Construct,
        }
    }
}

/// A unified import statement extracted from source code.
///
/// Language-specific import types (`UseStatement`, `UsingDirective`) can convert
/// to this common representation for cross-language analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStatement {
    /// Path segments (e.g., `["crate", "auth"]` or `["System", "Collections"]`)
    pub path: Vec<String>,
    /// Names being imported (e.g., `["HashMap", "HashSet"]`)
    pub imported_names: Vec<String>,
    /// Whether this is a glob/wildcard import
    pub is_glob: bool,
    /// Alias if present
    pub alias: Option<String>,
    /// Line number (1-indexed)
    pub line: u32,
}
