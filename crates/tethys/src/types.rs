//! Domain types for Tethys code intelligence.
//!
//! These types represent the core domain model:
//! - **Entities**: `IndexedFile`, `Symbol`, `Reference` (stored in database)
//! - **Transient**: `FileAnalysis` (parsing result, not stored directly)
//! - **Results**: `IndexStats`, `IndexUpdate`, `Impact`, `Cycle` (query results)
//!
//! ## Design Decisions
//!
//! | Decision | Choice | Rationale |
//! |----------|--------|-----------|
//! | Language | Enum not String | Type-safe; adding language requires trait impl |
//! | module_path | Separate from qualified_name | Enables "exports from module" queries |
//! | full_path | Computed on read | No redundancy; concatenation is cheap |
//! | Span | Optional | Tree-sitter provides it, but not all sources do |

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::error::IndexError;

// ============================================================================
// Strongly-typed ID wrappers
// ============================================================================

/// A strongly-typed symbol ID to prevent mixing with file IDs.
///
/// This newtype provides type safety for function signatures that accept
/// both symbol and file IDs, preventing accidental parameter swaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub i64);

impl SymbolId {
    /// Extract the raw i64 value.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl From<i64> for SymbolId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

/// A strongly-typed file ID to prevent mixing with symbol IDs.
///
/// This newtype provides type safety for function signatures that accept
/// both symbol and file IDs, preventing accidental parameter swaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub i64);

impl FileId {
    /// Extract the raw i64 value.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl From<i64> for FileId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

// ============================================================================
// Enums
// ============================================================================

/// Supported programming languages.
///
/// Adding a new language requires implementing the `LanguageSupport` trait.
/// This enum ensures we only claim to support languages we actually handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Rust source files (`.rs`)
    Rust,
    /// C# source files (`.cs`)
    CSharp,
}

impl Language {
    /// File extensions handled by this language.
    #[must_use]
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["rs"],
            Self::CSharp => &["cs"],
        }
    }

    /// Detect language from file extension.
    ///
    /// # Returns
    ///
    /// `None` if the extension is not recognized.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "cs" => Some(Self::CSharp),
            _ => None,
        }
    }

    /// Convert to database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::CSharp => "csharp",
        }
    }
}

/// Symbol kinds tracked by Tethys.
///
/// These are normalized across languages. Not all languages have all kinds
/// (e.g., Rust has traits, C# has interfaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// Free function (not associated with a type)
    Function,
    /// Method (function associated with a type)
    Method,
    /// Struct (Rust) or struct (C#)
    Struct,
    /// Class (C# only)
    Class,
    /// Enum type
    Enum,
    /// Trait (Rust only)
    Trait,
    /// Interface (C# only)
    Interface,
    /// Constant value
    Const,
    /// Static variable
    Static,
    /// Module (Rust) or namespace (C#)
    Module,
    /// Type alias
    TypeAlias,
    /// Macro (Rust only)
    Macro,
}

impl SymbolKind {
    /// Convert to database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::Const => "const",
            Self::Static => "static",
            Self::Module => "module",
            Self::TypeAlias => "type_alias",
            Self::Macro => "macro",
        }
    }
}

/// Visibility levels, normalized across languages.
///
/// Rust and C# have different visibility models; this enum represents the
/// semantic meaning rather than the syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Visible everywhere (`pub` in Rust, `public` in C#)
    Public,
    /// Visible within the crate/assembly (`pub(crate)` in Rust, `internal` in C#)
    Crate,
    /// Visible within parent module (`pub(super)` or `pub(in path)` in Rust)
    Module,
    /// Visible only within defining scope (default in both languages)
    Private,
}

impl Visibility {
    /// Convert to database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Crate => "crate",
            Self::Module => "module",
            Self::Private => "private",
        }
    }
}

/// Reference kinds - how a symbol is used at a reference site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// Import statement (`use` in Rust, `using` in C#)
    Import,
    /// Function or method call
    Call,
    /// Type annotation or generic parameter
    Type,
    /// Trait implementation or class inheritance
    Inherit,
    /// Constructor call (struct literal in Rust, `new` in C#)
    Construct,
    /// Field access on a struct/class instance
    FieldAccess,
}

impl ReferenceKind {
    /// Convert to database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Call => "call",
            Self::Type => "type",
            Self::Inherit => "inherit",
            Self::Construct => "construct",
            Self::FieldAccess => "field_access",
        }
    }
}

// ============================================================================
// Core Entities (stored in database)
// ============================================================================

/// A source/end position span in a file.
///
/// Positions are 1-indexed (first line is 1, first column is 1) to match
/// editor conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// Starting line (1-indexed)
    pub start_line: u32,
    /// Starting column (1-indexed)
    pub start_column: u32,
    /// Ending line (1-indexed, inclusive)
    pub end_line: u32,
    /// Ending column (1-indexed, exclusive)
    pub end_column: u32,
}

impl Span {
    /// Create a new span with validation.
    ///
    /// Returns `None` if the end position is before the start position.
    #[must_use]
    pub fn new(start_line: u32, start_column: u32, end_line: u32, end_column: u32) -> Option<Self> {
        // End must be >= start (either on a later line, or same line with >= column)
        if end_line < start_line || (end_line == start_line && end_column < start_column) {
            return None;
        }
        Some(Self {
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }
}

// ============================================================================
// Function Signature Types
// ============================================================================

/// Structured representation of a function/method signature.
///
/// Provides programmatic access to signature components for queries like
/// "find all functions returning Result" or "find functions with >3 parameters".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionSignature {
    /// Function parameters in order
    pub parameters: Vec<Parameter>,
    /// Return type (None for functions returning unit/void)
    pub return_type: Option<String>,
    /// Whether the function is async
    pub is_async: bool,
    /// Whether the function is unsafe
    pub is_unsafe: bool,
    /// Whether the function is const
    pub is_const: bool,
    /// Generic parameters (e.g., "<T: Clone, U>")
    pub generics: Option<String>,
}

impl FunctionSignature {
    /// Check if this function returns a Result type.
    #[must_use]
    pub fn returns_result(&self) -> bool {
        self.return_type
            .as_ref()
            .is_some_and(|rt| rt.starts_with("Result") || rt.contains("Result<"))
    }

    /// Check if this function returns an Option type.
    #[must_use]
    pub fn returns_option(&self) -> bool {
        self.return_type
            .as_ref()
            .is_some_and(|rt| rt.starts_with("Option") || rt.contains("Option<"))
    }

    /// Check if this is a method (has self parameter).
    #[must_use]
    pub fn is_method(&self) -> bool {
        self.parameters.first().is_some_and(Parameter::is_self)
    }

    /// Get the number of non-self parameters.
    #[must_use]
    pub fn param_count(&self) -> usize {
        self.parameters.iter().filter(|p| !p.is_self()).count()
    }
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name (e.g., "`user_id`")
    pub name: String,
    /// Type annotation (e.g., "i64", "&str", "`Option<User>`")
    pub type_annotation: Option<String>,
}

impl Parameter {
    /// Check if this is a self parameter (&self, &mut self, self).
    #[must_use]
    pub fn is_self(&self) -> bool {
        self.name == "self" || self.name == "&self" || self.name == "&mut self"
    }

    /// Check if this is a mutable self parameter.
    #[must_use]
    pub fn is_mut_self(&self) -> bool {
        self.name == "&mut self"
    }

    /// Check if this is a reference parameter (starts with &).
    #[must_use]
    pub fn is_reference(&self) -> bool {
        self.type_annotation
            .as_ref()
            .is_some_and(|t| t.starts_with('&'))
    }
}

/// A source file in the index.
///
/// Represents metadata about an indexed file. The actual symbols and references
/// are stored separately and linked by `file_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    /// Database primary key
    pub id: i64,
    /// Path relative to workspace root
    pub path: PathBuf,
    /// Detected language
    pub language: Language,
    /// File modification time in nanoseconds since epoch
    pub mtime_ns: i64,
    /// File size in bytes
    pub size_bytes: u64,
    /// xxHash64 of file content (for change detection)
    pub content_hash: Option<u64>,
    /// When this file was last indexed (unix timestamp)
    pub indexed_at: i64,
}

/// A code symbol definition.
///
/// Represents a named entity in code: function, struct, trait, class, etc.
/// Symbols form a hierarchy via `parent_symbol_id` (e.g., method inside struct).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Database primary key
    pub id: i64,
    /// Foreign key to the containing file
    pub file_id: i64,
    /// Simple name without qualification (e.g., "save")
    pub name: String,
    /// Module path to this symbol (e.g., "`crate::storage::issue`")
    pub module_path: String,
    /// Symbol hierarchy path (e.g., "`IssueStorage::save`")
    pub qualified_name: String,
    /// What kind of symbol this is
    pub kind: SymbolKind,
    /// Line number where the symbol is defined (1-indexed)
    pub line: u32,
    /// Column number where the symbol starts (1-indexed)
    pub column: u32,
    /// Full extent of the symbol in the source (optional)
    pub span: Option<Span>,
    /// Function/method signature as string (e.g., "fn save(&self, issue: &Issue) -> Result<()>")
    pub signature: Option<String>,
    /// Structured signature details for programmatic access (functions/methods only)
    pub signature_details: Option<FunctionSignature>,
    /// Visibility level
    pub visibility: Visibility,
    /// Parent symbol ID for nested definitions
    pub parent_symbol_id: Option<i64>,
}

impl Symbol {
    /// Compute the full path: `module_path` + `qualified_name`.
    ///
    /// # Example
    ///
    /// ```
    /// # use tethys::Symbol;
    /// // Given: module_path = "crate::storage::issue", qualified_name = "IssueStorage::save"
    /// // Returns: "crate::storage::issue::IssueStorage::save"
    /// ```
    #[must_use]
    pub fn full_path(&self) -> String {
        if self.module_path.is_empty() {
            self.qualified_name.clone()
        } else {
            format!("{}::{}", self.module_path, self.qualified_name)
        }
    }
}

/// A reference to a symbol (usage, not definition).
///
/// Tracks where symbols are used throughout the codebase. Combined with
/// `in_symbol_id`, enables "who calls X?" queries at symbol granularity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    /// Database primary key
    pub id: i64,
    /// Foreign key to the referenced symbol
    pub symbol_id: i64,
    /// Foreign key to the file containing this reference
    pub file_id: i64,
    /// How the symbol is being used
    pub kind: ReferenceKind,
    /// Line number of the reference (1-indexed)
    pub line: u32,
    /// Column number of the reference (1-indexed)
    pub column: u32,
    /// Full extent of the reference (optional)
    pub span: Option<Span>,
    /// Symbol that contains this reference (for "who calls X?" queries)
    pub in_symbol_id: Option<i64>,
}

// ============================================================================
// Transient Types (not stored directly)
// ============================================================================

/// Analysis results from parsing a single file.
///
/// This is the intermediate representation produced by the parser before
/// being stored in the database. It contains all extracted symbols and
/// references for one file.
#[derive(Debug, Clone)]
pub struct FileAnalysis {
    /// Path to the analyzed file
    pub path: PathBuf,
    /// Detected language
    pub language: Language,
    /// File modification time
    pub mtime_ns: i64,
    /// File size
    pub size_bytes: u64,
    /// Content hash for change detection
    pub content_hash: Option<u64>,
    /// Symbols defined in this file
    pub symbols: Vec<Symbol>,
    /// References to other symbols
    pub references: Vec<Reference>,
}

// ============================================================================
// Operation Results
// ============================================================================

/// Statistics from a full index operation.
///
/// Returned by `Tethys::index()` and `Tethys::rebuild()`.
#[derive(Debug, Clone)]
pub struct IndexStats {
    /// Number of files successfully indexed
    pub files_indexed: usize,
    /// Total symbols found across all files
    pub symbols_found: usize,
    /// Total references found across all files
    pub references_found: usize,
    /// How long the indexing took
    pub duration: Duration,
    /// Files skipped (unsupported language, binary, etc.)
    pub files_skipped: usize,
    /// Directories that could not be read (path, error reason)
    pub directories_skipped: Vec<(PathBuf, String)>,
    /// Errors encountered (file-level, non-fatal)
    pub errors: Vec<IndexError>,
    /// Dependencies that couldn't be resolved (`from_file`, `dep_path`).
    /// These are typically external crate dependencies or missing files.
    pub unresolved_dependencies: Vec<(PathBuf, PathBuf)>,
}

/// Statistics from an incremental update.
///
/// Returned by `Tethys::update()`.
#[derive(Debug, Clone)]
pub struct IndexUpdate {
    /// Number of files re-indexed due to changes
    pub files_changed: usize,
    /// Number of files unchanged since last index
    pub files_unchanged: usize,
    /// How long the update took
    pub duration: Duration,
    /// Errors encountered
    pub errors: Vec<IndexError>,
}

// ============================================================================
// Query Results
// ============================================================================

/// Result of impact analysis.
///
/// Shows which files/symbols would be affected by changes to a target.
#[derive(Debug, Clone)]
pub struct Impact {
    /// The file or symbol being analyzed
    pub target: PathBuf,
    /// Files/symbols that directly depend on the target
    pub direct_dependents: Vec<Dependent>,
    /// Files/symbols that transitively depend on the target
    pub transitive_dependents: Vec<Dependent>,
}

/// A file that depends on an analyzed target.
#[derive(Debug, Clone)]
pub struct Dependent {
    /// Path to the dependent file
    pub file: PathBuf,
    /// Which symbols from the target are used
    pub symbols_used: Vec<String>,
    /// Number of reference sites in this file
    pub line_count: usize,
}

/// A circular dependency detected in the codebase.
#[derive(Debug, Clone)]
pub struct Cycle {
    /// Files involved in the cycle, in dependency order
    pub files: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_extension_recognizes_rust() {
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("RS"), Some(Language::Rust));
    }

    #[test]
    fn language_from_extension_recognizes_csharp() {
        assert_eq!(Language::from_extension("cs"), Some(Language::CSharp));
        assert_eq!(Language::from_extension("CS"), Some(Language::CSharp));
    }

    #[test]
    fn language_from_extension_returns_none_for_unknown() {
        assert_eq!(Language::from_extension("py"), None);
        assert_eq!(Language::from_extension("js"), None);
        assert_eq!(Language::from_extension(""), None);
    }

    #[test]
    fn symbol_full_path_with_module() {
        let symbol = Symbol {
            id: 1,
            file_id: 1,
            name: "save".to_string(),
            module_path: "crate::storage::issue".to_string(),
            qualified_name: "IssueStorage::save".to_string(),
            kind: SymbolKind::Method,
            line: 10,
            column: 1,
            span: None,
            signature: None,
            signature_details: None,
            visibility: Visibility::Public,
            parent_symbol_id: None,
        };

        assert_eq!(
            symbol.full_path(),
            "crate::storage::issue::IssueStorage::save"
        );
    }

    #[test]
    fn symbol_full_path_without_module() {
        let symbol = Symbol {
            id: 1,
            file_id: 1,
            name: "main".to_string(),
            module_path: String::new(),
            qualified_name: "main".to_string(),
            kind: SymbolKind::Function,
            line: 1,
            column: 1,
            span: None,
            signature: None,
            signature_details: None,
            visibility: Visibility::Private,
            parent_symbol_id: None,
        };

        assert_eq!(symbol.full_path(), "main");
    }

    // === FunctionSignature helper tests ===

    fn make_signature(return_type: Option<&str>, params: Vec<Parameter>) -> FunctionSignature {
        FunctionSignature {
            parameters: params,
            return_type: return_type.map(String::from),
            is_async: false,
            is_unsafe: false,
            is_const: false,
            generics: None,
        }
    }

    #[test]
    fn returns_result_true_for_result_type() {
        let sig = make_signature(Some("Result<(), Error>"), vec![]);
        assert!(sig.returns_result());
    }

    #[test]
    fn returns_result_true_for_nested_result() {
        let sig = make_signature(Some("Result<Result<T, E1>, E2>"), vec![]);
        assert!(sig.returns_result());
    }

    #[test]
    fn returns_result_false_for_none_return_type() {
        let sig = make_signature(None, vec![]);
        assert!(!sig.returns_result());
    }

    #[test]
    fn returns_result_false_for_other_type() {
        let sig = make_signature(Some("Option<i32>"), vec![]);
        assert!(!sig.returns_result());
    }

    #[test]
    fn returns_option_true_for_option_type() {
        let sig = make_signature(Some("Option<User>"), vec![]);
        assert!(sig.returns_option());
    }

    #[test]
    fn returns_option_true_for_nested_option() {
        let sig = make_signature(Some("Option<Option<T>>"), vec![]);
        assert!(sig.returns_option());
    }

    #[test]
    fn returns_option_false_for_none_return_type() {
        let sig = make_signature(None, vec![]);
        assert!(!sig.returns_option());
    }

    #[test]
    fn returns_option_false_for_other_type() {
        let sig = make_signature(Some("Result<(), Error>"), vec![]);
        assert!(!sig.returns_option());
    }

    #[test]
    fn is_method_true_for_self_parameter() {
        let sig = make_signature(
            None,
            vec![Parameter {
                name: "&self".to_string(),
                type_annotation: None,
            }],
        );
        assert!(sig.is_method());
    }

    #[test]
    fn is_method_false_for_empty_parameters() {
        let sig = make_signature(None, vec![]);
        assert!(!sig.is_method());
    }

    #[test]
    fn is_method_false_for_non_self_first_param() {
        let sig = make_signature(
            None,
            vec![Parameter {
                name: "other".to_string(),
                type_annotation: Some("i32".to_string()),
            }],
        );
        assert!(!sig.is_method());
    }

    #[test]
    fn param_count_excludes_self() {
        let sig = make_signature(
            None,
            vec![
                Parameter {
                    name: "&self".to_string(),
                    type_annotation: None,
                },
                Parameter {
                    name: "x".to_string(),
                    type_annotation: Some("i32".to_string()),
                },
                Parameter {
                    name: "y".to_string(),
                    type_annotation: Some("i32".to_string()),
                },
            ],
        );
        assert_eq!(sig.param_count(), 2);
    }

    #[test]
    fn param_count_for_function_without_self() {
        let sig = make_signature(
            None,
            vec![Parameter {
                name: "x".to_string(),
                type_annotation: Some("i32".to_string()),
            }],
        );
        assert_eq!(sig.param_count(), 1);
    }

    // === Parameter helper tests ===

    #[test]
    fn parameter_is_self_for_self_variants() {
        assert!(Parameter {
            name: "self".to_string(),
            type_annotation: None
        }
        .is_self());
        assert!(Parameter {
            name: "&self".to_string(),
            type_annotation: None
        }
        .is_self());
        assert!(Parameter {
            name: "&mut self".to_string(),
            type_annotation: None
        }
        .is_self());
    }

    #[test]
    fn parameter_is_self_false_for_regular_param() {
        assert!(!Parameter {
            name: "other".to_string(),
            type_annotation: None
        }
        .is_self());
        assert!(!Parameter {
            name: "self_ref".to_string(),
            type_annotation: None
        }
        .is_self());
    }

    #[test]
    fn parameter_is_mut_self_only_for_mut_self() {
        assert!(Parameter {
            name: "&mut self".to_string(),
            type_annotation: None
        }
        .is_mut_self());
        assert!(!Parameter {
            name: "&self".to_string(),
            type_annotation: None
        }
        .is_mut_self());
        assert!(!Parameter {
            name: "self".to_string(),
            type_annotation: None
        }
        .is_mut_self());
    }

    #[test]
    fn parameter_is_reference_for_reference_types() {
        assert!(Parameter {
            name: "x".to_string(),
            type_annotation: Some("&str".to_string())
        }
        .is_reference());
        assert!(Parameter {
            name: "x".to_string(),
            type_annotation: Some("&mut String".to_string())
        }
        .is_reference());
        assert!(Parameter {
            name: "x".to_string(),
            type_annotation: Some("&'a T".to_string())
        }
        .is_reference());
    }

    #[test]
    fn parameter_is_reference_false_for_owned_types() {
        assert!(!Parameter {
            name: "x".to_string(),
            type_annotation: Some("String".to_string())
        }
        .is_reference());
        assert!(!Parameter {
            name: "x".to_string(),
            type_annotation: Some("i32".to_string())
        }
        .is_reference());
    }

    #[test]
    fn parameter_is_reference_false_for_none_type() {
        assert!(!Parameter {
            name: "x".to_string(),
            type_annotation: None
        }
        .is_reference());
    }

    // === Span validation tests ===

    #[test]
    fn span_new_valid_same_line() {
        let span = Span::new(10, 5, 10, 20);
        assert!(span.is_some());
        let span = span.unwrap();
        assert_eq!(span.start_line, 10);
        assert_eq!(span.start_column, 5);
        assert_eq!(span.end_line, 10);
        assert_eq!(span.end_column, 20);
    }

    #[test]
    fn span_new_valid_different_lines() {
        let span = Span::new(5, 10, 15, 3);
        assert!(span.is_some());
        let span = span.unwrap();
        assert_eq!(span.start_line, 5);
        assert_eq!(span.end_line, 15);
    }

    #[test]
    fn span_new_valid_single_character() {
        // Same position is valid (represents a single character or cursor position)
        let span = Span::new(1, 1, 1, 1);
        assert!(span.is_some());
    }

    #[test]
    fn span_new_invalid_end_line_before_start() {
        let span = Span::new(10, 5, 8, 5);
        assert!(span.is_none());
    }

    #[test]
    fn span_new_invalid_end_column_before_start_same_line() {
        let span = Span::new(10, 20, 10, 5);
        assert!(span.is_none());
    }
}
