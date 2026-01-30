//! Helper functions for database row conversion and parsing.
//!
//! These utilities convert between database representations and domain types.
//! Also provides SQL column list constants to reduce duplication across query modules.

use std::path::PathBuf;

use crate::types::{
    FileId, Import, IndexedFile, Language, Reference, ReferenceKind, Span, Symbol, SymbolId,
    SymbolKind, Visibility,
};

/// SQL column list for files table.
///
/// Use with `row_to_indexed_file` for consistent column ordering.
pub(crate) const FILES_COLUMNS: &str =
    "id, path, language, mtime_ns, size_bytes, content_hash, indexed_at";

/// SQL column list for symbols table.
///
/// Use with `row_to_symbol` for consistent column ordering.
pub(crate) const SYMBOLS_COLUMNS: &str =
    "id, file_id, name, module_path, qualified_name, kind, line, column, \
     end_line, end_column, signature, visibility, parent_symbol_id, is_test";

/// SQL column list for refs table.
///
/// Use with `row_to_reference` for consistent column ordering.
pub(crate) const REFS_COLUMNS: &str =
    "id, symbol_id, file_id, kind, line, column, end_line, end_column, in_symbol_id, reference_name";

/// Parse a language string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
pub(crate) fn parse_language(s: &str) -> rusqlite::Result<Language> {
    match s {
        "rust" => Ok(Language::Rust),
        "csharp" => Ok(Language::CSharp),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown language '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Parse a symbol kind string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
pub(crate) fn parse_symbol_kind(s: &str) -> rusqlite::Result<SymbolKind> {
    match s {
        "function" => Ok(SymbolKind::Function),
        "method" => Ok(SymbolKind::Method),
        "struct" => Ok(SymbolKind::Struct),
        "class" => Ok(SymbolKind::Class),
        "enum" => Ok(SymbolKind::Enum),
        "trait" => Ok(SymbolKind::Trait),
        "interface" => Ok(SymbolKind::Interface),
        "const" => Ok(SymbolKind::Const),
        "static" => Ok(SymbolKind::Static),
        "module" => Ok(SymbolKind::Module),
        "type_alias" => Ok(SymbolKind::TypeAlias),
        "macro" => Ok(SymbolKind::Macro),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown symbol kind '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Parse a visibility string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
pub(crate) fn parse_visibility(s: &str) -> rusqlite::Result<Visibility> {
    match s {
        "public" => Ok(Visibility::Public),
        "crate" => Ok(Visibility::Crate),
        "module" => Ok(Visibility::Module),
        "private" => Ok(Visibility::Private),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown visibility '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Parse a reference kind string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
pub(crate) fn parse_reference_kind(s: &str) -> rusqlite::Result<ReferenceKind> {
    match s {
        "import" => Ok(ReferenceKind::Import),
        "call" => Ok(ReferenceKind::Call),
        "type" => Ok(ReferenceKind::Type),
        "inherit" => Ok(ReferenceKind::Inherit),
        "construct" => Ok(ReferenceKind::Construct),
        "field_access" => Ok(ReferenceKind::FieldAccess),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown reference kind '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Build a span from start and optional end positions.
///
/// Returns `None` if either `end_line` or `end_column` is missing, or if the
/// span would be invalid (end before start).
pub(crate) fn build_span(
    start_line: u32,
    start_column: u32,
    end_line: Option<u32>,
    end_column: Option<u32>,
) -> Option<Span> {
    end_line
        .zip(end_column)
        .and_then(|(el, ec)| Span::new(start_line, start_column, el, ec))
}

/// Convert a database row to an [`IndexedFile`].
///
/// Expected columns: id, path, language, `mtime_ns`, `size_bytes`, `content_hash`, `indexed_at`
pub(crate) fn row_to_indexed_file(row: &rusqlite::Row) -> rusqlite::Result<IndexedFile> {
    Ok(IndexedFile {
        id: FileId::from(row.get::<_, i64>(0)?),
        path: PathBuf::from(row.get::<_, String>(1)?),
        language: parse_language(row.get::<_, String>(2)?.as_str())?,
        mtime_ns: row.get(3)?,
        size_bytes: row.get::<_, i64>(4)? as u64,
        content_hash: row.get::<_, Option<i64>>(5)?.map(|h| h as u64),
        indexed_at: row.get(6)?,
    })
}

/// Convert a database row to a Symbol.
pub(crate) fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    let line: u32 = row.get(6)?;
    let column: u32 = row.get(7)?;
    let end_line: Option<u32> = row.get(8)?;
    let end_column: Option<u32> = row.get(9)?;

    Ok(Symbol {
        id: SymbolId::from(row.get::<_, i64>(0)?),
        file_id: FileId::from(row.get::<_, i64>(1)?),
        name: row.get(2)?,
        module_path: row.get(3)?,
        qualified_name: row.get(4)?,
        kind: parse_symbol_kind(&row.get::<_, String>(5)?)?,
        line,
        column,
        span: build_span(line, column, end_line, end_column),
        signature: row.get(10)?,
        signature_details: None, // Not persisted to database; populated by parsers only
        visibility: parse_visibility(&row.get::<_, String>(11)?)?,
        parent_symbol_id: row.get::<_, Option<i64>>(12)?.map(SymbolId::from),
        is_test: row.get(13)?,
    })
}

/// Convert a database row to a Reference.
pub(crate) fn row_to_reference(row: &rusqlite::Row) -> rusqlite::Result<Reference> {
    let line: u32 = row.get(4)?;
    let column: u32 = row.get(5)?;
    let end_line: Option<u32> = row.get(6)?;
    let end_column: Option<u32> = row.get(7)?;

    Ok(Reference {
        id: row.get(0)?,
        symbol_id: row.get::<_, Option<i64>>(1)?.map(SymbolId::from),
        file_id: FileId::from(row.get::<_, i64>(2)?),
        kind: parse_reference_kind(&row.get::<_, String>(3)?)?,
        line,
        column,
        span: build_span(line, column, end_line, end_column),
        in_symbol_id: row.get::<_, Option<i64>>(8)?.map(SymbolId::from),
        reference_name: row.get(9)?,
    })
}

/// Convert a database row to an Import.
pub(crate) fn row_to_import(row: &rusqlite::Row) -> rusqlite::Result<Import> {
    Ok(Import {
        file_id: FileId::from(row.get::<_, i64>(0)?),
        symbol_name: row.get(1)?,
        source_module: row.get(2)?,
        alias: row.get(3)?,
    })
}
