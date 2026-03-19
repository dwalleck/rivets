//! Cross-file reference resolution (Pass 2) and LSP-based resolution (Pass 3).
//!
//! This module contains the resolution methods on [`Tethys`] that handle:
//! - Import-based cross-file symbol resolution
//! - LSP `goto_definition` resolution
//! - LSP `find_references` for caller discovery

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tracing::{debug, info, trace, warn};

use crate::Tethys;
use crate::error::{Error, Result};
use crate::graph::{self, SymbolGraphOps};
use crate::lsp::{self, LspProvider};
use crate::resolver::resolve_module_path;
use crate::types::{
    Dependent, FileId, Import, Language, Reference, ReferenceKind, Symbol, SymbolId,
    UnresolvedRefForLsp,
};

/// Per-file context used during cross-file reference resolution (Pass 2).
///
/// Bundles the import tables and path information that stay constant while
/// resolving every reference within a single file.
pub(crate) struct ResolveContext<'a> {
    pub(crate) explicit_imports: &'a HashMap<&'a str, (&'a str, &'a str)>,
    pub(crate) glob_imports: &'a [&'a str],
    pub(crate) current_file_path: Option<&'a Path>,
    pub(crate) crate_root: &'a Path,
    pub(crate) file_id: FileId,
}

#[expect(
    clippy::missing_errors_doc,
    reason = "error docs deferred to avoid churn during active development"
)]
impl Tethys {
    /// Resolve cross-file references against the symbol database (Pass 2).
    ///
    /// After all files are indexed (Pass 1), this method resolves unresolved
    /// references by matching them to symbols discovered in other files via
    /// the imports table.
    ///
    /// Returns the number of references successfully resolved.
    pub(crate) fn resolve_cross_file_references(&self) -> Result<usize> {
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

        let ctx = ResolveContext {
            explicit_imports: &explicit_imports,
            glob_imports: &glob_imports,
            current_file_path: current_file_path.as_deref(),
            crate_root,
            file_id,
        };

        let mut resolved_count = 0;

        for ref_ in refs {
            let Some(ref_name) = &ref_.reference_name else {
                continue;
            };

            let resolved = self.try_resolve_reference(&ref_, ref_name, &ctx)?;

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
    fn try_resolve_reference(
        &self,
        ref_: &Reference,
        ref_name: &str,
        ctx: &ResolveContext<'_>,
    ) -> Result<bool> {
        let is_qualified = ref_name.contains("::");

        // Try explicit imports
        if let Some(symbol) = self.resolve_via_explicit_import(
            ref_name,
            ctx.explicit_imports,
            ctx.current_file_path,
            ctx.crate_root,
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
        for source_module in ctx.glob_imports {
            if let Some(symbol) = self.resolve_symbol_in_module(
                ref_name,
                source_module,
                ctx.current_file_path,
                ctx.crate_root,
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
            file_id = %ctx.file_id,
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
    #[expect(
        clippy::too_many_lines,
        reason = "LSP resolution involves multiple sequential protocol steps"
    )]
    pub(crate) fn resolve_via_lsp(
        &self,
        provider: &dyn lsp::LspProvider,
        language: Language,
        lsp_timeout_secs: u64,
    ) -> Result<(usize, Vec<String>)> {
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
            return Ok((0, vec![]));
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
                let error_msg = format!(
                    "LSP server for {language:?} failed to start: {e}. {}",
                    provider.install_hint()
                );
                tracing::error!(
                    error = %e,
                    language = ?language,
                    install_hint = %provider.install_hint(),
                    "LSP server failed to start - LSP resolution skipped. \
                     {} Or remove --lsp flag.",
                    provider.install_hint()
                );
                return Ok((0, vec![error_msg]));
            }
        };

        let mut resolved_count = 0;
        let mut lsp_errors = 0;
        let mut opened_files: HashSet<PathBuf> = HashSet::new();
        let mut did_open_warned = false;

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
                Ok(content) => match client.did_open(file_path, &content, language_id) {
                    Ok(()) => {
                        opened_files.insert(file_path.clone());
                    }
                    Err(e) => {
                        if did_open_warned {
                            trace!(
                                file = %file_path.display(),
                                error = %e,
                                "Failed to send didOpen notification"
                            );
                        } else {
                            warn!(
                                file = %file_path.display(),
                                error = %e,
                                "Failed to send didOpen notification"
                            );
                            did_open_warned = true;
                        }
                    }
                },
                Err(e) => {
                    if did_open_warned {
                        trace!(
                            file = %file_path.display(),
                            error = %e,
                            "Failed to read file for LSP pre-opening"
                        );
                    } else {
                        warn!(
                            file = %file_path.display(),
                            error = %e,
                            "Failed to read file for LSP pre-opening"
                        );
                        did_open_warned = true;
                    }
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
                &mut did_open_warned,
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

        Ok((resolved_count, vec![]))
    }

    /// Attempt to resolve a single reference via LSP.
    ///
    /// Returns `Ok(true)` if resolved, `Ok(false)` if not resolved (but no error),
    /// or `Err` for database errors.
    #[expect(
        clippy::too_many_lines,
        reason = "didOpen warn/trace branching adds lines but keeps logic cohesive"
    )]
    fn resolve_single_ref_via_lsp(
        &self,
        client: &mut lsp::LspClient,
        unresolved_ref: &UnresolvedRefForLsp,
        lsp_errors: &mut usize,
        opened_files: &mut HashSet<PathBuf>,
        language_id: &str,
        did_open_warned: &mut bool,
    ) -> Result<bool> {
        // Construct absolute file path for LSP
        let file_path = self.workspace_root.join(&unresolved_ref.file_path);

        // Ensure the file is opened in the LSP server (required by some servers like csharp-ls)
        if !opened_files.contains(&file_path) {
            // Read file content
            let content = match std::fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(e) => {
                    if *did_open_warned {
                        trace!(
                            file = %file_path.display(),
                            error = %e,
                            "Failed to read file for LSP didOpen"
                        );
                    } else {
                        warn!(
                            file = %file_path.display(),
                            error = %e,
                            "Failed to read file for LSP didOpen"
                        );
                        *did_open_warned = true;
                    }
                    return Ok(false);
                }
            };

            // Send didOpen notification
            if let Err(e) = client.did_open(&file_path, &content, language_id) {
                if *did_open_warned {
                    trace!(
                        file = %file_path.display(),
                        error = %e,
                        "Failed to send didOpen notification"
                    );
                } else {
                    warn!(
                        file = %file_path.display(),
                        error = %e,
                        "Failed to send didOpen notification"
                    );
                    *did_open_warned = true;
                }
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
    pub(crate) fn uri_to_path(uri: &str) -> Option<PathBuf> {
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
    #[expect(
        clippy::too_many_lines,
        reason = "combines DB lookup with LSP refinement in a single user-facing method"
    )]
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
                // User explicitly requested --lsp, so log at warn level.
                // Results will be incomplete: only tree-sitter-indexed callers are returned.
                warn!(
                    error = %e,
                    symbol = %qualified_name,
                    language = ?symbol_file.language,
                    install_hint = %provider.install_hint(),
                    "LSP server failed to start — returning DB-only callers \
                     (results may be incomplete). {} Or remove --lsp flag.",
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

    /// Convert a list of `CallerInfo` to `Dependent` for the crate-internal API.
    pub(crate) fn convert_callers_to_dependents(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileId, Import};

    // ========================================================================
    // build_import_maps Tests
    // ========================================================================

    #[test]
    fn build_import_maps_empty_imports() {
        let (explicit, glob) = Tethys::build_import_maps(&[]);
        assert!(explicit.is_empty());
        assert!(glob.is_empty());
    }

    #[test]
    fn build_import_maps_explicit_import() {
        let imports = vec![Import {
            file_id: FileId::from(1),
            symbol_name: "HashMap".to_string(),
            source_module: "std::collections".to_string(),
            alias: None,
        }];

        let (explicit, glob) = Tethys::build_import_maps(&imports);

        assert!(glob.is_empty());
        assert_eq!(explicit.len(), 1);
        let (symbol_name, source_module) = explicit["HashMap"];
        assert_eq!(symbol_name, "HashMap");
        assert_eq!(source_module, "std::collections");
    }

    #[test]
    fn build_import_maps_glob_import() {
        let imports = vec![Import {
            file_id: FileId::from(1),
            symbol_name: "*".to_string(),
            source_module: "crate::prelude".to_string(),
            alias: None,
        }];

        let (explicit, glob) = Tethys::build_import_maps(&imports);

        assert!(explicit.is_empty());
        assert_eq!(glob, vec!["crate::prelude"]);
    }

    #[test]
    fn build_import_maps_aliased_import_uses_alias_as_key() {
        let imports = vec![Import {
            file_id: FileId::from(1),
            symbol_name: "HashMap".to_string(),
            source_module: "std::collections".to_string(),
            alias: Some("Map".to_string()),
        }];

        let (explicit, _) = Tethys::build_import_maps(&imports);

        assert!(!explicit.contains_key("HashMap"));
        assert_eq!(explicit.len(), 1);
        let (symbol_name, source_module) = explicit["Map"];
        assert_eq!(symbol_name, "HashMap");
        assert_eq!(source_module, "std::collections");
    }

    #[test]
    fn build_import_maps_mixed_explicit_and_glob() {
        let imports = vec![
            Import {
                file_id: FileId::from(1),
                symbol_name: "Index".to_string(),
                source_module: "crate::db".to_string(),
                alias: None,
            },
            Import {
                file_id: FileId::from(1),
                symbol_name: "*".to_string(),
                source_module: "crate::prelude".to_string(),
                alias: None,
            },
            Import {
                file_id: FileId::from(1),
                symbol_name: "Error".to_string(),
                source_module: "crate::error".to_string(),
                alias: None,
            },
        ];

        let (explicit, glob) = Tethys::build_import_maps(&imports);

        assert_eq!(explicit.len(), 2);
        assert!(explicit.contains_key("Index"));
        assert!(explicit.contains_key("Error"));
        assert_eq!(glob, vec!["crate::prelude"]);
    }

    #[test]
    fn build_import_maps_duplicate_name_keeps_last() {
        let imports = vec![
            Import {
                file_id: FileId::from(1),
                symbol_name: "Error".to_string(),
                source_module: "std::io".to_string(),
                alias: None,
            },
            Import {
                file_id: FileId::from(1),
                symbol_name: "Error".to_string(),
                source_module: "crate::error".to_string(),
                alias: None,
            },
        ];

        let (explicit, _) = Tethys::build_import_maps(&imports);

        assert_eq!(explicit.len(), 1);
        let (_, source_module) = explicit["Error"];
        assert_eq!(source_module, "crate::error");
    }
}
