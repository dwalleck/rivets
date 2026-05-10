# Tethys: Architecture Analysis (Coupling Metrics)

**Issue:** [`rivets-byie`](../../.rivets/issues.jsonl)
**Status:** Design approved 2026-05-10
**Inspired by:** KiroGraph's architecture/coupling commands; see `crates/tethys/KIROGRAPH-COMPARISON.md`

## Summary

Add an always-on indexing phase that materializes per-crate package metadata, plus a `tethys coupling` CLI command and matching public API that report afferent coupling (`Ca`), efferent coupling (`Ce`), and instability per Cargo crate. These are Robert C. Martin's classic OO-design metrics, applied at the crate boundary in a Rust workspace.

The metrics ship as the killer view first; the broader architecture-analysis subsystem (layer detection, `tethys architecture` and `tethys package` commands, MCP tools) is explicitly deferred to follow-up work.

## Scope

### In scope (v1)

- New SQLite schema: three persisted tables and one view, all `IF NOT EXISTS`.
- New always-on indexing phase, run as the final write step.
- New public types and `Tethys` API methods for packages and coupling.
- New CLI subcommand `tethys coupling` with `--sort`, `--package`, `--json`.
- Tests: unit, integration, property, CLI snapshot, regression on the rivets workspace.

### Deferred (sibling issues)

- Layer detection (api/service/data/ui/shared from path patterns).
- `tethys architecture` CLI command (full graph view).
- `tethys package <NAME>` CLI command (file-list drill-down).
- MCP tool wiring (`rivets-o4re`).
- Directory-fallback packages for files outside any `Cargo.toml`.
- Sample file pairs in `arch_package_deps` (KiroGraph stores up to 5 per edge).

### Explicitly not goals

- Replacing `cargo metadata` for declared-dependency analysis. We derive coupling from *observed* cross-file references (file_deps), not from `[dependencies]` tables. This captures actual usage rather than declared intent — and matches the structural-coupling concept more closely.
- Changing existing tables, indexes, or query plans.

## Background

Tethys already has every input it needs:

- `cargo::discover_crates(workspace_root)` returns `Vec<CrateInfo>` with crate name and canonical path. Cached on `Tethys::crates`.
- `Tethys::get_crate_for_file(path)` resolves a file to its containing crate using "longest path prefix wins."
- The `file_deps` table is populated during the existing dependency-resolution pass, with rows `(from_file_id, to_file_id, ref_count)`.

This feature is therefore an aggregation layer: roll `file_deps` up to crate level, expose the result.

## Data flow

```
tethys index
├─ existing phases (parse → write symbols → resolve refs → file_deps → call_edges)
└─ NEW: run_architecture_phase()
      DELETE FROM arch_packages          -- cascades through children
      INSERT crates    INTO arch_packages
      INSERT files     INTO arch_file_packages    -- one row per file in a crate
      INSERT GROUP BY  INTO arch_package_deps     -- single SQL pass over file_deps

tethys coupling
└─ SELECT ... FROM arch_coupling JOIN arch_packages ORDER BY <key>
```

Reads always go through the `arch_coupling` SQL view, which derives Ca/Ce/instability from `arch_package_deps` on every query. This eliminates a class of staleness bug — the view cannot drift from the underlying edge table.

## Schema

Added to the existing `SCHEMA` constant in `db/schema.rs`. All objects use `IF NOT EXISTS` so existing databases pick them up on next open.

```sql
-- One row per discovered package. v1 only emits source = 'manifest'.
CREATE TABLE IF NOT EXISTS arch_packages (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE,
    path   TEXT NOT NULL,                     -- relative to workspace_root
    source TEXT NOT NULL CHECK(source IN ('manifest','directory'))
);

CREATE INDEX IF NOT EXISTS idx_arch_packages_path ON arch_packages(path);

-- File → package assignment. file_id is PK: each file belongs to exactly one package.
CREATE TABLE IF NOT EXISTS arch_file_packages (
    file_id    INTEGER PRIMARY KEY REFERENCES files(id)         ON DELETE CASCADE,
    package_id INTEGER NOT NULL    REFERENCES arch_packages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_arch_fp_pkg ON arch_file_packages(package_id);

-- Cross-package dependency edges, rolled up from file_deps.
-- CHECK forbids self-edges; intra-package deps are filtered at INSERT time.
CREATE TABLE IF NOT EXISTS arch_package_deps (
    source_pkg INTEGER NOT NULL REFERENCES arch_packages(id) ON DELETE CASCADE,
    target_pkg INTEGER NOT NULL REFERENCES arch_packages(id) ON DELETE CASCADE,
    dep_count  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (source_pkg, target_pkg),
    CHECK (source_pkg <> target_pkg)
);

CREATE INDEX IF NOT EXISTS idx_arch_pkgdep_tgt ON arch_package_deps(target_pkg);

-- Coupling metrics view. LEFT JOINs keep packages with zero edges (Ca=Ce=0 → I=0).
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

### Schema design notes

- **Integer IDs** for `arch_packages.id` follow the existing tethys convention (`FileId`, `SymbolId` are integer newtypes). Crate `name` is the human identifier; `id` is internal.
- **`arch_file_packages` uses `file_id` as PK** to enforce "one file → one package" at the schema level rather than relying on application logic.
- **`CHECK (source_pkg <> target_pkg)`** is defense-in-depth. The INSERT statement filters intra-crate deps, but the constraint prevents any future writer from violating the invariant.
- **`instability = 0` when both `Ca` and `Ce` are zero.** The standard formula is undefined at the origin; convention is to treat an isolated package as stable. Matches KiroGraph behavior and Martin's intent.
- **`LEFT JOIN` against subqueries** keeps packages with zero outgoing edges visible in `arch_coupling`. A naive `INNER JOIN ... GROUP BY` would silently drop isolated packages — and isolated packages are often the most diagnostically interesting (orphans, dead code, pre-extraction candidates).

## Indexing phase

### Module layout

- New file: **`src/db/architecture.rs`** — DB-layer SQL operations.
- Orchestration in **`src/indexing.rs`** via a new private method `run_architecture_phase` on `Tethys`, called as the last write step of `index_with_options`.

### DB-layer surface

```rust
// src/db/architecture.rs

pub struct ArchStats {
    pub packages_recorded:     usize,
    pub files_assigned:        usize,
    pub package_deps_recorded: usize,
}

pub struct PackageInsert<'a> {
    pub name:   &'a str,
    pub path:   &'a str,            // relative to workspace_root
    pub source: PackageSource,      // v1: always Manifest
}

impl Index {
    /// Rebuild all arch_* tables from current files + file_deps state.
    /// Idempotent: identical input produces identical state.
    /// Wrapped in a single transaction.
    pub fn repopulate_architecture(
        &self,
        packages: &[PackageInsert],
        file_to_package_name: &[(FileId, &str)],
    ) -> Result<ArchStats>;
}
```

The DB layer accepts pre-resolved `(FileId, crate_name)` tuples. Path canonicalization stays in the orchestration layer where `get_crate_for_file` already lives.

### Orchestration

```rust
fn run_architecture_phase(&self) -> Result<ArchStats> {
    // 1. Materialize crate list as PackageInsert.
    let packages: Vec<PackageInsert> = self.crates.iter()
        .map(|c| PackageInsert {
            name:   &c.name,
            path:   self.relative_path(&c.path).to_str().unwrap_or(""),
            source: PackageSource::Manifest,
        })
        .collect();

    // 2. Map every indexed file to its containing crate using existing logic.
    //    Files outside any crate are skipped silently (logged at trace!).
    let mut file_to_package = Vec::new();
    for indexed_file in self.db.list_all_files()? {
        if let Some(crate_info) = self.get_crate_for_file(&indexed_file.path) {
            file_to_package.push((indexed_file.id, crate_info.name.as_str()));
        }
    }

    self.db.repopulate_architecture(&packages, &file_to_package)
}
```

### SQL inside `repopulate_architecture`

Single transaction:

```sql
BEGIN;

-- Cascade clears arch_file_packages and arch_package_deps.
DELETE FROM arch_packages;

-- Batched prepared statement.
INSERT INTO arch_packages (name, path, source) VALUES (?, ?, ?);

-- Read back name → id map after package inserts complete (small loop).

-- Batched prepared statement.
INSERT INTO arch_file_packages (file_id, package_id) VALUES (?, ?);

-- Single GROUP BY pass.
INSERT INTO arch_package_deps (source_pkg, target_pkg, dep_count)
SELECT sp.package_id, tp.package_id, COUNT(*)
FROM file_deps fd
JOIN arch_file_packages sp ON sp.file_id = fd.from_file_id
JOIN arch_file_packages tp ON tp.file_id = fd.to_file_id
WHERE sp.package_id <> tp.package_id
GROUP BY sp.package_id, tp.package_id;

COMMIT;
```

### Why DELETE-then-rebuild rather than UPSERT

Using `INSERT OR REPLACE` keyed on `arch_packages.name` would generate new auto-incremented `id` values, silently invalidating `arch_file_packages` and `arch_package_deps` rows that referenced the old IDs. The cascade-then-rebuild pattern makes that whole class of bug impossible — there's no path where a stale FK survives.

### `dep_count` semantics

`dep_count = COUNT(*)` of cross-package file→file edges, **not** `SUM(file_deps.ref_count)`. Two crates with 5 file→file edges have the same `dep_count` whether each edge has 1 or 100 references. This matches structural-coupling intent better than weighting by raw reference volume.

The Ca/Ce computation in the view is independent of `dep_count` — it counts distinct `arch_package_deps` rows, not their weights.

### Pipeline placement

```
index_with_options(opts)
├─ existing phases
└─ NEW: run_architecture_phase()    ← single transaction, final write
```

The phase only runs if all prior phases succeeded. We never want to write architecture data based on an incomplete dependency graph.

### Performance

For the rivets workspace (~4 crates, ~600 files, ~3k file_deps): low single-digit milliseconds. For a hypothetical 100-crate, 10k-file, 100k-file_deps workspace: well under a second. Negligible compared to parsing.

## Public API

Types live in `src/types.rs`; methods are added in a new `// === Architecture ===` section of the `impl Tethys` block in `src/lib.rs`.

### Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId(i64);

impl PackageId {
    pub fn as_i64(self) -> i64 { self.0 }
}
impl From<i64> for PackageId { fn from(id: i64) -> Self { Self(id) } }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSource {
    Manifest,
    Directory,   // future: directory-fallback for non-manifest files
}

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub id:     PackageId,
    pub name:   String,
    pub path:   PathBuf,
    pub source: PackageSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CouplingMetrics {
    pub package:     Package,
    pub afferent:    u32,
    pub efferent:    u32,
    pub instability: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CouplingSort {
    #[default] Instability,
    Afferent,
    Efferent,
    Name,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackageDependency {
    pub package:   Package,
    pub dep_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CouplingDetail {
    pub metrics:  CouplingMetrics,
    pub incoming: Vec<PackageDependency>,
    pub outgoing: Vec<PackageDependency>,
}
```

### Methods

```rust
impl Tethys {
    /// List all packages discovered during the last index run.
    /// Empty for non-Rust workspaces or before any index has run.
    pub fn get_packages(&self) -> Result<Vec<Package>>;

    /// Coupling metrics for every package, sorted per the requested key.
    /// Sort delegated to SQLite via ORDER BY.
    pub fn get_coupling_metrics(&self, sort: CouplingSort) -> Result<Vec<CouplingMetrics>>;

    /// Detailed coupling for one package by exact name match.
    /// Returns Ok(None) when no package matches.
    pub fn get_package_coupling(&self, name: &str) -> Result<Option<CouplingDetail>>;
}
```

### Contracts

- **Exact name matching only.** Fuzzy resolution is a UI concern that lives in the CLI layer; the API stays strict so future MCP tools can rely on deterministic behavior.
- **Empty workspace.** Non-Rust workspace → `Vec::new()` from `get_packages()` and `get_coupling_metrics()`. Not an error.
- **Single-crate workspace.** Returns one row with Ca=Ce=0, instability=0. Important: this case stays visible thanks to the view's LEFT JOINs.
- **`Ok(None)` on not-found in `get_package_coupling`.** Reserves `Result::Err` for actual failures (DB I/O, schema corruption). Matches the existing pattern of `get_symbol`, `get_file`.

### `IndexStats` extension

```rust
pub struct IndexStats {
    // ... existing fields
    pub architecture: Option<ArchStats>,
}
```

`Option` is forward-looking: a future opt-out flag (e.g. `--no-architecture`) leaves the field as `None` without changing types.

## CLI

### Subcommand

```
tethys coupling [--sort <KEY>] [--package <NAME>] [--json]
```

| Flag | Type | Default | Notes |
|---|---|---|---|
| `--sort` | `instability \| ca \| ce \| name` | `instability` | Ignored when `--package` is set. |
| `--package <NAME>` | string | none | Exact crate-name match. Shows detail view. |
| `--json` | bool | false | Structured output. |

Implementation in `src/cli/coupling.rs` follows the `cli/stats.rs` pattern: free function `run(workspace, args)` returning `Result<(), tethys::Error>`. A `clap::ValueEnum` `SortFlag` in the CLI layer converts to the API's `CouplingSort`, keeping clap concerns out of the public API.

### Default table output

```
Tethys Coupling Metrics

  PACKAGE              Ca   Ce   INSTABILITY
  rivets-jsonl          0    1   ██████████  1.00
  rivets                1    3   ████████░░  0.75
  tethys                2    1   ███░░░░░░░  0.33
  rivets-mcp            3    1   ███░░░░░░░  0.25

  4 packages — sorted by instability (descending)
```

Visual conventions:
- Header `"Tethys Coupling Metrics"`: cyan + bold (matches `tethys stats`).
- Column header row: white, dim, all caps.
- Bar: 10 chars wide, `█` filled / `░` empty, fill width = `round_half_up(instability * 10.0)`.
- Bar color tier:
  - Green for `instability ≤ 0.40` (stable)
  - Yellow for `0.40 < I ≤ 0.70` (neutral)
  - Red for `I > 0.70` (unstable)
- Numeric columns right-aligned.
- Footer: dim count line.
- Color stripping when stdout is not a TTY: handled automatically by the `colored` crate.

### Detail output (`--package <NAME>`)

```
Package: rivets-mcp
  Path:    crates/rivets-mcp
  Source:  manifest

  Coupling
    Afferent (Ca):   3
    Efferent (Ce):   1
    Instability:     ███░░░░░░░  0.25

  Depends on (outgoing):
    rivets             5 edges

  Depended on by (incoming):
    cli-binary         3 edges
    rivets-test        2 edges
    rivets-bench       1 edge
```

Incoming and outgoing lists sorted by `dep_count` descending, then by name.

### Not-found handling

```
error: no package named 'auth' found

Did you mean: auth-server, auth-client?
```

Exit code 1. Suggestions are derived by simple substring containment over `get_packages()` — no extra fuzzy-match dependency.

### Empty workspace handling

```
No packages discovered.
'tethys coupling' requires a Cargo workspace.
```

Exit code 0. Matches the way `tethys stats` would render an empty index — not an error, just nothing to report.

### JSON output

`--json` (table mode):
```json
{
  "sort": "instability",
  "count": 4,
  "packages": [
    {
      "name": "rivets-jsonl",
      "path": "crates/rivets-jsonl",
      "source": "manifest",
      "afferent": 0,
      "efferent": 1,
      "instability": 1.0
    }
  ]
}
```

`--json` (detail mode):
```json
{
  "package": {
    "name": "rivets-mcp",
    "path": "crates/rivets-mcp",
    "source": "manifest"
  },
  "afferent": 3,
  "efferent": 1,
  "instability": 0.25,
  "outgoing": [
    {"name": "rivets", "dep_count": 5}
  ],
  "incoming": [
    {"name": "cli-binary",  "dep_count": 3},
    {"name": "rivets-test", "dep_count": 2},
    {"name": "rivets-bench","dep_count": 1}
  ]
}
```

`--json` not-found prints `null` on stdout and a one-line error on stderr; exit 1. Mirrors `jq`-friendly conventions.

`instability` is rounded to 4 decimals at serialization to keep diffs stable across runs.

### Behavior matrix

| Flags | Behavior |
|---|---|
| (none) | Sorted-by-instability table for all packages |
| `--sort name` | Same shape, alphabetical |
| `--package X` | Detail view; ignores `--sort` |
| `--package X --json` | Detail JSON; ignores `--sort` |
| `--json` | Table JSON |
| `--package X` (X not found) | Error message + suggestions; exit 1 |
| Empty workspace | "No packages" message; exit 0 |

## Error handling

| Situation | Behavior |
|---|---|
| DB I/O failure during phase | Transaction rolls back; previous good state preserved; `Error::Database` propagates from `Tethys::index_with_options`. |
| Non-Rust workspace | `arch_packages` ends up empty after the phase runs; no error. CLI prints "No packages discovered." |
| File outside any crate | Skipped from `arch_file_packages`. Logged at `trace!` with file path. |
| Schema CHECK violation (e.g. self-edge somehow inserted) | SQLite raises CHECK error → `Error::Database`. Defense-in-depth — should never fire. |
| `--package NAME` not found | API returns `Ok(None)`; CLI prints suggestions to stderr; exit 1. |
| Coupling queried before any index has run | API returns empty Vec; CLI prints "No packages discovered." |
| Coupling queried while index mid-update | Standard SQLite reader semantics: sees pre-transaction state. No special handling. |

## Edge cases

- Workspace with no `Cargo.toml`: phase runs, deletes nothing, inserts nothing. Coupling output empty.
- Single-crate workspace: one row, Ca=Ce=0, instability=0.
- Cyclic deps (A → B and B → A): both rows Ca=1, Ce=1, instability=0.5. The CHECK constraint forbids only self-edges, not cycles.
- Isolated crate within a multi-crate workspace: stays visible with Ca=Ce=0, instability=0.
- Crate path is a prefix of another (workspace member containing a sub-member): `get_crate_for_file` resolves with longest-prefix-wins.
- Re-index after a crate is removed: `DELETE FROM arch_packages` cascades; new INSERT misses the removed crate. Clean.

## Testing

### Unit tests

In `src/db/architecture.rs` and `src/types.rs`:

- `CouplingSort::default() == CouplingSort::Instability` (default contract).
- `PackageId` round-trip (`From<i64>` → `as_i64()`).
- Instability formula in isolation against tabular cases: (0,0)→0.0, (0,5)→1.0, (5,0)→0.0, (3,1)→0.25.

### Integration tests

In `tests/architecture.rs`. A multi-crate fixture under `tests/fixtures/multi_crate/`:

```
multi_crate/
  Cargo.toml             [workspace]
  crate_a/  → uses items from crate_b and crate_c
  crate_b/  → uses items from crate_c
  crate_c/  → leaf
```

Expected coupling:

| Package | Ca | Ce | I |
|---|---|---|---|
| crate_a | 0 | 2 | 1.00 |
| crate_b | 1 | 1 | 0.50 |
| crate_c | 2 | 0 | 0.00 |

Tests verify:
- `get_packages()` returns 3 packages with correct names, paths, source = `Manifest`.
- `get_coupling_metrics(Instability)` returns rows in expected order.
- `get_coupling_metrics(Name)` returns alphabetical order.
- `get_package_coupling("crate_b")` returns `CouplingDetail` with `incoming = [{crate_a, count}]` and `outgoing = [{crate_c, count}]`.
- `get_package_coupling("does-not-exist")` returns `Ok(None)`.
- Re-indexing twice yields identical results (idempotency).

The fixture is the smallest configuration where Ca, Ce, and instability take three distinct non-trivial values across the packages — a simpler A→B fixture would leave instability ∈ {0.0, 1.0}, missing bugs that flip Ca and Ce in the formula.

### Property tests (proptest)

Random `(packages, edges)` graphs inserted directly via SQL:
- `0.0 ≤ instability ≤ 1.0` for all rows.
- `Ca + Ce ≤ 2 × |packages_with_edges|`.
- Round-trip of `CouplingSort` through CLI flag parsing.

### CLI snapshot tests

In `tests/cli_coupling.rs` using `assert_cmd`. Run against the fixture and snapshot:
- `tethys coupling` (default table)
- `tethys coupling --sort name`
- `tethys coupling --package crate_b`
- `tethys coupling --json`
- `tethys coupling --package does-not-exist` (stderr + exit 1)
- `tethys coupling` against a non-Rust workspace fixture (empty case)

### Regression test on rivets workspace

A test that runs the phase against the actual rivets workspace and asserts:
- Each of `rivets`, `rivets-jsonl`, `rivets-mcp`, `tethys` is present in `get_packages()`.
- The phase completes without error.
- `Ca + Ce > 0` for at least the most-connected packages.

A smoke test — exact metric values are not asserted because they will drift naturally as the codebase evolves; only structural properties are checked.

## References

- Robert C. Martin, *OO Design Quality Metrics: An Analysis of Dependencies* (1994). Origin of Ca / Ce / instability.
- KiroGraph architecture analysis: `crates/tethys/KIROGRAPH-COMPARISON.md`.
- Sibling issue `rivets-o4re` for MCP server (will expose `tethys_coupling` once this lands).
