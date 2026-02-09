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

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::IndexError;

/// A strongly-typed symbol ID to prevent mixing with file IDs.
///
/// This newtype provides type safety for function signatures that accept
/// both symbol and file IDs, preventing accidental parameter swaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SymbolId(i64);

impl SymbolId {
    /// Extract the raw i64 value.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for SymbolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileId(i64);

impl FileId {
    /// Extract the raw i64 value.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for FileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for FileId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

/// Database primary key for a reference.
///
/// This newtype provides type safety for function signatures that accept
/// reference IDs, preventing accidental parameter swaps with symbol or file IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RefId(i64);

impl RefId {
    /// Extract the raw i64 value.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for RefId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for RefId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

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

/// Information about a Rust crate discovered from Cargo.toml.
///
/// Used to determine crate roots for module path resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateInfo {
    /// Crate name from `[package].name`
    pub name: String,
    /// Path to the crate directory (contains Cargo.toml)
    pub path: PathBuf,
    /// Library entry point relative to crate path (e.g., `src/lib.rs`)
    pub lib_path: Option<PathBuf>,
    /// Binary entry points: (name, path relative to crate)
    pub bin_paths: Vec<(String, PathBuf)>,
}

/// Symbol kinds tracked by Tethys.
///
/// These are normalized across languages. Not all languages have all kinds
/// (e.g., Rust has traits, C# has interfaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Unknown reference kind from database (possible version mismatch or corruption).
    /// Contains the raw string value that could not be parsed.
    Unknown(String),
}

impl ReferenceKind {
    /// Convert to database string representation.
    ///
    /// For known variants, returns the canonical string. For `Unknown`, returns `"unknown"`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Call => "call",
            Self::Type => "type",
            Self::Inherit => "inherit",
            Self::Construct => "construct",
            Self::FieldAccess => "field_access",
            Self::Unknown(_) => "unknown",
        }
    }

    /// Parse from database string representation.
    ///
    /// Returns `None` for unknown strings. Use [`ReferenceKind::parse_or_unknown`]
    /// to preserve unrecognized values as `Unknown(String)`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "import" => Some(Self::Import),
            "call" => Some(Self::Call),
            "type" => Some(Self::Type),
            "inherit" => Some(Self::Inherit),
            "construct" => Some(Self::Construct),
            "field_access" => Some(Self::FieldAccess),
            _ => None,
        }
    }

    /// Parse from database string, preserving unknown values.
    ///
    /// Unlike [`ReferenceKind::parse`], this never returns `None`. Unknown strings
    /// are wrapped in `ReferenceKind::Unknown(String)` to preserve the original value.
    #[must_use]
    pub fn parse_or_unknown(s: &str) -> Self {
        Self::parse(s).unwrap_or_else(|| Self::Unknown(s.to_string()))
    }

    /// Returns `true` if this is an `Unknown` variant.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }
}

/// Raw deserialization helper for [`Span`] that validates invariants on deserialize.
#[derive(Deserialize)]
struct SpanRaw {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

/// A start/end position span in a file.
///
/// Positions are 1-indexed (first line is 1, first column is 1) to match
/// editor conventions.
///
/// Use [`Span::new`] to construct a span with validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "SpanRaw")]
pub struct Span {
    /// Starting line (1-indexed)
    start_line: u32,
    /// Starting column (1-indexed)
    start_column: u32,
    /// Ending line (1-indexed, inclusive)
    end_line: u32,
    /// Ending column (1-indexed, exclusive)
    end_column: u32,
}

impl TryFrom<SpanRaw> for Span {
    type Error = String;

    fn try_from(raw: SpanRaw) -> std::result::Result<Self, Self::Error> {
        Span::new(
            raw.start_line,
            raw.start_column,
            raw.end_line,
            raw.end_column,
        )
        .ok_or_else(|| {
            format!(
                "invalid span: end ({},{}) is before start ({},{})",
                raw.end_line, raw.end_column, raw.start_line, raw.start_column
            )
        })
    }
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

    /// Returns the starting line (1-indexed).
    #[must_use]
    pub fn start_line(&self) -> u32 {
        self.start_line
    }

    /// Returns the starting column (1-indexed).
    #[must_use]
    pub fn start_column(&self) -> u32 {
        self.start_column
    }

    /// Returns the ending line (1-indexed, inclusive).
    #[must_use]
    pub fn end_line(&self) -> u32 {
        self.end_line
    }

    /// Returns the ending column (1-indexed, exclusive).
    #[must_use]
    pub fn end_column(&self) -> u32 {
        self.end_column
    }
}

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

/// Distinguishes self parameters from regular parameters at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParameterKind {
    /// `self` (owned)
    SelfValue,
    /// `&self` (shared reference)
    SelfRef,
    /// `&mut self` (mutable reference)
    SelfMutRef,
    /// A regular named parameter
    Regular,
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name (e.g., "`user_id`")
    pub name: String,
    /// Type annotation (e.g., "i64", "&str", "`Option<User>`")
    pub type_annotation: Option<String>,
    /// Whether this is a self parameter or a regular parameter.
    pub kind: ParameterKind,
}

impl Parameter {
    /// Check if this is a self parameter (&self, &mut self, self).
    #[must_use]
    pub fn is_self(&self) -> bool {
        matches!(
            self.kind,
            ParameterKind::SelfValue | ParameterKind::SelfRef | ParameterKind::SelfMutRef
        )
    }

    /// Check if this is a mutable self parameter.
    #[must_use]
    pub fn is_mut_self(&self) -> bool {
        self.kind == ParameterKind::SelfMutRef
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
    pub id: FileId,
    /// Path relative to workspace root
    pub path: PathBuf,
    /// Detected language
    pub language: Language,
    /// File modification time in nanoseconds since epoch
    pub mtime_ns: i64,
    /// File size in bytes
    pub size_bytes: u64,
    /// xxhash64 content hash for change detection
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
    pub id: SymbolId,
    /// Foreign key to the containing file
    pub file_id: FileId,
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
    pub parent_symbol_id: Option<SymbolId>,
    /// Whether this symbol is a test function.
    ///
    /// Detected by language-specific test attributes:
    /// - Rust: `#[test]`, `#[tokio::test]`, `#[async_std::test]`, `#[rstest]`, `#[quickcheck]`, `#[proptest]`
    /// - C#: `[Test]`, `[TestMethod]`, `[Fact]`, `[Theory]`, `[TestCase]`
    pub is_test: bool,
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

/// An import statement tracked for cross-file reference resolution.
///
/// Imports capture what each file imports from other modules, enabling
/// resolution of unqualified symbol references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    /// Foreign key to the file containing this import
    pub file_id: FileId,
    /// The imported symbol name (e.g., `Index`) or `*` for glob imports
    pub symbol_name: String,
    /// The source module path (e.g., `crate::db` or `MyApp.Services`)
    pub source_module: String,
    /// Optional alias (e.g., `use foo as bar` would have alias `bar`)
    pub alias: Option<String>,
}

/// An unresolved reference with file path information for LSP queries.
///
/// This is a denormalized view of references that includes the file path,
/// suitable for passing to LSP clients which need file paths, not database IDs.
#[derive(Debug, Clone)]
pub struct UnresolvedRefForLsp {
    /// Database primary key of the reference
    pub ref_id: RefId,
    /// Foreign key to the file containing this reference
    pub file_id: FileId,
    /// Path to the file (relative to workspace root)
    pub file_path: PathBuf,
    /// Line number of the reference (1-indexed, as stored in DB)
    pub line: u32,
    /// Column number of the reference (1-indexed, as stored in DB)
    pub column: u32,
    /// Name of the referenced symbol for resolution (e.g., `Index::open`)
    pub reference_name: String,
}

/// A reference to a symbol (usage, not definition).
///
/// Tracks where symbols are used throughout the codebase. Combined with
/// `in_symbol_id`, enables "who calls X?" queries at symbol granularity.
///
/// References may be unresolved (`symbol_id = None`) after initial indexing.
/// Pass 2 resolves these by matching `reference_name` to symbols in other files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    /// Database primary key
    pub id: i64,
    /// Foreign key to the referenced symbol (None if unresolved)
    pub symbol_id: Option<SymbolId>,
    /// Foreign key to the file containing this reference
    pub file_id: FileId,
    /// How the symbol is being used
    pub kind: ReferenceKind,
    /// Line number of the reference (1-indexed)
    pub line: u32,
    /// Column number of the reference (1-indexed)
    pub column: u32,
    /// Full extent of the reference (optional)
    pub span: Option<Span>,
    /// Symbol that contains this reference (for "who calls X?" queries)
    pub in_symbol_id: Option<SymbolId>,
    /// Name of the referenced symbol for later resolution (e.g., `Index::open`).
    /// Only populated for unresolved references.
    pub reference_name: Option<String>,
}

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

/// Default timeout for LSP solution loading (in seconds).
pub const DEFAULT_LSP_TIMEOUT_SECS: u64 = 60;

/// Options for configuring the indexing process.
#[derive(Debug, Clone, Copy)]
pub struct IndexOptions {
    /// Enable LSP-based resolution for references that tree-sitter cannot resolve.
    ///
    /// When enabled, after the tree-sitter resolution pass, tethys will spawn the
    /// appropriate language server and use `goto_definition` to resolve remaining
    /// unresolved references.
    use_lsp: bool,

    /// Timeout in seconds for LSP solution loading.
    ///
    /// Some language servers (like csharp-ls) load solutions asynchronously after
    /// initialization. This timeout controls how long to wait for loading to complete
    /// before proceeding with queries.
    ///
    /// Can be configured via the `TETHYS_LSP_TIMEOUT` environment variable.
    ///
    /// Default: 60 seconds
    lsp_timeout_secs: u64,

    /// Enable streaming writes during indexing.
    ///
    /// When enabled, parsed files are written to `SQLite` immediately via a background
    /// writer thread instead of accumulating all data in memory. This reduces memory
    /// usage from O(n) to O(1) for large codebases.
    ///
    /// Default: false (traditional batch mode)
    use_streaming: bool,

    /// Batch size for streaming writes.
    ///
    /// Number of files to accumulate before committing a transaction.
    /// Only used when `use_streaming` is true.
    ///
    /// Default: 100
    streaming_batch_size: usize,
}

impl IndexOptions {
    /// Create options with LSP resolution enabled.
    ///
    /// Checks the `TETHYS_LSP_TIMEOUT` environment variable for a custom timeout,
    /// defaulting to 60 seconds.
    #[must_use]
    pub fn with_lsp() -> Self {
        let timeout = std::env::var("TETHYS_LSP_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_LSP_TIMEOUT_SECS);

        Self {
            use_lsp: true,
            lsp_timeout_secs: timeout,
            use_streaming: false,
            streaming_batch_size: 100,
        }
    }

    /// Create options with streaming writes enabled.
    #[must_use]
    pub fn with_streaming() -> Self {
        Self {
            use_lsp: false,
            lsp_timeout_secs: DEFAULT_LSP_TIMEOUT_SECS,
            use_streaming: true,
            streaming_batch_size: 100,
        }
    }

    /// Create options with streaming writes and a custom batch size.
    #[must_use]
    pub fn with_streaming_batch_size(batch_size: usize) -> Self {
        Self {
            use_lsp: false,
            lsp_timeout_secs: DEFAULT_LSP_TIMEOUT_SECS,
            use_streaming: true,
            streaming_batch_size: batch_size,
        }
    }

    /// Enable streaming writes on this options instance.
    #[must_use]
    pub fn streaming(mut self) -> Self {
        self.use_streaming = true;
        self
    }

    /// Set a custom LSP timeout (in seconds).
    #[must_use]
    pub fn lsp_timeout(mut self, seconds: u64) -> Self {
        self.lsp_timeout_secs = seconds;
        self
    }

    /// Check if LSP-based resolution is enabled.
    #[must_use]
    pub fn use_lsp(&self) -> bool {
        self.use_lsp
    }

    /// Get the LSP solution loading timeout in seconds.
    #[must_use]
    pub fn lsp_timeout_secs(&self) -> u64 {
        self.lsp_timeout_secs
    }

    /// Check if streaming writes are enabled.
    #[must_use]
    pub fn use_streaming(&self) -> bool {
        self.use_streaming
    }

    /// Get the streaming batch size.
    #[must_use]
    pub fn streaming_batch_size(&self) -> usize {
        self.streaming_batch_size
    }
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            use_lsp: false,
            lsp_timeout_secs: DEFAULT_LSP_TIMEOUT_SECS,
            use_streaming: false,
            streaming_batch_size: 100,
        }
    }
}

/// Statistics from a full index operation.
///
/// Returned by [`Tethys::index()`], [`Tethys::index_with_options()`],
/// [`Tethys::rebuild()`], and [`Tethys::rebuild_with_options()`].
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
    /// Number of references resolved via LSP (Pass 3).
    ///
    /// Only non-zero when `IndexOptions::use_lsp` was set.
    pub lsp_resolved_count: usize,
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

/// Report of which files need re-indexing.
///
/// Returned by [`Tethys::get_stale_files()`](crate::Tethys::get_stale_files).
#[derive(Debug, Clone)]
pub struct StalenessReport {
    /// Files on disk whose mtime or size changed since last indexing
    pub modified: Vec<PathBuf>,
    /// Files on disk not yet in the index
    pub added: Vec<PathBuf>,
    /// Files in the index no longer on disk
    pub deleted: Vec<PathBuf>,
}

impl StalenessReport {
    /// Returns true if any files need re-indexing.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        !self.modified.is_empty() || !self.added.is_empty() || !self.deleted.is_empty()
    }

    /// Total number of files that need attention.
    #[must_use]
    pub fn total(&self) -> usize {
        self.modified.len() + self.added.len() + self.deleted.len()
    }
}

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

// === Reachability Analysis Types ===

/// Direction for reachability analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachabilityDirection {
    /// Forward: what can this symbol reach (follows callees)
    Forward,
    /// Backward: who can reach this symbol (follows callers)
    Backward,
}

/// A path from the source symbol to a reachable target.
///
/// Represents one reachable symbol and the call path used to reach it.
#[derive(Debug, Clone)]
pub struct ReachablePath {
    /// The reachable symbol at the end of this path.
    pub target: Symbol,
    /// Symbols along the path from source to target (excluding source, including target).
    pub path: Vec<Symbol>,
    /// Depth in the BFS traversal (1 = direct callee/caller).
    pub depth: usize,
}

/// Result of a reachability analysis query.
///
/// Contains all symbols reachable from a source, along with the paths used to reach them.
/// The BFS traversal terminates when:
/// - All reachable symbols within `max_depth` have been visited
/// - A cycle is detected (already-visited symbols are skipped)
/// - A database error occurs (fail-fast)
#[derive(Debug, Clone)]
pub struct ReachabilityResult {
    /// The starting symbol from which reachability was computed.
    /// For forward analysis, this is the caller; for backward, this is the callee.
    pub source: Symbol,
    /// All reachable symbols with their traversal paths.
    /// Ordered by discovery (BFS order), not alphabetically.
    pub reachable: Vec<ReachablePath>,
    /// Maximum depth that was used for the analysis.
    /// When `None` is passed to the query, this reflects the default (50).
    pub max_depth: usize,
    /// Direction of the analysis: `Forward` follows callees, `Backward` follows callers.
    pub direction: ReachabilityDirection,
}

impl ReachabilityResult {
    /// Get the count of unique reachable symbols.
    #[must_use]
    pub fn reachable_count(&self) -> usize {
        self.reachable.len()
    }

    /// Check if any symbols are reachable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reachable.is_empty()
    }

    /// Get symbols at a specific depth.
    #[must_use]
    pub fn at_depth(&self, depth: usize) -> Vec<&ReachablePath> {
        self.reachable.iter().filter(|r| r.depth == depth).collect()
    }
}

// === Panic Point Analysis Types ===

/// The type of panic-inducing call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanicKind {
    /// `.unwrap()` call
    Unwrap,
    /// `.expect()` call
    Expect,
}

impl PanicKind {
    /// Convert to database/display string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unwrap => "unwrap",
            Self::Expect => "expect",
        }
    }

    /// Parse from string representation.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unwrap" => Some(Self::Unwrap),
            "expect" => Some(Self::Expect),
            _ => None,
        }
    }
}

impl std::fmt::Display for PanicKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, ".{}()", self.as_str())
    }
}

/// A potential panic point in the codebase.
///
/// Represents a call to `.unwrap()` or `.expect()` that could panic at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct PanicPoint {
    /// Path to the file (relative to workspace root)
    pub path: PathBuf,
    /// Line number where the panic point occurs (1-indexed)
    pub line: u32,
    /// The type of panic call (unwrap or expect)
    pub kind: PanicKind,
    /// Name of the containing function/method
    pub containing_symbol: String,
    /// Whether this panic point is in test code
    pub is_test: bool,
}

impl PanicPoint {
    /// Create a new panic point with validation.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file (relative to workspace root)
    /// * `line` - Line number (1-indexed, must be >= 1)
    /// * `kind` - The type of panic call
    /// * `containing_symbol` - Name of the containing function/method (must be non-empty)
    /// * `is_test` - Whether this panic point is in test code
    ///
    /// # Panics
    ///
    /// Panics if `line` is 0 or `containing_symbol` is empty. These represent
    /// invalid state that should never occur from database queries.
    pub fn new(
        path: PathBuf,
        line: u32,
        kind: PanicKind,
        containing_symbol: String,
        is_test: bool,
    ) -> Self {
        debug_assert!(line >= 1, "line must be 1-indexed (got {line})");
        debug_assert!(
            !containing_symbol.is_empty(),
            "containing_symbol must be non-empty"
        );

        Self {
            path,
            line,
            kind,
            containing_symbol,
            is_test,
        }
    }
}

/// Statistics about the index database.
///
/// Returned by `Tethys::get_stats()`.
///
/// # Invariants
///
/// When returned from `Tethys::get_stats()`:
/// - `file_count` equals sum of `files_by_language` values (unknown languages are tracked
///   separately in `skipped_unknown_languages`)
/// - `symbol_count` equals sum of `symbols_by_kind` values (unknown kinds are tracked
///   separately in `skipped_unknown_kinds`)
#[derive(Debug, Clone, Default)]
pub struct DatabaseStats {
    /// Total number of indexed files
    pub file_count: usize,
    /// Files by language
    pub files_by_language: HashMap<Language, usize>,
    /// Total number of symbols
    pub symbol_count: usize,
    /// Symbols by kind
    pub symbols_by_kind: HashMap<SymbolKind, usize>,
    /// Total number of references
    pub reference_count: usize,
    /// Total number of file dependencies
    pub file_dependency_count: usize,
    /// Number of files with unrecognized language (possible version mismatch)
    pub skipped_unknown_languages: usize,
    /// Number of symbols with unrecognized kind (possible version mismatch)
    pub skipped_unknown_kinds: usize,
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
            id: SymbolId::from(1),
            file_id: FileId::from(1),
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
            is_test: false,
        };

        assert_eq!(
            symbol.full_path(),
            "crate::storage::issue::IssueStorage::save"
        );
    }

    #[test]
    fn symbol_full_path_without_module() {
        let symbol = Symbol {
            id: SymbolId::from(1),
            file_id: FileId::from(1),
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
            is_test: false,
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
                kind: ParameterKind::SelfRef,
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
                kind: ParameterKind::Regular,
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
                    kind: ParameterKind::SelfRef,
                },
                Parameter {
                    name: "x".to_string(),
                    type_annotation: Some("i32".to_string()),
                    kind: ParameterKind::Regular,
                },
                Parameter {
                    name: "y".to_string(),
                    type_annotation: Some("i32".to_string()),
                    kind: ParameterKind::Regular,
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
                kind: ParameterKind::Regular,
            }],
        );
        assert_eq!(sig.param_count(), 1);
    }

    // === Parameter helper tests ===

    #[test]
    fn parameter_is_self_for_self_variants() {
        assert!(Parameter {
            name: "self".to_string(),
            type_annotation: None,
            kind: ParameterKind::SelfValue,
        }
        .is_self());
        assert!(Parameter {
            name: "&self".to_string(),
            type_annotation: None,
            kind: ParameterKind::SelfRef,
        }
        .is_self());
        assert!(Parameter {
            name: "&mut self".to_string(),
            type_annotation: None,
            kind: ParameterKind::SelfMutRef,
        }
        .is_self());
    }

    #[test]
    fn parameter_is_self_false_for_regular_param() {
        assert!(!Parameter {
            name: "other".to_string(),
            type_annotation: None,
            kind: ParameterKind::Regular,
        }
        .is_self());
        assert!(!Parameter {
            name: "self_ref".to_string(),
            type_annotation: None,
            kind: ParameterKind::Regular,
        }
        .is_self());
    }

    #[test]
    fn parameter_is_mut_self_only_for_mut_self() {
        assert!(Parameter {
            name: "&mut self".to_string(),
            type_annotation: None,
            kind: ParameterKind::SelfMutRef,
        }
        .is_mut_self());
        assert!(!Parameter {
            name: "&self".to_string(),
            type_annotation: None,
            kind: ParameterKind::SelfRef,
        }
        .is_mut_self());
        assert!(!Parameter {
            name: "self".to_string(),
            type_annotation: None,
            kind: ParameterKind::SelfValue,
        }
        .is_mut_self());
    }

    #[test]
    fn parameter_is_reference_for_reference_types() {
        assert!(Parameter {
            name: "x".to_string(),
            type_annotation: Some("&str".to_string()),
            kind: ParameterKind::Regular,
        }
        .is_reference());
        assert!(Parameter {
            name: "x".to_string(),
            type_annotation: Some("&mut String".to_string()),
            kind: ParameterKind::Regular,
        }
        .is_reference());
        assert!(Parameter {
            name: "x".to_string(),
            type_annotation: Some("&'a T".to_string()),
            kind: ParameterKind::Regular,
        }
        .is_reference());
    }

    #[test]
    fn parameter_is_reference_false_for_owned_types() {
        assert!(!Parameter {
            name: "x".to_string(),
            type_annotation: Some("String".to_string()),
            kind: ParameterKind::Regular,
        }
        .is_reference());
        assert!(!Parameter {
            name: "x".to_string(),
            type_annotation: Some("i32".to_string()),
            kind: ParameterKind::Regular,
        }
        .is_reference());
    }

    #[test]
    fn parameter_is_reference_false_for_none_type() {
        assert!(!Parameter {
            name: "x".to_string(),
            type_annotation: None,
            kind: ParameterKind::Regular,
        }
        .is_reference());
    }

    // === Span validation tests ===

    #[test]
    fn span_new_valid_same_line() {
        let span = Span::new(10, 5, 10, 20);
        assert!(span.is_some());
        let span = span.unwrap();
        assert_eq!(span.start_line(), 10);
        assert_eq!(span.start_column(), 5);
        assert_eq!(span.end_line(), 10);
        assert_eq!(span.end_column(), 20);
    }

    #[test]
    fn span_new_valid_different_lines() {
        let span = Span::new(5, 10, 15, 3);
        assert!(span.is_some());
        let span = span.unwrap();
        assert_eq!(span.start_line(), 5);
        assert_eq!(span.end_line(), 15);
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

    #[test]
    fn span_deserialize_rejects_invalid() {
        let json = r#"{"start_line":10,"start_column":20,"end_line":10,"end_column":5}"#;
        let result: std::result::Result<Span, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should reject span where end < start");
    }

    #[test]
    fn span_deserialize_accepts_valid() {
        let json = r#"{"start_line":1,"start_column":1,"end_line":5,"end_column":10}"#;
        let span: Span = serde_json::from_str(json).expect("valid span should deserialize");
        assert_eq!(span.start_line(), 1);
        assert_eq!(span.end_line(), 5);
    }

    #[test]
    fn span_roundtrip_serde() {
        let span = Span::new(3, 5, 10, 20).expect("valid span");
        let json = serde_json::to_string(&span).expect("serialize");
        let deserialized: Span = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(span, deserialized);
    }

    // === ReferenceKind::parse() tests ===

    #[test]
    fn reference_kind_parse_import() {
        assert_eq!(ReferenceKind::parse("import"), Some(ReferenceKind::Import));
    }

    #[test]
    fn reference_kind_parse_call() {
        assert_eq!(ReferenceKind::parse("call"), Some(ReferenceKind::Call));
    }

    #[test]
    fn reference_kind_parse_type() {
        assert_eq!(ReferenceKind::parse("type"), Some(ReferenceKind::Type));
    }

    #[test]
    fn reference_kind_parse_inherit() {
        assert_eq!(
            ReferenceKind::parse("inherit"),
            Some(ReferenceKind::Inherit)
        );
    }

    #[test]
    fn reference_kind_parse_construct() {
        assert_eq!(
            ReferenceKind::parse("construct"),
            Some(ReferenceKind::Construct)
        );
    }

    #[test]
    fn reference_kind_parse_field_access() {
        assert_eq!(
            ReferenceKind::parse("field_access"),
            Some(ReferenceKind::FieldAccess)
        );
    }

    #[test]
    fn reference_kind_parse_unknown_returns_none() {
        assert_eq!(ReferenceKind::parse("unknown_kind"), None);
    }

    #[test]
    fn reference_kind_parse_empty_returns_none() {
        assert_eq!(ReferenceKind::parse(""), None);
    }

    #[test]
    fn reference_kind_parse_is_case_sensitive() {
        assert_eq!(ReferenceKind::parse("CALL"), None);
        assert_eq!(ReferenceKind::parse("Call"), None);
    }

    #[test]
    fn reference_kind_roundtrip_with_as_str() {
        // Ensure parse(kind.as_str()) == Some(kind) for all known variants.
        // Unknown doesn't roundtrip: parse_or_unknown("xyz") -> Unknown("xyz"),
        // Unknown.as_str() -> "unknown", so it's intentionally excluded.
        let variants = [
            ReferenceKind::Import,
            ReferenceKind::Call,
            ReferenceKind::Type,
            ReferenceKind::Inherit,
            ReferenceKind::Construct,
            ReferenceKind::FieldAccess,
        ];
        for kind in variants {
            let as_str = kind.as_str();
            let parsed = ReferenceKind::parse(as_str);
            assert_eq!(parsed, Some(kind.clone()), "roundtrip failed for {kind:?}");
        }
    }

    #[test]
    fn reference_kind_parse_or_unknown_preserves_unknown_values() {
        let kind = ReferenceKind::parse_or_unknown("some_future_kind");
        assert_eq!(kind, ReferenceKind::Unknown("some_future_kind".to_string()));
        assert!(kind.is_unknown());
    }

    #[test]
    fn reference_kind_parse_or_unknown_returns_known_for_valid_kinds() {
        assert_eq!(ReferenceKind::parse_or_unknown("call"), ReferenceKind::Call);
        assert!(!ReferenceKind::parse_or_unknown("call").is_unknown());
    }

    #[test]
    fn reference_kind_unknown_as_str_returns_unknown() {
        let kind = ReferenceKind::Unknown("anything".to_string());
        assert_eq!(kind.as_str(), "unknown");
    }

    // === Roundtrip tests for enum serialization/parsing ===

    #[test]
    fn symbol_kind_roundtrip() {
        let variants = [
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Struct,
            SymbolKind::Class,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Interface,
            SymbolKind::Const,
            SymbolKind::Static,
            SymbolKind::Module,
            SymbolKind::TypeAlias,
            SymbolKind::Macro,
        ];
        for kind in variants {
            let db_str = kind.as_str();
            let parsed = crate::db::parse_symbol_kind(db_str);
            assert_eq!(
                parsed.expect("parse should succeed"),
                kind,
                "roundtrip failed for {kind:?}"
            );
        }
    }

    #[test]
    fn visibility_roundtrip() {
        let variants = [
            Visibility::Public,
            Visibility::Crate,
            Visibility::Module,
            Visibility::Private,
        ];
        for vis in variants {
            let db_str = vis.as_str();
            let parsed = crate::db::parse_visibility(db_str);
            assert_eq!(
                parsed.expect("parse should succeed"),
                vis,
                "roundtrip failed for {vis:?}"
            );
        }
    }

    #[test]
    fn language_roundtrip() {
        let variants = [Language::Rust, Language::CSharp];
        for lang in variants {
            let db_str = lang.as_str();
            let parsed = crate::db::parse_language(db_str);
            assert_eq!(
                parsed.expect("parse should succeed"),
                lang,
                "roundtrip failed for {lang:?}"
            );
        }
    }

    // === PanicKind tests ===

    #[test]
    fn panic_kind_parse_unwrap() {
        assert_eq!(PanicKind::parse("unwrap"), Some(PanicKind::Unwrap));
    }

    #[test]
    fn panic_kind_parse_expect() {
        assert_eq!(PanicKind::parse("expect"), Some(PanicKind::Expect));
    }

    #[test]
    fn panic_kind_parse_unknown_returns_none() {
        assert_eq!(PanicKind::parse("panic"), None);
        assert_eq!(PanicKind::parse(""), None);
    }

    #[test]
    fn panic_kind_roundtrip() {
        let variants = [PanicKind::Unwrap, PanicKind::Expect];
        for kind in variants {
            let str_repr = kind.as_str();
            let parsed = PanicKind::parse(str_repr);
            assert_eq!(parsed, Some(kind), "roundtrip failed for {kind:?}");
        }
    }

    #[test]
    fn panic_kind_display() {
        assert_eq!(format!("{}", PanicKind::Unwrap), ".unwrap()");
        assert_eq!(format!("{}", PanicKind::Expect), ".expect()");
    }

    // === CrateInfo tests ===

    #[test]
    fn crate_info_default_lib_path() {
        let info = CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/workspace/crates/my_crate"),
            lib_path: None,
            bin_paths: vec![],
        };
        assert_eq!(info.name, "my_crate");
        assert!(info.lib_path.is_none());
    }

    // === Property-based tests ===

    mod proptest_tests {
        use super::*;
        use proptest::prelude::*;

        fn arb_symbol_kind() -> impl Strategy<Value = SymbolKind> {
            prop_oneof![
                Just(SymbolKind::Function),
                Just(SymbolKind::Method),
                Just(SymbolKind::Struct),
                Just(SymbolKind::Class),
                Just(SymbolKind::Enum),
                Just(SymbolKind::Trait),
                Just(SymbolKind::Interface),
                Just(SymbolKind::Const),
                Just(SymbolKind::Static),
                Just(SymbolKind::Module),
                Just(SymbolKind::TypeAlias),
                Just(SymbolKind::Macro),
            ]
        }

        fn arb_language() -> impl Strategy<Value = Language> {
            prop_oneof![Just(Language::Rust), Just(Language::CSharp),]
        }

        fn arb_visibility() -> impl Strategy<Value = Visibility> {
            prop_oneof![
                Just(Visibility::Public),
                Just(Visibility::Crate),
                Just(Visibility::Module),
                Just(Visibility::Private),
            ]
        }

        fn arb_panic_kind() -> impl Strategy<Value = PanicKind> {
            prop_oneof![Just(PanicKind::Unwrap), Just(PanicKind::Expect),]
        }

        fn arb_known_reference_kind() -> impl Strategy<Value = ReferenceKind> {
            prop_oneof![
                Just(ReferenceKind::Import),
                Just(ReferenceKind::Call),
                Just(ReferenceKind::Type),
                Just(ReferenceKind::Inherit),
                Just(ReferenceKind::Construct),
                Just(ReferenceKind::FieldAccess),
            ]
        }

        proptest! {
            #[test]
            fn symbol_kind_as_str_roundtrips(kind in arb_symbol_kind()) {
                let parsed = crate::db::parse_symbol_kind(kind.as_str())
                    .expect("parse_symbol_kind should succeed for valid as_str output");
                prop_assert_eq!(parsed, kind);
            }

            #[test]
            fn language_as_str_roundtrips(lang in arb_language()) {
                let parsed = crate::db::parse_language(lang.as_str())
                    .expect("parse_language should succeed for valid as_str output");
                prop_assert_eq!(parsed, lang);
            }

            #[test]
            fn visibility_as_str_roundtrips(vis in arb_visibility()) {
                let parsed = crate::db::parse_visibility(vis.as_str())
                    .expect("parse_visibility should succeed for valid as_str output");
                prop_assert_eq!(parsed, vis);
            }

            #[test]
            fn panic_kind_as_str_roundtrips(kind in arb_panic_kind()) {
                let parsed = PanicKind::parse(kind.as_str());
                prop_assert_eq!(parsed, Some(kind));
            }

            #[test]
            fn reference_kind_as_str_roundtrips(kind in arb_known_reference_kind()) {
                let parsed = ReferenceKind::parse(kind.as_str());
                prop_assert_eq!(parsed, Some(kind));
            }

            #[test]
            fn reference_kind_parse_or_unknown_never_panics(s in "\\PC{0,50}") {
                let _ = ReferenceKind::parse_or_unknown(&s);
            }

            #[test]
            fn span_valid_construction_preserves_invariant(
                sl in 1u32..10000,
                sc in 1u32..10000,
                extra_lines in 0u32..1000,
                extra_cols in 0u32..1000,
            ) {
                let el = sl + extra_lines;
                let ec = if el == sl { sc + extra_cols } else { 1 + extra_cols };
                if let Some(span) = Span::new(sl, sc, el, ec) {
                    prop_assert!(span.end_line() >= span.start_line());
                    if span.end_line() == span.start_line() {
                        prop_assert!(span.end_column() >= span.start_column());
                    }
                }
            }

            #[test]
            fn span_serde_roundtrip(
                sl in 1u32..10000,
                sc in 1u32..10000,
                extra_lines in 0u32..1000,
                extra_cols in 0u32..1000,
            ) {
                let el = sl + extra_lines;
                let ec = if el == sl { sc + extra_cols } else { 1 + extra_cols };
                if let Some(span) = Span::new(sl, sc, el, ec) {
                    let json = serde_json::to_string(&span)
                        .expect("span serialization should succeed");
                    let deserialized: Span = serde_json::from_str(&json)
                        .expect("span deserialization should succeed");
                    prop_assert_eq!(span, deserialized);
                }
            }

            #[test]
            fn span_rejects_invalid_same_line(
                line in 1u32..10000,
                start_col in 2u32..10000,
                end_col_offset in 1u32..10000,
            ) {
                let end_col = start_col.saturating_sub(end_col_offset);
                if end_col < start_col {
                    prop_assert!(Span::new(line, start_col, line, end_col).is_none());
                }
            }
        }
    }
}
