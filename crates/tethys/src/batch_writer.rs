//! Streaming `SQLite` writer for parallel file indexing.
//!
//! This module provides [`BatchWriter`], a background writer that receives parsed
//! file data over an MPSC channel and writes it to `SQLite` in batches. This reduces
//! memory usage from O(n) to `O(batch_size)` during indexing of large codebases.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     index_with_options                          │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Main Thread          │  Background Writer Thread               │
//! │  ──────────────       │  ────────────────────────               │
//! │  rayon::par_iter()    │  recv() from channel                    │
//! │  parse files          │  accumulate until batch_size            │
//! │  send to channel ─────┼→ write batch (file-level transactions)  │
//! │  ...                  │  log errors, continue on failure        │
//! │  drop sender          │  return WriteStats                      │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! let batch_writer = BatchWriter::new(db, 100)?;
//!
//! source_files.par_iter().for_each(|(path, lang)| {
//!     if let Ok(data) = Tethys::parse_file_static(&workspace_root, path, *lang) {
//!         let _ = batch_writer.send(data);
//!     }
//! });
//!
//! let result = batch_writer.finish()?;
//! // result.stats contains write statistics
//! // Dependencies are computed after all files are written
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use tracing::{debug, error, trace, warn};

use crate::db::{Index, SymbolData};
use crate::error::{Error, Result};
use crate::languages::common::{ExtractedReference, ImportStatement};
use crate::parallel::OwnedSymbolData;
use crate::parallel::ParsedFileData;
use crate::types::{FileId, Language, Span, SymbolId};

/// Statistics about the batch writing process.
#[derive(Debug, Default, Clone)]
pub struct WriteStats {
    /// Number of files successfully written to the database.
    pub files_written: usize,
    /// Number of symbols written.
    pub symbols_written: usize,
    /// Number of references written.
    pub references_written: usize,
    /// Number of batches committed (transactions).
    pub batches_committed: usize,
}

/// Result returned when the batch writer finishes.
#[derive(Debug)]
pub struct BatchWriteResult {
    /// Statistics about what was written.
    pub stats: WriteStats,
}

/// A background writer that receives parsed file data and writes it to `SQLite`
/// in batches.
///
/// This struct owns the sending end of an MPSC channel. Parsed files are sent
/// via [`send()`](Self::send) and accumulated in the background thread until
/// [`batch_size`](Self::new) files are collected, at which point they're written
/// in a single `SQLite` transaction.
///
/// When [`finish()`](Self::finish) is called, the sender is dropped, the
/// background thread completes any remaining writes, and the final statistics
/// are returned.
pub struct BatchWriter {
    /// Channel sender for parsed file data.
    sender: Sender<ParsedFileData>,
    /// Handle to the background writer thread.
    handle: JoinHandle<Result<BatchWriteResult>>,
}

impl BatchWriter {
    /// Create a new batch writer with the given database and batch size.
    ///
    /// # Arguments
    /// * `db_path` - Path to the `SQLite` database file
    /// * `batch_size` - Number of files to accumulate before committing a transaction
    ///
    /// # Panics
    /// Panics if `batch_size` is 0 (would cause infinite accumulation without writes).
    #[must_use]
    pub fn new(db_path: PathBuf, batch_size: usize) -> Self {
        assert!(batch_size > 0, "batch_size must be at least 1");

        let (sender, receiver) = mpsc::channel();

        let handle = thread::spawn(move || Self::writer_thread(db_path, receiver, batch_size));

        Self { sender, handle }
    }

    /// Send parsed file data to the background writer.
    ///
    /// This is non-blocking. If the channel is disconnected (background thread
    /// panicked), the data is silently dropped and an error is logged.
    ///
    /// # Arguments
    /// * `data` - The parsed file data to write
    pub fn send(&self, data: ParsedFileData) {
        if let Err(e) = self.sender.send(data) {
            error!(
                file = %e.0.relative_path.display(),
                "Failed to send to batch writer (receiver disconnected)"
            );
        }
    }

    /// Finish writing and return the final statistics.
    ///
    /// This drops the sender, causing the background thread to complete any
    /// remaining batch and return. Blocks until the background thread finishes.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The background thread panicked
    /// - A database write failed
    pub fn finish(self) -> Result<BatchWriteResult> {
        // Drop sender to signal the background thread to finish
        drop(self.sender);

        // Wait for the background thread
        match self.handle.join() {
            Ok(result) => result,
            Err(panic_payload) => {
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    format!("Batch writer thread panicked: {s}")
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    format!("Batch writer thread panicked: {s}")
                } else {
                    "Batch writer thread panicked with unknown payload".to_string()
                };
                error!(panic_msg = %msg, "Background batch writer thread panicked");
                Err(Error::Internal(msg))
            }
        }
    }

    /// Background thread function that receives and writes file data.
    #[allow(clippy::needless_pass_by_value)] // Receiver is consumed by loop, PathBuf owned by thread
    fn writer_thread(
        db_path: PathBuf,
        receiver: Receiver<ParsedFileData>,
        batch_size: usize,
    ) -> Result<BatchWriteResult> {
        let mut db = Index::open(&db_path)?;
        let mut stats = WriteStats::default();
        let mut batch: Vec<ParsedFileData> = Vec::with_capacity(batch_size);

        loop {
            if let Ok(data) = receiver.recv() {
                batch.push(data);

                if batch.len() >= batch_size {
                    Self::write_batch(&mut db, &mut batch, &mut stats);
                }
            } else {
                // Channel closed, write any remaining files
                if !batch.is_empty() {
                    Self::write_batch(&mut db, &mut batch, &mut stats);
                }
                break;
            }
        }

        debug!(
            files = stats.files_written,
            symbols = stats.symbols_written,
            references = stats.references_written,
            batches = stats.batches_committed,
            "Batch writer finished"
        );

        Ok(BatchWriteResult { stats })
    }

    /// Write a batch of files.
    fn write_batch(db: &mut Index, batch: &mut Vec<ParsedFileData>, stats: &mut WriteStats) {
        trace!(batch_size = batch.len(), "Writing batch");

        // Note: Each file is written atomically via index_file_atomic, which uses
        // its own transaction. For true batch transactions, we would need to
        // modify the Index API. For now, we batch at the file level to reduce
        // channel overhead while maintaining file-level atomicity.
        for data in batch.drain(..) {
            match Self::write_single_file(db, &data) {
                Ok((sym_count, ref_count)) => {
                    stats.files_written += 1;
                    stats.symbols_written += sym_count;
                    stats.references_written += ref_count;
                }
                Err(e) => {
                    // Log but continue - we don't want one bad file to stop everything
                    warn!(
                        file = %data.relative_path.display(),
                        error = %e,
                        "Failed to write file to database"
                    );
                }
            }
        }

        stats.batches_committed += 1;
    }

    /// Write a single file to the database.
    ///
    /// Writes file record, symbols, references, and imports. Does NOT compute
    /// file-level dependencies - that requires access to Tethys state (workspace
    /// root, module path resolution) and is done after all files are written.
    fn write_single_file(db: &mut Index, data: &ParsedFileData) -> Result<(usize, usize)> {
        // Convert owned symbols to borrowed for insertion
        let symbol_data: Vec<SymbolData<'_>> =
            data.symbols.iter().map(|s| s.as_symbol_data()).collect();

        // Insert file and symbols atomically, capturing generated symbol IDs
        let (file_id, symbol_ids) = db.index_file_atomic(
            &data.relative_path,
            data.language,
            data.mtime_ns,
            data.size_bytes,
            data.content_hash,
            &symbol_data,
        )?;

        // Build lookup maps directly from input data + generated IDs
        let (name_to_id, span_to_id) = build_symbol_maps_from_insert(&data.symbols, &symbol_ids);

        // Store references
        let refs_stored =
            store_references(db, file_id, &data.references, &name_to_id, &span_to_id)?;

        // Store imports
        store_imports(db, file_id, &data.imports, data.language)?;

        Ok((data.symbols.len(), refs_stored))
    }
}

// === Helper functions for write_single_file ===
// These mirror the Tethys methods but operate on raw Index

/// Build lookup maps from inserted symbols and their generated IDs.
///
/// Pairs each `OwnedSymbolData` with its corresponding `SymbolId` from the
/// database insert, avoiding a round-trip query to read symbols back.
fn build_symbol_maps_from_insert(
    symbols: &[OwnedSymbolData],
    symbol_ids: &[SymbolId],
) -> (HashMap<String, SymbolId>, HashMap<Span, SymbolId>) {
    let mut name_to_id: HashMap<String, SymbolId> = HashMap::new();
    let mut span_to_id: HashMap<Span, SymbolId> = HashMap::new();

    for (sym, &id) in symbols.iter().zip(symbol_ids) {
        if let Some(_prev_id) = name_to_id.insert(sym.name.clone(), id) {
            trace!(
                name = %sym.name,
                id = %id,
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
fn store_references(
    db: &Index,
    file_id: FileId,
    refs: &[ExtractedReference],
    name_to_id: &HashMap<String, SymbolId>,
    span_to_id: &HashMap<Span, SymbolId>,
) -> Result<usize> {
    let mut count = 0;

    for r in refs {
        let qualified_name = build_qualified_name(&r.name, r.path.as_deref());

        // Try same-file resolution: simple name first, then qualified name
        let symbol_id = name_to_id
            .get(&r.name)
            .or_else(|| name_to_id.get(&qualified_name))
            .copied();

        // For unresolved references, store the name for Pass 2 cross-file resolution
        let reference_name = if symbol_id.is_none() {
            Some(qualified_name.clone())
        } else {
            None
        };

        // Find containing symbol via containing_symbol_span
        let in_symbol_id = r
            .containing_symbol_span
            .and_then(|span| span_to_id.get(&span).copied());

        db.insert_reference(
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
/// Matches the behavior of `Tethys::build_qualified_name`.
fn build_qualified_name(name: &str, path: Option<&[String]>) -> String {
    match path {
        Some(segments) if !segments.is_empty() => {
            format!("{}::{}", segments.join("::"), name)
        }
        _ => name.to_string(),
    }
}

/// Store imports in the database.
///
/// Matches the behavior of `Tethys::store_imports`.
fn store_imports(
    db: &Index,
    file_id: FileId,
    imports: &[ImportStatement],
    language: Language,
) -> Result<()> {
    db.clear_imports_for_file(file_id)?;

    // Determine path separator based on language
    let separator = match language {
        Language::Rust => "::",
        Language::CSharp => ".",
    };

    for import in imports {
        let source = import.path.join(separator);

        // Handle glob imports
        if import.is_glob {
            db.insert_import(file_id, "*", &source, import.alias.as_deref())?;
            continue;
        }

        // For explicit imports: store each imported name
        if import.imported_names.is_empty() {
            // Namespace/module import (C# style) or module import without braces
            // Store with "*" to indicate "all from this module"
            db.insert_import(file_id, "*", &source, import.alias.as_deref())?;
        } else {
            // Store each explicitly imported name
            for name in &import.imported_names {
                db.insert_import(file_id, name, &source, import.alias.as_deref())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel::OwnedSymbolData;
    use crate::types::{SymbolKind, Visibility};
    use tempfile::TempDir;

    fn temp_db_path() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("test.db");
        (dir, path)
    }

    #[test]
    fn batch_writer_writes_single_file() {
        let (_dir, db_path) = temp_db_path();

        let writer = BatchWriter::new(db_path.clone(), 10);

        let data = ParsedFileData::new(
            PathBuf::from("src/main.rs"),
            Language::Rust,
            1_234_567_890,
            100,
            Some(0x1234_5678_9abc_def0),
            vec![OwnedSymbolData::new(
                "main".to_string(),
                "crate".to_string(),
                "crate::main".to_string(),
                SymbolKind::Function,
                1,
                0,
                None,
                Some("fn main()".to_string()),
                Visibility::Public,
                None,
                false,
            )],
            vec![],
            vec![],
        );

        writer.send(data);

        let result = writer.finish().expect("finish");

        assert_eq!(result.stats.files_written, 1);
        assert_eq!(result.stats.symbols_written, 1);
        assert_eq!(result.stats.batches_committed, 1);
    }

    #[test]
    fn batch_writer_respects_batch_size() {
        let (_dir, db_path) = temp_db_path();

        // Batch size of 3
        let writer = BatchWriter::new(db_path.clone(), 3);

        // Send 7 files - should result in 3 batches (3 + 3 + 1)
        for i in 0u32..7 {
            let data = ParsedFileData::new(
                PathBuf::from(format!("src/file{i}.rs")),
                Language::Rust,
                1_234_567_890 + i64::from(i),
                100,
                Some(u64::from(i)),
                vec![],
                vec![],
                vec![],
            );
            writer.send(data);
        }

        let result = writer.finish().expect("finish");

        assert_eq!(result.stats.files_written, 7);
        assert_eq!(result.stats.batches_committed, 3);
    }

    #[test]
    fn batch_writer_handles_empty_input() {
        let (_dir, db_path) = temp_db_path();

        let writer = BatchWriter::new(db_path.clone(), 10);

        // Don't send any files
        let result = writer.finish().expect("finish");

        assert_eq!(result.stats.files_written, 0);
        assert_eq!(result.stats.batches_committed, 0);
    }

    #[test]
    fn write_stats_default() {
        let stats = WriteStats::default();
        assert_eq!(stats.files_written, 0);
        assert_eq!(stats.symbols_written, 0);
        assert_eq!(stats.references_written, 0);
        assert_eq!(stats.batches_committed, 0);
    }

    #[test]
    fn build_qualified_name_with_path() {
        assert_eq!(
            build_qualified_name("foo", Some(&["bar".to_string(), "baz".to_string()])),
            "bar::baz::foo"
        );
        assert_eq!(build_qualified_name("foo", Some(&[])), "foo");
        assert_eq!(build_qualified_name("foo", None), "foo");
    }
}
