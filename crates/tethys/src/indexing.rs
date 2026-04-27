//! Indexing pipeline: file discovery, parsing, and database population.
//!
//! This module contains the core indexing methods on [`Tethys`] that handle:
//! - File discovery and workspace scanning
//! - Parallel parsing with tree-sitter
//! - Database writes (both batch and streaming modes)
//! - File-level dependency computation
//! - C# namespace resolution
//! - Pending dependency resolution passes

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, UNIX_EPOCH};

use rayon::prelude::*;
use tracing::{debug, info, trace, warn};

use crate::Tethys;
use crate::batch_writer::BatchWriter;
use crate::db::{InsertReferenceParams, SymbolData};
use crate::error::{Error, IndexError, IndexErrorKind, Result};
use crate::languages::{self, common};
use crate::lsp;
use crate::parallel::{OwnedSymbolData, ParsedFileData};
use crate::resolver::resolve_module_path;
use crate::types::{
    FileId, Import, IndexOptions, IndexStats, Language, Span, SymbolId, SymbolKind,
};

/// A dependency that couldn't be resolved because the target file wasn't indexed yet.
///
/// These are collected during the first indexing pass and resolved in subsequent passes.
#[derive(Debug)]
pub(crate) struct PendingDependency {
    /// The file ID that has the dependency.
    pub(crate) from_file_id: FileId,
    /// The path to the file being depended on (relative to workspace root).
    pub(crate) dep_path: PathBuf,
}

impl PendingDependency {
    /// Asserts struct invariants in debug builds.
    ///
    /// - `dep_path` must not be absolute (it should be relative to workspace root).
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_valid(&self) {
        debug_assert!(
            !self.dep_path.is_absolute(),
            "dep_path should not be absolute: {}",
            self.dep_path.display()
        );
    }

    /// No-op in release builds.
    #[cfg(not(debug_assertions))]
    #[inline]
    pub(crate) fn debug_assert_valid(&self) {}
}

#[expect(
    clippy::missing_errors_doc,
    reason = "error docs deferred to avoid churn during active development"
)]
impl Tethys {
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
    /// println!("Resolved {} references via LSP", stats.total_lsp_resolved());
    /// # Ok::<(), tethys::Error>(())
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "orchestration method with sequential indexing phases"
    )]
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
                        let kind = IndexErrorKind::from(&e);
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

            if write_result.stats.files_failed > 0 {
                warn!(
                    files_failed = write_result.stats.files_failed,
                    files_written = write_result.stats.files_written,
                    "Some files failed to write to database during streaming indexing"
                );
            }

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
                            let kind = IndexErrorKind::from(&e);
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
                        let kind = IndexErrorKind::from(&e);
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
        let lsp_sessions = if options.use_lsp() {
            let mut sessions = Vec::new();

            let rust_provider = lsp::AnyProvider::for_language(Language::Rust);
            let rust_result =
                self.resolve_via_lsp(&rust_provider, Language::Rust, options.lsp_timeout_secs())?;
            if rust_result.has_resolutions() {
                tracing::info!(
                    language = "rust",
                    resolved_count = rust_result.resolved_count(),
                    "Resolved references via LSP"
                );
            }
            sessions.push(rust_result);

            let csharp_provider = lsp::AnyProvider::for_language(Language::CSharp);
            let csharp_result = self.resolve_via_lsp(
                &csharp_provider,
                Language::CSharp,
                options.lsp_timeout_secs(),
            )?;
            if csharp_result.has_resolutions() {
                tracing::info!(
                    language = "csharp",
                    resolved_count = csharp_result.resolved_count(),
                    "Resolved references via LSP"
                );
            }
            sessions.push(csharp_result);

            sessions
        } else {
            Vec::new()
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

        // Update query planner statistics after bulk writes
        self.db.analyze()?;

        Ok(IndexStats {
            files_indexed,
            symbols_found,
            references_found,
            duration: start.elapsed(),
            files_skipped,
            directories_skipped,
            errors,
            unresolved_dependencies,
            lsp_sessions,
        })
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
    pub(crate) fn parse_file_static(
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

        let lang_support = languages::get_language_support(language);

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
        #[expect(
            clippy::cast_possible_truncation,
            reason = "nanosecond timestamp fits in i64 until year 2262"
        )]
        let mtime_ns = match metadata.modified() {
            Ok(mtime) => match mtime.duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos() as i64,
                Err(e) => {
                    debug!(path = %file_path.display(), error = %e, "File modification time before Unix epoch, using 0");
                    0
                }
            },
            Err(e) => {
                debug!(path = %file_path.display(), error = %e, "Cannot read file modification time, using 0");
                0
            }
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
                let owned = OwnedSymbolData {
                    name: sym.name.clone(),
                    module_path: String::new(),
                    qualified_name,
                    kind: sym.kind,
                    line: sym.line,
                    column: sym.column,
                    span: sym.span,
                    signature: sym.signature.clone(),
                    visibility: sym.visibility,
                    parent_symbol_id: None,
                    is_test: sym.is_test,
                    attributes: sym.attributes.clone(),
                };
                owned.debug_assert_valid();
                owned
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

        let parsed = ParsedFileData {
            relative_path,
            language,
            mtime_ns,
            size_bytes,
            symbols,
            references,
            imports,
        };
        parsed.debug_assert_valid();
        Ok(parsed)
    }

    /// Write a single parsed file to the database and compute its dependencies.
    ///
    /// This is Phase 1b of indexing - the sequential database write that must
    /// happen after parallel parsing.
    pub(crate) fn write_parsed_file(
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
            .map(|s| {
                let mut sd = s.as_symbol_data();
                sd.module_path = &module_path;
                sd
            })
            .collect();

        // Insert file and symbols atomically
        let (file_id, symbol_ids) = self.db.index_file_atomic(
            &data.relative_path,
            data.language,
            data.mtime_ns,
            data.size_bytes,
            None,
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

            self.db.insert_reference(&InsertReferenceParams {
                symbol_id,
                file_id,
                kind: r.kind.to_db_kind().as_str(),
                line: r.line,
                column: r.column,
                in_symbol_id,
                reference_name: reference_name.as_deref(),
            })?;
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
    pub(crate) fn build_qualified_name(name: &str, path: Option<&[String]>) -> String {
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
            if let Some(path) = &r.path
                && let Some(first) = path.first()
            {
                referenced_names.insert(first);
            }
        }

        let crate_root = self.workspace_root.join("src");

        // Track which files we depend on (dedupe)
        let mut depended_files: HashSet<PathBuf> = HashSet::new();

        for import_stmt in imports {
            // Skip glob imports - can't determine what's used
            if import_stmt.is_glob {
                continue;
            }

            // Check if any imported name from this import statement is actually referenced
            let is_used = import_stmt.imported_names.iter().any(|name| {
                let lookup_name = import_stmt.alias.as_ref().unwrap_or(name);
                referenced_names.contains(lookup_name.as_str())
            });

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
            if let Some(dep_file_id) = self.db.get_file_id(&dep_path)? {
                self.db.insert_file_dependency(file_id, dep_file_id)?;
            } else {
                // Target file not indexed yet - queue for resolution pass
                let dep = PendingDependency {
                    from_file_id: file_id,
                    dep_path,
                };
                dep.debug_assert_valid();
                pending.push(dep);
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
            let is_used = import_stmt.imported_names.iter().any(|name| {
                let lookup_name = import_stmt.alias.as_ref().unwrap_or(name);
                refs_set.contains(lookup_name.as_str())
            });

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
            if let Some(dep_file_id) = self.db.get_file_id(&dep_path)? {
                self.db.insert_file_dependency(file_id, dep_file_id)?;
            } else {
                let dep = PendingDependency {
                    from_file_id: file_id,
                    dep_path,
                };
                dep.debug_assert_valid();
                pending.push(dep);
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
        let lang_support = languages::get_language_support(Language::CSharp);

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
    pub(crate) fn discover_files(
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
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && (name.starts_with('.') || Self::is_excluded_dir(name))
            {
                continue;
            }

            if path.is_dir() {
                Self::walk_dir(&path, files, directories_skipped)?;
            } else if path.is_file() {
                // Check if it's a supported file type
                if let Some(ext) = path.extension().and_then(|e| e.to_str())
                    && Language::from_extension(ext).is_some()
                {
                    files.push(path);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileId, Import, Language};

    // ========================================================================
    // is_excluded_dir Tests
    // ========================================================================

    #[test]
    fn is_excluded_dir_excludes_target() {
        assert!(Tethys::is_excluded_dir("target"));
    }

    #[test]
    fn is_excluded_dir_excludes_node_modules() {
        assert!(Tethys::is_excluded_dir("node_modules"));
    }

    #[test]
    fn is_excluded_dir_does_not_match_dot_git() {
        // .git is NOT in the exclusion list — it's handled separately by
        // the hidden-directory filter (starts with '.'). Verify it is not
        // matched here so the two filters stay orthogonal.
        assert!(!Tethys::is_excluded_dir(".git"));
    }

    #[test]
    fn is_excluded_dir_excludes_bin() {
        assert!(Tethys::is_excluded_dir("bin"));
    }

    #[test]
    fn is_excluded_dir_excludes_obj() {
        assert!(Tethys::is_excluded_dir("obj"));
    }

    #[test]
    fn is_excluded_dir_excludes_build() {
        assert!(Tethys::is_excluded_dir("build"));
    }

    #[test]
    fn is_excluded_dir_excludes_dist() {
        assert!(Tethys::is_excluded_dir("dist"));
    }

    #[test]
    fn is_excluded_dir_excludes_vendor() {
        assert!(Tethys::is_excluded_dir("vendor"));
    }

    #[test]
    fn is_excluded_dir_excludes_pycache() {
        assert!(Tethys::is_excluded_dir("__pycache__"));
    }

    #[test]
    fn is_excluded_dir_allows_src() {
        assert!(!Tethys::is_excluded_dir("src"));
    }

    #[test]
    fn is_excluded_dir_allows_lib() {
        assert!(!Tethys::is_excluded_dir("lib"));
    }

    #[test]
    fn is_excluded_dir_allows_tests() {
        assert!(!Tethys::is_excluded_dir("tests"));
    }

    #[test]
    fn is_excluded_dir_allows_my_module() {
        assert!(!Tethys::is_excluded_dir("my_module"));
    }

    #[test]
    fn is_excluded_dir_is_case_sensitive() {
        assert!(!Tethys::is_excluded_dir("Target"));
        assert!(!Tethys::is_excluded_dir("NODE_MODULES"));
        assert!(!Tethys::is_excluded_dir("Vendor"));
    }

    #[test]
    fn is_excluded_dir_rejects_empty_string() {
        assert!(!Tethys::is_excluded_dir(""));
    }

    // ========================================================================
    // convert_imports_to_statements Tests
    // ========================================================================

    #[test]
    fn convert_imports_to_statements_empty_imports() {
        let result = Tethys::convert_imports_to_statements(&[], Language::Rust);
        assert!(result.is_empty());
    }

    #[test]
    fn convert_imports_to_statements_single_rust_import() {
        let imports = vec![Import {
            file_id: FileId::from(1),
            symbol_name: "HashMap".to_string(),
            source_module: "std::collections".to_string(),
            alias: None,
        }];

        let result = Tethys::convert_imports_to_statements(&imports, Language::Rust);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, vec!["std", "collections"]);
        assert_eq!(result[0].imported_names, vec!["HashMap"]);
        assert!(!result[0].is_glob);
        assert!(result[0].alias.is_none());
    }

    #[test]
    fn convert_imports_to_statements_glob_import() {
        let imports = vec![Import {
            file_id: FileId::from(1),
            symbol_name: "*".to_string(),
            source_module: "crate::prelude".to_string(),
            alias: None,
        }];

        let result = Tethys::convert_imports_to_statements(&imports, Language::Rust);

        assert_eq!(result.len(), 1);
        assert!(result[0].is_glob);
        assert!(result[0].imported_names.is_empty());
    }

    #[test]
    fn convert_imports_to_statements_groups_by_source_module() {
        let imports = vec![
            Import {
                file_id: FileId::from(1),
                symbol_name: "HashMap".to_string(),
                source_module: "std::collections".to_string(),
                alias: None,
            },
            Import {
                file_id: FileId::from(1),
                symbol_name: "HashSet".to_string(),
                source_module: "std::collections".to_string(),
                alias: None,
            },
        ];

        let result = Tethys::convert_imports_to_statements(&imports, Language::Rust);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, vec!["std", "collections"]);
        let mut names = result[0].imported_names.clone();
        names.sort();
        assert_eq!(names, vec!["HashMap", "HashSet"]);
    }

    #[test]
    fn convert_imports_to_statements_csharp_uses_dot_separator() {
        let imports = vec![Import {
            file_id: FileId::from(1),
            symbol_name: "List".to_string(),
            source_module: "System.Collections.Generic".to_string(),
            alias: None,
        }];

        let result = Tethys::convert_imports_to_statements(&imports, Language::CSharp);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, vec!["System", "Collections", "Generic"]);
    }

    #[test]
    fn convert_imports_to_statements_preserves_alias() {
        let imports = vec![Import {
            file_id: FileId::from(1),
            symbol_name: "HashMap".to_string(),
            source_module: "std::collections".to_string(),
            alias: Some("Map".to_string()),
        }];

        let result = Tethys::convert_imports_to_statements(&imports, Language::Rust);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, Some("Map".to_string()));
    }
}
