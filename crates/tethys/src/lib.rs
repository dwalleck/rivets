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

mod batch_writer;
pub mod cargo;
mod db;
mod error;
mod graph;
mod indexing;
mod languages;
pub mod lsp;
mod parallel;
mod reindex;
mod resolve;
mod resolver;
mod types;

pub use cargo::discover_crates;
pub use error::{Error, IndexError, IndexErrorKind, Result};
pub use types::{
    CrateInfo, Cycle, DatabaseStats, Dependent, FileAnalysis, FileId, FunctionSignature, Impact,
    Import, IndexOptions, IndexStats, IndexUpdate, IndexedFile, Language, LspCompletedSession,
    LspOutcome, LspSessionResult, PanicKind, PanicPoint, Parameter, ParameterKind,
    ReachabilityDirection, ReachabilityResult, ReachablePath, Reference, ReferenceKind, Span,
    Symbol, SymbolId, SymbolKind, UnresolvedRefForLsp, Visibility,
};

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use db::Index;
use graph::{FileGraphOps, SymbolGraphOps};
use tracing::{debug, trace, warn};

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
    crates: Vec<CrateInfo>,
}

#[expect(
    clippy::missing_errors_doc,
    reason = "error docs deferred to avoid churn during active development"
)]
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
        let crates = cargo::discover_crates(&workspace_root);

        Ok(Self {
            workspace_root,
            db_path,
            db,
            parser,
            crates,
        })
    }

    /// Create a Tethys instance with LSP refinement enabled.
    ///
    /// LSP integration is controlled via [`IndexOptions::with_lsp()`] when calling
    /// [`index_with_options()`](Self::index_with_options). The `lsp_command` parameter
    /// is reserved for future use (custom LSP server paths); currently LSP providers
    /// are selected automatically based on language.
    pub fn with_lsp(workspace_root: &Path, _lsp_command: &str) -> Result<Self> {
        Self::new(workspace_root)
    }

    /// Compute the module path for a file in this workspace.
    ///
    /// Returns an empty string if the file is not part of any crate's module tree
    /// (e.g., files in `examples/`, `benches/`, or outside any crate).
    fn compute_module_path_for_file(&self, file_path: &Path) -> String {
        let canonical = match file_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                debug!(
                    file = %file_path.display(),
                    error = %e,
                    "Failed to canonicalize path for module path computation, using original"
                );
                file_path.to_path_buf()
            }
        };

        let Some(crate_info) = cargo::get_crate_for_file(&canonical, &self.crates) else {
            debug!(
                file = %canonical.display(),
                crate_count = self.crates.len(),
                "File not within any known crate"
            );
            return String::new();
        };

        if let Some(module_path) = cargo::compute_module_path(&canonical, crate_info) {
            trace!(
                file = %canonical.display(),
                crate_name = %crate_info.name,
                module_path = %module_path,
                "Computed module path"
            );
            module_path
        } else {
            debug!(
                file = %canonical.display(),
                crate_name = %crate_info.name,
                "File is within crate but not in module tree (examples/benches/tests?)"
            );
            String::new()
        }
    }

    /// Get the path relative to the workspace root.
    ///
    /// Handles symlink differences (e.g., `/var` -> `/private/var` on macOS) by
    /// attempting canonicalization when the initial `strip_prefix` fails on
    /// absolute paths. Returns `Cow::Borrowed` for the common fast path,
    /// `Cow::Owned` only when canonicalization was needed.
    fn relative_path<'a>(&self, path: &'a Path) -> Cow<'a, Path> {
        if let Ok(relative) = path.strip_prefix(&self.workspace_root) {
            return Cow::Borrowed(relative);
        }

        // For absolute paths, try canonicalizing to resolve symlinks
        if path.is_absolute()
            && let Ok(canonical) = path.canonicalize()
            && let Ok(relative) = canonical.strip_prefix(&self.workspace_root)
        {
            return Cow::Owned(relative.to_path_buf());
        }

        warn!(
            path = %path.display(),
            workspace = %self.workspace_root.display(),
            "Path is outside workspace root, using as-is"
        );
        Cow::Borrowed(path)
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

    // === File Queries ===

    /// Get metadata for an indexed file.
    pub fn get_file(&self, path: &Path) -> Result<Option<IndexedFile>> {
        self.db.get_file(&self.relative_path(path))
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
            .get_file_id(&self.relative_path(path))?
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
            .get_file_id(&self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        self.db.list_references_in_file(file_id)
    }

    // === Import Queries ===

    /// List all imports for a file.
    ///
    /// Returns the import statements extracted from the file during indexing.
    /// Each import includes the symbol name, source module, and optional alias.
    pub fn list_imports_in_file(&self, path: &Path) -> Result<Vec<Import>> {
        let file_id = self
            .db
            .get_file_id(&self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        self.db.get_imports_for_file(file_id)
    }

    // === Dependency Queries ===

    /// Get files that directly depend on the given file.
    pub fn get_dependents(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let file_id = self
            .db
            .get_file_id(&self.relative_path(path))?
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
            .get_file_id(&self.relative_path(path))?
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
            .get_file_id(&self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        let file_impact = self.db.get_transitive_dependents(file_id, Some(50))?;

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

        let callers = self.db.get_callers(symbol.id)?;

        self.convert_callers_to_dependents(callers)
    }

    /// Get symbols that the given symbol calls/uses.
    pub fn get_symbol_dependencies(&self, qualified_name: &str) -> Result<Vec<Symbol>> {
        let symbol = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let callees = self.db.get_callees(symbol.id)?;

        Ok(callees.into_iter().map(|c| c.symbol).collect())
    }

    /// Get impact analysis: direct and transitive callers of a symbol.
    pub fn get_symbol_impact(&self, qualified_name: &str) -> Result<Impact> {
        let symbol = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let impact = self.db.get_transitive_callers(symbol.id, Some(50))?;

        let direct_dependents = self.convert_callers_to_dependents(impact.direct_callers)?;
        let transitive_dependents =
            self.convert_callers_to_dependents(impact.transitive_callers)?;

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
        self.db.detect_cycles()
    }

    /// Get the shortest dependency path between two files.
    pub fn get_dependency_chain(&self, from: &Path, to: &Path) -> Result<Option<Vec<PathBuf>>> {
        let from_id = self
            .db
            .get_file_id(&self.relative_path(from))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", from.display())))?;
        let to_id = self
            .db
            .get_file_id(&self.relative_path(to))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", to.display())))?;

        let path = self.db.find_dependency_path(from_id, to_id)?;

        Ok(path.map(|p| p.into_files().into_iter().map(|f| f.path).collect()))
    }

    // === Reachability Analysis ===

    /// BFS traversal of the call graph in a given direction.
    ///
    /// Shared implementation for both forward (callees) and backward (callers) reachability.
    /// The `get_neighbors` closure determines the direction by returning either callees or
    /// callers for a given symbol.
    ///
    /// The BFS uses fail-fast error handling: if a database error occurs while fetching
    /// neighbors for any symbol, traversal stops immediately and the error is returned.
    #[expect(
        clippy::unused_self,
        reason = "method is a private helper called on self; callers pass closures that capture self"
    )]
    fn bfs_reachable(
        &self,
        start_id: SymbolId,
        max_depth: usize,
        get_neighbors: impl Fn(SymbolId) -> Result<Vec<Symbol>>,
        direction: types::ReachabilityDirection,
        source: Symbol,
    ) -> Result<types::ReachabilityResult> {
        use std::collections::{HashSet, VecDeque};

        let mut visited: HashSet<SymbolId> = HashSet::new();
        let mut results: Vec<types::ReachablePath> = Vec::new();
        let mut queue: VecDeque<(SymbolId, Vec<Symbol>, usize)> = VecDeque::new();

        queue.push_back((start_id, vec![], 0));
        visited.insert(start_id);

        while let Some((current_id, path, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            for neighbor in get_neighbors(current_id)? {
                if visited.insert(neighbor.id) {
                    let mut new_path = path.clone();
                    new_path.push(neighbor.clone());

                    results.push(types::ReachablePath {
                        target: neighbor.clone(),
                        path: new_path.clone(),
                        depth: depth + 1,
                    });

                    queue.push_back((neighbor.id, new_path, depth + 1));
                }
            }
        }

        Ok(types::ReachabilityResult {
            source,
            reachable: results,
            max_depth,
            direction,
        })
    }

    /// Get forward reachable symbols: what can this symbol reach?
    ///
    /// Performs BFS traversal of the call graph following callees (outgoing edges).
    /// Returns all symbols that can be reached from the source symbol within `max_depth`.
    ///
    /// The BFS uses fail-fast error handling: if a database error occurs while fetching
    /// callees for any symbol, traversal stops immediately and the error is returned.
    ///
    /// # Arguments
    ///
    /// * `qualified_name` - Qualified name of the symbol to analyze (e.g., `"auth::validate"`)
    /// * `max_depth` - Maximum depth to traverse (None uses default of 50)
    ///
    /// # Errors
    ///
    /// Returns `Error::NotFound` if no symbol matches `qualified_name`.
    /// Returns database errors if the call graph lookup fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tethys::Tethys;
    /// use std::path::Path;
    ///
    /// let tethys = Tethys::new(Path::new("/path/to/workspace"))?;
    /// let result = tethys.get_forward_reachable("main::run", Some(3))?;
    /// println!("main::run can reach {} symbols", result.reachable_count());
    /// # Ok::<(), tethys::Error>(())
    /// ```
    pub fn get_forward_reachable(
        &self,
        qualified_name: &str,
        max_depth: Option<usize>,
    ) -> Result<types::ReachabilityResult> {
        let source = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        self.bfs_reachable(
            source.id,
            max_depth.unwrap_or(50),
            |id| {
                Ok(self
                    .db
                    .get_callees(id)?
                    .into_iter()
                    .map(|c| c.symbol)
                    .collect())
            },
            types::ReachabilityDirection::Forward,
            source,
        )
    }

    /// Get backward reachable symbols: who can reach this symbol?
    ///
    /// Performs BFS traversal of the call graph following callers (incoming edges).
    /// Returns all symbols that can reach the analyzed symbol within `max_depth`.
    ///
    /// The BFS uses fail-fast error handling: if a database error occurs while fetching
    /// callers for any symbol, traversal stops immediately and the error is returned.
    ///
    /// # Arguments
    ///
    /// * `qualified_name` - Qualified name of the symbol to analyze (e.g., `"db::query"`)
    /// * `max_depth` - Maximum depth to traverse (None uses default of 50)
    ///
    /// # Errors
    ///
    /// Returns `Error::NotFound` if no symbol matches `qualified_name`.
    /// Returns database errors if the call graph lookup fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tethys::Tethys;
    /// use std::path::Path;
    ///
    /// let tethys = Tethys::new(Path::new("/path/to/workspace"))?;
    /// let result = tethys.get_backward_reachable("db::query", Some(3))?;
    /// println!("{} symbols can reach db::query", result.reachable_count());
    /// # Ok::<(), tethys::Error>(())
    /// ```
    pub fn get_backward_reachable(
        &self,
        qualified_name: &str,
        max_depth: Option<usize>,
    ) -> Result<types::ReachabilityResult> {
        let source = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        self.bfs_reachable(
            source.id,
            max_depth.unwrap_or(50),
            |id| {
                Ok(self
                    .db
                    .get_callers(id)?
                    .into_iter()
                    .map(|c| c.symbol)
                    .collect())
            },
            types::ReachabilityDirection::Backward,
            source,
        )
    }

    // === Crate Resolution ===

    /// Get all discovered crates in this workspace.
    pub fn crates(&self) -> &[CrateInfo] {
        &self.crates
    }

    /// Find the crate that contains a given file path.
    ///
    /// Returns the crate whose `path` is a prefix of the given file path.
    /// For workspaces with multiple crates, this finds the most specific match
    /// (longest path). This handles nested crate structures where a file could
    /// technically be under multiple crate directories.
    ///
    /// Returns `None` if the file path cannot be canonicalized or is not under
    /// any discovered crate.
    pub fn get_crate_for_file(&self, file_path: &Path) -> Option<&CrateInfo> {
        let file_path = match file_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                debug!(
                    path = %file_path.display(),
                    error = %e,
                    "Failed to canonicalize path for crate lookup"
                );
                return None;
            }
        };

        self.crates
            .iter()
            .filter(|c| file_path.starts_with(&c.path))
            .max_by_key(|c| c.path.components().count())
    }

    /// Get the crate root directory for a given file path.
    ///
    /// This is a convenience method that returns just the path component
    /// of the containing crate.
    pub fn get_crate_root_for_file(&self, file_path: &Path) -> Option<&Path> {
        self.get_crate_for_file(file_path).map(|c| c.path.as_path())
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

    // === Test Topology ===

    /// Get all test symbols in the index.
    ///
    /// Returns symbols where `is_test = true`. These are functions/methods
    /// annotated with test framework attributes:
    /// - Rust: `#[test]`, `#[tokio::test]`, `#[rstest]`, etc.
    /// - C#: `[Test]`, `[Fact]`, `[Theory]`, `[TestMethod]`, etc.
    pub fn get_test_symbols(&self) -> Result<Vec<Symbol>> {
        self.db.get_test_symbols()
    }

    /// Get tests that are affected by changes to the specified files.
    ///
    /// This uses the file dependency graph to find test files that depend
    /// (directly or transitively) on the changed files, then returns the
    /// test symbols from those files.
    ///
    /// # Arguments
    ///
    /// * `changed_files` - Paths to files that have changed (relative to workspace root)
    ///
    /// # Returns
    ///
    /// A list of test symbols from files that depend on the changed files.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tethys::Tethys;
    /// use std::path::{Path, PathBuf};
    ///
    /// let tethys = Tethys::new(Path::new("/path/to/workspace"))?;
    /// let changed = vec![PathBuf::from("src/auth.rs")];
    /// let affected_tests = tethys.get_affected_tests(&changed)?;
    /// for test in affected_tests {
    ///     println!("Run test: {} in {:?}", test.qualified_name, test.file_id);
    /// }
    /// # Ok::<(), tethys::Error>(())
    /// ```
    pub fn get_affected_tests(&self, changed_files: &[PathBuf]) -> Result<Vec<Symbol>> {
        use std::collections::HashSet;

        // Get file IDs for the changed files
        let changed_file_ids: Vec<FileId> = changed_files
            .iter()
            .filter_map(|path| {
                let relative = self.relative_path(path);
                match self.db.get_file_id(&relative) {
                    Ok(Some(id)) => Some(id),
                    Ok(None) => {
                        debug!(
                            path = %path.display(),
                            "Changed file not in index, skipping"
                        );
                        None
                    }
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "Error looking up changed file"
                        );
                        None
                    }
                }
            })
            .collect();

        if changed_file_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Use reverse traversal: find all files that depend on changed files
        // This is O(V+E) total instead of O(T * V) where T = test files
        let mut affected_file_ids: HashSet<FileId> = HashSet::new();

        // Changed files themselves are affected
        affected_file_ids.extend(changed_file_ids.iter().copied());

        // For each changed file, get all transitive dependents using the graph infrastructure
        for &file_id in &changed_file_ids {
            match self.db.get_transitive_dependents(file_id, None) {
                Ok(impact) => {
                    // Add direct dependents
                    for dep in &impact.direct_dependents {
                        affected_file_ids.insert(dep.file.id);
                    }
                    // Add transitive dependents
                    for dep in &impact.transitive_dependents {
                        affected_file_ids.insert(dep.file.id);
                    }
                    debug!(
                        file_id = %file_id,
                        direct = impact.direct_dependents.len(),
                        transitive = impact.transitive_dependents.len(),
                        "Found dependents for changed file"
                    );
                }
                Err(e) => {
                    // File might not exist or other error - log and continue
                    warn!(
                        file_id = %file_id,
                        error = %e,
                        "Error getting transitive dependents"
                    );
                }
            }
        }

        // Get all test symbols and filter to affected files
        let all_tests = self.db.get_test_symbols()?;
        let affected_tests: Vec<Symbol> = all_tests
            .into_iter()
            .filter(|test| affected_file_ids.contains(&test.file_id))
            .collect();

        debug!(
            affected_test_count = affected_tests.len(),
            affected_file_count = affected_file_ids.len(),
            changed_file_count = changed_files.len(),
            "Found affected tests"
        );

        Ok(affected_tests)
    }

    // === Panic Points Analysis ===

    /// Get all panic points in the codebase.
    ///
    /// Panic points are `.unwrap()` and `.expect()` calls that could panic at runtime.
    /// Only calls within functions and methods are included.
    ///
    /// # Arguments
    ///
    /// * `include_tests` - If true, include panic points in test code
    /// * `file_filter` - If provided, only return panic points in the specified file
    ///   (path should be relative to workspace root)
    ///
    /// # Returns
    ///
    /// A vector of `PanicPoint` structs, ordered by file path and line number.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tethys::Tethys;
    /// use std::path::Path;
    ///
    /// let tethys = Tethys::new(Path::new("/path/to/workspace"))?;
    ///
    /// // Get all production panic points
    /// let prod_panics = tethys.get_panic_points(false, None)?;
    /// println!("Found {} panic points in production code", prod_panics.len());
    ///
    /// // Get panic points in a specific file, including tests
    /// let file_panics = tethys.get_panic_points(true, Some("src/lib.rs"))?;
    /// # Ok::<(), tethys::Error>(())
    /// ```
    pub fn get_panic_points(
        &self,
        include_tests: bool,
        file_filter: Option<&str>,
    ) -> Result<Vec<types::PanicPoint>> {
        self.db.get_panic_points(include_tests, file_filter)
    }

    /// Count panic points grouped by test/production code.
    ///
    /// This is useful for summary statistics without retrieving all the details.
    ///
    /// # Returns
    ///
    /// Returns `(production_count, test_count)`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tethys::Tethys;
    /// use std::path::Path;
    ///
    /// let tethys = Tethys::new(Path::new("/path/to/workspace"))?;
    /// let (prod, test) = tethys.count_panic_points()?;
    /// println!("Production: {prod}, Test: {test}");
    /// # Ok::<(), tethys::Error>(())
    /// ```
    pub fn count_panic_points(&self) -> Result<(usize, usize)> {
        self.db.count_panic_points()
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
        // Canonicalize expected path to match Tethys::new() which canonicalizes workspace_root
        // (resolves /var -> /private/var on macOS, short names on Windows)
        let expected = workspace
            .path()
            .canonicalize()
            .expect("temp dir should be canonicalizable")
            .join(".rivets")
            .join("index")
            .join("tethys.db");
        assert_eq!(tethys.db_path(), expected);
    }

    #[test]
    fn new_fails_for_nonexistent_workspace() {
        let result = Tethys::new(Path::new("/nonexistent/path/that/does/not/exist"));

        assert!(result.is_err());
    }

    #[test]
    fn build_qualified_name_with_single_segment_path() {
        let result = Tethys::build_qualified_name("open", Some(&["Index".to_string()]));
        assert_eq!(result, "Index::open");
    }

    #[test]
    fn build_qualified_name_with_multi_segment_path() {
        let result = Tethys::build_qualified_name(
            "open",
            Some(&["crate".to_string(), "db".to_string(), "Index".to_string()]),
        );
        assert_eq!(result, "crate::db::Index::open");
    }

    #[test]
    fn build_qualified_name_with_empty_path() {
        let result = Tethys::build_qualified_name("foo", Some(&[]));
        assert_eq!(result, "foo");
    }

    #[test]
    fn build_qualified_name_with_none_path() {
        let result = Tethys::build_qualified_name("bar", None);
        assert_eq!(result, "bar");
    }

    // ========================================================================
    // uri_to_path Tests
    // ========================================================================

    #[test]
    #[cfg(not(windows))]
    fn uri_to_path_handles_unix_path() {
        let uri = "file:///home/user/project/src/main.rs";
        let result = Tethys::uri_to_path(uri);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/home/user/project/src/main.rs")
        );
    }

    #[test]
    fn uri_to_path_returns_none_for_non_file_uri() {
        let uri = "https://example.com/file.rs";
        let result = Tethys::uri_to_path(uri);
        assert!(result.is_none());
    }

    #[test]
    fn uri_to_path_returns_none_for_empty_string() {
        let result = Tethys::uri_to_path("");
        assert!(result.is_none());
    }

    #[test]
    #[cfg(not(windows))]
    fn uri_to_path_decodes_percent_encoded_spaces() {
        let uri = "file:///home/user/my%20project/src/main.rs";
        let result = Tethys::uri_to_path(uri);
        assert_eq!(
            result,
            Some(PathBuf::from("/home/user/my project/src/main.rs"))
        );
    }

    // ========================================================================
    // IndexOptions Tests
    // ========================================================================

    #[test]
    fn index_options_default_has_lsp_disabled() {
        let options = IndexOptions::default();
        assert!(!options.use_lsp());
    }

    #[test]
    fn index_options_with_lsp_enables_lsp() {
        let options = IndexOptions::with_lsp();
        assert!(options.use_lsp());
    }

    #[test]
    fn index_with_options_returns_zero_lsp_resolved_when_disabled() {
        let workspace = temp_workspace();

        // Create a simple Rust file
        let src_dir = workspace.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");
        std::fs::write(src_dir.join("lib.rs"), "pub fn hello() {}").expect("write file");

        let mut tethys = Tethys::new(workspace.path()).expect("create tethys");
        let stats = tethys
            .index_with_options(IndexOptions::default())
            .expect("index");

        assert_eq!(
            stats.total_lsp_resolved(),
            0,
            "LSP resolved count should be 0 when use_lsp is false"
        );
        assert!(
            stats.lsp_sessions.is_empty(),
            "LSP sessions should be empty when use_lsp is false"
        );
    }
}
