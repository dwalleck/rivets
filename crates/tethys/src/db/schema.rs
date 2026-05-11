//! Database schema definition for Tethys.

/// Database schema definition.
pub(crate) const SCHEMA: &str = r"
-- Indexed source files
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    language TEXT NOT NULL,
    mtime_ns INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    content_hash INTEGER,
    indexed_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);

-- Symbol definitions
CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    module_path TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    end_line INTEGER,
    end_column INTEGER,
    signature TEXT,
    visibility TEXT NOT NULL,
    parent_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    is_test INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_module_path ON symbols(module_path);
CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON symbols(qualified_name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);
CREATE INDEX IF NOT EXISTS idx_symbols_is_test ON symbols(is_test) WHERE is_test = 1;

-- References (usages of symbols)
-- symbol_id is NULL for unresolved references (to be resolved in Pass 2)
-- reference_name stores the name for resolution (e.g., Index_open)
CREATE TABLE IF NOT EXISTS refs (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    end_line INTEGER,
    end_column INTEGER,
    in_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    reference_name TEXT
);

CREATE INDEX IF NOT EXISTS idx_refs_symbol ON refs(symbol_id);
CREATE INDEX IF NOT EXISTS idx_refs_file ON refs(file_id);
CREATE INDEX IF NOT EXISTS idx_refs_in_symbol ON refs(in_symbol_id);
CREATE INDEX IF NOT EXISTS idx_refs_unresolved ON refs(symbol_id) WHERE symbol_id IS NULL;

-- File-level dependencies (denormalized for fast queries)
CREATE TABLE IF NOT EXISTS file_deps (
    from_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    to_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    ref_count INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (from_file_id, to_file_id)
);

CREATE INDEX IF NOT EXISTS idx_file_deps_to ON file_deps(to_file_id);

-- Import statements for cross-file reference resolution
CREATE TABLE IF NOT EXISTS imports (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    symbol_name TEXT NOT NULL,      -- e.g. Index or * for globs
    source_module TEXT NOT NULL,    -- e.g. crate::db or MyApp.Services
    alias TEXT,                      -- for use foo as bar
    PRIMARY KEY (file_id, symbol_name, source_module)
);

CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_id);
CREATE INDEX IF NOT EXISTS idx_imports_symbol ON imports(symbol_name);

-- Pre-computed call graph edges (caller -> callee relationships)
-- Populated from refs where both in_symbol_id (caller) and symbol_id (callee) are resolved.
-- Enables efficient indexed lookups for get_callers/get_callees.
CREATE TABLE IF NOT EXISTS call_edges (
    caller_symbol_id INTEGER NOT NULL,
    callee_symbol_id INTEGER NOT NULL,
    call_count INTEGER DEFAULT 1,
    PRIMARY KEY (caller_symbol_id, callee_symbol_id),
    FOREIGN KEY (caller_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE,
    FOREIGN KEY (callee_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_call_edges_callee ON call_edges(callee_symbol_id);
CREATE INDEX IF NOT EXISTS idx_call_edges_caller ON call_edges(caller_symbol_id);

-- Attributes attached to symbols (e.g. #[derive(Clone)], #[source], #[cfg_attr(...)]).
-- args holds the raw text inside the outermost parens with parens stripped, or NULL
-- for marker attributes like #[source]. name holds the attribute path's leading
-- identifier (e.g. 'derive', 'source', 'cfg_attr', 'tauri::command').
CREATE TABLE IF NOT EXISTS attributes (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    args TEXT,
    line INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_attributes_symbol ON attributes(symbol_id);
CREATE INDEX IF NOT EXISTS idx_attributes_name ON attributes(name);

-- === Architecture analysis ===

-- One row per discovered package. v1: only source = 'manifest'.
CREATE TABLE IF NOT EXISTS arch_packages (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE,
    path   TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('manifest','directory'))
);

-- No index on arch_packages(path): every read goes through `id` (FK joins
-- from arch_file_packages and arch_package_deps) or `name` (UNIQUE already
-- has an implicit index). Adding one would be pure write-side overhead.

-- File → package assignment. PK enforces one package per file.
CREATE TABLE IF NOT EXISTS arch_file_packages (
    file_id    INTEGER PRIMARY KEY REFERENCES files(id)         ON DELETE CASCADE,
    package_id INTEGER NOT NULL    REFERENCES arch_packages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_arch_fp_pkg ON arch_file_packages(package_id);

-- Cross-package dependency edges, rolled up from file_deps.
CREATE TABLE IF NOT EXISTS arch_package_deps (
    source_pkg INTEGER NOT NULL REFERENCES arch_packages(id) ON DELETE CASCADE,
    target_pkg INTEGER NOT NULL REFERENCES arch_packages(id) ON DELETE CASCADE,
    dep_count  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (source_pkg, target_pkg),
    CHECK (source_pkg <> target_pkg)
);

CREATE INDEX IF NOT EXISTS idx_arch_pkgdep_tgt ON arch_package_deps(target_pkg);

-- Coupling metrics view. LEFT JOINs keep packages with zero edges visible.
-- Instability is NOT computed here; it is a method on CouplingMetrics in Rust,
-- keeping the formula in a single place.
--
-- Ca/Ce use COUNT(*) over arch_package_deps. Because that table's PRIMARY KEY
-- is (source_pkg, target_pkg), each row is one distinct package-pair, so
-- COUNT(*) ≡ COUNT(DISTINCT source_pkg|target_pkg) here — matching Martin's
-- definition of Ca/Ce as counts of distinct dependent packages.
CREATE VIEW IF NOT EXISTS arch_coupling AS
SELECT
    p.id   AS package_id,
    p.name AS package_name,
    COALESCE(ca.afferent, 0) AS afferent,
    COALESCE(ce.efferent, 0) AS efferent
FROM arch_packages p
LEFT JOIN (
    SELECT target_pkg AS pkg, COUNT(*) AS afferent
    FROM arch_package_deps GROUP BY target_pkg
) ca ON ca.pkg = p.id
LEFT JOIN (
    SELECT source_pkg AS pkg, COUNT(*) AS efferent
    FROM arch_package_deps GROUP BY source_pkg
) ce ON ce.pkg = p.id;
";

#[cfg(test)]
mod schema_tests {
    use super::SCHEMA;
    use rusqlite::Connection;

    fn open_test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory");
        // Match Index::open's pragma setup so FK semantics are uniform in tests.
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("enable fks");
        conn.execute_batch(SCHEMA).expect("apply schema");
        conn
    }

    #[test]
    fn schema_creates_arch_objects() {
        let conn = open_test_conn();

        let count_object = |name: &str, kind: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                rusqlite::params![kind, name],
                |row| row.get::<_, i64>(0),
            )
            .expect("query schema")
        };

        assert_eq!(count_object("arch_packages", "table"), 1);
        assert_eq!(count_object("arch_file_packages", "table"), 1);
        assert_eq!(count_object("arch_package_deps", "table"), 1);
        assert_eq!(count_object("arch_coupling", "view"), 1);
    }

    #[test]
    fn arch_coupling_view_handles_empty_state() {
        let conn = open_test_conn();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM arch_coupling", [], |row| row.get(0))
            .expect("query view");
        assert_eq!(count, 0, "empty arch_packages → empty view");
    }
}
