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
mod languages;
pub mod lsp;
mod parallel;
mod resolver;
mod types;

pub use cargo::discover_crates;
pub use error::{Error, IndexError, IndexErrorKind, Result};
pub use types::{
    CrateInfo, Cycle, DatabaseStats, Dependent, FileAnalysis, FileId, FunctionSignature, Impact,
    Import, IndexOptions, IndexStats, IndexUpdate, IndexedFile, Language, PanicKind, PanicPoint,
    Parameter, ParameterKind, ReachabilityDirection, ReachabilityResult, ReachablePath, Reference,
    ReferenceKind, Span, Symbol, SymbolId, SymbolKind, UnresolvedRefForLsp, Visibility,
};

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Instant, UNIX_EPOCH};

use rayon::prelude::*;

use batch_writer::BatchWriter;
use db::{Index, SymbolData};
use graph::{FileGraphOps, SymbolGraphOps};
use languages::common;
use lsp::LspProvider;
use parallel::{OwnedSymbolData, ParsedFileData};
use resolver::resolve_module_path;
use tracing::{debug, info, trace, warn};

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
    crates: Vec<CrateInfo>,
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
    #[allow(unused_variables)]
    pub fn with_lsp(workspace_root: &Path, lsp_command: &str) -> Result<Self> {
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

    // === Indexing ===

    /// Index all source files in the workspace.
    ///
    /// Uses deferred dependency resolution to handle circular dependencies:
    /// 1. First pass: Index all files, queue dependencies that can't resolve
    /// 2. Resolution passes: Retry pending dependencies until no progress
    ///
    /// For LSP-based resolution, use [`Self::index_with_options`] with
    /// [`IndexOptions::with_lsp()`].
    pub fn index(&mut self) -> Result<IndexStats> {
        self.index_with_options(IndexOptions::default())
    }

    /// Index all source files with custom options.
    ///
    /// # Arguments
    ///
    /// * `options` - Configuration for the indexing process, including whether
    ///   to use LSP for additional resolution.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tethys::{Tethys, IndexOptions};
    /// use std::path::Path;
    ///
    /// let mut tethys = Tethys::new(Path::new("/path/to/workspace"))?;
    ///
    /// // Index with LSP refinement enabled
    /// let stats = tethys.index_with_options(IndexOptions::with_lsp())?;
    /// println!("Resolved {} references via LSP", stats.lsp_resolved_count);
    /// # Ok::<(), tethys::Error>(())
    /// ```
    #[allow(clippy::too_many_lines)]
    pub fn index_with_options(&mut self, options: IndexOptions) -> Result<IndexStats> {
        let start = Instant::now();
        let mut files_indexed = 0;
        let mut symbols_found = 0;
        let mut references_found = 0;
        let mut files_skipped = 0;
        let mut directories_skipped = Vec::new();
        let mut errors = Vec::new();
        let mut pending: Vec<PendingDependency> = Vec::new();

        // Walk the workspace and find source files
        let all_files = self.discover_files(&mut directories_skipped)?;

        // Filter to supported languages and count skipped files
        let source_files: Vec<(PathBuf, Language)> = all_files
            .into_iter()
            .filter_map(|file_path| {
                let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if let Some(language) = Language::from_extension(ext) {
                    Some((file_path, language))
                } else {
                    files_skipped += 1;
                    None
                }
            })
            .collect();

        let total_files = source_files.len();
        let workspace_root = self.workspace_root.clone();

        if options.use_streaming() {
            // =====================================================================
            // STREAMING MODE: Parse in parallel, write immediately to background thread
            // Memory usage is O(batch_size) instead of O(n)
            // =====================================================================
            info!(
                total_files,
                batch_size = options.streaming_batch_size(),
                "Starting streaming indexing (parse + write in parallel)"
            );

            let batch_writer =
                BatchWriter::new(self.db_path.clone(), options.streaming_batch_size());

            let progress_counter = AtomicUsize::new(0);
            let parse_errors: Mutex<Vec<IndexError>> = Mutex::new(Vec::new());

            // Parse in parallel and send to background writer
            source_files.par_iter().for_each(|(file_path, language)| {
                let current = progress_counter.fetch_add(1, Ordering::Relaxed);
                if current.is_multiple_of(100) {
                    trace!(progress = current, total = total_files, "Parsing files...");
                }

                match Self::parse_file_static(&workspace_root, file_path, *language) {
                    Ok(data) => {
                        batch_writer.send(data);
                    }
                    Err(e) => {
                        let kind = match &e {
                            Error::Io(_) => IndexErrorKind::IoError,
                            Error::Database(_) => IndexErrorKind::DatabaseError,
                            Error::Parser(_) => IndexErrorKind::ParseFailed,
                            Error::Config(_) | Error::NotFound(_) | Error::Internal(_) => {
                                IndexErrorKind::ParseFailed
                            }
                        };
                        match parse_errors.lock() {
                            Ok(mut guard) => {
                                guard.push(IndexError::new(file_path.clone(), kind, e.to_string()));
                            }
                            Err(poisoned) => {
                                tracing::warn!(
                                    file = %file_path.display(),
                                    "Mutex poisoned during error collection, recovering"
                                );
                                poisoned.into_inner().push(IndexError::new(
                                    file_path.clone(),
                                    kind,
                                    e.to_string(),
                                ));
                            }
                        }
                    }
                }
            });

            // Wait for batch writer to finish
            let write_result = batch_writer.finish()?;
            files_indexed = write_result.stats.files_written;
            symbols_found = write_result.stats.symbols_written;
            references_found = write_result.stats.references_written;

            // Collect parse errors
            match parse_errors.into_inner() {
                Ok(parse_errors_vec) => {
                    errors.extend(parse_errors_vec);
                }
                Err(poisoned) => {
                    tracing::warn!(
                        "Mutex was poisoned during parallel parsing, recovering collected errors"
                    );
                    errors.extend(poisoned.into_inner());
                }
            }

            info!(
                files_indexed,
                symbols_found,
                references_found,
                batches = write_result.stats.batches_committed,
                "Streaming indexing complete"
            );

            // Compute file-level dependencies from stored data
            // This is done after all files are written so all file IDs are known
            self.compute_all_dependencies(&mut pending)?;
        } else {
            // =====================================================================
            // BATCH MODE (default): Parse all files in parallel, then write sequentially
            // Memory usage is O(n) where n = number of files
            // =====================================================================
            info!(total_files, "Starting parallel file parsing (Pass 1a)");

            // Phase 1a: Parallel parsing with rayon
            // Use AtomicUsize for thread-safe progress tracking
            let progress_counter = AtomicUsize::new(0);
            let parse_errors: Mutex<Vec<IndexError>> = Mutex::new(Vec::new());

            let parsed_files: Vec<ParsedFileData> = source_files
                .par_iter()
                .filter_map(|(file_path, language)| {
                    let current = progress_counter.fetch_add(1, Ordering::Relaxed);
                    if current.is_multiple_of(100) {
                        trace!(progress = current, total = total_files, "Parsing files...");
                    }

                    match Self::parse_file_static(&workspace_root, file_path, *language) {
                        Ok(data) => Some(data),
                        Err(e) => {
                            let kind = match &e {
                                Error::Io(_) => IndexErrorKind::IoError,
                                Error::Database(_) => IndexErrorKind::DatabaseError,
                                Error::Parser(_) => IndexErrorKind::ParseFailed,
                                Error::Config(_) | Error::NotFound(_) | Error::Internal(_) => {
                                    IndexErrorKind::ParseFailed
                                }
                            };
                            // Handle mutex poisoning - we still want to collect errors even if
                            // another thread panicked. PoisonError contains the guard.
                            match parse_errors.lock() {
                                Ok(mut guard) => {
                                    guard.push(IndexError::new(
                                        file_path.clone(),
                                        kind,
                                        e.to_string(),
                                    ));
                                }
                                Err(poisoned) => {
                                    tracing::warn!(
                                        file = %file_path.display(),
                                        "Mutex poisoned during error collection, recovering"
                                    );
                                    poisoned.into_inner().push(IndexError::new(
                                        file_path.clone(),
                                        kind,
                                        e.to_string(),
                                    ));
                                }
                            }
                            None
                        }
                    }
                })
                .collect();

            // Collect parse errors, recovering from mutex poisoning if needed
            match parse_errors.into_inner() {
                Ok(parse_errors_vec) => {
                    errors.extend(parse_errors_vec);
                }
                Err(poisoned) => {
                    tracing::warn!(
                        "Mutex was poisoned during parallel parsing, recovering collected errors"
                    );
                    errors.extend(poisoned.into_inner());
                }
            }

            info!(
                parsed_count = parsed_files.len(),
                error_count = errors.len(),
                "Parallel parsing complete (Pass 1a), starting sequential write (Pass 1b)"
            );

            // Phase 1b: Sequential database writes
            // This must be sequential because rusqlite Connection is not Sync
            for data in &parsed_files {
                match self.write_parsed_file(data, &mut pending) {
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
                            Error::Config(_) | Error::NotFound(_) | Error::Internal(_) => {
                                IndexErrorKind::ParseFailed
                            }
                        };
                        errors.push(IndexError::new(
                            data.relative_path.clone(),
                            kind,
                            e.to_string(),
                        ));
                    }
                }
            }

            info!(
                files_indexed,
                symbols_found, references_found, "Sequential write complete (Pass 1b)"
            );
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

        // Pass 2: Resolve cross-file references using import information
        let resolved_refs = self.resolve_cross_file_references()?;
        if resolved_refs > 0 {
            tracing::info!(
                resolved_count = resolved_refs,
                "Resolved cross-file references"
            );
        }

        // Pass 3: LSP-based resolution (optional)
        // Resolve each language separately using its appropriate LSP server
        let lsp_resolved_count = if options.use_lsp() {
            let mut total_resolved = 0;

            // Resolve Rust files with rust-analyzer
            let rust_provider = lsp::AnyProvider::for_language(Language::Rust);
            let rust_count =
                self.resolve_via_lsp(&rust_provider, Language::Rust, options.lsp_timeout_secs())?;
            if rust_count > 0 {
                tracing::info!(
                    language = "rust",
                    resolved_count = rust_count,
                    "Resolved references via LSP"
                );
            }
            total_resolved += rust_count;

            // Resolve C# files with csharp-ls
            let csharp_provider = lsp::AnyProvider::for_language(Language::CSharp);
            let csharp_count = self.resolve_via_lsp(
                &csharp_provider,
                Language::CSharp,
                options.lsp_timeout_secs(),
            )?;
            if csharp_count > 0 {
                tracing::info!(
                    language = "csharp",
                    resolved_count = csharp_count,
                    "Resolved references via LSP"
                );
            }
            total_resolved += csharp_count;

            total_resolved
        } else {
            0
        };

        // Populate pre-computed call graph edges after all resolution passes
        self.db.clear_all_call_edges()?;
        let call_edges_count = self.db.populate_call_edges()?;
        if call_edges_count > 0 {
            tracing::debug!(call_edges = call_edges_count, "Populated call graph edges");
        }

        // Derive file-level dependencies from call edges
        // This captures actual function calls, not just explicit imports
        let file_deps_from_calls = self.db.populate_file_deps_from_call_edges()?;
        if file_deps_from_calls > 0 {
            tracing::debug!(
                file_deps = file_deps_from_calls,
                "Derived file deps from call edges"
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
            lsp_resolved_count,
        })
    }

    /// Index a single file (sequential version).
    ///
    /// Uses a database transaction to ensure atomicity - either the file and all
    /// its symbols are stored, or nothing is changed on failure.
    ///
    /// Unresolved dependencies (target file not yet indexed) are added to `pending`.
    ///
    /// Returns (`symbol_count`, `reference_count`).
    ///
    /// Note: This method is preserved for reference but is no longer used. The parallel
    /// implementation uses `parse_file_static` + `write_parsed_file` instead.
    #[allow(dead_code)]
    fn index_file(
        &mut self,
        path: &Path,
        language: Language,
        pending: &mut Vec<PendingDependency>,
    ) -> Result<(usize, usize)> {
        let content = std::fs::read(path)?;
        let content_str = std::str::from_utf8(&content)
            .map_err(|_| Error::Parser("file is not valid UTF-8".to_string()))?;

        let lang_support = languages::get_language_support(language)
            .ok_or_else(|| Error::Parser(format!("no support for language: {language:?}")))?;

        self.parser
            .set_language(&lang_support.tree_sitter_language())
            .map_err(|e| Error::Parser(e.to_string()))?;

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

        // Compute module path once for all symbols in this file
        let module_path = self.compute_module_path_for_file(path);

        let symbol_data: Vec<SymbolData> = extracted
            .iter()
            .zip(qualified_names.iter())
            .map(|(sym, qn)| SymbolData {
                name: &sym.name,
                module_path: &module_path,
                qualified_name: qn,
                kind: sym.kind,
                line: sym.line,
                column: sym.column,
                span: sym.span,
                signature: sym.signature.as_deref(),
                visibility: sym.visibility,
                parent_symbol_id: None, // TODO: parent_symbol_id
                is_test: sym.is_test,
            })
            .collect();

        // Store in database atomically
        let (file_id, symbol_ids) = self.db.index_file_atomic(
            &self.relative_path(path),
            language,
            mtime_ns,
            size_bytes,
            None, // TODO: compute content hash
            &symbol_data,
        )?;

        // Build lookup maps from inserted data + generated IDs (no DB round-trip)
        let (name_to_id, span_to_id) = Self::build_symbol_maps_from_data(&symbol_data, &symbol_ids);

        // Store references for resolvable symbols
        let refs_stored = self.store_references(file_id, &refs, &name_to_id, &span_to_id)?;

        // Store imports in the database for cross-file reference resolution
        self.store_imports(file_id, &imports, language)?;

        // Compute and store file dependencies (L2: only for actually used symbols)
        self.compute_dependencies(path, file_id, &imports, &refs, pending)?;

        Ok((extracted.len(), refs_stored))
    }

    /// Parse a single file for parallel indexing (Phase 1a).
    ///
    /// This is a static method that can be called from parallel threads.
    /// It reads the file, parses it with tree-sitter, and extracts symbols,
    /// references, and imports without touching the database.
    ///
    /// # Arguments
    ///
    /// * `workspace_root` - The workspace root for computing relative paths
    /// * `file_path` - Absolute path to the file to parse
    /// * `language` - The detected language of the file
    ///
    /// # Returns
    ///
    /// `ParsedFileData` containing all extracted information, ready for
    /// database insertion via `BatchWriter`.
    fn parse_file_static(
        workspace_root: &Path,
        file_path: &Path,
        language: Language,
    ) -> Result<ParsedFileData> {
        use std::cell::RefCell;

        // Thread-local parser to avoid re-initialization overhead
        thread_local! {
            static PARSER: RefCell<tree_sitter::Parser> = RefCell::new(tree_sitter::Parser::new());
        }

        // Read file content
        let content = std::fs::read(file_path)?;
        let content_str = std::str::from_utf8(&content)
            .map_err(|_| Error::Parser("file is not valid UTF-8".to_string()))?;

        let lang_support = languages::get_language_support(language)
            .ok_or_else(|| Error::Parser(format!("no support for language: {language:?}")))?;

        // try_borrow_mut prevents panics if code structure changes to allow re-entrant calls
        let tree = PARSER.with(|parser| {
            let mut parser = parser.try_borrow_mut().map_err(|_| {
                Error::Parser("thread-local parser already borrowed (re-entrant call?)".to_string())
            })?;
            parser
                .set_language(&lang_support.tree_sitter_language())
                .map_err(|e| {
                    tracing::error!(
                        language = ?language,
                        file = %file_path.display(),
                        error = %e,
                        "Failed to set parser language"
                    );
                    Error::Parser(format!("failed to set language {language:?}: {e}"))
                })?;
            parser
                .parse(content_str, None)
                .ok_or_else(|| Error::Parser("failed to parse file".to_string()))
        })?;

        let extracted = lang_support.extract_symbols(&tree, content_str.as_bytes());
        let imports = lang_support.extract_imports(&tree, content_str.as_bytes());
        let references = lang_support.extract_references(&tree, content_str.as_bytes());

        let metadata = std::fs::metadata(file_path)?;
        #[allow(clippy::cast_possible_truncation)]
        let mtime_ns = match metadata.modified() {
            Ok(mtime) => match mtime.duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos() as i64,
                Err(_) => 0,
            },
            Err(_) => 0,
        };
        let size_bytes = metadata.len();

        // Convert extracted symbols to owned versions
        let symbols: Vec<OwnedSymbolData> = extracted
            .iter()
            .map(|sym| {
                let qualified_name = if let Some(parent) = &sym.parent_name {
                    format!("{}::{}", parent, sym.name)
                } else {
                    sym.name.clone()
                };
                // NOTE: module_path is left empty here because parse_file_static runs in
                // parallel threads without access to the crate list. The module_path is
                // computed later during write_parsed_file (batch mode) or post-indexing
                // for streaming mode. Streaming mode currently does not populate module_path.
                OwnedSymbolData::new(
                    sym.name.clone(),
                    String::new(),
                    qualified_name,
                    sym.kind,
                    sym.line,
                    sym.column,
                    sym.span,
                    sym.signature.clone(),
                    sym.visibility,
                    None, // TODO: parent_symbol_id
                    sym.is_test,
                )
            })
            .collect();

        // Compute relative path — reject files outside the workspace boundary
        let relative_path = file_path
            .strip_prefix(workspace_root)
            .map_err(|_| {
                Error::Config(format!(
                    "file '{}' is outside workspace root '{}'",
                    file_path.display(),
                    workspace_root.display()
                ))
            })?
            .to_path_buf();

        Ok(ParsedFileData::new(
            relative_path,
            language,
            mtime_ns,
            size_bytes,
            symbols,
            references,
            imports,
        ))
    }

    /// Write a single parsed file to the database and compute its dependencies.
    ///
    /// This is Phase 1b of indexing - the sequential database write that must
    /// happen after parallel parsing.
    fn write_parsed_file(
        &mut self,
        data: &ParsedFileData,
        pending: &mut Vec<PendingDependency>,
    ) -> Result<(usize, usize)> {
        // Compute module path for all symbols in this file
        let full_path = self.workspace_root.join(&data.relative_path);
        let module_path = self.compute_module_path_for_file(&full_path);

        // Convert owned symbols to borrowed for insertion, using computed module_path
        let symbol_data: Vec<SymbolData<'_>> = data
            .symbols
            .iter()
            .map(|s| SymbolData {
                name: &s.name,
                module_path: &module_path,
                qualified_name: &s.qualified_name,
                kind: s.kind,
                line: s.line,
                column: s.column,
                span: s.span,
                signature: s.signature.as_deref(),
                visibility: s.visibility,
                parent_symbol_id: s.parent_symbol_id,
                is_test: s.is_test,
            })
            .collect();

        // Insert file and symbols atomically
        let (file_id, symbol_ids) = self.db.index_file_atomic(
            &data.relative_path,
            data.language,
            data.mtime_ns,
            data.size_bytes,
            None, // TODO: content hash
            &symbol_data,
        )?;

        // Build lookup maps from inserted data + generated IDs (no DB round-trip)
        let (name_to_id, span_to_id) = Self::build_symbol_maps_from_data(&symbol_data, &symbol_ids);

        // Store references
        let refs_stored =
            self.store_references(file_id, &data.references, &name_to_id, &span_to_id)?;

        // Store imports
        self.store_imports(file_id, &data.imports, data.language)?;

        // Compute and store file dependencies (reusing full_path from above)
        self.compute_dependencies(
            &full_path,
            file_id,
            &data.imports,
            &data.references,
            pending,
        )?;

        Ok((data.symbols.len(), refs_stored))
    }

    /// Build lookup maps from inserted symbol data and their generated IDs.
    ///
    /// Pairs each `SymbolData` with its corresponding `SymbolId` returned by
    /// `index_file_atomic`, avoiding a round-trip query to read symbols back.
    fn build_symbol_maps_from_data(
        symbols: &[SymbolData<'_>],
        symbol_ids: &[SymbolId],
    ) -> (HashMap<String, SymbolId>, HashMap<Span, SymbolId>) {
        let mut name_to_id: HashMap<String, SymbolId> = HashMap::new();
        let mut span_to_id: HashMap<Span, SymbolId> = HashMap::new();

        for (sym, &id) in symbols.iter().zip(symbol_ids) {
            if let Some(prev_id) = name_to_id.insert(sym.name.to_string(), id) {
                trace!(
                    name = %sym.name,
                    new_id = %id,
                    prev_id = %prev_id,
                    "Duplicate symbol name in file, using newer"
                );
            }

            if let Some(span) = sym.span {
                span_to_id.insert(span, id);
            }
        }

        (name_to_id, span_to_id)
    }

    /// Store extracted references in the database.
    ///
    /// Stores ALL references, including unresolved ones. For unresolved references
    /// (cross-file symbols), `symbol_id` is set to `None` and `reference_name` is
    /// populated for later resolution in Pass 2.
    ///
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
            let qualified_name = Self::build_qualified_name(&r.name, r.path.as_deref());

            // Try same-file resolution: simple name first, then qualified name
            let symbol_id = name_to_id
                .get(&r.name)
                .or_else(|| name_to_id.get(&qualified_name))
                .copied();

            // For unresolved references, store the name for Pass 2 cross-file resolution
            let reference_name = if symbol_id.is_none() {
                trace!(
                    reference_name = %qualified_name,
                    line = r.line,
                    "Storing unresolved reference for later resolution"
                );
                Some(qualified_name)
            } else {
                None
            };

            let in_symbol_id = r
                .containing_symbol_span
                .and_then(|span| span_to_id.get(&span).copied());

            self.db.insert_reference(
                symbol_id,
                file_id,
                r.kind.to_db_kind().as_str(),
                r.line,
                r.column,
                in_symbol_id,
                reference_name.as_deref(),
            )?;
            count += 1;
        }

        Ok(count)
    }

    /// Build a qualified name from a simple name and optional path segments.
    ///
    /// Examples:
    /// - `("open", Some(["Index"]))` -> `"Index::open"`
    /// - `("Foo", None)` -> `"Foo"`
    /// - `("bar", Some([]))` -> `"bar"`
    fn build_qualified_name(name: &str, path: Option<&[String]>) -> String {
        match path {
            Some(segments) if !segments.is_empty() => {
                format!("{}::{}", segments.join("::"), name)
            }
            _ => name.to_string(),
        }
    }

    /// Store extracted imports in the database for cross-file reference resolution.
    ///
    /// Imports are stored with language-appropriate path separators:
    /// - Rust: `::` (e.g., `crate::db`, `std::collections`)
    /// - C#: `.` (e.g., `MyApp.Services`, `System.Collections.Generic`)
    ///
    /// Clears old imports for this file before storing new ones (for re-indexing).
    fn store_imports(
        &self,
        file_id: FileId,
        imports: &[common::ImportStatement],
        language: Language,
    ) -> Result<()> {
        // Clear old imports for this file (for re-indexing)
        self.db.clear_imports_for_file(file_id)?;

        // Determine path separator based on language
        let separator = match language {
            Language::Rust => "::",
            Language::CSharp => ".",
        };

        for import in imports {
            let source = import.path.join(separator);

            // Handle glob imports
            if import.is_glob {
                self.db
                    .insert_import(file_id, "*", &source, import.alias.as_deref())?;
                continue;
            }

            // For explicit imports: store each imported name
            if import.imported_names.is_empty() {
                // Namespace/module import (C# style) or module import without braces
                // Store with "*" to indicate "all from this module"
                self.db
                    .insert_import(file_id, "*", &source, import.alias.as_deref())?;
            } else {
                // Store each explicitly imported name
                for name in &import.imported_names {
                    self.db
                        .insert_import(file_id, name, &source, import.alias.as_deref())?;
                }
            }
        }

        trace!(
            file_id = %file_id,
            import_count = imports.len(),
            "Stored imports for file"
        );

        Ok(())
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

    /// Compute file-level dependencies for all indexed files from stored data.
    ///
    /// This is used in streaming mode where dependencies cannot be computed during
    /// writing because the batch writer operates in a separate thread without access
    /// to Tethys state (`workspace_root`, module resolution).
    ///
    /// The method iterates over all indexed files, loads their imports and references
    /// from the database, and computes dependencies using the same logic as
    /// `compute_dependencies`.
    fn compute_all_dependencies(&mut self, pending: &mut Vec<PendingDependency>) -> Result<()> {
        let files = self.db.list_all_files()?;
        let file_count = files.len();

        debug!(
            file_count,
            "Computing file-level dependencies from stored data"
        );

        for file in files {
            // Load imports and references from database
            let imports = self.db.get_imports_for_file(file.id)?;
            let refs = self.db.list_references_in_file(file.id)?;

            // Convert database imports to the format compute_dependencies expects
            let import_statements = Self::convert_imports_to_statements(&imports, file.language);

            // Extract just the reference names for dependency checking
            let ref_names: Vec<String> = refs
                .iter()
                .filter_map(|r| r.reference_name.clone())
                .collect();

            // Compute dependencies using the converted data
            let full_path = self.workspace_root.join(&file.path);
            self.compute_dependencies_from_stored(
                &full_path,
                file.id,
                &import_statements,
                &ref_names,
                pending,
            )?;
        }

        debug!(
            file_count,
            pending_count = pending.len(),
            "File dependency computation complete"
        );

        Ok(())
    }

    /// Convert database Import records back to `ImportStatement` format.
    ///
    /// This is needed for streaming mode where we need to compute dependencies
    /// from stored data rather than the original parsed data.
    fn convert_imports_to_statements(
        imports: &[Import],
        language: Language,
    ) -> Vec<common::ImportStatement> {
        use std::collections::HashMap;

        // Group imports by source_module to reconstruct ImportStatement
        let mut grouped: HashMap<&str, Vec<&Import>> = HashMap::new();
        for import in imports {
            grouped
                .entry(&import.source_module)
                .or_default()
                .push(import);
        }

        let separator = match language {
            Language::Rust => "::",
            Language::CSharp => ".",
        };

        grouped
            .into_iter()
            .map(|(source_module, module_imports)| {
                let path: Vec<String> = source_module.split(separator).map(String::from).collect();
                let is_glob = module_imports.iter().any(|i| i.symbol_name == "*");
                let imported_names: Vec<String> = module_imports
                    .iter()
                    .filter(|i| i.symbol_name != "*")
                    .map(|i| i.symbol_name.clone())
                    .collect();
                let alias = module_imports.first().and_then(|i| i.alias.clone());

                common::ImportStatement {
                    path,
                    imported_names,
                    is_glob,
                    alias,
                    line: 0, // Not needed for dependency computation
                }
            })
            .collect()
    }

    /// Compute dependencies from stored import/reference data.
    ///
    /// Similar to `compute_dependencies` but takes pre-processed data rather than
    /// `ExtractedReference` objects. Used in streaming mode.
    fn compute_dependencies_from_stored(
        &self,
        current_file: &Path,
        file_id: FileId,
        imports: &[common::ImportStatement],
        reference_names: &[String],
        pending: &mut Vec<PendingDependency>,
    ) -> Result<()> {
        use std::collections::HashSet;

        // Build a set of actually referenced names
        let refs_set: HashSet<&str> = reference_names.iter().map(String::as_str).collect();

        let crate_root = self.workspace_root.join("src");
        let mut depended_files: HashSet<PathBuf> = HashSet::new();

        for import_stmt in imports {
            if import_stmt.is_glob {
                continue;
            }

            // Check if any imported name is actually referenced
            let mut is_used = false;
            for name in &import_stmt.imported_names {
                let lookup_name = import_stmt.alias.as_ref().unwrap_or(name);
                if refs_set.contains(lookup_name.as_str()) {
                    is_used = true;
                    break;
                }
            }

            if !is_used {
                continue;
            }

            if let Some(resolved) =
                resolve_module_path(&import_stmt.path, current_file, &crate_root)
            {
                let dep_path = self.relative_path(&resolved).to_path_buf();
                depended_files.insert(dep_path);
            }
        }

        for dep_path in depended_files {
            match self.db.get_file_id(&dep_path)? {
                Some(dep_file_id) => {
                    self.db.insert_file_dependency(file_id, dep_file_id)?;
                }
                None => {
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

    /// Resolve cross-file references against the symbol database (Pass 2).
    ///
    /// After all files are indexed (Pass 1), this method resolves unresolved
    /// references by matching them to symbols discovered in other files via
    /// the imports table.
    ///
    /// Returns the number of references successfully resolved.
    fn resolve_cross_file_references(&self) -> Result<usize> {
        let unresolved = self.db.get_unresolved_references()?;
        if unresolved.is_empty() {
            return Ok(0);
        }

        debug!(
            unresolved_count = unresolved.len(),
            "Starting cross-file reference resolution (Pass 2)"
        );

        let mut resolved_count = 0;

        // Group by file for efficiency - avoids repeated import lookups
        let mut by_file: HashMap<FileId, Vec<Reference>> = HashMap::new();
        for ref_ in unresolved {
            by_file.entry(ref_.file_id).or_default().push(ref_);
        }

        // FIXME: Assumes crate root is workspace_root/src/. Does not detect actual
        // main/lib location from Cargo.toml. Needs Cargo.toml parsing support.
        let crate_root = self.workspace_root.join("src");

        for (file_id, refs) in by_file {
            resolved_count += self.resolve_refs_for_file(file_id, refs, &crate_root)?;
        }

        Ok(resolved_count)
    }

    /// Resolve references for a single file using its imports.
    fn resolve_refs_for_file(
        &self,
        file_id: FileId,
        refs: Vec<Reference>,
        crate_root: &Path,
    ) -> Result<usize> {
        let imports = self.db.get_imports_for_file(file_id)?;
        if imports.is_empty() {
            return Ok(0);
        }

        // Get the current file's path for relative path resolution
        let current_file_path = if let Some(f) = self.db.get_file_by_id(file_id)? {
            Some(self.workspace_root.join(&f.path))
        } else {
            warn!(
                file_id = %file_id,
                "File not found during reference resolution - possible database inconsistency"
            );
            None
        };

        // Build import structures
        let (explicit_imports, glob_imports) = Self::build_import_maps(&imports);

        let mut resolved_count = 0;

        for ref_ in refs {
            let Some(ref_name) = &ref_.reference_name else {
                continue;
            };

            let resolved = self.try_resolve_reference(
                &ref_,
                ref_name,
                &explicit_imports,
                &glob_imports,
                current_file_path.as_deref(),
                crate_root,
                file_id,
            )?;

            if resolved {
                resolved_count += 1;
            }
        }

        Ok(resolved_count)
    }

    /// Build lookup maps from imports for reference resolution.
    fn build_import_maps(imports: &[Import]) -> (HashMap<&str, (&str, &str)>, Vec<&str>) {
        let mut explicit_imports: HashMap<&str, (&str, &str)> = HashMap::new();
        let mut glob_imports: Vec<&str> = Vec::new();

        for imp in imports {
            if imp.symbol_name == "*" {
                glob_imports.push(&imp.source_module);
            } else {
                let lookup_name = imp.alias.as_deref().unwrap_or(&imp.symbol_name);
                if let Some((prev_symbol, prev_module)) =
                    explicit_imports.insert(lookup_name, (&imp.symbol_name, &imp.source_module))
                {
                    trace!(
                        lookup_name = %lookup_name,
                        prev_symbol = %prev_symbol,
                        prev_module = %prev_module,
                        new_symbol = %imp.symbol_name,
                        new_module = %imp.source_module,
                        "Import name collision: overwriting previous import"
                    );
                }
            }
        }

        (explicit_imports, glob_imports)
    }

    /// Try to resolve a single reference using imports and fallback search.
    #[allow(clippy::too_many_arguments)]
    fn try_resolve_reference(
        &self,
        ref_: &Reference,
        ref_name: &str,
        explicit_imports: &HashMap<&str, (&str, &str)>,
        glob_imports: &[&str],
        current_file_path: Option<&Path>,
        crate_root: &Path,
        file_id: FileId,
    ) -> Result<bool> {
        let is_qualified = ref_name.contains("::");

        // Try explicit imports
        if let Some(symbol) = self.resolve_via_explicit_import(
            ref_name,
            explicit_imports,
            current_file_path,
            crate_root,
            is_qualified,
        )? {
            trace!(
                ref_id = ref_.id,
                ref_name = %ref_name,
                symbol_id = %symbol.id,
                "Resolved reference via explicit import"
            );
            self.db.resolve_reference(ref_.id, symbol.id)?;
            return Ok(true);
        }

        // Try glob imports
        for source_module in glob_imports {
            if let Some(symbol) = self.resolve_symbol_in_module(
                ref_name,
                source_module,
                current_file_path,
                crate_root,
                is_qualified,
            )? {
                trace!(
                    ref_id = ref_.id,
                    ref_name = %ref_name,
                    symbol_id = %symbol.id,
                    "Resolved reference via glob import"
                );
                self.db.resolve_reference(ref_.id, symbol.id)?;
                return Ok(true);
            }
        }

        // Fallback search differs for qualified vs simple names
        if let Some(symbol) = self.fallback_symbol_search(ref_name, is_qualified)? {
            trace!(
                ref_id = ref_.id,
                ref_name = %ref_name,
                symbol_id = %symbol.id,
                "Resolved reference via fallback search"
            );
            self.db.resolve_reference(ref_.id, symbol.id)?;
            return Ok(true);
        }

        trace!(
            ref_name = %ref_name,
            file_id = %file_id,
            "Reference remains unresolved (likely external crate)"
        );
        Ok(false)
    }

    /// Resolve a reference via explicit import lookup.
    ///
    /// For qualified references like `Index::open`, looks up the first segment (`Index`)
    /// and searches for the full qualified name in that module.
    fn resolve_via_explicit_import(
        &self,
        ref_name: &str,
        explicit_imports: &HashMap<&str, (&str, &str)>,
        current_file_path: Option<&Path>,
        crate_root: &Path,
        is_qualified: bool,
    ) -> Result<Option<Symbol>> {
        let lookup_name = if is_qualified {
            ref_name
                .split_once("::")
                .map_or(ref_name, |(first, _)| first)
        } else {
            ref_name
        };

        let Some((symbol_name, source_module)) = explicit_imports.get(lookup_name) else {
            return Ok(None);
        };

        // For qualified refs, build the full qualified name using the imported symbol
        let search_name = if is_qualified {
            if let Some((_, rest)) = ref_name.split_once("::") {
                format!("{symbol_name}::{rest}")
            } else {
                (*symbol_name).to_string()
            }
        } else {
            (*symbol_name).to_string()
        };

        self.resolve_symbol_in_module(
            &search_name,
            source_module,
            current_file_path,
            crate_root,
            is_qualified,
        )
    }

    /// Resolve a symbol within a specific module (source path).
    ///
    /// Translates the module path to a file path, then searches for the symbol.
    /// Uses qualified name matching for qualified references, simple name for others.
    fn resolve_symbol_in_module(
        &self,
        symbol_name: &str,
        source_module: &str,
        current_file_path: Option<&Path>,
        crate_root: &Path,
        use_qualified_search: bool,
    ) -> Result<Option<Symbol>> {
        let Some(target_file_id) =
            self.resolve_module_to_file_id(source_module, current_file_path, crate_root)?
        else {
            return Ok(None);
        };

        if use_qualified_search {
            self.db
                .search_symbol_by_qualified_name_in_file(symbol_name, target_file_id)
        } else {
            self.db.search_symbol_in_file(symbol_name, target_file_id)
        }
    }

    /// Translate a module path (e.g., `crate::db`) to a file ID.
    fn resolve_module_to_file_id(
        &self,
        source_module: &str,
        current_file_path: Option<&Path>,
        crate_root: &Path,
    ) -> Result<Option<FileId>> {
        let Some(current_path) = current_file_path else {
            trace!(
                source_module = %source_module,
                "Cannot resolve module: no current file path"
            );
            return Ok(None);
        };

        let path_segments: Vec<String> = source_module.split("::").map(String::from).collect();

        let Some(resolved_file) = resolve_module_path(&path_segments, current_path, crate_root)
        else {
            trace!(
                source_module = %source_module,
                "Cannot resolve module: path resolution failed (likely external crate)"
            );
            return Ok(None);
        };

        let relative_path = self.relative_path(&resolved_file);
        let file_id = self.db.get_file_id(&relative_path)?;

        if file_id.is_none() {
            trace!(
                source_module = %source_module,
                resolved_file = %resolved_file.display(),
                "Cannot resolve module: target file not indexed"
            );
        }

        Ok(file_id)
    }

    /// Fallback symbol search when import-based resolution fails.
    ///
    /// For qualified names, searches by exact `qualified_name` match.
    /// For simple names, searches by name across all files (safe for unambiguous symbols).
    fn fallback_symbol_search(&self, ref_name: &str, is_qualified: bool) -> Result<Option<Symbol>> {
        if is_qualified {
            self.db.get_symbol_by_qualified_name(ref_name)
        } else {
            let Some(symbol) = self.db.search_symbol_by_name(ref_name)? else {
                return Ok(None);
            };
            // Verify the symbol's file exists
            if self.db.get_file_by_id(symbol.file_id)?.is_some() {
                Ok(Some(symbol))
            } else {
                warn!(
                    ref_name = %ref_name,
                    symbol_id = %symbol.id,
                    file_id = %symbol.file_id,
                    "Symbol found but file record missing - database may be inconsistent"
                );
                Ok(None)
            }
        }
    }

    /// Resolve references using LSP `goto_definition` (Pass 3).
    ///
    /// After tree-sitter resolution (Pass 2), some references may still be unresolved
    /// (e.g., external crate symbols, complex type inference). This pass uses the
    /// language server to resolve them.
    ///
    /// # Design
    ///
    /// - LSP is spawned lazily (only if there are unresolved refs for this language)
    /// - LSP stays alive for batch queries (amortizes startup cost)
    /// - Shutdown on completion
    /// - Matches LSP definition locations to symbols by file path + line number
    ///
    /// # Arguments
    ///
    /// * `provider` - The LSP provider to use (e.g., `RustAnalyzerProvider`)
    /// * `language` - The language to filter references by (e.g., `Language::Rust`)
    ///
    /// # Returns
    ///
    /// The number of references successfully resolved via LSP.
    #[allow(clippy::too_many_lines)]
    fn resolve_via_lsp(
        &self,
        provider: &dyn lsp::LspProvider,
        language: Language,
        lsp_timeout_secs: u64,
    ) -> Result<usize> {
        // Get unresolved references with file path information, filtered by language
        let all_unresolved = self.db.get_unresolved_references_for_lsp()?;
        let unresolved: Vec<_> = all_unresolved
            .into_iter()
            .filter(|r| {
                r.file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| Language::from_extension(ext) == Some(language))
            })
            .collect();

        if unresolved.is_empty() {
            debug!(
                language = ?language,
                "No unresolved references for LSP resolution"
            );
            return Ok(0);
        }

        debug!(
            language = ?language,
            unresolved_count = unresolved.len(),
            "Starting LSP resolution pass (Pass 3)"
        );

        // Start LSP lazily - only if there are refs to resolve
        let mut client = match lsp::LspClient::start(provider, &self.workspace_root) {
            Ok(c) => c,
            Err(e) => {
                // User explicitly requested LSP with --lsp flag, so log at error level
                tracing::error!(
                    error = %e,
                    language = ?language,
                    install_hint = %provider.install_hint(),
                    "LSP server failed to start - LSP resolution skipped. \
                     {} Or remove --lsp flag.",
                    provider.install_hint()
                );
                return Ok(0);
            }
        };

        let mut resolved_count = 0;
        let mut lsp_errors = 0;
        let mut opened_files: HashSet<PathBuf> = HashSet::new();

        // Language::as_str() returns the LSP language identifier ("rust", "csharp")
        let language_id = language.as_str();

        // Pre-open all unique files for servers like csharp-ls that need time to process
        let unique_files: HashSet<_> = unresolved
            .iter()
            .map(|r| self.workspace_root.join(&r.file_path))
            .collect();

        debug!(
            language = ?language,
            file_count = unique_files.len(),
            "Pre-opening files for LSP"
        );

        for file_path in &unique_files {
            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    if client.did_open(file_path, &content, language_id).is_ok() {
                        opened_files.insert(file_path.clone());
                    } else {
                        trace!(
                            file = %file_path.display(),
                            "Failed to send didOpen notification"
                        );
                    }
                }
                Err(e) => {
                    trace!(
                        file = %file_path.display(),
                        error = %e,
                        "Failed to read file for LSP pre-opening"
                    );
                }
            }
        }

        // For servers like csharp-ls that load solutions asynchronously, wait for
        // solution loading to complete by monitoring $/progress notifications.
        // rust-analyzer indexes on startup and responds immediately, so no wait needed.
        if language == Language::CSharp {
            let timeout = std::time::Duration::from_secs(lsp_timeout_secs);
            match client.wait_for_solution_load(timeout) {
                Ok(true) => {
                    debug!(language = ?language, "Solution loading completed");
                }
                Ok(false) => {
                    warn!(
                        language = ?language,
                        "Solution loading not detected or timed out, queries may fail"
                    );
                }
                Err(e) => {
                    warn!(
                        language = ?language,
                        error = %e,
                        "Error while waiting for solution load"
                    );
                }
            }
        }

        for unresolved_ref in &unresolved {
            match self.resolve_single_ref_via_lsp(
                &mut client,
                unresolved_ref,
                &mut lsp_errors,
                &mut opened_files,
                language_id,
            ) {
                Ok(true) => resolved_count += 1,
                Ok(false) => {}
                Err(e) => {
                    warn!(
                        ref_id = %unresolved_ref.ref_id,
                        error = %e,
                        "Database error during LSP resolution"
                    );
                }
            }
        }

        // Graceful shutdown
        if let Err(e) = client.shutdown() {
            warn!(error = %e, "LSP shutdown failed");
        }

        if lsp_errors > 5 {
            info!(
                total_errors = lsp_errors,
                "Additional LSP errors suppressed"
            );
        }

        debug!(
            language = ?language,
            resolved_count = resolved_count,
            total_unresolved = unresolved.len(),
            lsp_errors = lsp_errors,
            "LSP resolution pass complete"
        );

        Ok(resolved_count)
    }

    /// Attempt to resolve a single reference via LSP.
    ///
    /// Returns `Ok(true)` if resolved, `Ok(false)` if not resolved (but no error),
    /// or `Err` for database errors.
    fn resolve_single_ref_via_lsp(
        &self,
        client: &mut lsp::LspClient,
        unresolved_ref: &UnresolvedRefForLsp,
        lsp_errors: &mut usize,
        opened_files: &mut HashSet<PathBuf>,
        language_id: &str,
    ) -> Result<bool> {
        // Construct absolute file path for LSP
        let file_path = self.workspace_root.join(&unresolved_ref.file_path);

        // Ensure the file is opened in the LSP server (required by some servers like csharp-ls)
        if !opened_files.contains(&file_path) {
            // Read file content
            let content = match std::fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(e) => {
                    trace!(
                        file = %file_path.display(),
                        error = %e,
                        "Failed to read file for LSP didOpen"
                    );
                    return Ok(false);
                }
            };

            // Send didOpen notification
            if let Err(e) = client.did_open(&file_path, &content, language_id) {
                trace!(
                    file = %file_path.display(),
                    error = %e,
                    "Failed to send didOpen notification"
                );
                // Continue anyway - some servers might work without it
            }
            opened_files.insert(file_path.clone());
        }

        // LSP uses 0-indexed positions, our DB uses 1-indexed
        let lsp_line = unresolved_ref.line.saturating_sub(1);
        let lsp_col = unresolved_ref.column.saturating_sub(1);

        // Call goto_definition - we wait once after initialization for solution loading,
        // so no per-query retries needed here
        let definition = match client.goto_definition(&file_path, lsp_line, lsp_col) {
            Ok(Some(loc)) => loc,
            Ok(None) => {
                trace!(
                    ref_id = %unresolved_ref.ref_id,
                    ref_name = %unresolved_ref.reference_name,
                    "LSP returned no definition"
                );
                return Ok(false);
            }
            Err(e) => {
                *lsp_errors += 1;
                // Log first error at warn level so users see something went wrong
                if *lsp_errors == 1 {
                    warn!(
                        ref_id = %unresolved_ref.ref_id,
                        error = %e,
                        "LSP goto_definition failed (further errors logged at trace level)"
                    );
                } else if *lsp_errors <= 5 {
                    trace!(
                        ref_id = %unresolved_ref.ref_id,
                        error = %e,
                        "LSP goto_definition failed"
                    );
                }
                return Ok(false);
            }
        };

        // Extract file path from LSP URI and convert to relative path
        let Some(def_path) = Self::uri_to_path(definition.uri.as_str()) else {
            trace!(
                uri = definition.uri.as_str(),
                "Cannot parse LSP definition URI"
            );
            return Ok(false);
        };

        // Make the path relative to workspace root
        let Ok(relative_def_path) = def_path.strip_prefix(&self.workspace_root) else {
            trace!(
                def_path = %def_path.display(),
                "Definition outside workspace, skipping"
            );
            return Ok(false);
        };

        // Look up the file in our DB
        let Some(def_file_id) = self.db.get_file_id(relative_def_path)? else {
            trace!(
                def_path = %relative_def_path.display(),
                "Definition file not in index"
            );
            return Ok(false);
        };

        // LSP returns 0-indexed, convert to 1-indexed for DB lookup
        let def_line = definition.range.start.line + 1;

        // Find the symbol at that line
        let Some(symbol) = self.db.find_symbol_at_line(def_file_id, def_line)? else {
            trace!(
                def_path = %relative_def_path.display(),
                def_line = def_line,
                "No symbol found at definition line"
            );
            return Ok(false);
        };

        // Resolve the reference
        self.db
            .resolve_reference(unresolved_ref.ref_id.as_i64(), symbol.id)?;

        trace!(
            ref_id = %unresolved_ref.ref_id,
            symbol_id = %symbol.id,
            symbol_name = %symbol.name,
            "Resolved reference via LSP"
        );

        Ok(true)
    }

    /// Convert a file URI to a filesystem path.
    ///
    /// Handles `file://` URIs from LSP responses, including percent-encoded
    /// characters (e.g., `%20` for spaces).
    fn uri_to_path(uri: &str) -> Option<PathBuf> {
        use percent_encoding::percent_decode_str;

        // Strip file:// prefix
        let path_str = uri.strip_prefix("file://")?;

        // Decode percent-encoded characters (%20 -> space, etc.)
        let decoded = percent_decode_str(path_str).decode_utf8().ok()?;

        // On Unix, paths start with /, so we have file:///path
        // On Windows, paths start with drive letter, so we have file:///C:/path
        #[cfg(windows)]
        {
            // Remove leading / before drive letter: /C:/path -> C:/path
            let path_str = decoded.strip_prefix('/').unwrap_or(&decoded);
            Some(PathBuf::from(path_str))
        }

        #[cfg(not(windows))]
        {
            Some(PathBuf::from(decoded.as_ref()))
        }
    }

    /// Discover source files in the workspace.
    fn discover_files(
        &self,
        directories_skipped: &mut Vec<(PathBuf, String)>,
    ) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        Self::walk_dir(&self.workspace_root, &mut files, directories_skipped)?;
        Ok(files)
    }

    /// Recursively walk a directory, collecting source files.
    ///
    /// Directories that cannot be read (e.g., due to permissions) are tracked
    /// in `directories_skipped` for reporting.
    fn walk_dir(
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
                Self::walk_dir(&path, files, directories_skipped)?;
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
    /// Handles symlink differences (e.g., `/var` → `/private/var` on macOS) by
    /// attempting canonicalization when the initial `strip_prefix` fails on
    /// absolute paths. Returns `Cow::Borrowed` for the common fast path,
    /// `Cow::Owned` only when canonicalization was needed.
    fn relative_path<'a>(&self, path: &'a Path) -> Cow<'a, Path> {
        if let Ok(relative) = path.strip_prefix(&self.workspace_root) {
            return Cow::Borrowed(relative);
        }

        // For absolute paths, try canonicalizing to resolve symlinks
        if path.is_absolute() {
            if let Ok(canonical) = path.canonicalize() {
                if let Ok(relative) = canonical.strip_prefix(&self.workspace_root) {
                    return Cow::Owned(relative.to_path_buf());
                }
            }
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
    ///
    /// Deletes and recreates the database file, ensuring schema changes are
    /// applied cleanly. Use this instead of manually deleting the database.
    pub fn rebuild(&mut self) -> Result<IndexStats> {
        self.db.reset()?;
        self.index()
    }

    /// Rebuild the entire index from scratch with options.
    ///
    /// Deletes and recreates the database file, ensuring schema changes are
    /// applied cleanly. See [`index_with_options`](Self::index_with_options)
    /// for details on options.
    pub fn rebuild_with_options(&mut self, options: IndexOptions) -> Result<IndexStats> {
        self.db.reset()?;
        self.index_with_options(options)
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

    /// Get symbols that call/use the given symbol, with LSP refinement.
    ///
    /// Combines results from the tree-sitter index with references found by the
    /// language server. This catches callers that tree-sitter couldn't resolve
    /// during indexing (e.g., through complex type inference).
    ///
    /// # Design
    ///
    /// 1. Get callers from the database (tree-sitter indexed)
    /// 2. Find the symbol's definition location
    /// 3. Call LSP `find_references` at that location
    /// 4. For each LSP reference, find its containing symbol
    /// 5. Merge with DB callers, deduplicating by symbol ID
    ///
    /// # Fallback Behavior
    ///
    /// If LSP fails to start or returns errors, falls back to DB-only results
    /// and logs a warning.
    #[allow(clippy::too_many_lines)]
    pub fn get_callers_with_lsp(&self, qualified_name: &str) -> Result<Vec<Dependent>> {
        use std::collections::HashSet;

        // Step 1: Get callers from the database
        let symbol = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let db_callers = self.db.get_callers(symbol.id)?;

        // Build a set of symbol IDs we already know about
        let mut known_symbol_ids: HashSet<SymbolId> =
            db_callers.iter().map(|c| c.symbol.id).collect();

        // Step 2: Get the symbol's definition file path
        let symbol_file = self
            .db
            .get_file_by_id(symbol.file_id)?
            .ok_or_else(|| Error::NotFound(format!("file id: {}", symbol.file_id)))?;
        let symbol_file_path = self.workspace_root.join(&symbol_file.path);

        // Step 3: Spawn LSP and call find_references
        // Select the appropriate LSP provider based on the symbol's file language
        let provider = lsp::AnyProvider::for_language(symbol_file.language);
        let mut lsp_client = match lsp::LspClient::start(&provider, &self.workspace_root) {
            Ok(client) => client,
            Err(e) => {
                // User explicitly requested --lsp, so log at error level
                tracing::error!(
                    error = %e,
                    symbol = %qualified_name,
                    language = ?symbol_file.language,
                    install_hint = %provider.install_hint(),
                    "LSP server failed to start - returning DB-only callers. \
                     {} Or remove --lsp flag.",
                    provider.install_hint()
                );
                return self.convert_callers_to_dependents(db_callers);
            }
        };

        // LSP uses 0-indexed positions, our DB uses 1-indexed
        let lsp_line = symbol.line.saturating_sub(1);
        let lsp_col = symbol.column.saturating_sub(1);

        let lsp_refs = match lsp_client.find_references(&symbol_file_path, lsp_line, lsp_col) {
            Ok(refs) => refs,
            Err(e) => {
                // User explicitly requested --lsp, so log at error level
                tracing::error!(
                    error = %e,
                    symbol = %qualified_name,
                    "LSP find_references failed - returning DB-only callers"
                );
                if let Err(shutdown_err) = lsp_client.shutdown() {
                    warn!(error = %shutdown_err, "LSP shutdown failed");
                }
                return self.convert_callers_to_dependents(db_callers);
            }
        };

        // Graceful shutdown
        if let Err(e) = lsp_client.shutdown() {
            warn!(error = %e, "LSP shutdown failed");
        }

        debug!(
            symbol = %qualified_name,
            db_callers = db_callers.len(),
            lsp_refs = lsp_refs.len(),
            "Merging DB and LSP caller results"
        );

        // Step 4: For each LSP reference, find its containing symbol
        let mut additional_callers: Vec<graph::CallerInfo> = Vec::new();

        for loc in lsp_refs {
            // Extract file path from LSP URI and convert to relative path
            let Some(ref_path) = Self::uri_to_path(loc.uri.as_str()) else {
                trace!(uri = loc.uri.as_str(), "Cannot parse LSP reference URI");
                continue;
            };

            // Make the path relative to workspace root
            let Ok(relative_ref_path) = ref_path.strip_prefix(&self.workspace_root) else {
                trace!(
                    ref_path = %ref_path.display(),
                    "Reference outside workspace, skipping"
                );
                continue;
            };

            // Look up the file in our DB
            let Some(ref_file_id) = self.db.get_file_id(relative_ref_path)? else {
                trace!(
                    ref_path = %relative_ref_path.display(),
                    "Reference file not in index"
                );
                continue;
            };

            // LSP returns 0-indexed, convert to 1-indexed for DB lookup
            let ref_line = loc.range.start.line + 1;

            // Find the symbol that contains this reference location
            let Some(containing_symbol) = self.db.find_symbol_at_line(ref_file_id, ref_line)?
            else {
                trace!(
                    ref_path = %relative_ref_path.display(),
                    ref_line = ref_line,
                    "No symbol found at reference line"
                );
                continue;
            };

            // Skip if we already have this caller from the DB
            if known_symbol_ids.contains(&containing_symbol.id) {
                continue;
            }

            // Add this as a new caller
            known_symbol_ids.insert(containing_symbol.id);
            additional_callers.push(graph::CallerInfo {
                symbol: containing_symbol,
                reference_count: 1,
                reference_kinds: vec![ReferenceKind::Call],
            });
        }

        info!(
            symbol = %qualified_name,
            db_callers = db_callers.len(),
            lsp_additional = additional_callers.len(),
            "Caller merge complete"
        );

        // Step 5: Combine DB and LSP callers and convert to Dependent
        let all_callers: Vec<graph::CallerInfo> =
            db_callers.into_iter().chain(additional_callers).collect();

        self.convert_callers_to_dependents(all_callers)
    }

    /// Convert a list of `CallerInfo` to `Dependent` for the public API.
    fn convert_callers_to_dependents(
        &self,
        callers: Vec<graph::CallerInfo>,
    ) -> Result<Vec<Dependent>> {
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
        use std::collections::{HashSet, VecDeque};

        let source = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let max_depth = max_depth.unwrap_or(50);
        let mut visited: HashSet<SymbolId> = HashSet::new();
        let mut results: Vec<types::ReachablePath> = Vec::new();
        let mut queue: VecDeque<(SymbolId, Vec<Symbol>, usize)> = VecDeque::new();

        // Start BFS from the source
        queue.push_back((source.id, vec![], 0));
        visited.insert(source.id);

        while let Some((current_id, path, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            // Get callees (outgoing edges) for forward reachability
            for callee_info in self.db.get_callees(current_id)? {
                if visited.insert(callee_info.symbol.id) {
                    let mut new_path = path.clone();
                    new_path.push(callee_info.symbol.clone());

                    results.push(types::ReachablePath {
                        target: callee_info.symbol.clone(),
                        path: new_path.clone(),
                        depth: depth + 1,
                    });

                    queue.push_back((callee_info.symbol.id, new_path, depth + 1));
                }
            }
        }

        Ok(types::ReachabilityResult {
            source,
            reachable: results,
            max_depth,
            direction: types::ReachabilityDirection::Forward,
        })
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
        use std::collections::{HashSet, VecDeque};

        let source = self
            .db
            .get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {qualified_name}")))?;

        let max_depth = max_depth.unwrap_or(50);
        let mut visited: HashSet<SymbolId> = HashSet::new();
        let mut results: Vec<types::ReachablePath> = Vec::new();
        let mut queue: VecDeque<(SymbolId, Vec<Symbol>, usize)> = VecDeque::new();

        // Start BFS from the source
        queue.push_back((source.id, vec![], 0));
        visited.insert(source.id);

        while let Some((current_id, path, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            // Get callers (incoming edges) for backward reachability
            for caller_info in self.db.get_callers(current_id)? {
                if visited.insert(caller_info.symbol.id) {
                    let mut new_path = path.clone();
                    new_path.push(caller_info.symbol.clone());

                    results.push(types::ReachablePath {
                        target: caller_info.symbol.clone(),
                        path: new_path.clone(),
                        depth: depth + 1,
                    });

                    queue.push_back((caller_info.symbol.id, new_path, depth + 1));
                }
            }
        }

        Ok(types::ReachabilityResult {
            source,
            reachable: results,
            max_depth,
            direction: types::ReachabilityDirection::Backward,
        })
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
            stats.lsp_resolved_count, 0,
            "LSP resolved count should be 0 when use_lsp is false"
        );
    }
}
