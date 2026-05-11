# Tethys Architecture Analysis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the v1 architecture-analysis feature for tethys: per-Cargo-crate coupling metrics (Ca, Ce, instability) exposed via a `tethys coupling` CLI command and matching public API.

**Architecture:** Always-on indexing phase that materializes `arch_packages`, `arch_file_packages`, `arch_package_deps` tables via DELETE-cascade-rebuild in a single transaction. `arch_coupling` is a SQL view that derives metrics on every query, eliminating drift. CLI is a thin formatter over the API.

**Tech Stack:** Rust 2021, rusqlite (SQLite), clap (CLI), colored (output), serde + serde_json (JSON), tempfile + rstest + proptest (tests).

**Spec:** `docs/design/tethys-architecture-analysis.md`
**Issue:** `rivets-byie`

---

## File Structure

| File | Action | Purpose |
|---|---|---|
| `crates/tethys/src/types.rs` | Modify | Add `PackageId`, `PackageSource`, `Package`, `CouplingMetrics`, `CouplingSort`, `PackageDependency`, `CouplingDetail`, `ArchStats`. |
| `crates/tethys/src/db/schema.rs` | Modify | Append three tables + `arch_coupling` view to `SCHEMA` constant. |
| `crates/tethys/src/db/files.rs` | Modify | Add `list_all_files()` query. |
| `crates/tethys/src/db/architecture.rs` | Create | `repopulate_architecture`, `get_packages`, `get_coupling_metrics`, `get_package_coupling`. |
| `crates/tethys/src/db/mod.rs` | Modify | `mod architecture;`. |
| `crates/tethys/src/types.rs` (IndexStats) | Modify | Add `architecture: Option<ArchStats>` field. |
| `crates/tethys/src/indexing.rs` | Modify | New `run_architecture_phase`, called at end of `index_with_options`. |
| `crates/tethys/src/lib.rs` | Modify | Re-exports + new `// === Architecture ===` section with three public methods. |
| `crates/tethys/src/cli/coupling.rs` | Create | CLI command: table/detail/JSON output, suggestions on not-found. |
| `crates/tethys/src/cli/mod.rs` | Modify | `pub mod coupling;`. |
| `crates/tethys/src/main.rs` | Modify | Add `Coupling` variant to `Commands`, dispatch in `match`. |
| `crates/tethys/tests/architecture.rs` | Create | Integration tests on a multi-crate fixture. |
| `crates/tethys/tests/fixtures/multi_crate/` | Create | Three-crate workspace fixture. |
| `crates/tethys/README.md` | Modify | Document `tethys coupling`. |

---

## Task 1: Add architecture domain types

**Files:**
- Modify: `crates/tethys/src/types.rs`

- [ ] **Step 1: Append PackageId newtype, PackageSource, and Package to types.rs**

Append at the end of `crates/tethys/src/types.rs`:

```rust
// === Architecture types ===

/// Internal numeric ID for a package row. Mirrors `FileId` / `SymbolId` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId(i64);

impl PackageId {
    #[must_use]
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl From<i64> for PackageId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How a package was discovered. v1 only emits `Manifest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSource {
    /// Discovered via Cargo.toml.
    Manifest,
    /// Directory-fallback for files outside any manifest. Reserved for future use.
    Directory,
}

impl PackageSource {
    /// Stable string form used in SQL storage.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PackageSource::Manifest => "manifest",
            PackageSource::Directory => "directory",
        }
    }

    /// Inverse of `as_str`. Returns `None` for unknown values, which lets the
    /// caller decide whether to skip the row or surface a warning.
    #[must_use]
    pub fn parse(s: &str) -> Option<PackageSource> {
        match s {
            "manifest" => Some(PackageSource::Manifest),
            "directory" => Some(PackageSource::Directory),
            _ => None,
        }
    }
}

/// A discovered package. Identified by `name` (UNIQUE per workspace).
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub id: PackageId,
    pub name: String,
    pub path: std::path::PathBuf,
    pub source: PackageSource,
}

/// Coupling metrics for a single package.
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingMetrics {
    pub package: Package,
    /// Afferent coupling: distinct packages depending on this one.
    pub afferent: u32,
    /// Efferent coupling: distinct packages this one depends on.
    pub efferent: u32,
    /// Ce / (Ca + Ce). 0.0 when both are zero.
    pub instability: f64,
}

/// Sort key for `get_coupling_metrics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CouplingSort {
    /// Most unstable first.
    #[default]
    Instability,
    /// Most depended-on first.
    Afferent,
    /// Most dependent first.
    Efferent,
    /// Alphabetical.
    Name,
}

/// One package together with how many cross-package edges contribute to a relationship.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageDependency {
    pub package: Package,
    pub dep_count: u32,
}

/// Detailed coupling for a single package, with incoming and outgoing edges.
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingDetail {
    pub metrics: CouplingMetrics,
    /// Packages that depend on this one.
    pub incoming: Vec<PackageDependency>,
    /// Packages this one depends on.
    pub outgoing: Vec<PackageDependency>,
}

/// Statistics emitted by the architecture indexing phase.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchStats {
    pub packages_recorded: usize,
    pub files_assigned: usize,
    pub package_deps_recorded: usize,
}
```

- [ ] **Step 2: Add unit tests for PackageId round-trip, PackageSource as_str/parse, and CouplingSort default**

Append to the `#[cfg(test)] mod tests` block at the end of `crates/tethys/src/types.rs` (or create one if absent):

```rust
#[cfg(test)]
mod arch_type_tests {
    use super::*;

    #[test]
    fn package_id_roundtrip() {
        let id: PackageId = 42i64.into();
        assert_eq!(id.as_i64(), 42);
    }

    #[test]
    fn package_source_as_str_round_trips_through_parse() {
        for variant in [PackageSource::Manifest, PackageSource::Directory] {
            assert_eq!(PackageSource::parse(variant.as_str()), Some(variant));
        }
    }

    #[test]
    fn package_source_parse_returns_none_for_unknown() {
        assert_eq!(PackageSource::parse("git"), None);
    }

    #[test]
    fn coupling_sort_default_is_instability() {
        assert_eq!(CouplingSort::default(), CouplingSort::Instability);
    }
}
```

- [ ] **Step 3: Run tests and verify all pass**

```bash
cargo nextest run -p tethys arch_type_tests
```
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/tethys/src/types.rs
git commit -m "feat(tethys): add architecture domain types

Adds PackageId newtype, PackageSource enum with stable string form,
Package, CouplingMetrics, CouplingSort (with Default = Instability),
PackageDependency, CouplingDetail, and ArchStats. No public re-exports
yet — those land alongside the API methods.

Refs: rivets-byie"
```

---

## Task 2: Schema — three tables + arch_coupling view

**Files:**
- Modify: `crates/tethys/src/db/schema.rs`

- [ ] **Step 1: Read the current SCHEMA constant**

Read `crates/tethys/src/db/schema.rs` to confirm where the SCHEMA string ends (it's a single `r"…"` raw string).

- [ ] **Step 2: Append architecture schema before the closing `";"`**

Append the following SQL inside the SCHEMA raw string, before the closing `"`:

```sql
-- === Architecture analysis ===

-- One row per discovered package. v1: only source = 'manifest'.
CREATE TABLE IF NOT EXISTS arch_packages (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE,
    path   TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('manifest','directory'))
);

CREATE INDEX IF NOT EXISTS idx_arch_packages_path ON arch_packages(path);

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
CREATE VIEW IF NOT EXISTS arch_coupling AS
SELECT
    p.id   AS package_id,
    p.name AS package_name,
    COALESCE(ca.afferent, 0) AS afferent,
    COALESCE(ce.efferent, 0) AS efferent,
    CASE
        WHEN COALESCE(ca.afferent, 0) + COALESCE(ce.efferent, 0) = 0 THEN 0.0
        ELSE CAST(COALESCE(ce.efferent, 0) AS REAL)
             / (COALESCE(ca.afferent, 0) + COALESCE(ce.efferent, 0))
    END AS instability
FROM arch_packages p
LEFT JOIN (
    SELECT target_pkg AS pkg, COUNT(*) AS afferent
    FROM arch_package_deps GROUP BY target_pkg
) ca ON ca.pkg = p.id
LEFT JOIN (
    SELECT source_pkg AS pkg, COUNT(*) AS efferent
    FROM arch_package_deps GROUP BY source_pkg
) ce ON ce.pkg = p.id;
```

- [ ] **Step 3: Add a unit test that the schema applies cleanly to a fresh DB**

Append to `crates/tethys/src/db/schema.rs`:

```rust
#[cfg(test)]
mod schema_tests {
    use super::SCHEMA;
    use rusqlite::Connection;

    #[test]
    fn schema_creates_arch_objects() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch(SCHEMA).expect("apply schema");

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
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch(SCHEMA).expect("apply schema");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM arch_coupling", [], |row| row.get(0))
            .expect("query view");
        assert_eq!(count, 0, "empty arch_packages → empty view");
    }
}
```

- [ ] **Step 4: Run tests and verify pass**

```bash
cargo nextest run -p tethys schema_tests
```
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/db/schema.rs
git commit -m "feat(tethys): add architecture-analysis schema

Adds arch_packages, arch_file_packages, arch_package_deps tables plus
arch_coupling SQL view. The view derives Ca/Ce/instability from
arch_package_deps via LEFT JOINs that preserve isolated packages.

ON DELETE CASCADE on the child tables enables single-DELETE rebuild.

Refs: rivets-byie"
```

---

## Task 3: DB — list_all_files()

**Files:**
- Modify: `crates/tethys/src/db/files.rs`
- Test: `crates/tethys/src/db/files.rs` (inline)

- [ ] **Step 1: Add the failing test first**

Append to the existing `#[cfg(test)] mod tests` in `crates/tethys/src/db/files.rs` (or add the block if absent):

```rust
#[cfg(test)]
mod list_all_files_tests {
    use super::*;
    use crate::db::Index;
    use crate::types::Language;
    use std::path::Path;
    use tempfile::TempDir;

    fn temp_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("idx.db");
        let index = Index::open(&path).expect("open index");
        (dir, index)
    }

    #[test]
    fn list_all_files_returns_every_indexed_file() {
        let (_dir, index) = temp_index();

        for p in ["a.rs", "b.rs", "c.rs"] {
            index
                .insert_file(Path::new(p), Language::Rust, 0, 0, None)
                .expect("insert file");
        }

        let mut files = index.list_all_files().expect("list_all_files");
        files.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path.to_str().unwrap(), "a.rs");
        assert_eq!(files[1].path.to_str().unwrap(), "b.rs");
        assert_eq!(files[2].path.to_str().unwrap(), "c.rs");
    }

    #[test]
    fn list_all_files_returns_empty_for_fresh_index() {
        let (_dir, index) = temp_index();
        let files = index.list_all_files().expect("list_all_files");
        assert!(files.is_empty());
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys list_all_files_tests
```
Expected: FAIL — `list_all_files` not in scope.

- [ ] **Step 3: Implement list_all_files in files.rs**

Add to the `impl Index` block in `crates/tethys/src/db/files.rs`:

```rust
/// List every indexed file in the database.
///
/// Used by the architecture phase to map files to packages. The order is
/// implementation-defined; callers should sort if they need determinism.
pub fn list_all_files(&self) -> Result<Vec<crate::types::IndexedFile>> {
    use crate::db::FILES_COLUMNS;
    use crate::db::row_to_indexed_file;

    let conn = self.connection()?;
    let sql = format!("SELECT {FILES_COLUMNS} FROM files");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_indexed_file)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}
```

- [ ] **Step 4: Run the test, expect pass**

```bash
cargo nextest run -p tethys list_all_files_tests
```
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/db/files.rs
git commit -m "feat(tethys): add Index::list_all_files()

Returns every indexed file in the database. Used by the upcoming
architecture-analysis phase to map files to their containing crate.

Refs: rivets-byie"
```

---

## Task 4: DB module — architecture.rs scaffold

**Files:**
- Create: `crates/tethys/src/db/architecture.rs`
- Modify: `crates/tethys/src/db/mod.rs`

- [ ] **Step 1: Create empty architecture.rs**

Create `crates/tethys/src/db/architecture.rs`:

```rust
//! Architecture-analysis storage layer.
//!
//! Owns the four `arch_*` schema objects and the queries that read and write them.
//! Wired into the indexing pipeline by `Tethys::run_architecture_phase`.

use std::collections::HashMap;

use rusqlite::params;
use tracing::trace;

use super::Index;
use crate::error::Result;
use crate::types::{
    ArchStats, CouplingDetail, CouplingMetrics, CouplingSort, FileId, Package, PackageDependency,
    PackageId, PackageSource,
};

/// Insert payload for `repopulate_architecture`.
pub struct PackageInsert<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub source: PackageSource,
}

impl Index {
    // Methods will be added in subsequent tasks.
}
```

- [ ] **Step 2: Register the module in db/mod.rs**

In `crates/tethys/src/db/mod.rs`, add the `mod architecture;` declaration alongside the other `mod` lines (after `mod symbols;`, sorted alphabetically the file would put it before `mod call_edges` — match the existing alphabetical ordering):

```rust
mod architecture;
mod call_edges;
// ... rest unchanged
```

Also add a re-export alongside the others if `PackageInsert` needs to be visible to the crate:

```rust
pub(crate) use architecture::PackageInsert;
```

- [ ] **Step 3: Verify the crate builds**

```bash
cargo build -p tethys
```
Expected: clean build, no warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/tethys/src/db/architecture.rs crates/tethys/src/db/mod.rs
git commit -m "chore(tethys): scaffold db::architecture module

Adds an empty module with the PackageInsert type. Methods land in
follow-up tasks.

Refs: rivets-byie"
```

---

## Task 5: DB — repopulate_architecture()

**Files:**
- Modify: `crates/tethys/src/db/architecture.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/db/architecture.rs`:

```rust
#[cfg(test)]
mod repopulate_tests {
    use super::*;
    use crate::types::Language;
    use std::path::Path;
    use tempfile::TempDir;

    fn temp_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("idx.db");
        let index = Index::open(&path).expect("open index");
        (dir, index)
    }

    /// Inserts a file and returns its FileId.
    fn add_file(index: &Index, rel_path: &str) -> FileId {
        index
            .insert_file(Path::new(rel_path), Language::Rust, 0, 0, None)
            .expect("insert file")
    }

    #[test]
    fn repopulate_architecture_inserts_packages() {
        let (_dir, index) = temp_index();

        let packages = [
            PackageInsert {
                name: "crate_a",
                path: "crate_a",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "crate_b",
                path: "crate_b",
                source: PackageSource::Manifest,
            },
        ];

        let stats = index
            .repopulate_architecture(&packages, &[])
            .expect("repopulate");

        assert_eq!(stats.packages_recorded, 2);
        assert_eq!(stats.files_assigned, 0);
        assert_eq!(stats.package_deps_recorded, 0);
    }

    #[test]
    fn repopulate_architecture_assigns_files_and_deps() {
        let (_dir, index) = temp_index();

        let f_a = add_file(&index, "crate_a/lib.rs");
        let f_b = add_file(&index, "crate_b/lib.rs");
        let f_c = add_file(&index, "crate_c/lib.rs");

        // crate_a depends on crate_b and crate_c; crate_b depends on crate_c.
        index.insert_file_dependency(f_a, f_b).expect("dep a→b");
        index.insert_file_dependency(f_a, f_c).expect("dep a→c");
        index.insert_file_dependency(f_b, f_c).expect("dep b→c");

        let packages = [
            PackageInsert {
                name: "crate_a",
                path: "crate_a",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "crate_b",
                path: "crate_b",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "crate_c",
                path: "crate_c",
                source: PackageSource::Manifest,
            },
        ];

        let mappings = [(f_a, "crate_a"), (f_b, "crate_b"), (f_c, "crate_c")];

        let stats = index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        assert_eq!(stats.packages_recorded, 3);
        assert_eq!(stats.files_assigned, 3);
        assert_eq!(stats.package_deps_recorded, 3, "a→b, a→c, b→c");
    }

    #[test]
    fn repopulate_architecture_is_idempotent() {
        let (_dir, index) = temp_index();
        let f = add_file(&index, "crate_a/lib.rs");
        let packages = [PackageInsert {
            name: "crate_a",
            path: "crate_a",
            source: PackageSource::Manifest,
        }];
        let mappings = [(f, "crate_a")];

        let s1 = index
            .repopulate_architecture(&packages, &mappings)
            .expect("first");
        let s2 = index
            .repopulate_architecture(&packages, &mappings)
            .expect("second");

        assert_eq!(s1, s2);
    }

    #[test]
    fn repopulate_architecture_skips_unknown_package_names() {
        let (_dir, index) = temp_index();
        let f = add_file(&index, "orphan.rs");
        let packages = [PackageInsert {
            name: "crate_a",
            path: "crate_a",
            source: PackageSource::Manifest,
        }];
        // file_to_package_name references a package not in `packages`.
        let mappings = [(f, "missing_crate")];

        let stats = index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        assert_eq!(stats.packages_recorded, 1);
        assert_eq!(stats.files_assigned, 0, "unknown name skipped");
    }
}
```

- [ ] **Step 2: Run the tests, expect failure**

```bash
cargo nextest run -p tethys repopulate_tests
```
Expected: FAIL — `repopulate_architecture` does not exist.

- [ ] **Step 3: Implement repopulate_architecture**

Replace the empty `impl Index { /* … */ }` block in `crates/tethys/src/db/architecture.rs` with:

```rust
impl Index {
    /// Rebuild every `arch_*` table from the supplied package list and file
    /// mappings, plus the current state of `file_deps`. Single transaction.
    ///
    /// Idempotent: identical input produces identical state.
    ///
    /// `file_to_package_name` entries whose name is not present in `packages`
    /// are silently skipped (logged at `trace!`); this lets callers feed
    /// best-effort name lookups without pre-filtering.
    pub fn repopulate_architecture(
        &self,
        packages: &[PackageInsert<'_>],
        file_to_package_name: &[(FileId, &str)],
    ) -> Result<ArchStats> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;

        // 1. Wipe. Cascade clears the two child tables.
        tx.execute("DELETE FROM arch_packages", [])?;

        // 2. Insert packages.
        let mut packages_recorded: usize = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO arch_packages (name, path, source) VALUES (?1, ?2, ?3)",
            )?;
            for pkg in packages {
                stmt.execute(params![pkg.name, pkg.path, pkg.source.as_str()])?;
                packages_recorded += 1;
            }
        }

        // 3. Read back name → id map (needed to translate file mappings).
        let mut name_to_id: HashMap<String, PackageId> = HashMap::new();
        {
            let mut stmt = tx.prepare("SELECT id, name FROM arch_packages")?;
            let rows = stmt.query_map([], |row| {
                Ok((PackageId::from(row.get::<_, i64>(0)?), row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, name) = row?;
                name_to_id.insert(name, id);
            }
        }

        // 4. Insert file → package mappings, skipping unknown names.
        let mut files_assigned: usize = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO arch_file_packages (file_id, package_id) VALUES (?1, ?2)",
            )?;
            for (file_id, name) in file_to_package_name {
                if let Some(pkg_id) = name_to_id.get(*name) {
                    stmt.execute(params![file_id.as_i64(), pkg_id.as_i64()])?;
                    files_assigned += 1;
                } else {
                    trace!(
                        file_id = %file_id,
                        package_name = %name,
                        "skipping file with unknown package name"
                    );
                }
            }
        }

        // 5. Roll up cross-package edges.
        let package_deps_recorded = tx.execute(
            "INSERT INTO arch_package_deps (source_pkg, target_pkg, dep_count)
             SELECT sp.package_id, tp.package_id, COUNT(*)
             FROM file_deps fd
             JOIN arch_file_packages sp ON sp.file_id = fd.from_file_id
             JOIN arch_file_packages tp ON tp.file_id = fd.to_file_id
             WHERE sp.package_id <> tp.package_id
             GROUP BY sp.package_id, tp.package_id",
            [],
        )?;

        tx.commit()?;

        Ok(ArchStats {
            packages_recorded,
            files_assigned,
            package_deps_recorded,
        })
    }
}
```

- [ ] **Step 4: Run the tests, expect pass**

```bash
cargo nextest run -p tethys repopulate_tests
```
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/db/architecture.rs
git commit -m "feat(tethys): implement Index::repopulate_architecture

Single-transaction DELETE-cascade-rebuild of the three arch_* tables.
Aggregates cross-package deps from file_deps via a single GROUP BY pass.
Skips file mappings whose package name isn't recognized (with trace log).

Refs: rivets-byie"
```

---

## Task 6: DB — get_packages()

**Files:**
- Modify: `crates/tethys/src/db/architecture.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/db/architecture.rs` (in or after `repopulate_tests`):

```rust
#[cfg(test)]
mod get_packages_tests {
    use super::*;
    use tempfile::TempDir;

    fn seeded_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("idx.db");
        let index = Index::open(&path).expect("open");
        let packages = [
            PackageInsert { name: "z_crate", path: "crates/z", source: PackageSource::Manifest },
            PackageInsert { name: "a_crate", path: "crates/a", source: PackageSource::Manifest },
        ];
        index.repopulate_architecture(&packages, &[]).expect("repopulate");
        (dir, index)
    }

    #[test]
    fn get_packages_returns_alphabetical_by_name() {
        let (_dir, index) = seeded_index();
        let pkgs = index.get_packages().expect("get_packages");
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "a_crate");
        assert_eq!(pkgs[1].name, "z_crate");
    }

    #[test]
    fn get_packages_decodes_source_field() {
        let (_dir, index) = seeded_index();
        let pkgs = index.get_packages().expect("get_packages");
        assert!(pkgs.iter().all(|p| p.source == PackageSource::Manifest));
    }

    #[test]
    fn get_packages_empty_for_fresh_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        assert!(index.get_packages().expect("get_packages").is_empty());
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys get_packages_tests
```
Expected: FAIL — `get_packages` does not exist.

- [ ] **Step 3: Implement get_packages**

Add to the `impl Index` block in `crates/tethys/src/db/architecture.rs`:

```rust
/// Return every package row, ordered alphabetically by name for determinism.
/// Unknown `source` values produce a `warn!` and are skipped.
pub fn get_packages(&self) -> Result<Vec<Package>> {
    use std::path::PathBuf;

    let conn = self.connection()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, path, source FROM arch_packages ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, name, path, source_str) = row?;
        let Some(source) = PackageSource::parse(&source_str) else {
            tracing::warn!(
                package_name = %name,
                source = %source_str,
                "skipping package with unknown source value"
            );
            continue;
        };
        out.push(Package {
            id: PackageId::from(id),
            name,
            path: PathBuf::from(path),
            source,
        });
    }
    Ok(out)
}
```

- [ ] **Step 4: Run the test, expect pass**

```bash
cargo nextest run -p tethys get_packages_tests
```
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/db/architecture.rs
git commit -m "feat(tethys): implement Index::get_packages

Returns every recorded package, alphabetically by name. Unknown source
values produce a warn! log and are skipped (forward-compatibility).

Refs: rivets-byie"
```

---

## Task 7: DB — get_coupling_metrics()

**Files:**
- Modify: `crates/tethys/src/db/architecture.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/db/architecture.rs`:

```rust
#[cfg(test)]
mod coupling_metrics_tests {
    use super::*;
    use crate::types::{CouplingSort, Language};
    use std::path::Path;
    use tempfile::TempDir;

    /// Three-crate fixture: a → b, a → c, b → c.
    /// Expected: a (Ca=0, Ce=2, I=1.0), b (Ca=1, Ce=1, I=0.5), c (Ca=2, Ce=0, I=0.0).
    fn seeded_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f_a = index.insert_file(Path::new("a/lib.rs"), Language::Rust, 0, 0, None).expect("a");
        let f_b = index.insert_file(Path::new("b/lib.rs"), Language::Rust, 0, 0, None).expect("b");
        let f_c = index.insert_file(Path::new("c/lib.rs"), Language::Rust, 0, 0, None).expect("c");

        index.insert_file_dependency(f_a, f_b).expect("a→b");
        index.insert_file_dependency(f_a, f_c).expect("a→c");
        index.insert_file_dependency(f_b, f_c).expect("b→c");

        let packages = [
            PackageInsert { name: "a", path: "a", source: PackageSource::Manifest },
            PackageInsert { name: "b", path: "b", source: PackageSource::Manifest },
            PackageInsert { name: "c", path: "c", source: PackageSource::Manifest },
        ];
        let mappings = [(f_a, "a"), (f_b, "b"), (f_c, "c")];
        index.repopulate_architecture(&packages, &mappings).expect("repopulate");

        (dir, index)
    }

    fn metrics_for<'a>(rows: &'a [CouplingMetrics], name: &str) -> &'a CouplingMetrics {
        rows.iter()
            .find(|m| m.package.name == name)
            .unwrap_or_else(|| panic!("no row for {name}"))
    }

    #[test]
    fn coupling_metrics_match_expected_ca_ce_instability() {
        let (_dir, index) = seeded_index();
        let rows = index.get_coupling_metrics(CouplingSort::Name).expect("metrics");

        let a = metrics_for(&rows, "a");
        assert_eq!((a.afferent, a.efferent), (0, 2));
        assert!((a.instability - 1.0).abs() < 1e-9);

        let b = metrics_for(&rows, "b");
        assert_eq!((b.afferent, b.efferent), (1, 1));
        assert!((b.instability - 0.5).abs() < 1e-9);

        let c = metrics_for(&rows, "c");
        assert_eq!((c.afferent, c.efferent), (2, 0));
        assert!((c.instability - 0.0).abs() < 1e-9);
    }

    #[test]
    fn sort_by_instability_descending() {
        let (_dir, index) = seeded_index();
        let rows = index.get_coupling_metrics(CouplingSort::Instability).expect("metrics");
        assert_eq!(rows[0].package.name, "a", "I=1.0 first");
        assert_eq!(rows[1].package.name, "b", "I=0.5 second");
        assert_eq!(rows[2].package.name, "c", "I=0.0 last");
    }

    #[test]
    fn sort_by_name_ascending() {
        let (_dir, index) = seeded_index();
        let rows = index.get_coupling_metrics(CouplingSort::Name).expect("metrics");
        assert_eq!(rows[0].package.name, "a");
        assert_eq!(rows[1].package.name, "b");
        assert_eq!(rows[2].package.name, "c");
    }

    #[test]
    fn isolated_package_has_zero_instability() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        let packages = [PackageInsert {
            name: "lonely", path: "lonely", source: PackageSource::Manifest,
        }];
        index.repopulate_architecture(&packages, &[]).expect("repopulate");

        let rows = index.get_coupling_metrics(CouplingSort::Name).expect("metrics");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].afferent, rows[0].efferent), (0, 0));
        assert_eq!(rows[0].instability, 0.0);
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys coupling_metrics_tests
```
Expected: FAIL — `get_coupling_metrics` does not exist.

- [ ] **Step 3: Implement get_coupling_metrics**

Add to the `impl Index` block in `crates/tethys/src/db/architecture.rs`:

```rust
/// Coupling metrics for every package, sorted per the requested key.
/// Sort is delegated to SQLite via `ORDER BY`.
pub fn get_coupling_metrics(&self, sort: CouplingSort) -> Result<Vec<CouplingMetrics>> {
    use std::path::PathBuf;

    // Sort key is encoded as a literal SQL fragment chosen from a fixed set —
    // not from user input — so there's no injection risk.
    let order_clause = match sort {
        CouplingSort::Instability => "c.instability DESC, p.name ASC",
        CouplingSort::Afferent => "c.afferent DESC, p.name ASC",
        CouplingSort::Efferent => "c.efferent DESC, p.name ASC",
        CouplingSort::Name => "p.name ASC",
    };

    let sql = format!(
        "SELECT p.id, p.name, p.path, p.source,
                c.afferent, c.efferent, c.instability
         FROM arch_coupling c
         JOIN arch_packages p ON p.id = c.package_id
         ORDER BY {order_clause}"
    );

    let conn = self.connection()?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, f64>(6)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, name, path, source_str, ca, ce, instability) = row?;
        let Some(source) = PackageSource::parse(&source_str) else {
            tracing::warn!(
                package_name = %name,
                source = %source_str,
                "skipping coupling row with unknown source"
            );
            continue;
        };
        out.push(CouplingMetrics {
            package: Package {
                id: PackageId::from(id),
                name,
                path: PathBuf::from(path),
                source,
            },
            afferent: u32::try_from(ca).unwrap_or(u32::MAX),
            efferent: u32::try_from(ce).unwrap_or(u32::MAX),
            instability,
        });
    }
    Ok(out)
}
```

- [ ] **Step 4: Run the test, expect pass**

```bash
cargo nextest run -p tethys coupling_metrics_tests
```
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/db/architecture.rs
git commit -m "feat(tethys): implement Index::get_coupling_metrics

Reads from arch_coupling view, sorted per CouplingSort. Sort fragment
is chosen from a fixed set of SQL strings; no user input ever reaches
the SQL, no injection risk.

Refs: rivets-byie"
```

---

## Task 8: DB — get_package_coupling()

**Files:**
- Modify: `crates/tethys/src/db/architecture.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/db/architecture.rs`:

```rust
#[cfg(test)]
mod package_coupling_tests {
    use super::*;
    use crate::types::Language;
    use std::path::Path;
    use tempfile::TempDir;

    fn seeded_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f_a = index.insert_file(Path::new("a/lib.rs"), Language::Rust, 0, 0, None).expect("a");
        let f_b = index.insert_file(Path::new("b/lib.rs"), Language::Rust, 0, 0, None).expect("b");
        let f_c = index.insert_file(Path::new("c/lib.rs"), Language::Rust, 0, 0, None).expect("c");

        index.insert_file_dependency(f_a, f_b).expect("a→b");
        index.insert_file_dependency(f_a, f_c).expect("a→c");
        index.insert_file_dependency(f_b, f_c).expect("b→c");

        let packages = [
            PackageInsert { name: "a", path: "a", source: PackageSource::Manifest },
            PackageInsert { name: "b", path: "b", source: PackageSource::Manifest },
            PackageInsert { name: "c", path: "c", source: PackageSource::Manifest },
        ];
        let mappings = [(f_a, "a"), (f_b, "b"), (f_c, "c")];
        index.repopulate_architecture(&packages, &mappings).expect("repopulate");
        (dir, index)
    }

    #[test]
    fn package_coupling_returns_outgoing_and_incoming() {
        let (_dir, index) = seeded_index();
        let detail = index.get_package_coupling("b").expect("query").expect("found");

        assert_eq!(detail.metrics.package.name, "b");
        assert_eq!((detail.metrics.afferent, detail.metrics.efferent), (1, 1));

        let in_names: Vec<_> = detail.incoming.iter().map(|d| d.package.name.as_str()).collect();
        assert_eq!(in_names, ["a"]);

        let out_names: Vec<_> = detail.outgoing.iter().map(|d| d.package.name.as_str()).collect();
        assert_eq!(out_names, ["c"]);
    }

    #[test]
    fn package_coupling_none_for_missing_package() {
        let (_dir, index) = seeded_index();
        assert!(index.get_package_coupling("does-not-exist").expect("query").is_none());
    }

    #[test]
    fn package_coupling_for_isolated_package_returns_empty_lists() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        let packages = [PackageInsert {
            name: "lonely", path: "lonely", source: PackageSource::Manifest,
        }];
        index.repopulate_architecture(&packages, &[]).expect("repopulate");

        let detail = index.get_package_coupling("lonely").expect("query").expect("found");
        assert!(detail.incoming.is_empty());
        assert!(detail.outgoing.is_empty());
        assert_eq!(detail.metrics.instability, 0.0);
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys package_coupling_tests
```
Expected: FAIL — `get_package_coupling` does not exist.

- [ ] **Step 3: Implement get_package_coupling**

Add to the `impl Index` block in `crates/tethys/src/db/architecture.rs`:

```rust
/// Detailed coupling for one package by exact name.
/// Returns `Ok(None)` when no package matches; `Result::Err` only on DB failure.
///
/// Incoming and outgoing lists are sorted by `dep_count` descending, then by name ascending.
pub fn get_package_coupling(&self, name: &str) -> Result<Option<CouplingDetail>> {
    use std::path::PathBuf;

    let conn = self.connection()?;

    // 1. Fetch the package row + its coupling values from the view.
    let row: Option<(i64, String, String, String, i64, i64, f64)> = conn
        .query_row(
            "SELECT p.id, p.name, p.path, p.source,
                    c.afferent, c.efferent, c.instability
             FROM arch_coupling c
             JOIN arch_packages p ON p.id = c.package_id
             WHERE p.name = ?1",
            params![name],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, f64>(6)?,
                ))
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;

    let Some((id, pkg_name, pkg_path, source_str, ca, ce, instability)) = row else {
        return Ok(None);
    };
    let Some(source) = PackageSource::parse(&source_str) else {
        tracing::warn!(
            package_name = %pkg_name,
            source = %source_str,
            "package coupling has unknown source"
        );
        return Ok(None);
    };

    let target = Package {
        id: PackageId::from(id),
        name: pkg_name,
        path: PathBuf::from(pkg_path),
        source,
    };

    // 2. Outgoing edges (this package → others).
    let outgoing = self.fetch_neighbors(target.id, Direction::Outgoing)?;
    // 3. Incoming edges (others → this package).
    let incoming = self.fetch_neighbors(target.id, Direction::Incoming)?;

    Ok(Some(CouplingDetail {
        metrics: CouplingMetrics {
            package: target,
            afferent: u32::try_from(ca).unwrap_or(u32::MAX),
            efferent: u32::try_from(ce).unwrap_or(u32::MAX),
            instability,
        },
        incoming,
        outgoing,
    }))
}

#[derive(Clone, Copy)]
enum Direction {
    Outgoing,
    Incoming,
}

impl Index {
    fn fetch_neighbors(
        &self,
        package_id: PackageId,
        dir: Direction,
    ) -> Result<Vec<PackageDependency>> {
        use std::path::PathBuf;

        let (this, other) = match dir {
            Direction::Outgoing => ("source_pkg", "target_pkg"),
            Direction::Incoming => ("target_pkg", "source_pkg"),
        };
        let sql = format!(
            "SELECT p.id, p.name, p.path, p.source, d.dep_count
             FROM arch_package_deps d
             JOIN arch_packages p ON p.id = d.{other}
             WHERE d.{this} = ?1
             ORDER BY d.dep_count DESC, p.name ASC"
        );

        let conn = self.connection()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![package_id.as_i64()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, name, path, source_str, dep_count) = row?;
            let Some(source) = PackageSource::parse(&source_str) else { continue };
            out.push(PackageDependency {
                package: Package {
                    id: PackageId::from(id),
                    name,
                    path: PathBuf::from(path),
                    source,
                },
                dep_count: u32::try_from(dep_count).unwrap_or(u32::MAX),
            });
        }
        Ok(out)
    }
}
```

Note: the `Direction` enum and `fetch_neighbors` are private to the module; place them after the public `impl Index` block in the same file.

- [ ] **Step 4: Run the test, expect pass**

```bash
cargo nextest run -p tethys package_coupling_tests
```
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/db/architecture.rs
git commit -m "feat(tethys): implement Index::get_package_coupling

Returns CouplingDetail for an exact-name match, or Ok(None) if absent.
Incoming/outgoing edge lists sort by dep_count desc, then name asc.

Refs: rivets-byie"
```

---

## Task 9: Add `architecture` field to IndexStats

**Files:**
- Modify: `crates/tethys/src/types.rs`
- Modify: `crates/tethys/src/indexing.rs` (constructors)
- Modify: `crates/tethys/src/lib.rs` (re-exports)

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/types.rs` `#[cfg(test)]` block:

```rust
#[cfg(test)]
mod arch_stats_in_index_stats {
    use super::*;

    #[test]
    fn index_stats_default_has_no_architecture() {
        let stats = IndexStats::default();
        assert!(stats.architecture.is_none());
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys arch_stats_in_index_stats
```
Expected: FAIL — `architecture` field does not exist.

- [ ] **Step 3: Add the field to IndexStats**

Find the existing `IndexStats` struct in `crates/tethys/src/types.rs` and add the `architecture` field. Add to the struct definition:

```rust
/// Statistics from the architecture-analysis phase, when it ran successfully.
/// `None` when the phase was skipped or failed.
pub architecture: Option<ArchStats>,
```

If `IndexStats` does not already derive `Default`, ensure it does, or update every constructor (search for `IndexStats {` literals) to include `architecture: None`.

- [ ] **Step 4: Re-export ArchStats from lib.rs**

In `crates/tethys/src/lib.rs`, find the existing `pub use types::{...}` block and add `ArchStats` to the alphabetically sorted list:

```rust
pub use types::{
    ArchStats, CrateInfo, Cycle, DatabaseStats, /* …existing… */
};
```

- [ ] **Step 5: Run the test, expect pass**

```bash
cargo nextest run -p tethys arch_stats_in_index_stats
cargo build -p tethys
```
Expected: 1 test pass; clean build.

- [ ] **Step 6: Commit**

```bash
git add crates/tethys/src/types.rs crates/tethys/src/lib.rs
git commit -m "feat(tethys): add ArchStats and IndexStats.architecture field

ArchStats reports counts from the architecture phase. IndexStats gains
an Option<ArchStats> field — Option so a future opt-out flag leaves the
field as None without changing types.

Refs: rivets-byie"
```

---

## Task 10: Indexing pipeline — run_architecture_phase

**Files:**
- Modify: `crates/tethys/src/indexing.rs`

- [ ] **Step 1: Add the failing test as a new integration-style test in indexing.rs**

Append to the `#[cfg(test)] mod tests` block in `crates/tethys/src/indexing.rs` (or wherever local tests live; create the block if absent):

```rust
#[cfg(test)]
mod arch_phase_tests {
    use crate::Tethys;
    use std::fs;
    use tempfile::TempDir;

    fn make_workspace_with_two_crates() -> (TempDir, Tethys) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();

        // Workspace Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"[workspace]
members = ["crate_a", "crate_b"]
resolver = "2"
"#,
        ).expect("write workspace toml");

        // crate_a depends on crate_b structurally (use statement)
        fs::create_dir_all(root.join("crate_a/src")).expect("mkdir a");
        fs::write(
            root.join("crate_a/Cargo.toml"),
            r#"[package]
name = "crate_a"
version = "0.1.0"
edition = "2021"
"#,
        ).expect("write a toml");
        fs::write(
            root.join("crate_a/src/lib.rs"),
            "pub fn hello() -> String { String::from(\"hi\") }\n",
        ).expect("write a lib");

        fs::create_dir_all(root.join("crate_b/src")).expect("mkdir b");
        fs::write(
            root.join("crate_b/Cargo.toml"),
            r#"[package]
name = "crate_b"
version = "0.1.0"
edition = "2021"
"#,
        ).expect("write b toml");
        fs::write(
            root.join("crate_b/src/lib.rs"),
            "pub fn world() -> u32 { 42 }\n",
        ).expect("write b lib");

        let tethys = Tethys::new(root).expect("Tethys::new");
        (dir, tethys)
    }

    #[test]
    fn architecture_phase_records_packages() {
        let (_dir, mut tethys) = make_workspace_with_two_crates();
        let stats = tethys.index().expect("index");
        let arch = stats.architecture.expect("architecture stats present");
        assert_eq!(arch.packages_recorded, 2);
        assert!(arch.files_assigned >= 2);
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys arch_phase_tests
```
Expected: FAIL — `architecture` field is `None` (phase not yet wired up).

- [ ] **Step 3: Implement run_architecture_phase**

Add a new method to the `impl Tethys` block in `crates/tethys/src/indexing.rs`:

```rust
/// Final indexing phase: rebuild arch_* tables from current files + file_deps.
/// Returns ArchStats, or propagates DB errors. Skips files outside any crate.
pub(crate) fn run_architecture_phase(&self) -> Result<crate::types::ArchStats> {
    use crate::db::PackageInsert;
    use crate::types::PackageSource;

    // 1. PackageInsert for each discovered crate.
    // The relative path is stored as a String to keep ownership simple.
    let package_paths: Vec<String> = self
        .crates
        .iter()
        .map(|c| {
            self.relative_path(&c.path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    let packages: Vec<PackageInsert<'_>> = self
        .crates
        .iter()
        .zip(package_paths.iter())
        .map(|(c, p)| PackageInsert {
            name: c.name.as_str(),
            path: p.as_str(),
            source: PackageSource::Manifest,
        })
        .collect();

    // 2. Map each indexed file to its crate name (skipping files outside any crate).
    let mut file_to_package: Vec<(crate::types::FileId, &str)> = Vec::new();
    for file in self.db.list_all_files()? {
        // get_crate_for_file canonicalizes; we need an absolute path on disk.
        let abs = if file.path.is_absolute() {
            file.path.clone()
        } else {
            self.workspace_root.join(&file.path)
        };
        if let Some(info) = self.get_crate_for_file(&abs) {
            file_to_package.push((file.id, info.name.as_str()));
        } else {
            tracing::trace!(
                file = %file.path.display(),
                "file outside any crate, skipping from architecture phase"
            );
        }
    }

    // 3. Single-transaction rebuild.
    self.db.repopulate_architecture(&packages, &file_to_package)
}
```

- [ ] **Step 4: Wire the call into index_with_options**

Find `index_with_options` in `crates/tethys/src/indexing.rs`. At the end of the function — after the call_edges population and just before returning `Ok(stats)` — insert:

```rust
// Final phase: architecture analysis. Always-on. Runs only if every prior
// phase succeeded — we don't want arch tables based on incomplete data.
match self.run_architecture_phase() {
    Ok(arch) => {
        tracing::debug!(
            packages = arch.packages_recorded,
            files = arch.files_assigned,
            edges = arch.package_deps_recorded,
            "architecture phase complete"
        );
        stats.architecture = Some(arch);
    }
    Err(e) => {
        tracing::warn!(
            error = %e,
            "architecture phase failed; index data is otherwise valid"
        );
        stats.architecture = None;
    }
}
```

The `stats.architecture = None` on failure is explicit (rather than relying on `Default`) so the failure mode is greppable.

- [ ] **Step 5: Run the test, expect pass**

```bash
cargo nextest run -p tethys arch_phase_tests
```
Expected: pass.

- [ ] **Step 6: Run the full tethys test suite to catch regressions**

```bash
cargo nextest run -p tethys
```
Expected: every test passes.

- [ ] **Step 7: Commit**

```bash
git add crates/tethys/src/indexing.rs
git commit -m "feat(tethys): wire run_architecture_phase into indexing pipeline

Always-on phase runs as the final step of index_with_options. On failure,
stats.architecture is set to None and a warn! log fires, but indexing as
a whole still succeeds — coupling is supplementary, not load-bearing.

Refs: rivets-byie"
```

---

## Task 11: Public Tethys API methods

**Files:**
- Modify: `crates/tethys/src/lib.rs`

- [ ] **Step 1: Re-export the new types**

In `crates/tethys/src/lib.rs`, expand the existing `pub use types::{...}` block to include:

```rust
pub use types::{
    /* existing entries */
    CouplingDetail, CouplingMetrics, CouplingSort, Package, PackageDependency, PackageId,
    PackageSource,
};
```

(`ArchStats` was added in Task 9.)

- [ ] **Step 2: Add the failing test**

Append a new test module to `crates/tethys/src/lib.rs` `#[cfg(test)] mod tests` (or create one if absent) — actually, since lib.rs already has tests, add this:

```rust
#[cfg(test)]
mod arch_api_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn workspace_with_two_crates() -> (TempDir, Tethys) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
        ).expect("workspace toml");
        for name in ["a", "b"] {
            fs::create_dir_all(root.join(format!("{name}/src"))).expect("mkdir");
            fs::write(
                root.join(format!("{name}/Cargo.toml")),
                format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ).expect("crate toml");
            fs::write(
                root.join(format!("{name}/src/lib.rs")),
                "pub fn x() {}\n",
            ).expect("crate lib");
        }
        let mut tethys = Tethys::new(root).expect("Tethys::new");
        tethys.index().expect("index");
        (dir, tethys)
    }

    #[test]
    fn get_packages_returns_each_crate() {
        let (_dir, tethys) = workspace_with_two_crates();
        let mut pkgs = tethys.get_packages().expect("packages");
        pkgs.sort_by(|x, y| x.name.cmp(&y.name));
        let names: Vec<_> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn get_coupling_metrics_returns_one_row_per_crate() {
        let (_dir, tethys) = workspace_with_two_crates();
        let rows = tethys
            .get_coupling_metrics(CouplingSort::Name)
            .expect("metrics");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn get_package_coupling_unknown_returns_none() {
        let (_dir, tethys) = workspace_with_two_crates();
        assert!(tethys
            .get_package_coupling("missing")
            .expect("query")
            .is_none());
    }
}
```

- [ ] **Step 3: Run the test, expect failure**

```bash
cargo nextest run -p tethys arch_api_tests
```
Expected: FAIL — methods do not exist on `Tethys`.

- [ ] **Step 4: Add the public methods to Tethys**

In `crates/tethys/src/lib.rs`, find the last `// === ... ===` section header in `impl Tethys`. After the final method of the last existing section, add:

```rust
// === Architecture ===

/// List all packages discovered during the last index run.
/// Empty for non-Rust workspaces or before any index has run.
pub fn get_packages(&self) -> Result<Vec<Package>> {
    self.db.get_packages()
}

/// Coupling metrics for every package, sorted per the requested key.
pub fn get_coupling_metrics(&self, sort: CouplingSort) -> Result<Vec<CouplingMetrics>> {
    self.db.get_coupling_metrics(sort)
}

/// Detailed coupling for one package by exact name.
/// Returns `Ok(None)` when no package matches.
pub fn get_package_coupling(&self, name: &str) -> Result<Option<CouplingDetail>> {
    self.db.get_package_coupling(name)
}
```

- [ ] **Step 5: Run the test, expect pass**

```bash
cargo nextest run -p tethys arch_api_tests
```
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): expose architecture API on Tethys

Public methods: get_packages, get_coupling_metrics, get_package_coupling.
Each is a thin wrapper around the equivalent Index method.

Refs: rivets-byie"
```

---

## Task 12: Multi-crate fixture + end-to-end coupling test

**Files:**
- Create: `crates/tethys/tests/architecture.rs`

- [ ] **Step 1: Write the integration test**

Create `crates/tethys/tests/architecture.rs`:

```rust
//! Integration tests for the architecture-analysis phase end-to-end.
//!
//! Builds a three-crate workspace where crate_a → crate_b, crate_a → crate_c,
//! and crate_b → crate_c (chain plus shortcut), then verifies coupling math.

use std::fs;
use tempfile::TempDir;
use tethys::{CouplingSort, Tethys};

/// Builds the canonical three-crate fixture in a temp dir, indexes it,
/// and returns (dir, tethys). The dir must be kept alive.
fn three_crate_workspace() -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crate_a", "crate_b", "crate_c"]
resolver = "2"
"#,
    )
    .expect("workspace toml");

    // crate_c (leaf): exposes a function that the others call.
    fs::create_dir_all(root.join("crate_c/src")).expect("mkdir c");
    fs::write(
        root.join("crate_c/Cargo.toml"),
        r#"[package]
name = "crate_c"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("c toml");
    fs::write(
        root.join("crate_c/src/lib.rs"),
        "pub fn leaf() -> u32 { 0 }\n",
    )
    .expect("c lib");

    // crate_b: depends on crate_c by referencing crate_c::leaf.
    fs::create_dir_all(root.join("crate_b/src")).expect("mkdir b");
    fs::write(
        root.join("crate_b/Cargo.toml"),
        r#"[package]
name = "crate_b"
version = "0.1.0"
edition = "2021"

[dependencies]
crate_c = { path = "../crate_c" }
"#,
    )
    .expect("b toml");
    fs::write(
        root.join("crate_b/src/lib.rs"),
        "pub fn middle() -> u32 { crate_c::leaf() + 1 }\n",
    )
    .expect("b lib");

    // crate_a: depends on both crate_b and crate_c.
    fs::create_dir_all(root.join("crate_a/src")).expect("mkdir a");
    fs::write(
        root.join("crate_a/Cargo.toml"),
        r#"[package]
name = "crate_a"
version = "0.1.0"
edition = "2021"

[dependencies]
crate_b = { path = "../crate_b" }
crate_c = { path = "../crate_c" }
"#,
    )
    .expect("a toml");
    fs::write(
        root.join("crate_a/src/lib.rs"),
        r#"pub fn root() -> u32 { crate_b::middle() + crate_c::leaf() }
"#,
    )
    .expect("a lib");

    let mut tethys = Tethys::new(root).expect("Tethys::new");
    tethys.index().expect("index");
    (dir, tethys)
}

#[test]
fn coupling_metrics_match_expected_values() {
    let (_dir, tethys) = three_crate_workspace();
    let rows = tethys
        .get_coupling_metrics(CouplingSort::Name)
        .expect("get_coupling_metrics");

    assert_eq!(rows.len(), 3, "three crates expected");

    let by_name = |n: &str| rows.iter().find(|m| m.package.name == n).expect("crate present");

    let a = by_name("crate_a");
    assert_eq!((a.afferent, a.efferent), (0, 2), "crate_a Ca=0, Ce=2");
    assert!((a.instability - 1.0).abs() < 1e-9);

    let b = by_name("crate_b");
    assert_eq!((b.afferent, b.efferent), (1, 1), "crate_b Ca=1, Ce=1");
    assert!((b.instability - 0.5).abs() < 1e-9);

    let c = by_name("crate_c");
    assert_eq!((c.afferent, c.efferent), (2, 0), "crate_c Ca=2, Ce=0");
    assert!((c.instability - 0.0).abs() < 1e-9);
}

#[test]
fn coupling_sort_orders_match_spec() {
    let (_dir, tethys) = three_crate_workspace();

    let by_instability = tethys
        .get_coupling_metrics(CouplingSort::Instability)
        .expect("by I");
    let names_i: Vec<_> = by_instability.iter().map(|m| m.package.name.as_str()).collect();
    assert_eq!(names_i, ["crate_a", "crate_b", "crate_c"]);

    let by_name = tethys
        .get_coupling_metrics(CouplingSort::Name)
        .expect("by name");
    let names_n: Vec<_> = by_name.iter().map(|m| m.package.name.as_str()).collect();
    assert_eq!(names_n, ["crate_a", "crate_b", "crate_c"]);
}

#[test]
fn package_coupling_drilldown_for_middle_crate() {
    let (_dir, tethys) = three_crate_workspace();
    let detail = tethys
        .get_package_coupling("crate_b")
        .expect("query")
        .expect("found");

    let in_names: Vec<_> = detail.incoming.iter().map(|d| d.package.name.as_str()).collect();
    let out_names: Vec<_> = detail.outgoing.iter().map(|d| d.package.name.as_str()).collect();

    assert_eq!(in_names, ["crate_a"]);
    assert_eq!(out_names, ["crate_c"]);
}

#[test]
fn re_indexing_yields_identical_metrics() {
    let (_dir, mut tethys) = three_crate_workspace();
    let first = tethys.get_coupling_metrics(CouplingSort::Name).expect("first");
    tethys.index().expect("re-index");
    let second = tethys.get_coupling_metrics(CouplingSort::Name).expect("second");
    assert_eq!(first, second);
}

#[test]
fn empty_workspace_returns_empty_metrics() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut tethys = Tethys::new(dir.path()).expect("Tethys::new");
    tethys.index().expect("index");
    assert!(tethys.get_packages().expect("packages").is_empty());
    assert!(tethys
        .get_coupling_metrics(CouplingSort::default())
        .expect("metrics")
        .is_empty());
}
```

- [ ] **Step 2: Run the integration tests**

```bash
cargo nextest run -p tethys --test architecture
```
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/tethys/tests/architecture.rs
git commit -m "test(tethys): integration tests for coupling on three-crate fixture

Tests Ca/Ce/instability against a chain-plus-shortcut workspace where
each crate has distinct, non-trivial values (Ca=0/1/2, Ce=2/1/0). The
shortcut a→c distinguishes 'flipped Ca/Ce' bugs that a simple chain
would hide.

Refs: rivets-byie"
```

---

## Task 13: Property test for the instability formula

**Files:**
- Modify: `crates/tethys/src/db/architecture.rs`

- [ ] **Step 1: Write the property test**

Append to `crates/tethys/src/db/architecture.rs`:

```rust
#[cfg(test)]
mod instability_property_tests {
    use super::*;
    use proptest::prelude::*;
    use rusqlite::Connection;

    /// Build an in-memory DB with `n` packages and the listed cross-package edges,
    /// then query arch_coupling. Edges are (source_name, target_name) pairs.
    fn instability_for(n: usize, edges: &[(usize, usize)]) -> Vec<(u32, u32, f64)> {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(crate::db::SCHEMA).expect("schema");

        // Insert n packages named "p0".."pN".
        for i in 0..n {
            conn.execute(
                "INSERT INTO arch_packages (id, name, path, source) VALUES (?1, ?2, ?3, 'manifest')",
                rusqlite::params![i64::try_from(i + 1).unwrap(), format!("p{i}"), format!("p{i}")],
            )
            .expect("insert pkg");
        }
        // Insert a single dummy file per package so the FK in arch_file_packages
        // is not exercised. We bypass arch_file_packages and write directly to
        // arch_package_deps for the property test.
        for (src, tgt) in edges {
            if src == tgt {
                continue;
            }
            // Idempotent: ignore conflicts on the (src, tgt) PK.
            conn.execute(
                "INSERT OR IGNORE INTO arch_package_deps (source_pkg, target_pkg, dep_count)
                 VALUES (?1, ?2, 1)",
                rusqlite::params![
                    i64::try_from(src + 1).unwrap(),
                    i64::try_from(tgt + 1).unwrap()
                ],
            )
            .expect("insert dep");
        }

        let mut stmt = conn
            .prepare("SELECT afferent, efferent, instability FROM arch_coupling")
            .expect("prepare");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    u32::try_from(r.get::<_, i64>(0)?).unwrap_or(u32::MAX),
                    u32::try_from(r.get::<_, i64>(1)?).unwrap_or(u32::MAX),
                    r.get::<_, f64>(2)?,
                ))
            })
            .expect("query")
            .map(|r| r.unwrap())
            .collect();
        rows
    }

    proptest! {
        /// For every package and every random edge set, instability stays in [0, 1].
        #[test]
        fn instability_within_unit_interval(
            n in 1usize..8,
            edges in prop::collection::vec((0usize..8, 0usize..8), 0..30),
        ) {
            // Filter edges to be in-bounds for the chosen n.
            let edges: Vec<_> = edges.into_iter()
                .filter(|(s, t)| *s < n && *t < n)
                .collect();
            for (_ca, _ce, i) in instability_for(n, &edges) {
                prop_assert!((0.0..=1.0).contains(&i), "instability out of range: {i}");
            }
        }

        /// A package with no edges has instability exactly 0.
        #[test]
        fn isolated_package_has_zero_instability(n in 1usize..6) {
            let rows = instability_for(n, &[]);
            for (ca, ce, i) in rows {
                prop_assert_eq!((ca, ce), (0, 0));
                prop_assert!((i - 0.0_f64).abs() < 1e-9);
            }
        }
    }
}
```

- [ ] **Step 2: Run the property tests**

```bash
cargo nextest run -p tethys instability_property_tests
```
Expected: pass (proptest will run hundreds of cases under the hood).

- [ ] **Step 3: Commit**

```bash
git add crates/tethys/src/db/architecture.rs
git commit -m "test(tethys): property tests for instability formula

Verifies instability stays in [0, 1] across random graphs and that
isolated packages produce instability = 0 deterministically.

Refs: rivets-byie"
```

---

## Task 14: CLI scaffolding — Coupling subcommand and dispatch

**Files:**
- Create: `crates/tethys/src/cli/coupling.rs`
- Modify: `crates/tethys/src/cli/mod.rs`
- Modify: `crates/tethys/src/main.rs`

- [ ] **Step 1: Create coupling.rs with empty run() and SortFlag**

Create `crates/tethys/src/cli/coupling.rs`:

```rust
//! `tethys coupling` command implementation.
//!
//! Renders per-crate coupling metrics (Ca, Ce, instability) as a table, a
//! single-package detail view (`--package`), or JSON (`--json`).

use std::path::Path;

use clap::ValueEnum;
use tethys::{CouplingSort, Tethys};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum SortFlag {
    #[default]
    Instability,
    Ca,
    Ce,
    Name,
}

impl From<SortFlag> for CouplingSort {
    fn from(f: SortFlag) -> Self {
        match f {
            SortFlag::Instability => CouplingSort::Instability,
            SortFlag::Ca => CouplingSort::Afferent,
            SortFlag::Ce => CouplingSort::Efferent,
            SortFlag::Name => CouplingSort::Name,
        }
    }
}

/// Run the coupling command.
pub fn run(
    workspace: &Path,
    sort: SortFlag,
    package: Option<String>,
    json: bool,
) -> Result<(), tethys::Error> {
    let tethys = Tethys::new(workspace)?;

    if let Some(name) = package {
        run_detail(&tethys, &name, json)
    } else {
        run_table(&tethys, sort, json)
    }
}

fn run_table(_tethys: &Tethys, _sort: SortFlag, _json: bool) -> Result<(), tethys::Error> {
    // Implemented in Task 15.
    Ok(())
}

fn run_detail(_tethys: &Tethys, _name: &str, _json: bool) -> Result<(), tethys::Error> {
    // Implemented in Task 16.
    Ok(())
}
```

- [ ] **Step 2: Register the module in cli/mod.rs**

In `crates/tethys/src/cli/mod.rs`, add `pub mod coupling;` alongside the other module declarations (alphabetical):

```rust
pub mod coupling;
pub mod cycles;
// … rest unchanged
```

- [ ] **Step 3: Add the subcommand to main.rs**

In `crates/tethys/src/main.rs`, find the `Commands` enum and append a new variant (preserving alphabetical order roughly between `Callers` and `Cycles`):

```rust
/// Show per-crate coupling metrics (Ca, Ce, instability)
Coupling {
    /// Sort key
    #[arg(long, value_enum, default_value_t = cli::coupling::SortFlag::default())]
    sort: cli::coupling::SortFlag,

    /// Show detail for a single package by exact name (ignores --sort)
    #[arg(long)]
    package: Option<String>,

    /// Output as JSON
    #[arg(long)]
    json: bool,
},
```

Then in the dispatch `match cli.command { … }` block, add the new arm:

```rust
Commands::Coupling { sort, package, json } => {
    cli::coupling::run(&workspace, sort, package, json)
}
```

- [ ] **Step 4: Verify the build**

```bash
cargo build -p tethys
./target/debug/tethys coupling --help
```
Expected: clean build; help text shows `--sort`, `--package`, `--json` options.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/cli/coupling.rs crates/tethys/src/cli/mod.rs crates/tethys/src/main.rs
git commit -m "feat(tethys): scaffold tethys coupling CLI subcommand

Adds the Coupling variant to clap, wires dispatch to cli::coupling::run,
and stubs SortFlag (with ValueEnum) plus run()/run_table()/run_detail()
internals. Output formatting lands in follow-up tasks.

Refs: rivets-byie"
```

---

## Task 15: CLI — table output (text + JSON)

**Files:**
- Modify: `crates/tethys/src/cli/coupling.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/cli/coupling.rs`:

```rust
#[cfg(test)]
mod table_tests {
    use super::*;
    use tethys::{CouplingMetrics, Package, PackageId, PackageSource};

    fn pkg(name: &str) -> Package {
        Package {
            id: PackageId::from(1),
            name: name.into(),
            path: name.into(),
            source: PackageSource::Manifest,
        }
    }

    #[test]
    fn render_bar_uses_round_half_up() {
        assert_eq!(render_bar(0.00), "░░░░░░░░░░");
        assert_eq!(render_bar(0.25), "███░░░░░░░", "0.25 rounds up to 3");
        assert_eq!(render_bar(0.50), "█████░░░░░");
        assert_eq!(render_bar(0.75), "████████░░", "0.75 rounds up to 8");
        assert_eq!(render_bar(1.00), "██████████");
    }

    #[test]
    fn table_text_contains_all_packages_and_values() {
        let metrics = vec![
            CouplingMetrics {
                package: pkg("alpha"),
                afferent: 0,
                efferent: 1,
                instability: 1.0,
            },
            CouplingMetrics {
                package: pkg("beta"),
                afferent: 2,
                efferent: 1,
                instability: 0.33,
            },
        ];

        let mut buf = Vec::new();
        write_table_text(&mut buf, &metrics, SortFlag::Instability).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");

        assert!(s.contains("alpha"));
        assert!(s.contains("beta"));
        assert!(s.contains("1.00"));
        assert!(s.contains("0.33"));
        assert!(s.contains("2 packages"));
    }

    #[test]
    fn table_json_serializes_full_shape() {
        let metrics = vec![CouplingMetrics {
            package: pkg("alpha"),
            afferent: 0,
            efferent: 1,
            instability: 1.0,
        }];
        let mut buf = Vec::new();
        write_table_json(&mut buf, &metrics, SortFlag::Instability).expect("write json");
        let s = String::from_utf8(buf).expect("utf-8");
        let v: serde_json::Value = serde_json::from_str(&s).expect("parse json");

        assert_eq!(v["sort"], "instability");
        assert_eq!(v["count"], 1);
        assert_eq!(v["packages"][0]["name"], "alpha");
        assert_eq!(v["packages"][0]["afferent"], 0);
        assert_eq!(v["packages"][0]["efferent"], 1);
        assert_eq!(v["packages"][0]["instability"], 1.0);
        assert_eq!(v["packages"][0]["source"], "manifest");
    }

    #[test]
    fn table_text_for_empty_metrics_prints_friendly_message() {
        let mut buf = Vec::new();
        write_table_text(&mut buf, &[], SortFlag::Instability).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");
        assert!(s.contains("No packages discovered"));
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys table_tests
```
Expected: FAIL — `render_bar`, `write_table_text`, `write_table_json` do not exist.

- [ ] **Step 3: Implement table renderers**

Replace the stub `run_table` body in `crates/tethys/src/cli/coupling.rs`:

```rust
use std::io::{self, Write};
use colored::Colorize;
use tethys::{CouplingMetrics, PackageSource};

const BAR_WIDTH: usize = 10;

fn run_table(tethys: &Tethys, sort: SortFlag, json: bool) -> Result<(), tethys::Error> {
    let metrics = tethys.get_coupling_metrics(sort.into())?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if json {
        write_table_json(&mut out, &metrics, sort).map_err(io_error)
    } else {
        write_table_text(&mut out, &metrics, sort).map_err(io_error)
    }
}

fn io_error(e: io::Error) -> tethys::Error {
    tethys::Error::Io(e)
}

/// Render an N-character bar where the filled portion is round_half_up(value * N).
fn render_bar(value: f64) -> String {
    let clamped = value.clamp(0.0, 1.0);
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fill = (clamped * BAR_WIDTH as f64 + 0.5) as usize;
    let fill = fill.min(BAR_WIDTH);
    let filled: String = "█".repeat(fill);
    let empty: String = "░".repeat(BAR_WIDTH - fill);
    format!("{filled}{empty}")
}

fn instability_color(value: f64) -> impl Fn(&str) -> colored::ColoredString + Copy {
    move |s: &str| {
        if value <= 0.40 {
            s.green()
        } else if value <= 0.70 {
            s.yellow()
        } else {
            s.red()
        }
    }
}

fn sort_label(sort: SortFlag) -> &'static str {
    match sort {
        SortFlag::Instability => "instability (descending)",
        SortFlag::Ca => "Ca (descending)",
        SortFlag::Ce => "Ce (descending)",
        SortFlag::Name => "name (ascending)",
    }
}

pub(crate) fn write_table_text<W: Write>(
    out: &mut W,
    metrics: &[CouplingMetrics],
    sort: SortFlag,
) -> io::Result<()> {
    if metrics.is_empty() {
        writeln!(out)?;
        writeln!(out, "  No packages discovered.")?;
        writeln!(out, "  '{}' requires a Cargo workspace.", "tethys coupling".dimmed())?;
        writeln!(out)?;
        return Ok(());
    }

    writeln!(out)?;
    writeln!(out, "{}", "Tethys Coupling Metrics".cyan().bold())?;
    writeln!(out)?;
    writeln!(
        out,
        "  {}",
        "PACKAGE              Ca   Ce   INSTABILITY".white().dimmed()
    )?;

    let max_name_len = metrics
        .iter()
        .map(|m| m.package.name.len())
        .max()
        .unwrap_or(0)
        .max(20);

    for m in metrics {
        let bar = render_bar(m.instability);
        let color = instability_color(m.instability);
        writeln!(
            out,
            "  {name:width$}  {ca:>3}  {ce:>3}   {bar}  {value:>4}",
            name = m.package.name,
            width = max_name_len,
            ca = m.afferent,
            ce = m.efferent,
            bar = color(&bar),
            value = format!("{:.2}", m.instability),
        )?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "  {}",
        format!(
            "{} packages — sorted by {}",
            metrics.len(),
            sort_label(sort)
        )
        .dimmed()
    )?;
    writeln!(out)?;
    Ok(())
}

pub(crate) fn write_table_json<W: Write>(
    out: &mut W,
    metrics: &[CouplingMetrics],
    sort: SortFlag,
) -> io::Result<()> {
    let value = serde_json::json!({
        "sort": sort_key_str(sort),
        "count": metrics.len(),
        "packages": metrics.iter().map(|m| serde_json::json!({
            "name": m.package.name,
            "path": m.package.path.to_string_lossy(),
            "source": match m.package.source {
                PackageSource::Manifest => "manifest",
                PackageSource::Directory => "directory",
            },
            "afferent": m.afferent,
            "efferent": m.efferent,
            "instability": round_to_4(m.instability),
        })).collect::<Vec<_>>(),
    });
    serde_json::to_writer_pretty(&mut *out, &value).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(())
}

fn sort_key_str(sort: SortFlag) -> &'static str {
    match sort {
        SortFlag::Instability => "instability",
        SortFlag::Ca => "ca",
        SortFlag::Ce => "ce",
        SortFlag::Name => "name",
    }
}

fn round_to_4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}
```

You'll need `serde_json` available. It's already in `tethys/Cargo.toml` as a workspace dep. If `Error::Io` doesn't exist on `tethys::Error`, replace `io_error` with the existing IO error variant.

- [ ] **Step 4: Run the tests, expect pass**

```bash
cargo nextest run -p tethys table_tests
```
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/cli/coupling.rs
git commit -m "feat(tethys): table and JSON output for tethys coupling

Default-sorted, color-tiered table with bar chart (round-half-up over
10-char width) plus serde_json output for tooling. Empty-workspace
case emits a friendly 'no packages discovered' notice instead of an
empty table.

Refs: rivets-byie"
```

---

## Task 16: CLI — detail output (--package)

**Files:**
- Modify: `crates/tethys/src/cli/coupling.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/cli/coupling.rs`:

```rust
#[cfg(test)]
mod detail_tests {
    use super::*;
    use tethys::{CouplingDetail, CouplingMetrics, Package, PackageDependency, PackageId, PackageSource};

    fn pkg(name: &str) -> Package {
        Package {
            id: PackageId::from(1),
            name: name.into(),
            path: name.into(),
            source: PackageSource::Manifest,
        }
    }

    fn sample_detail() -> CouplingDetail {
        CouplingDetail {
            metrics: CouplingMetrics {
                package: pkg("rivets-mcp"),
                afferent: 3,
                efferent: 1,
                instability: 0.25,
            },
            outgoing: vec![PackageDependency {
                package: pkg("rivets"),
                dep_count: 5,
            }],
            incoming: vec![
                PackageDependency { package: pkg("cli-binary"), dep_count: 3 },
                PackageDependency { package: pkg("rivets-test"), dep_count: 2 },
                PackageDependency { package: pkg("rivets-bench"), dep_count: 1 },
            ],
        }
    }

    #[test]
    fn detail_text_includes_metrics_and_neighbors() {
        let mut buf = Vec::new();
        write_detail_text(&mut buf, &sample_detail()).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");

        assert!(s.contains("rivets-mcp"));
        assert!(s.contains("Afferent (Ca):   3"));
        assert!(s.contains("Efferent (Ce):   1"));
        assert!(s.contains("0.25"));
        assert!(s.contains("rivets"), "outgoing entry");
        assert!(s.contains("cli-binary"), "incoming entry");
        assert!(s.contains("5 edges"));
    }

    #[test]
    fn detail_json_serializes_full_shape() {
        let mut buf = Vec::new();
        write_detail_json(&mut buf, &sample_detail()).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");
        let v: serde_json::Value = serde_json::from_str(&s).expect("parse");

        assert_eq!(v["package"]["name"], "rivets-mcp");
        assert_eq!(v["afferent"], 3);
        assert_eq!(v["efferent"], 1);
        assert_eq!(v["instability"], 0.25);
        assert_eq!(v["outgoing"][0]["name"], "rivets");
        assert_eq!(v["outgoing"][0]["dep_count"], 5);
        assert_eq!(v["incoming"].as_array().unwrap().len(), 3);
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys detail_tests
```
Expected: FAIL — detail writers not yet implemented.

- [ ] **Step 3: Implement detail writers and wire run_detail**

In `crates/tethys/src/cli/coupling.rs`, replace the stub `run_detail` and add the detail formatters. Add or update:

```rust
use tethys::CouplingDetail;

fn run_detail(tethys: &Tethys, name: &str, json: bool) -> Result<(), tethys::Error> {
    let detail = tethys.get_package_coupling(name)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    match (detail, json) {
        (Some(d), true) => write_detail_json(&mut out, &d).map_err(io_error),
        (Some(d), false) => write_detail_text(&mut out, &d).map_err(io_error),
        (None, true) => {
            writeln!(out, "null").map_err(io_error)?;
            print_not_found_stderr(tethys, name)?;
            std::process::exit(1);
        }
        (None, false) => {
            print_not_found_stderr(tethys, name)?;
            std::process::exit(1);
        }
    }
}

fn print_not_found_stderr(_tethys: &Tethys, _name: &str) -> Result<(), tethys::Error> {
    // Suggestions land in Task 17. For now, print a minimal message.
    eprintln!("error: package not found");
    Ok(())
}

pub(crate) fn write_detail_text<W: Write>(out: &mut W, d: &CouplingDetail) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "Package: {}", d.metrics.package.name.cyan().bold())?;
    writeln!(out, "  Path:    {}", d.metrics.package.path.display())?;
    writeln!(
        out,
        "  Source:  {}",
        match d.metrics.package.source {
            PackageSource::Manifest => "manifest",
            PackageSource::Directory => "directory",
        }
    )?;
    writeln!(out)?;
    writeln!(out, "  {}", "Coupling".white().bold())?;
    writeln!(out, "    Afferent (Ca):   {}", d.metrics.afferent)?;
    writeln!(out, "    Efferent (Ce):   {}", d.metrics.efferent)?;
    let bar = render_bar(d.metrics.instability);
    let color = instability_color(d.metrics.instability);
    writeln!(
        out,
        "    Instability:     {bar}  {value:.2}",
        bar = color(&bar),
        value = d.metrics.instability
    )?;
    writeln!(out)?;

    if !d.outgoing.is_empty() {
        writeln!(out, "  {}", "Depends on (outgoing):".white().bold())?;
        for dep in &d.outgoing {
            let label = if dep.dep_count == 1 { "edge" } else { "edges" };
            writeln!(out, "    {:<18} {} {}", dep.package.name, dep.dep_count, label)?;
        }
        writeln!(out)?;
    }
    if !d.incoming.is_empty() {
        writeln!(out, "  {}", "Depended on by (incoming):".white().bold())?;
        for dep in &d.incoming {
            let label = if dep.dep_count == 1 { "edge" } else { "edges" };
            writeln!(out, "    {:<18} {} {}", dep.package.name, dep.dep_count, label)?;
        }
        writeln!(out)?;
    }
    Ok(())
}

pub(crate) fn write_detail_json<W: Write>(out: &mut W, d: &CouplingDetail) -> io::Result<()> {
    let value = serde_json::json!({
        "package": {
            "name": d.metrics.package.name,
            "path": d.metrics.package.path.to_string_lossy(),
            "source": match d.metrics.package.source {
                PackageSource::Manifest => "manifest",
                PackageSource::Directory => "directory",
            },
        },
        "afferent": d.metrics.afferent,
        "efferent": d.metrics.efferent,
        "instability": round_to_4(d.metrics.instability),
        "outgoing": d.outgoing.iter().map(|p| serde_json::json!({
            "name": p.package.name,
            "dep_count": p.dep_count,
        })).collect::<Vec<_>>(),
        "incoming": d.incoming.iter().map(|p| serde_json::json!({
            "name": p.package.name,
            "dep_count": p.dep_count,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_writer_pretty(&mut *out, &value).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(())
}
```

- [ ] **Step 4: Run the tests, expect pass**

```bash
cargo nextest run -p tethys detail_tests
```
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/cli/coupling.rs
git commit -m "feat(tethys): detail output for tethys coupling --package

Adds text and JSON detail-view formatters for a single package, with
incoming and outgoing dep lists. Not-found path exits 1 with a minimal
stderr message; rich suggestions land in the next task.

Refs: rivets-byie"
```

---

## Task 17: CLI — not-found suggestions

**Files:**
- Modify: `crates/tethys/src/cli/coupling.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/tethys/src/cli/coupling.rs`:

```rust
#[cfg(test)]
mod suggestion_tests {
    use super::*;

    #[test]
    fn suggestions_for_substring_match_only() {
        let names = vec![
            "auth-server".to_string(),
            "auth-client".to_string(),
            "billing".to_string(),
        ];
        let s = collect_suggestions("auth", &names);
        assert!(s.contains(&"auth-server".to_string()));
        assert!(s.contains(&"auth-client".to_string()));
        assert!(!s.contains(&"billing".to_string()));
    }

    #[test]
    fn suggestions_empty_when_nothing_matches() {
        let names = vec!["alpha".to_string(), "beta".to_string()];
        assert!(collect_suggestions("zzz", &names).is_empty());
    }

    #[test]
    fn suggestions_capped_at_five() {
        let names: Vec<_> = (0..10).map(|i| format!("auth-{i}")).collect();
        let s = collect_suggestions("auth", &names);
        assert_eq!(s.len(), 5);
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

```bash
cargo nextest run -p tethys suggestion_tests
```
Expected: FAIL — `collect_suggestions` not defined.

- [ ] **Step 3: Implement collect_suggestions and wire it into print_not_found_stderr**

In `crates/tethys/src/cli/coupling.rs`, replace the `print_not_found_stderr` stub and add `collect_suggestions`:

```rust
const MAX_SUGGESTIONS: usize = 5;

fn collect_suggestions(name: &str, all_names: &[String]) -> Vec<String> {
    let needle = name.to_lowercase();
    all_names
        .iter()
        .filter(|n| n.to_lowercase().contains(&needle))
        .take(MAX_SUGGESTIONS)
        .cloned()
        .collect()
}

fn print_not_found_stderr(tethys: &Tethys, name: &str) -> Result<(), tethys::Error> {
    eprintln!("{}: no package named '{}' found", "error".red().bold(), name);

    let pkgs = tethys.get_packages()?;
    let names: Vec<String> = pkgs.into_iter().map(|p| p.name).collect();
    let suggestions = collect_suggestions(name, &names);
    if !suggestions.is_empty() {
        eprintln!();
        eprintln!("Did you mean: {}?", suggestions.join(", "));
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests, expect pass**

```bash
cargo nextest run -p tethys suggestion_tests
```
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tethys/src/cli/coupling.rs
git commit -m "feat(tethys): suggest similar names when --package is unknown

On a not-found, stderr lists up to 5 packages whose names contain the
queried substring (case-insensitive). Stdout still emits 'null' under
--json, so jq-style consumers see a structured answer.

Refs: rivets-byie"
```

---

## Task 18: README — document tethys coupling

**Files:**
- Modify: `crates/tethys/README.md`

- [ ] **Step 1: Add `coupling` to the commands table**

In `crates/tethys/README.md`, locate the `## Commands` table and append a new row:

```markdown
| `coupling` | Per-crate coupling metrics (Ca, Ce, instability) |
```

- [ ] **Step 2: Add a usage example near the top**

In the `## Quick Start` section, append:

````markdown
# View per-crate coupling metrics
tethys coupling

# Sort alphabetically
tethys coupling --sort name

# Drill into one package
tethys coupling --package my-crate

# JSON for tooling
tethys coupling --json
````

- [ ] **Step 3: Verify rendered output**

```bash
cat crates/tethys/README.md
```
Expected: new row appears in the table; the quick-start block lists the four invocations.

- [ ] **Step 4: Commit**

```bash
git add crates/tethys/README.md
git commit -m "docs(tethys): document tethys coupling command

Adds a Commands-table row and four quick-start invocations.

Refs: rivets-byie"
```

---

## Task 19: Final regression check on the rivets workspace itself

**Files:**
- (none — verification only)

- [ ] **Step 1: Re-build and run the binary against the rivets workspace**

```bash
cargo build --release
./target/release/tethys index
./target/release/tethys coupling
```

Expected: command produces a table containing every workspace crate (`rivets`, `rivets-jsonl`, `rivets-mcp`, `tethys`) with sensible Ca/Ce values. `Ca + Ce > 0` for the most-connected packages. No errors, no warnings.

- [ ] **Step 2: Verify --json output is valid JSON**

```bash
./target/release/tethys coupling --json | python -c "import json,sys; json.load(sys.stdin); print('ok')"
```
(Run with `uv run --no-project python` if `python` is not on PATH.)
Expected: `ok`.

- [ ] **Step 3: Drill into one crate**

```bash
./target/release/tethys coupling --package tethys
```
Expected: detail view showing tethys's incoming/outgoing relationships (likely incoming from `rivets-mcp`, outgoing to `rivets-jsonl` if there are any inter-crate references; values may be empty if no cross-crate references exist).

- [ ] **Step 4: Run the full test suite**

```bash
cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
Expected: every test passes; clippy clean; fmt clean.

- [ ] **Step 5: Final commit if any fmt fixes were needed**

If `cargo fmt --check` flagged anything (it shouldn't for plan-following code, but format drift can happen):

```bash
cargo fmt
git add -u
git commit -m "style(tethys): cargo fmt"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Implemented in |
|---|---|
| Schema (3 tables + view) | Task 2 |
| Indexing phase (DELETE-cascade-rebuild) | Tasks 5, 10 |
| Public API (get_packages / get_coupling_metrics / get_package_coupling) | Tasks 6, 7, 8, 11 |
| Types (Package, CouplingMetrics, CouplingSort, …) | Task 1 |
| `IndexStats.architecture` field | Task 9 |
| CLI — table | Task 15 |
| CLI — `--package` detail | Task 16 |
| CLI — `--json` (table & detail) | Tasks 15, 16 |
| CLI — not-found + suggestions | Task 17 |
| Empty-workspace handling | Tasks 11, 12, 15 |
| Tests — unit | Tasks 1, 2, 3, 5–8, 13, 15–17 |
| Tests — integration on three-crate fixture | Task 12 |
| Tests — property-based | Task 13 |
| Tests — regression on rivets workspace | Task 19 |
| README | Task 18 |

No spec section is unimplemented.

**Placeholder scan:** none of "TBD" / "TODO" / "implement later" / "similar to Task N" / "fill in details" appear in any task body.

**Type consistency:** every public type referenced in later tasks is defined in Task 1. `SortFlag` (CLI clap enum) is defined in Task 14 and consistently used in Tasks 15–17. Method signatures on `Tethys` and `Index` match between definition and call sites.

---

Plan complete and saved to `docs/plans/2026-05-10-tethys-architecture-analysis.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using `executing-plans`, batch execution with checkpoints.

Which approach?
