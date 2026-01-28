//! # Tethys: Code Intelligence Cache and Query Interface
//!
//! Tethys provides fast, approximate code intelligence by indexing source files
//! with tree-sitter and caching results in `SQLite`. It is designed for programmatic
//! use by CLI tools, MCP servers, and AI agents.
//!
//! ## Design Philosophy
//!
//! - **Cache, not analyzer** - Tethys indexes and caches; LSPs do the hard semantic work
//! - **Layered accuracy** - Fast approximate results (tree-sitter), optional precision (LSP integration)
//! - **Language extensible** - Start with Rust + C#, design for adding more
//! - **Embeddable** - Library first, CLI second
//! - **Intelligence, not policy** - Reports facts ("12 callers"), not judgments ("too risky")
//!
//! ## Quick Start
//!
//! ```no_run
//! use tethys::Tethys;
//! use std::path::Path;
//!
//! let mut tethys = Tethys::new(Path::new("/path/to/workspace"))?;
//!
//! // Index the workspace
//! let stats = tethys.index()?;
//! println!("Indexed {} files, found {} symbols", stats.files_indexed, stats.symbols_found);
//!
//! // Search for symbols
//! let symbols = tethys.search_symbols("authenticate")?;
//!
//! // Get impact analysis
//! let impact = tethys.get_impact(Path::new("src/auth.rs"))?;
//! println!("{} direct dependents", impact.direct_dependents.len());
//! # Ok::<(), tethys::Error>(())
//! ```

mod db;
mod error;
mod graph;
mod languages;
mod resolver;
mod types;

pub use error::{Error, IndexError, IndexErrorKind, Result};
pub use types::{
    Cycle, DatabaseStats, Dependent, FileAnalysis, FileId, FunctionSignature, Impact, IndexStats,
    IndexUpdate, IndexedFile, Language, Parameter, ParameterKind, Reference, ReferenceKind, Span,
    Symbol, SymbolId, SymbolKind, Visibility,
};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use db::{Index, SymbolData};
use graph::{FileGraphOps, SqlFileGraph, SqlSymbolGraph, SymbolGraphOps};
use languages::common;
use resolver::resolve_module_path;
use tracing::{debug, trace, warn};

/// A dependency that couldn't be resolved because the target file wasn't indexed yet.
///
/// These are collected during the first indexing pass and resolved in subsequent passes.
#[derive(Debug)]
struct PendingDependency {
    /// The file ID that has the dependency.
    from_file_id: FileId,
    /// The path to the file being depended on (relative to workspace root).
    dep_path: PathBuf,
}

/// Code intelligence cache and query interface.
///
/// `Tethys` is the main entry point for code intelligence operations. It manages
/// a `SQLite` index of symbols and references extracted from source files using
/// tree-sitter, and provides query methods for searching, dependency analysis,
/// and impact assessment.
pub struct Tethys {
    workspace_root: PathBuf,
    db_path: PathBuf,
    db: Index,
    parser: tree_sitter::Parser,
    symbol_graph: Box<dyn SymbolGraphOps>,
    file_graph: Box<dyn FileGraphOps>,
}

// Note: `# Errors` docs deferred to avoid documentation churn during active development.
// See https://rust-lang.github.io/api-guidelines/documentation.html#c-failure
#[allow(clippy::missing_errors_doc)]
impl Tethys {
    /// Create a new Tethys instance for a workspace.
    ///
    /// Uses convention-based defaults:
    /// - Excludes hidden directories (starting with `.`)
    /// - Excludes common build directories (`target/`, `node_modules/`, `bin/`, `obj/`, `build/`, `dist/`, `vendor/`, `__pycache__`)
    /// - Database stored at `.rivets/index/tethys.db`
    pub fn new(workspace_root: &Path) -> Result<Self> {
        let workspace_root = workspace_root.canonicalize().map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("workspace root not found: {}", workspace_root.display()),
            ))
        })?;

        let db_path = workspace_root
            .join(".rivets")
            .join("index")
            .join("tethys.db");
        let db = Index::open(&db_path)?;

        let parser = tree_sitter::Parser::new();

        // Initialize graph operations with their own DB connections
        let symbol_graph: Box<dyn SymbolGraphOps> = Box::new(SqlSymbolGraph::new(&db_path)?);
        let file_graph: Box<dyn FileGraphOps> = Box::new(SqlFileGraph::new(&db_path)?);

        Ok(Self {
            workspace_root,
            db_path,
            db,
            parser,
            symbol_graph,
            file_graph,
        })
    }

    /// Create with LSP refinement (placeholder - not yet implemented).
    ///
    /// # Note
    ///
    /// This method is a **placeholder**. LSP integration is planned for Phase 6.
    /// Currently behaves identically to [`Self::new`] and ignores `lsp_command`.
    #[allow(unused_variables)]
    pub fn with_lsp(workspace_root: &Path, lsp_command: &str) -> Result<Self> {
        tracing::warn!(
            lsp_command,
            "LSP integration not yet implemented, falling back to tree-sitter only"
        );
        Self::new(workspace_root)
    }

    // === Indexing ===

    /// Index all source files in the workspace.
    ///
    /// Uses deferred dependency resolution to handle circular dependencies:
    /// 1. First pass: Index all files, queue dependencies that can't resolve
    /// 2. Resolution passes: Retry pending dependencies until no progress
    pub fn index(&mut self) -> Result<IndexStats> {
        let start = Instant::now();
        let mut files_indexed = 0;
        let mut symbols_found = 0;
        let mut references_found = 0;
        let mut files_skipped = 0;
        let mut directories_skipped = Vec::new();
        let mut errors = Vec::new();
        let mut pending: Vec<PendingDependency> = Vec::new();

        // Walk the workspace and find source files
        let files = self.discover_files(&mut directories_skipped)?;

        // First pass: index all files, collecting unresolved dependencies
        for file_path in files {
            // Check if it's a supported language
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

            let Some(language) = Language::from_extension(ext) else {
                files_skipped += 1;
                continue;
            };

            // Read and parse the file
            match self.index_file(&file_path, language, &mut pending) {
                Ok((sym_count, ref_count)) => {
                    files_indexed += 1;
                    symbols_found += sym_count;
                    references_found += ref_count;
                }
                Err(e) => {
                    let kind = match &e {
                        Error::Io(_) => IndexErrorKind::IoError,
                        Error::Database(_) => IndexErrorKind::DatabaseError,
                        Error::Parser(_) => IndexErrorKind::ParseFailed,
                        // Fallback for other error types
                        Error::Config(_) | Error::NotFound(_) | Error::Internal(_) => {
                            IndexErrorKind::ParseFailed
                        }
                    };
                    errors.push(IndexError::new(file_path.clone(), kind, e.to_string()));
                }
            }
        }

        // C# namespace resolution pass: resolve using directives via namespace map
        // This must happen after all files are indexed so we have the complete namespace map
        let namespace_map = self.build_namespace_map()?;
        if !namespace_map.is_empty() {
            debug!(
                namespace_count = namespace_map.len(),
                "Resolving C# dependencies via namespace map"
            );
            self.resolve_csharp_dependencies(&namespace_map)?;
        }

        // Resolution passes: retry pending dependencies until stable
        let mut prev_count = pending.len() + 1;
        let mut pass = 0;
        while !pending.is_empty() && pending.len() < prev_count {
            pass += 1;
            let before = pending.len();
            prev_count = before;
            pending = self.resolve_pending(pending)?;
            debug!(
                pass,
                resolved = before - pending.len(),
                remaining = pending.len(),
                "Dependency resolution pass completed"
            );
        }

        // Convert remaining pending to (from_path, dep_path) for reporting
        let unresolved_dependencies: Vec<(PathBuf, PathBuf)> = pending
            .into_iter()
            .filter_map(|p| match self.db.get_file_by_id(p.from_file_id) {
                Ok(Some(f)) => Some((f.path, p.dep_path)),
                Ok(None) => {
                    warn!(
                        file_id = %p.from_file_id,
                        "File not found when building unresolved deps list"
                    );
                    None
                }
                Err(e) => {
                    warn!(
                        file_id = %p.from_file_id,
                        error = %e,
                        "DB error when building unresolved deps list"
                    );
                    None
                }
            })
            .collect();

        // Log unresolved dependencies with actual file paths
        for (from_path, dep_path) in &unresolved_dependencies {
            debug!(
                from_file = %from_path.display(),
                dep_path = %dep_path.display(),
                "Dependency unresolved after all passes (likely external crate)"
            );
        }

        Ok(IndexStats {
            files_indexed,
            symbols_found,
            references_found,
            duration: start.elapsed(),
            files_skipped,
            directories_skipped,
            errors,
            unresolved_dependencies,
        })
    }

    /// Index a single file.
    ///
    /// Uses a database transaction to ensure atomicity - either the file and all
    /// its symbols are stored, or nothing is changed on failure.
    ///
    /// Unresolved dependencies (target file not yet indexed) are added to `pending`.
    ///
    /// Returns (`symbol_count`, `reference_count`).
    fn index_file(
        &mut self,
        path: &Path,
        language: Language,
        pending: &mut Vec<PendingDependency>,
    ) -> Result<(usize, usize)> {
        let content = std::fs::read(path)?;
        let content_str = std::str::from_utf8(&content)
            .map_err(|_| Error::Parser("file is not valid UTF-8".to_string()))?;

        // Get language support for extraction
        let lang_support = languages::get_language_support(language)
            .ok_or_else(|| Error::Parser(format!("no support for language: {language:?}")))?;

        // Set parser to the correct tree-sitter language
        self.parser
            .set_language(&lang_support.tree_sitter_language())
            .map_err(|e| Error::Parser(e.to_string()))?;

        // Get file metadata
        let metadata = std::fs::metadata(path)?;
        #[allow(clippy::cast_possible_truncation)] // Nanoseconds fit in i64 for centuries
        let mtime_ns = match metadata.modified() {
            Ok(mtime) => match mtime.duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos() as i64,
                Err(e) => {
                    warn!(
                        file = %path.display(),
                        error = %e,
                        "File modification time is before Unix epoch, using 0"
                    );
                    0
                }
            },
            Err(e) => {
                warn!(
                    file = %path.display(),
                    error = %e,
                    "Platform does not support file modification time, using 0"
                );
                0
            }
        };
        let size_bytes = metadata.len();

        // Parse with tree-sitter
        let tree = self
            .parser
            .parse(content_str, None)
            .ok_or_else(|| Error::Parser("failed to parse file".to_string()))?;

        // Extract symbols
        let extracted = lang_support.extract_symbols(&tree, content_str.as_bytes());

        // Extract import statements and references for dependency detection
        let imports = lang_support.extract_imports(&tree, content_str.as_bytes());
        let refs = lang_support.extract_references(&tree, content_str.as_bytes());

        // Convert to SymbolData for atomic insertion
        let qualified_names: Vec<String> = extracted
            .iter()
            .map(|sym| {
                if let Some(parent) = &sym.parent_name {
                    format!("{}::{}", parent, sym.name)
                } else {
                    sym.name.clone()
                }
            })
            .collect();

        let symbol_data: Vec<SymbolData> = extracted
            .iter()
            .zip(qualified_names.iter())
            .map(|(sym, qn)| SymbolData {
                name: &sym.name,
                module_path: "", // TODO: compute module_path
                qualified_name: qn,
                kind: sym.kind,
                line: sym.line,
                column: sym.column,
                span: sym.span,
                signature: sym.signature.as_deref(),
                visibility: sym.visibility,
                parent_symbol_id: None, // TODO: parent_symbol_id
            })
            .collect();

        // Store in database atomically
        let file_id = self.db.index_file_atomic(
            self.relative_path(path),
            language,
            mtime_ns,
            size_bytes,
            None, // TODO: compute content hash
            &symbol_data,
        )?;

        // Build lookup maps for reference insertion
        let stored_symbols = self.db.list_symbols_in_file(file_id)?;
        let (name_to_id, span_to_id) = Self::build_symbol_maps(&stored_symbols);

        // Store references for resolvable symbols
        let refs_stored = self.store_references(file_id, &refs, &name_to_id, &span_to_id)?;

        // Compute and store file dependencies (L2: only for actually used symbols)
        self.compute_dependencies(path, file_id, &imports, &refs, pending)?;

        Ok((extracted.len(), refs_stored))
    }

    /// Build lookup maps from symbols for reference resolution.
    ///
    /// Returns (`name -> id`, `span -> id`) maps.
    fn build_symbol_maps(
        symbols: &[Symbol],
    ) -> (HashMap<String, SymbolId>, HashMap<Span, SymbolId>) {
        let mut name_to_id: HashMap<String, SymbolId> = HashMap::new();
        let mut span_to_id: HashMap<Span, SymbolId> = HashMap::new();

        for sym in symbols {
            // Map name to ID (log if duplicate, last one wins)
            if let Some(prev_id) = name_to_id.insert(sym.name.clone(), sym.id) {
                trace!(
                    name = %sym.name,
                    new_id = %sym.id,
                    prev_id = %prev_id,
                    "Duplicate symbol name in file, using newer"
                );
            }

            // Map span to ID for containing symbol resolution
            if let Some(span) = sym.span {
                span_to_id.insert(span, sym.id);
            }
        }

        (name_to_id, span_to_id)
    }

    /// Store extracted references in the database.
    ///
    /// Only stores references where the target symbol can be resolved.
    /// Returns the count of references stored.
    fn store_references(
        &self,
        file_id: FileId,
        refs: &[common::ExtractedReference],
        name_to_id: &HashMap<String, SymbolId>,
        span_to_id: &HashMap<Span, SymbolId>,
    ) -> Result<usize> {
        let mut count = 0;

        for r in refs {
            // Try to resolve the target symbol by name
            let Some(&symbol_id) = name_to_id.get(&r.name) else {
                // Cross-file symbol resolution not yet implemented
                trace!(
                    reference_name = %r.name,
                    line = r.line,
                    "Skipping cross-file reference (symbol not in this file)"
                );
                continue;
            };

            // Resolve containing symbol if present
            let in_symbol_id = r
                .containing_symbol_span
                .and_then(|span| span_to_id.get(&span).copied());

            // Insert the reference
            let db_kind = r.kind.to_db_kind();
            self.db.insert_reference(
                symbol_id,
                file_id,
                db_kind.as_str(),
                r.line,
                r.column,
                in_symbol_id,
            )?;
            count += 1;
        }

        Ok(count)
    }

    /// Compute and store file-level dependencies based on use statements and actual references.
    ///
    /// This is L2 dependency detection: we only count a dependency if the imported symbol
    /// is actually used in the code, not just imported.
    ///
    /// Dependencies that can't be resolved (target file not yet indexed) are added to
    /// `pending` for retry in subsequent passes.
    fn compute_dependencies(
        &self,
        current_file: &Path,
        file_id: FileId,
        imports: &[common::ImportStatement],
        refs: &[common::ExtractedReference],
        pending: &mut Vec<PendingDependency>,
    ) -> Result<()> {
        use std::collections::HashSet;

        // Build a set of actually referenced names (both direct names and path prefixes)
        let mut referenced_names: HashSet<&str> = HashSet::new();
        for r in refs {
            referenced_names.insert(&r.name);
            // Also add the first path component if present (for `Foo::bar()` style calls)
            if let Some(path) = &r.path {
                if let Some(first) = path.first() {
                    referenced_names.insert(first);
                }
            }
        }

        // FIXME: Assumes crate root is workspace_root/src/. Does not detect actual
        // main/lib location from Cargo.toml. Needs Cargo.toml parsing support.
        let crate_root = self.workspace_root.join("src");

        // Track which files we depend on (dedupe)
        let mut depended_files: HashSet<PathBuf> = HashSet::new();

        for import_stmt in imports {
            // Skip glob imports - can't determine what's used
            if import_stmt.is_glob {
                continue;
            }

            // Check if any imported name from this import statement is actually referenced
            let mut is_used = false;
            for name in &import_stmt.imported_names {
                // Check both the original name and alias
                let lookup_name = import_stmt.alias.as_ref().unwrap_or(name);
                if referenced_names.contains(lookup_name.as_str()) {
                    is_used = true;
                    break;
                }
            }

            // Only record dependency if the import is actually used (L2 behavior)
            if !is_used {
                continue;
            }

            // Resolve the module path to a file
            if let Some(resolved) =
                resolve_module_path(&import_stmt.path, current_file, &crate_root)
            {
                // Make the path relative to workspace root
                let dep_path = self.relative_path(&resolved).to_path_buf();
                depended_files.insert(dep_path);
            }
        }

        // Store dependencies in the database, queueing unresolved ones for later
        for dep_path in depended_files {
            match self.db.get_file_id(&dep_path)? {
                Some(dep_file_id) => {
                    self.db.insert_file_dependency(file_id, dep_file_id)?;
                }
                None => {
                    // Target file not indexed yet - queue for resolution pass
                    pending.push(PendingDependency {
                        from_file_id: file_id,
                        dep_path,
                    });
                }
            }
        }

        Ok(())
    }

    /// Maximum number of namespace symbols to query for C# dependency resolution.
    ///
    /// This limit exists to prevent unbounded queries on very large codebases.
    /// 10,000 covers typical enterprise C# projects (1000-5000 files averaging 2-3
    /// namespaces each). If this limit is reached, some C# dependencies may not
    /// be resolved and a warning is logged.
    const NAMESPACE_QUERY_LIMIT: usize = 10_000;

    /// Build a namespace-to-file map from indexed C# Module symbols.
    ///
    /// This enables C# `using` directive resolution: `using MyApp.Services`
    /// resolves to whichever files declare `namespace MyApp.Services`.
    fn build_namespace_map(&self) -> Result<HashMap<String, Vec<FileId>>> {
        use std::collections::HashSet;

        let mut map: HashMap<String, Vec<FileId>> = HashMap::new();

        // Query all Module-kind symbols (namespaces)
        let symbols = self
            .db
            .search_symbols_by_kind(SymbolKind::Module, Self::NAMESPACE_QUERY_LIMIT)?;

        if symbols.len() >= Self::NAMESPACE_QUERY_LIMIT {
            warn!(
                limit = Self::NAMESPACE_QUERY_LIMIT,
                "Namespace query limit reached, some C# dependencies may not be resolved"
            );
        }

        // Fetch all C# file IDs upfront to avoid N+1 queries
        let csharp_file_ids: HashSet<FileId> = self
            .db
            .get_files_by_language(Language::CSharp)?
            .into_iter()
            .map(|f| f.id)
            .collect();

        for sym in symbols {
            // Only include C# files (Rust modules use different resolution)
            if csharp_file_ids.contains(&sym.file_id) {
                map.entry(sym.name.clone()).or_default().push(sym.file_id);
            }
        }

        Ok(map)
    }

    /// Resolve C# file dependencies using namespace-to-file mapping.
    ///
    /// For each C# file, look at its `using` directives and find which files
    /// declare those namespaces. Record file-level dependencies.
    fn resolve_csharp_dependencies(
        &mut self,
        namespace_map: &HashMap<String, Vec<FileId>>,
    ) -> Result<()> {
        // Track processing statistics for visibility
        let mut files_processed: usize = 0;
        let mut files_skipped_read: usize = 0;
        let mut files_skipped_utf8: usize = 0;
        let mut files_skipped_parse: usize = 0;

        // Get language support once before the loop
        let lang_support = languages::get_language_support(Language::CSharp).ok_or_else(|| {
            Error::Parser("No language support for C#, cannot resolve C# dependencies".to_string())
        })?;

        self.parser
            .set_language(&lang_support.tree_sitter_language())
            .map_err(|e| Error::Parser(format!("Failed to set parser language to C#: {e}")))?;

        // Get all C# files
        let csharp_files = self.db.get_files_by_language(Language::CSharp)?;

        for file in &csharp_files {
            // Get the using directives for this file by re-parsing
            let full_path = self.workspace_root.join(&file.path);
            let content = match std::fs::read(&full_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        file = %full_path.display(),
                        error = %e,
                        "Failed to read C# file for dependency resolution"
                    );
                    files_skipped_read += 1;
                    continue;
                }
            };
            let content_str = match std::str::from_utf8(&content) {
                Ok(s) => s,
                Err(e) => {
                    warn!(
                        file = %full_path.display(),
                        error = %e,
                        "C# file is not valid UTF-8, skipping dependency resolution"
                    );
                    files_skipped_utf8 += 1;
                    continue;
                }
            };

            let Some(tree) = self.parser.parse(content_str, None) else {
                warn!(
                    file = %full_path.display(),
                    "Failed to parse C# file for dependency resolution"
                );
                files_skipped_parse += 1;
                continue;
            };

            let imports = lang_support.extract_imports(&tree, content_str.as_bytes());

            for import in &imports {
                // Join path segments to form namespace name: ["MyApp", "Services"] -> "MyApp.Services"
                let namespace = import.path.join(".");

                if let Some(file_ids) = namespace_map.get(&namespace) {
                    for &dep_file_id in file_ids {
                        // Don't add self-dependency
                        if dep_file_id != file.id {
                            self.db.insert_file_dependency(file.id, dep_file_id)?;
                        }
                    }
                }
            }

            files_processed += 1;
        }

        // Log summary if any files were skipped
        let total_skipped = files_skipped_read + files_skipped_utf8 + files_skipped_parse;
        let total_files = files_processed + total_skipped;
        if total_skipped > 0 {
            if total_skipped > files_processed {
                // More than half failed - likely a systemic issue
                tracing::error!(
                    total_files,
                    files_processed,
                    files_skipped_read,
                    files_skipped_utf8,
                    files_skipped_parse,
                    "C# dependency resolution mostly failed - check file permissions and encoding"
                );
            } else {
                warn!(
                    total_files,
                    files_processed,
                    files_skipped_read,
                    files_skipped_utf8,
                    files_skipped_parse,
                    "C# dependency resolution completed with some files skipped"
                );
            }
        }

        Ok(())
    }

    /// Retry resolving pending dependencies.
    ///
    /// Returns dependencies that still couldn't be resolved.
    fn resolve_pending(&self, pending: Vec<PendingDependency>) -> Result<Vec<PendingDependency>> {
        let mut still_pending = Vec::new();

        for p in pending {
            match self.db.get_file_id(&p.dep_path)? {
                Some(dep_file_id) => {
                    self.db
                        .insert_file_dependency(p.from_file_id, dep_file_id)?;
                }
                None => {
                    // Still not found - keep for next pass or final logging
                    still_pending.push(p);
                }
            }
        }

        Ok(still_pending)
    }

    /// Discover source files in the workspace.
    fn discover_files(
        &self,
        directories_skipped: &mut Vec<(PathBuf, String)>,
    ) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        self.walk_dir(&self.workspace_root, &mut files, directories_skipped)?;
        Ok(files)
    }

    /// Recursively walk a directory, collecting source files.
    ///
    /// Directories that cannot be read (e.g., due to permissions) are tracked
    /// in `directories_skipped` for reporting.
    #[allow(clippy::only_used_in_recursion)] // Method design, may use self in future
    fn walk_dir(
        &self,
        dir: &Path,
        files: &mut Vec<PathBuf>,
        directories_skipped: &mut Vec<(PathBuf, String)>,
    ) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    directory = %dir.display(),
                    error = %e,
                    "Cannot read directory, skipping"
                );
                directories_skipped.push((dir.to_path_buf(), e.to_string()));
                return Ok(());
            }
        };

        for entry in entries {
            // Explicitly handle entry errors instead of silently skipping with flatten()
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!(
                        directory = %dir.display(),
                        error = %e,
                        "Failed to read directory entry, skipping"
                    );
                    continue;
                }
            };

            let path = entry.path();

            // Skip hidden directories and common build directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || Self::is_excluded_dir(name) {
                    continue;
                }
            }

            if path.is_dir() {
                self.walk_dir(&path, files, directories_skipped)?;
            } else if path.is_file() {
                // Check if it's a supported file type
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if Language::from_extension(ext).is_some() {
                        files.push(path);
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a directory should be excluded from indexing.
    fn is_excluded_dir(name: &str) -> bool {
        matches!(
            name,
            "target" | "node_modules" | "vendor" | "bin" | "obj" | "build" | "dist" | "__pycache__"
        )
    }

    /// Get the path relative to the workspace root.
    ///
    /// Returns the original path if it's not under the workspace root.
    fn relative_path<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.workspace_root).unwrap_or(path)
    }

    /// Convert a list of file IDs to their paths, tracking missing files.
    ///
    /// Returns `(found_paths, missing_count)` where `missing_count` is the number of
    /// file IDs that could not be resolved (logged as warnings).
    fn file_ids_to_paths(
        &self,
        file_ids: Vec<FileId>,
        source_file_id: FileId,
    ) -> Result<(Vec<PathBuf>, usize)> {
        let mut paths = Vec::new();
        let mut missing_count = 0;
        for dep_id in file_ids {
            if let Some(file) = self.db.get_file_by_id(dep_id)? {
                paths.push(file.path);
            } else {
                warn!(
                    source_file_id = %source_file_id,
                    missing_file_id = %dep_id,
                    "file_deps references non-existent file, possible database corruption"
                );
                missing_count += 1;
            }
        }
        Ok((paths, missing_count))
    }

    /// Incrementally update index for changed files.
    pub fn update(&mut self) -> Result<IndexUpdate> {
        // For now, just re-index everything
        // TODO: implement proper incremental update
        let stats = self.index()?;
        Ok(IndexUpdate {
            files_changed: stats.files_indexed,
            files_unchanged: 0,
            duration: stats.duration,
            errors: stats.errors,
        })
    }

    /// Check if any indexed files have changed since last update.
    pub fn needs_update(&self) -> Result<bool> {
        // TODO: implement proper staleness check
        Ok(true)
    }

    /// Rebuild the entire index from scratch.
    pub fn rebuild(&mut self) -> Result<IndexStats> {
        self.db.clear()?;
        self.index()
    }

    // === File Queries ===

    /// Get metadata for an indexed file.
    pub fn get_file(&self, path: &Path) -> Result<Option<IndexedFile>> {
        self.db.get_file(self.relative_path(path))
    }

    // === Symbol Queries ===

    /// Search for symbols by name (fuzzy/partial matching).
    pub fn search_symbols(&self, query: &str) -> Result<Vec<Symbol>> {
        self.db.search_symbols(query, 100)
    }

    /// List all symbols defined in a file.
    pub fn list_symbols(&self, path: &Path) -> Result<Vec<Symbol>> {
        let file_id = self
            .db
            .get_file_id(self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;
        self.db.list_symbols_in_file(file_id)
    }

    /// Get a symbol by its qualified name (exact match).
    pub fn get_symbol(&self, qualified_name: &str) -> Result<Option<Symbol>> {
        self.db.get_symbol_by_qualified_name(qualified_name)
    }

    /// Get a symbol by its database ID.
    pub fn get_symbol_by_id(&self, id: SymbolId) -> Result<Option<Symbol>> {
        self.db.get_symbol_by_id(id)
    }

    /// Get file information by its database ID.
    ///
    /// Returns the indexed file metadata including its path.
    #[must_use = "returns file info without side effects"]
    pub fn get_file_by_id(&self, id: FileId) -> Result<Option<IndexedFile>> {
        self.db.get_file_by_id(id)
    }

    // === Reference Queries ===

    /// Get all references to a symbol.
    pub fn get_references(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        // First find the symbol by qualified name
        let symbol = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        // Then get all references to it
        self.db.get_references_to_symbol(symbol.id)
    }

    /// List all outgoing references from a file.
    pub fn list_references_in_file(&self, path: &Path) -> Result<Vec<Reference>> {
        let file_id = self
            .db
            .get_file_id(self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        self.db.list_references_in_file(file_id)
    }

    // === Dependency Queries ===

    /// Get files that directly depend on the given file.
    pub fn get_dependents(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let file_id = self
            .db
            .get_file_id(self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        let dependent_ids = self.db.get_file_dependents(file_id)?;
        let (paths, missing_count) = self.file_ids_to_paths(dependent_ids, file_id)?;
        if missing_count > 0 {
            debug!(
                file = %path.display(),
                missing_count,
                "Some dependent file IDs could not be resolved"
            );
        }
        Ok(paths)
    }

    /// Get files that the given file directly depends on.
    pub fn get_dependencies(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let file_id = self
            .db
            .get_file_id(self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        let dep_ids = self.db.get_file_dependencies(file_id)?;
        let (paths, missing_count) = self.file_ids_to_paths(dep_ids, file_id)?;
        if missing_count > 0 {
            debug!(
                file = %path.display(),
                missing_count,
                "Some dependency file IDs could not be resolved"
            );
        }
        Ok(paths)
    }

    /// Get impact analysis: direct and transitive dependents of a file.
    pub fn get_impact(&self, path: &Path) -> Result<Impact> {
        let file_id = self
            .db
            .get_file_id(self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        let file_impact = self
            .file_graph
            .get_transitive_dependents(file_id, Some(50))?;

        // Convert FileImpact to public Impact type
        Ok(Impact {
            target: file_impact.target.path,
            direct_dependents: file_impact
                .direct_dependents
                .into_iter()
                .map(|d| Dependent {
                    file: d.file.path,
                    symbols_used: vec![],
                    line_count: d.ref_count,
                })
                .collect(),
            transitive_dependents: file_impact
                .transitive_dependents
                .into_iter()
                .map(|d| Dependent {
                    file: d.file.path,
                    symbols_used: vec![],
                    line_count: d.ref_count,
                })
                .collect(),
        })
    }

    /// Get symbols that call/use the given symbol.
    pub fn get_callers(&self, qualified_name: &str) -> Result<Vec<Dependent>> {
        let symbol = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let callers = self.symbol_graph.get_callers(symbol.id)?;

        // Convert CallerInfo to Dependent
        callers
            .into_iter()
            .map(|c| {
                let file = self
                    .db
                    .get_file_by_id(c.symbol.file_id)?
                    .ok_or_else(|| Error::NotFound(format!("file id: {}", c.symbol.file_id)))?;
                Ok(Dependent {
                    file: file.path,
                    symbols_used: vec![c.symbol.qualified_name],
                    line_count: c.reference_count,
                })
            })
            .collect()
    }

    /// Get symbols that the given symbol calls/uses.
    pub fn get_symbol_dependencies(&self, qualified_name: &str) -> Result<Vec<Symbol>> {
        let symbol = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let callees = self.symbol_graph.get_callees(symbol.id)?;

        Ok(callees.into_iter().map(|c| c.symbol).collect())
    }

    /// Get impact analysis: direct and transitive callers of a symbol.
    pub fn get_symbol_impact(&self, qualified_name: &str) -> Result<Impact> {
        let symbol = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let impact = self
            .symbol_graph
            .get_transitive_callers(symbol.id, Some(50))?;

        // Convert CallerInfo to Dependent
        let caller_to_dependent = |caller: graph::CallerInfo| -> Result<Dependent> {
            let file = self
                .db
                .get_file_by_id(caller.symbol.file_id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", caller.symbol.file_id)))?;
            Ok(Dependent {
                file: file.path,
                symbols_used: vec![caller.symbol.qualified_name],
                line_count: caller.reference_count,
            })
        };

        let direct_dependents = impact
            .direct_callers
            .into_iter()
            .map(&caller_to_dependent)
            .collect::<Result<Vec<_>>>()?;

        let transitive_dependents = impact
            .transitive_callers
            .into_iter()
            .map(caller_to_dependent)
            .collect::<Result<Vec<_>>>()?;

        let target_file = self
            .db
            .get_file_by_id(symbol.file_id)?
            .ok_or_else(|| Error::NotFound(format!("file id: {}", symbol.file_id)))?;

        Ok(Impact {
            target: target_file.path,
            direct_dependents,
            transitive_dependents,
        })
    }

    // === Graph Analysis ===

    /// Detect circular dependencies in the codebase.
    pub fn detect_cycles(&self) -> Result<Vec<Cycle>> {
        self.file_graph.detect_cycles()
    }

    /// Get the shortest dependency path between two files.
    pub fn get_dependency_chain(&self, from: &Path, to: &Path) -> Result<Option<Vec<PathBuf>>> {
        let from_id = self
            .db
            .get_file_id(self.relative_path(from))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", from.display())))?;
        let to_id = self
            .db
            .get_file_id(self.relative_path(to))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", to.display())))?;

        let path = self.file_graph.find_dependency_path(from_id, to_id)?;

        Ok(path.map(|p| p.into_files().into_iter().map(|f| f.path).collect()))
    }

    // === Database ===

    /// Get path to the `SQLite` database file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Vacuum the database to reclaim space.
    pub fn vacuum(&self) -> Result<()> {
        self.db.vacuum()
    }

    /// Get statistics about the index database.
    pub fn get_stats(&self) -> Result<types::DatabaseStats> {
        self.db.get_stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_workspace() -> TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    #[test]
    fn new_creates_instance_for_valid_workspace() {
        let workspace = temp_workspace();
        let result = Tethys::new(workspace.path());

        assert!(result.is_ok());
        let tethys = result.unwrap();
        assert_eq!(
            tethys.db_path(),
            workspace.path().join(".rivets/index/tethys.db")
        );
    }

    #[test]
    fn new_fails_for_nonexistent_workspace() {
        let result = Tethys::new(Path::new("/nonexistent/path/that/does/not/exist"));

        assert!(result.is_err());
    }
}
