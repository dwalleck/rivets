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
mod parser;
mod resolver;
mod types;

pub use error::{Error, IndexError, IndexErrorKind, Result};
pub use types::{
    Cycle, Dependent, FileAnalysis, FunctionSignature, Impact, IndexStats, IndexUpdate,
    IndexedFile, Language, Parameter, Reference, ReferenceKind, Span, Symbol, SymbolKind,
    Visibility,
};

use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use db::{Index, SymbolData};
use languages::rust;
use resolver::resolve_module_path;
use tracing::{debug, warn};

/// A dependency that couldn't be resolved because the target file wasn't indexed yet.
///
/// These are collected during the first indexing pass and resolved in subsequent passes.
#[derive(Debug)]
struct PendingDependency {
    /// The file ID that has the dependency.
    from_file_id: i64,
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
}

// TODO: Add `# Errors` documentation to public methods when implementations are complete
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

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| Error::Parser(e.to_string()))?;

        Ok(Self {
            workspace_root,
            db_path,
            db,
            parser,
        })
    }

    /// Create with LSP refinement enabled (Phase 6).
    #[allow(unused_variables)]
    pub fn with_lsp(workspace_root: &Path, lsp_command: &str) -> Result<Self> {
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
        let references_found = 0;
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

            // Only Rust is implemented for now
            if language != Language::Rust {
                files_skipped += 1;
                continue;
            }

            // Read and parse the file
            match self.index_file(&file_path, language, &mut pending) {
                Ok(count) => {
                    files_indexed += 1;
                    symbols_found += count;
                }
                Err(e) => {
                    errors.push(IndexError::new(
                        file_path.clone(),
                        IndexErrorKind::ParseFailed,
                        e.to_string(),
                    ));
                }
            }
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
            .filter_map(|p| {
                let from_path = self
                    .db
                    .get_file_by_id(p.from_file_id)
                    .ok()
                    .flatten()
                    .map(|f| f.path)?;
                Some((from_path, p.dep_path))
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
    fn index_file(
        &mut self,
        path: &Path,
        language: Language,
        pending: &mut Vec<PendingDependency>,
    ) -> Result<usize> {
        let content = std::fs::read(path)?;
        let content_str = std::str::from_utf8(&content)
            .map_err(|_| Error::Parser("file is not valid UTF-8".to_string()))?;

        // Get file metadata
        let metadata = std::fs::metadata(path)?;
        #[allow(clippy::cast_possible_truncation)] // Nanoseconds fit in i64 for centuries
        let mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as i64);
        let size_bytes = metadata.len();

        // Parse with tree-sitter
        let tree = self
            .parser
            .parse(content_str, None)
            .ok_or_else(|| Error::Parser("failed to parse file".to_string()))?;

        // Extract symbols
        let extracted = rust::extract_symbols(&tree, content_str.as_bytes());

        // Extract use statements and references for dependency detection
        let uses = rust::extract_use_statements(&tree, content_str.as_bytes());
        let refs = rust::extract_references(&tree, content_str.as_bytes());

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

        // Compute and store file dependencies (L2: only for actually used symbols)
        self.compute_dependencies(path, file_id, &uses, &refs, pending)?;

        Ok(extracted.len())
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
        file_id: i64,
        uses: &[rust::UseStatement],
        refs: &[rust::ExtractedReference],
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

        // Crate root is the src/ directory (or where the main/lib file lives)
        let crate_root = self.workspace_root.join("src");

        // Track which files we depend on (dedupe)
        let mut depended_files: HashSet<PathBuf> = HashSet::new();

        for use_stmt in uses {
            // Skip glob imports - can't determine what's used
            if use_stmt.is_glob {
                continue;
            }

            // Check if any imported name from this use statement is actually referenced
            let mut is_used = false;
            for name in &use_stmt.imported_names {
                // Check both the original name and alias
                let lookup_name = use_stmt.alias.as_ref().unwrap_or(name);
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
            if let Some(resolved) = resolve_module_path(&use_stmt.path, current_file, &crate_root) {
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

    /// Convert a list of file IDs to their paths, logging warnings for missing files.
    fn file_ids_to_paths(&self, file_ids: Vec<i64>, source_file_id: i64) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for dep_id in file_ids {
            match self.db.get_file_by_id(dep_id)? {
                Some(file) => paths.push(file.path),
                None => {
                    warn!(
                        source_file_id,
                        missing_file_id = dep_id,
                        "file_deps references non-existent file, possible database corruption"
                    );
                }
            }
        }
        Ok(paths)
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
        match self.db.get_file_id(self.relative_path(path))? {
            Some(file_id) => self.db.list_symbols_in_file(file_id),
            None => Ok(vec![]),
        }
    }

    /// Get a symbol by its qualified name (exact match).
    #[allow(unused_variables)]
    pub fn get_symbol(&self, qualified_name: &str) -> Result<Option<Symbol>> {
        todo!("Phase 2: Implement get_symbol")
    }

    /// Get a symbol by its database ID.
    #[allow(unused_variables)]
    pub fn get_symbol_by_id(&self, id: i64) -> Result<Option<Symbol>> {
        todo!("Phase 2: Implement get_symbol_by_id")
    }

    // === Reference Queries ===

    /// Get all references to a symbol.
    #[allow(unused_variables)]
    pub fn get_references(&self, qualified_name: &str) -> Result<Vec<Reference>> {
        todo!("Phase 2: Implement get_references")
    }

    /// List all outgoing references from a file.
    #[allow(unused_variables)]
    pub fn list_references_in_file(&self, path: &Path) -> Result<Vec<Reference>> {
        todo!("Phase 2: Implement list_references_in_file")
    }

    // === Dependency Queries ===

    /// Get files that directly depend on the given file.
    pub fn get_dependents(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let Some(file_id) = self.db.get_file_id(self.relative_path(path))? else {
            return Ok(vec![]);
        };

        let dependent_ids = self.db.get_file_dependents(file_id)?;
        self.file_ids_to_paths(dependent_ids, file_id)
    }

    /// Get files that the given file directly depends on.
    pub fn get_dependencies(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let Some(file_id) = self.db.get_file_id(self.relative_path(path))? else {
            return Ok(vec![]);
        };

        let dep_ids = self.db.get_file_dependencies(file_id)?;
        self.file_ids_to_paths(dep_ids, file_id)
    }

    /// Get impact analysis: direct and transitive dependents of a file.
    #[allow(unused_variables)]
    pub fn get_impact(&self, path: &Path) -> Result<Impact> {
        todo!("Phase 3: Implement get_impact")
    }

    /// Get symbols that call/use the given symbol.
    #[allow(unused_variables)]
    pub fn get_callers(&self, qualified_name: &str) -> Result<Vec<Dependent>> {
        todo!("Phase 3: Implement get_callers")
    }

    /// Get symbols that the given symbol calls/uses.
    #[allow(unused_variables)]
    pub fn get_symbol_dependencies(&self, qualified_name: &str) -> Result<Vec<Symbol>> {
        todo!("Phase 3: Implement get_symbol_dependencies")
    }

    /// Get impact analysis: direct and transitive callers of a symbol.
    #[allow(unused_variables)]
    pub fn get_symbol_impact(&self, qualified_name: &str) -> Result<Impact> {
        todo!("Phase 3: Implement get_symbol_impact")
    }

    // === Graph Analysis ===

    /// Detect circular dependencies in the codebase.
    pub fn detect_cycles(&self) -> Result<Vec<Cycle>> {
        todo!("Phase 3: Implement detect_cycles")
    }

    /// Get the shortest dependency path between two files.
    #[allow(unused_variables)]
    pub fn get_dependency_chain(&self, from: &Path, to: &Path) -> Result<Option<Vec<PathBuf>>> {
        todo!("Phase 3: Implement get_dependency_chain")
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
