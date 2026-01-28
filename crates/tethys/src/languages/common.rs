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
    #[allow(dead_code)] // Populated for future use by callers
    pub signature_details: Option<FunctionSignature>,
    pub visibility: Visibility,
    pub parent_name: Option<String>,
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
