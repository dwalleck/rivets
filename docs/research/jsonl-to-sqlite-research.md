# JSONL → SQLite Migration Research

**Status**: Research Complete
**Date**: 2026-08-09
**Related**: [Storage Architecture](../storage-architecture.md), [Architecture Overview](../architecture.md), [Event Sourcing Design](../design/event-sourcing.md), [Automerge Research](automerge-research.md)

This note assesses the level of effort and the strategic/technical implications of migrating Rivets' issue-tracking persistence from JSONL (`.rivets/issues.jsonl`) to SQLite. It is research only: no code, manifests, or existing documentation were changed.

Every material claim is cited inline. Repository citations use repo-relative paths with line ranges or exact symbol/test names; web citations link official primary sources (sqlite.org, docs.rs). Observation (what the code/docs say), recommendation (what I propose), and uncertainty (what cannot be resolved from primary sources alone) are distinguished in the text.

---

## Table of Contents

1. [Executive Conclusion](#1-executive-conclusion)
2. [Assumptions and Scope](#2-assumptions-and-scope)
3. [Current-State Inventory](#3-current-state-inventory)
4. [Target Options](#4-target-options)
5. [Affected Components and Implementation Slices](#5-affected-components-and-implementation-slices)
6. [Proposed Schema, Migration, and Rollback Outline](#6-proposed-schema-migration-and-rollback-outline)
7. [Behavior / Test Matrix](#7-behavior--test-matrix)
8. [Quantified Level of Effort](#8-quantified-level-of-effort)
9. [Risks and Unknowns](#9-risks-and-unknowns)
10. [Recommendation](#10-recommendation)
11. [Sources](#11-sources)

---

## 1. Executive Conclusion

**Replacing JSONL with SQLite as the repository-facing persistence format is not advisable for Rivets' stated product goals today.** The product's differentiating promise is a Git-native, human-readable, text-tool-accessible issue store that "branch[es] with your code, merge[s] with your PRs" and can be "grep[ped], diff[ed], and edit[ed] directly" (`README.md:14`, `README.md:20`). SQLite's own documentation states that text tools such as grep and awk are not useful against a database file (`https://www.sqlite.org/appfileformat.html#_3_4_accessible_content`), and WAL mode adds quasi-persistent `-wal`/`-shm` companion files that SQLite itself flags as a downside for application file formats (`https://www.sqlite.org/wal.html#_1_overview`, disadvantage 6). A hard cutover would trade away that load-bearing format property for stronger transaction and multi-process coordination. The coordination benefit is real—the agent guide says Assignment updates are not cross-process atomic until `rivets-8rj9` lands—but no primary source quantifies how often concurrent writers collide (`AGENTS.md` "Quick Reference").

The migration **is** technically well-scoped and moderate-sized, because the architecture was built for backend swaps:

- All command and MCP code already goes through one object-safe `IssueStorage` trait (`crates/rivets/src/storage/mod.rs:110`), and the backend factory already anticipates non-JSONL backends (`crates/rivets/src/storage/mod.rs:391-401`, `:648-682`).
- The sibling [Tethys repository](https://github.com/dwalleck/tethys) demonstrates the likely SQLite stack: `rusqlite` with the `bundled` feature ([`Cargo.toml`](https://github.com/dwalleck/tethys/blob/main/Cargo.toml)) plus `busy_timeout`, `PRAGMA journal_mode=WAL`, `foreign_keys=ON`, and a `Mutex<Connection>` ([`Index::open`](https://github.com/dwalleck/tethys/blob/main/src/db/mod.rs)). Rivets already gitignores `.rivets/index/` as a regenerated SQLite index (`.gitignore:37-38`).
- The persisted record shape is isolated behind `IssueRecord`/`CanonicalIssueRecord` (`crates/rivets/src/storage/in_memory/issue_record.rs:276-313,437-507`), but those DTOs are private to the `storage::in_memory` module (`crates/rivets/src/storage/in_memory/mod.rs:76-90`). Reuse requires first relocating the compatibility adapter to a storage-level module rather than creating a second wire-format convention.

**Estimate**: a production-grade hard cutover (SQLite as the only store, with JSONL export for portability/rollback) is roughly **23–33 engineer-days after the accepted relationship/readiness model is implemented**. If the SQLite project must also remove the current generic relationship model and parent-blocking implementation debt, budget **27–40 engineer-days**. Keeping JSONL as a co-equal dual backend adds **2–4 days**; building SQLite only as a *derived query index* over the existing JSONL is **5–8 days**. The dominant cost drivers are not storage mechanics but: (1) implementing the full storage interface against the canonical relationship model, (2) redefining the "resilient partial load" contract (which has no SQLite analogue), and (3) a behavioral-parity test matrix across CLI, MCP, and legacy-field migration.

**Recommended path, in order of urgency**: keep JSONL as source of truth; (a) if query performance or a read-heavy daemon becomes the driver, add a regenerable SQLite *index* in the already-gitignored `.rivets/index/` location (`.gitignore:37-38` precedent); (b) if concurrent-writer correctness is the immediate driver, use SQLite as an optional authoritative backend or put all writes behind a single-writer daemon; (c) an append-only event log remains attractive for clean Git diffs and audit history, but its current design uses only a process-local `Mutex` (`docs/design/event-sourcing.md:263-307`) and does not by itself solve cross-process writes; (d) only make SQLite the default if maintainers explicitly choose database transactions over Rivets' current text/Git format contract.

---

## 2. Assumptions and Scope

### 2.1 Assumptions

- **Scale**: Real-world data is small. This repository's own `.rivets/issues.jsonl` was observed at 252 issues / ~430 KB on 2026-08-09, and the documented load target is 1,000 issues in <200 ms (`docs/architecture.md` "Performance Targets"). Performance is therefore not the demonstrated migration driver; correctness and format properties are.
- **Concurrency model**: One CLI invocation is short-lived and single-threaded, and one MCP server serializes access to each cached workspace through `Arc<RwLock<Box<dyn IssueStorage>>>` (`crates/rivets-mcp/src/context.rs:34-45,130-150`). Separate CLI and MCP processes can still execute the JSONL load-modify-save sequence concurrently; there is no cross-process lock, and Assignment claims are explicitly documented as non-atomic until `rivets-8rj9` lands (`AGENTS.md` "Quick Reference"). SQLite would address a real correctness gap; only its observed frequency is unknown.
- **Maintainer context**: Estimates assume one engineer already familiar with this codebase, writing focused contract and failure-path tests alongside each slice. Review and CI iteration are excluded.
- **Binding**: If SQLite is adopted, `rusqlite` with `bundled` SQLite is the assumed binding because Tethys already uses it ([Tethys `Cargo.toml`](https://github.com/dwalleck/tethys/blob/main/Cargo.toml)); its first-party crate documentation exposes `Connection`, `Transaction`, and backup support (`https://docs.rs/rusqlite/latest/rusqlite/`).

### 2.2 Scope

In scope: current persistence architecture trace, SQLite primary-source facts relevant to the tradeoffs, target-option comparison, schema/migration/rollback outline, behavior/test matrix, effort decomposition, recommendation.

Out of scope: any code changes; PostgreSQL (a distinct, already-planned backend, `crates/rivets/src/storage/mod.rs:397` + issue `rivets-d71` in `.rivets/issues.jsonl`); event-sourcing implementation (already designed in `docs/design/event-sourcing.md`); performance benchmarking.

---

## 3. Current-State Inventory

### 3.1 Product promises and Git integration

- README positions Rivets as "a fast, Git-friendly issue tracker that lives in your repository": "**Git-native** — Issues live in your repo, branch with your code, merge with your PRs" (`README.md:8`, `README.md:14`) and "**Human-readable** — JSONL storage you can grep, diff, and edit directly" (`README.md:20`).
- The dogfooding workflow treats `.rivets/issues.jsonl` as a committed, evolving file: `.rivets/` is checked into git (`CLAUDE.md` "Work Tracking"), `.gitattributes` exists for merge-driver style configuration (currently for Beads JSONL only, `.gitattributes:1-2`), and `git log` shows `.rivets/issues.jsonl` being committed regularly (observed: `a9be0af chore(rivets): close rivets-p1g4 (PR #90 merged)` and prior commits).
- The architecture doc's design decisions explicitly justify JSONL by format properties: "**Git-friendly**: JSONL can be diffed and merged" (`docs/architecture.md` §3) and "**Git merge friendly**: Partial corruption after merge remains inspectable without becoming a destructive rewrite" (`docs/architecture.md` §8), and design principle 5 is "**Backward compatibility**: Maintain JSONL format stability" (`docs/architecture.md` §"Design principles for extensibility").
- The event-sourcing design repeats the same value: "**Git-friendly**: Append-only log produces clean diffs" (`docs/design/event-sourcing.md` "Benefits").

### 3.2 Persistence architecture (data flow)

```
CLI (crates/rivets/src/cli/) ─┐
MCP (crates/rivets-mcp/) ─────┼─► App / Context ─► create_storage(StorageBackend, prefix)
                             │                        │
                             ▼                        ▼
                     Box<dyn IssueStorage>   JsonlBackedStorage (guarded wrapper)
                             │                        │
                             ▼                        ▼
                  InMemoryStorage (Arc<Mutex<InMemoryStorageInner>>:
                  HashMap<IssueId, Issue> + petgraph DiGraph + node_map)
                             │                        │
                             ▼                        ▼
                  load_from_jsonl / save_to_jsonl ──► .rivets/issues.jsonl
```

- **Trait**: `IssueStorage` is an object-safe `#[async_trait]` with ~22 methods: CRUD; dependency add/remove/queries/tree/cycle-check; `list`, `ready_to_work`, `blocked_issues`; atomic label ops; associated-resource ops; `import_issues`, `export_all`; `save`, `reload` (`crates/rivets/src/storage/mod.rs:110-385`). `save(&self)` takes a shared reference by design; `reload(&mut self)` restores in-memory state to disk state after a failed save (`crates/rivets/src/storage/mod.rs:340-384`).
- **Backend selection**: `StorageBackend` has `InMemory`, `Jsonl(PathBuf)`, and `PostgreSQL(String)` variants (`crates/rivets/src/storage/mod.rs:391-401`); `data_path()` returns the file path only for `Jsonl` (`:403-413`). `create_storage` loads existing JSONL via `load_from_jsonl` or starts empty, wraps it in `JsonlBackedStorage`, and rejects PostgreSQL as unsupported (`:648-682`).
- **Guarded wrapper**: `JsonlBackedStorage` holds `{ inner, path, prefix, load_warnings }` (`crates/rivets/src/storage/mod.rs:421-426`). `unsafe_partial_load()` and `ensure_writable()` block every mutation and save after an Issue record was omitted (`:438-481`); `save()` rewrites JSONL and `reload()` replaces the in-memory state from disk (`:595-618`).
- **In-memory backend and relationship debt**: `InMemoryStorage = Arc<Mutex<InMemoryStorageInner>>` (`crates/rivets/src/storage/in_memory/mod.rs:92-97`); the implementation uses a `HashMap`, `petgraph::DiGraph`, and node map. Cycle detection uses `has_path_connecting` (`crates/rivets/src/storage/in_memory/graph.rs:83-103`). The current `find_blocked_issues` also propagates blockedness through `ParentChild` edges to depth 50 (`graph.rs:105-199`), but that is explicitly **legacy implementation debt**, not a migration contract: canonical Rivets says only unresolved Blocking Dependencies block readiness, Parentage is non-blocking, Related is symmetric, and Discovery Origin is provenance (`CONTEXT.md:99-123`; `docs/adr/0002-issue-relationships-and-readiness.md:3-7`).
- **JSONL persistence**: `load_from_jsonl` parses resiliently, converts through the compatibility adapter, and reconstructs the graph in three passes (`crates/rivets/src/storage/in_memory/jsonl.rs:193-322`); its documented memory profile is ~2-3× file size (`:164-187`). `save_to_jsonl` sorts records for deterministic Git diffs, writes a temporary file, flushes the buffered writer, and renames it (`:324-373`). It does **not** call `sync_all`, so the code supports atomic replacement semantics on POSIX but does not establish power-loss durability.
- **Persisted record shape**: `IssueRecord` carries canonical and legacy fields (`crates/rivets/src/storage/in_memory/issue_record.rs:276-313`), and `CanonicalIssueRecord` emits only the current format (`:437-507`). Legacy `issue_type`, string `notes`, and `external_ref` are normalized in `IssueRecord::into_domain` (`:315-434`). The adapter must move out of `in_memory` before SQLite can reuse it.
- **Config/init**: `init` writes `.rivets/config.yaml` (`issue-prefix`, `storage: {backend: "jsonl", data_file: ".rivets/issues.jsonl"}`), an empty `issues.jsonl`, and a `.gitignore` whose comment says "The issues.jsonl file should be tracked for collaboration" (`crates/rivets/src/commands/init.rs:250-265`; `DEFAULT_BACKEND = "jsonl"` at `:67`). `StorageConfig::to_backend` validates `data_file` is a relative path without `..`, maps `"jsonl"` → `StorageBackend::Jsonl`, and rejects `"postgresql"` and unknown backends (`:100-121`). `App::from_directory` walks up to `.rivets/`, loads config, and builds storage (`crates/rivets/src/app.rs:55-83`); `App::save()` delegates to the trait (`:110-114`).
- **CLI auto-save**: mutating commands call `app.save()` after the storage mutation (create: `crates/rivets/src/cli/execute.rs:190-191`; delete: `:680-681`; dep add/remove: `:772-775`, `:808-811`; batched update uses `save_or_record_failure`, `:376-377`). On save failure, the CLI attempts `reload()` to restore a consistent view (`:407-413`).
- **MCP server**: `Context` caches up to `MAX_CACHED_WORKSPACES = 32` per-workspace storage instances behind `Arc<RwLock<Box<dyn IssueStorage>>>`, with FIFO eviction; per-workspace DB path resolution mirrors the CLI via `config.storage.to_backend(&canonical)` (`crates/rivets-mcp/src/context.rs:25-32`, `:37-56`, `:76-153`). Tools escalate a read lock to write only on first use (`crates/rivets-mcp/src/tools.rs:121-137`) and use `save_or_reload` after mutations (`:98-104`).
- **Test surface that pins the current contract**:
  - `crates/rivets-jsonl/tests/roundtrip.rs` and `resilient_loading.rs` (format library: atomic-write round trips, permission-denied preservation, serialization-failure preservation, warning collection).
  - `crates/rivets/tests/in_memory_storage.rs` (CRUD/dependency/query semantics), `in_memory_resilient_loading.rs` (partial-load guard behavior), `cli_tests.rs` (CLI behaviors incl. `--json` shapes), `init_integration.rs` (workspace setup).
  - `crates/rivets-mcp/tests/integration.rs` (MCP tool parity, multi-workspace isolation, workspace-root override, JSON shape parity with CLI, Z-suffixed timestamps, close/reopen rejection).
  - Domain/CLI unit tests across `crates/rivets/src/domain/`, `cli/`, `storage/` (e.g., `test_save_or_record_failure_save_error` at `crates/rivets/src/cli/execute.rs:1644-1650`).

### 3.3 Existing SQLite precedent in the workspace ecosystem

- The root `.gitignore` already reserves `.rivets/index/` for a "Local SQLite index (regenerated from .rivets/issues.jsonl)" (`.gitignore:37-38`). That is the repository's existing policy: JSONL remains authoritative and derived database state is not committed.
- Tethys provides first-party ecosystem precedent: [`rusqlite` with `bundled`](https://github.com/dwalleck/tethys/blob/main/Cargo.toml), and an `Index` module that wraps `Connection` in a `Mutex`, sets a 30-second busy timeout, enables WAL and foreign keys, and applies its schema ([`src/db/mod.rs`, `Index::open`](https://github.com/dwalleck/tethys/blob/main/src/db/mod.rs)).
- The current Rivets workspace has **no** SQLite dependency: `Cargo.lock` contains no rusqlite/libsqlite3 entries, and workspace members are only `rivets-jsonl`, `rivets`, and `rivets-mcp` (`Cargo.toml:1-7`).

### 3.4 Roadmap context

- The architecture doc's Phase 3 (production/multi-user) plans a PostgreSQL backend with "Recursive CTEs for complex graph queries", and a "Migration system: Import from JSONL to PostgreSQL; Export from PostgreSQL to JSONL (backup, portability); Schema versioning and automatic migrations" (`docs/architecture.md` "Phase 3: Production Multi-User"). SQLite slots into the same slot with lower operational cost — it is effectively the "PostgreSQL without a server."
- The tracker contains the corresponding issues: `rivets-d71` "Implement PostgreSQL storage backend with recursive CTEs", `rivets-x51` "Implement JSONL import/export system", `rivets-yis` "Implement storage backend selection via configuration" (titles read from `.rivets/issues.jsonl`, 2026-08-09).
- `docs/design/implementation-roadmap.md` plans an event-sourced daemon (REST + client SDK) where `rivets-events` would hold a `JsonlEventStore` (Phase 1, est. 2-3 days there) and the CLI/MCP would move behind a client. This is the scenario in which SQLite's multi-process concurrency would actually matter (a daemon plus CLI plus MCP touching one workspace).

---

## 4. Target Options

### 4.1 Option A — Hard cutover (SQLite becomes the only store)

`StorageBackend::Jsonl` is replaced by a SQLite-backed implementation; `issues.jsonl` is migrated once and then no longer written (kept only as an export format). Each mutating storage method owns and commits its transaction; `save()` and `reload()` become no-ops for SQLite, as the trait documentation already anticipates for database adapters (`crates/rivets/src/storage/mod.rs:340-384`). Checkpointing is an explicit lifecycle/administrative concern, not the commit point hidden behind `save()`.

- **Gains**: true ACID transactions and crash atomicity (`https://www.sqlite.org/atomiccommit.html#_1_introduction`); incremental page-level writes instead of whole-file rewrites (`https://www.sqlite.org/appfileformat.html#_3_7_incremental_and_continuous_updates`); real multi-process concurrency with WAL (single writer, concurrent readers, `https://www.sqlite.org/wal.html#_2_2_concurrency`); queries/indexes without loading everything into memory (`https://www.sqlite.org/appfileformat.html#_3_9_performance`).
- **Losses**: text-tool accessibility ("command-line tools such as text editors or 'grep' or 'awk' are not useful on an SQLite database", `https://www.sqlite.org/appfileformat.html#_3_4_accessible_content`); Git's default binary diff reports only that files differ and its built-in binary merge driver leaves the path conflicted for the user to resolve (`https://git-scm.com/docs/gitattributes`, "Generating diff text" and "Built-in merge drivers"); WAL adds `-wal`/`-shm` files that must be managed (`https://www.sqlite.org/wal.html#_1_overview`, disadvantage 6); WAL does not work on network filesystems (`https://www.sqlite.org/wal.html#_1_overview`, disadvantage 1); and the resilient-partial-load behavior disappears.

### 4.2 Option B — Dual backend (config-selected)

`StorageBackend::Sqlite(PathBuf)` is added alongside `Jsonl`; `config.yaml` `storage.backend` accepts `"sqlite"` while `"jsonl"` stays default (`crates/rivets/src/commands/init.rs:100-121` already centralizes backend-string validation). Both backends implement `IssueStorage`; shared behavioral tests are parameterized over both.

- **Gains**: keeps the committed, diffable JSONL for users who want it; SQLite available for daemon/multi-user scenarios; the trait and factory were explicitly designed for this ("Backend agnostic… Progressive enhancement", `docs/architecture.md` §5).
- **Costs**: every future domain change must land in two implementations; parity test matrix runs twice; `save()`/`reload()` semantics differ per backend and every caller must keep working with both; config plumbing (`to_backend`, `data_path`, init) gains a branch. Net incremental cost over Option A is modest (≈2-4 days, §8) because Option A already requires a complete, tested second implementation of the trait.

### 4.3 Option C — SQLite as derived index/projection (JSONL stays source of truth)

Generalize the Tethys pattern: keep `.rivets/issues.jsonl` as the only authoritative store; maintain a regenerable `.rivets/index/rivets.db` (the location is already gitignored with exactly this description, `.gitignore:37-38`) that is rebuilt from the JSONL and serves fast queries, the daemon, and read-heavy tooling. Optionally, the MCP server or a future daemon reads from the index and invalidates/rebuilds it on JSONL change.

- **Gains**: zero migration risk; no format-property loss; ~1/3 the effort of A/B; directly extends the existing Tethys integration; can later evolve into Option B by making the index authoritative.
- **Costs/limits**: does not solve atomicity of issue *mutations* (the write path still rewrites the JSONL); index staleness must be managed (the `.gitignore` comment says "regenerated from .rivets/issues.jsonl", i.e., rebuild-on-change, not incremental); two representations to keep consistent — the very consistency problem the current design avoids by having one file.

### 4.4 Why not "SQLite for everything including git"

The naive variant "SQLite file committed to git, drop JSONL entirely" is Option A with the worst-case downsides (binary diffs, opaque merges, WAL companion files in the repo) and is therefore not separately costed.

---

## 5. Affected Components and Implementation Slices

All slices are for Option A/B unless noted; Option C is costed separately in §8. Slices are ordered for independent landing, and each is small enough to become a task.

| # | Slice | Affected files / surfaces | Content | Key risks |
|---|-------|---------------------------|---------|-----------|
| S1 | Spike: binding + schema round-trip | new scratch code only (no committed Rust) | `rusqlite` (bundled) connection with `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout` (`https://www.sqlite.org/pragma.html#pragma_busy_timeout`); map a real canonical Issue into rows and back; verify rollback; measure migration of this repo's 252-issue file | timestamp fidelity; relationship-model prerequisite; WAL/DELETE choice |
| S2 | SQLite storage module | new `crates/rivets/src/storage/sqlite/` (or new `rivets-sqlite` crate); `crates/rivets/src/storage/mod.rs`; relocate `in_memory/issue_record.rs` to a storage-level compatibility module | connection wrapper based on Tethys [`Index::open`](https://github.com/dwalleck/tethys/blob/main/src/db/mod.rs); validate the workspace-relative data path before use; schema DDL; migration runner keyed on `PRAGMA user_version`; set `PRAGMA application_id` | database path lifecycle; busy timeout; typed SQLite errors; compatibility-adapter visibility |
| S3 | Implement the storage interface against canonical relationships | `crates/rivets/src/storage/sqlite/` (CRUD, explicit relationships, queries, labels, resources, notes, import/export) | implement current trait methods needed for cutover, but do not freeze the legacy generic relationship semantics into the schema; target Blocking Dependency, Parentage, Related Association, and Discovery Origin as specified by `CONTEXT.md:103-123` and ADR-0002; transactions own each mutation; choose SQL queries vs a synchronized in-memory projection only after S1 | **D0** relationship-interface alignment; cycle/tree query equivalence; transaction boundaries |
| S4 | Behavioral contract harness | `crates/rivets/tests/`; new `sqlite_storage.rs`; canonical relationship tests shared with in-memory adapter after D0 | run backend-agnostic scenarios against both adapters; SQLite-specific rollback, busy-error, foreign-key, child-process crash-recovery, reopen, and migration tests; treat JSONL partial-load tests as adapter-specific rather than forcing false parity | existing implementation contradicts canonical Parentage semantics; concurrency-test determinism |
| S5 | Migration tooling | new `crates/rivets/src/storage/migrate.rs` or CLI subcommands | JSONL→SQLite through the relocated compatibility adapter; insert issues, relationships, notes, and resources in one transaction; verify row counts, `integrity_check`, canonical export, and idempotency; SQLite→JSONL through `CanonicalIssueRecord`; rename the old file only after verification | partial migrations; legacy-field fidelity; migration from legacy generic relationships into canonical tables |
| S6 | Config, init, CLI, MCP wiring | `crates/rivets/src/commands/init.rs`, `app.rs`, `cli/execute.rs`; `crates/rivets-mcp/src/context.rs` | accept `"sqlite"`; initialize `.rivets/issues.sqlite`; map SQLite failures to typed `StorageError`; return mutation/transaction errors directly; keep JSONL's save-failure/reload recovery only where it applies; support mixed backends in the MCP cache | two distinct persistence lifecycles; output/backend identity; cross-process contention UX |
| S7 | Release hardening + docs | `.gitignore` guidance, `docs/adr/` (new ADR), existing architecture docs, CHANGELOG | decide WAL vs DELETE and `synchronous` level; backup/restore and checkpoint guidance; record the explicit Git-format tradeoff; update `CONTEXT.md`/`AGENTS.md` only for behavior that actually lands | stale invariants; committing a DB without its recent WAL transactions |
| S8 | Option B extras (if dual backend) | same as S6 + config docs | backend selection UX, parity runs for both backends in CI, `docs/design/` note on when to pick each | long-term dual-maintenance drag |

Slices S1-S7 = Option A; S8 only for Option B.

---

## 6. Proposed Schema, Migration, and Rollback Outline

### 6.1 Schema sketch (normative-ish, for scoping; final DDL is an S2/S3 artifact)

```sql
PRAGMA application_id = <rivets-assigned id>;   -- file(1) identification (pragma.html#pragma_application_id)
PRAGMA user_version = 1;                        -- schema version (pragma.html#pragma_user_version)

CREATE TABLE issues (
    id                  TEXT PRIMARY KEY,               -- IssueId (e.g. "rivets-a3f8")
    title               TEXT NOT NULL,
    description         TEXT NOT NULL DEFAULT '',
    status              TEXT NOT NULL CHECK (status IN ('open','in_progress','closed')),
    priority            INTEGER NOT NULL CHECK (priority BETWEEN 0 AND 4),
    issue_kind          TEXT NOT NULL CHECK (issue_kind IN ('bug','feature','task','epic','chore')),
    assignee            TEXT,
    labels              TEXT NOT NULL DEFAULT '[]',     -- JSON array (or child table)
    design              TEXT,
    acceptance_criteria TEXT,
    next_resource_id    INTEGER NOT NULL DEFAULT 1,
    created_at          TEXT NOT NULL,                  -- RFC3339 with Z, matching current serde output
    updated_at          TEXT NOT NULL,
    closed_at           TEXT
);

CREATE TABLE notes (                                   -- append-only per CONTEXT.md
    issue_id    TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,                      -- insertion order
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (issue_id, seq)
);

CREATE TABLE resources (                               -- stable per-issue IDs, ordering, no reuse (README.md)
    issue_id    TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    id          TEXT NOT NULL,
    position    INTEGER NOT NULL,
    target_type TEXT NOT NULL CHECK (target_type IN ('web','path')),
    target      TEXT NOT NULL,
    role        TEXT NOT NULL,
    label       TEXT,
    PRIMARY KEY (issue_id, id)
);

CREATE TABLE blocking_dependencies (
    dependent     TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    prerequisite  TEXT NOT NULL REFERENCES issues(id) ON DELETE RESTRICT,
    PRIMARY KEY (dependent, prerequisite),
    CHECK (dependent <> prerequisite)
);

CREATE TABLE parentage (
    child   TEXT PRIMARY KEY REFERENCES issues(id) ON DELETE CASCADE,
    parent  TEXT NOT NULL REFERENCES issues(id) ON DELETE RESTRICT,
    CHECK (child <> parent)
);

CREATE TABLE related_associations (
    left_issue   TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    right_issue  TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    PRIMARY KEY (left_issue, right_issue),
    CHECK (left_issue < right_issue)
);

CREATE TABLE discovery_origins (
    discovered  TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    source      TEXT NOT NULL REFERENCES issues(id) ON DELETE RESTRICT,
    PRIMARY KEY (discovered, source),
    CHECK (discovered <> source)
);

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);  -- e.g. migration markers
CREATE INDEX idx_issues_status ON issues(status);
CREATE INDEX idx_issues_priority ON issues(priority);
CREATE INDEX idx_issues_updated ON issues(updated_at);
CREATE INDEX idx_blocking_prerequisite ON blocking_dependencies(prerequisite);
```

Mapping notes (observation and target decision): every persisted Issue field (`crates/rivets/src/storage/in_memory/issue_record.rs:276-313`) has a home; `labels` can stay a JSON TEXT column to preserve ordering; timestamps must retain today's `DateTime<Utc>` JSON shape (Z suffix pinned by `mcp_timestamps_use_z_suffix` in `crates/rivets-mcp/tests/integration.rs`). Legacy-only fields (`issue_type`, string `notes`, `external_ref`) are normalized by the read adapter and need no columns, but that adapter must move out of `storage::in_memory` before reuse. The four relationship tables intentionally model the accepted domain rather than the legacy `DependencyType` data bag (`CONTEXT.md:103-123`; ADR-0002).

### 6.2 Migration outline (JSONL → SQLite, Option A/B)

1. Detect state: `issues.jsonl` exists, DB missing/empty → migrate; DB present with `user_version` ≥ current → skip (idempotent).
2. Load via the existing resilient path (`load_from_jsonl`). If any Issue record was skipped, abort rather than migrating a partial dataset, preserving today's invariant that an incomplete view never replaces the complete source (`crates/rivets/src/storage/mod.rs:438-481`).
3. Convert legacy generic relationships into the accepted relationship roles, rejecting ambiguous or invalid records rather than silently coercing them; begin one transaction; insert issues, notes, resources, and relationships; validate count and semantic parity; run `PRAGMA foreign_key_check` and `PRAGMA integrity_check`; commit.
4. Write a migration marker (user_version / `meta`).
5. Only on success (Option A): rename `issues.jsonl` → `issues.jsonl.pre-sqlite`.

### 6.3 Rollback outline

- **Pre-cutover safety**: the JSONL file is untouched until step 5; any failure leaves the workspace fully operational on JSONL.
- **Post-cutover rollback**: export SQLite → JSONL through `CanonicalIssueRecord` (`crates/rivets/src/storage/in_memory/issue_record.rs:437-508`; byte-format parity test: export and diff against the pre-cutover file), then restore `issues.jsonl.pre-sqlite`/delete the DB and set `storage.backend: jsonl` in `config.yaml` (`crates/rivets/src/commands/init.rs:71-88`).
- **Backup/recovery**: SQLite's Online Backup API copies a live database without locking writers for the duration (`https://www.sqlite.org/backup.html#_1_using_the_sqlite_online_backup_api`), and `rusqlite` exposes a `backup` module (`https://docs.rs/rusqlite/latest/rusqlite/`); `VACUUM INTO` is the alternative single-statement snapshot (`https://www.sqlite.org/backup.html#_1_1_other_backup_techniques`). For git-tracked DBs, the *checkpointed* file is the portable artifact (D2, §9).

---

## 7. Behavior / Test Matrix

Each row is an observable contract today; the middle column is the SQLite-equivalent design; the right column marks the risk that parity breaks.

| # | Current contract (source) | SQLite equivalent | Parity risk |
|---|---------------------------|-------------------|-------------|
| 1 | Create/update/close/reopen with validation and typed errors (`crates/rivets/src/storage/mod.rs:113-152`; `close_rejects_already_closed_issue_without_mutation`, `crates/rivets-mcp/tests/integration.rs:661-706`) | row insert/update in one transaction; domain validation remains authoritative, SQL constraints are defense-in-depth | low |
| 2 | Delete rejects issues with dependents (`crates/rivets/src/storage/mod.rs:143-152`; `docs/architecture.md` "Safe Operations") | query typed dependents, then delete in the same transaction | low |
| 3 | Legacy implementation rejects generic graph cycles and propagates ParentChild blockedness to depth 50 (`crates/rivets/src/storage/in_memory/graph.rs:83-199`); canonical model says only Blocking Dependencies block and Parentage is non-blocking (`CONTEXT.md:103-123`; ADR-0002) | align the shared interface first; implement explicit relationship invariants and only the traversals each role requires | **high prerequisite risk**, not a parity target for the legacy parent-blocking behavior |
| 4 | `ready_to_work` filters then applies hybrid/priority/oldest sorting (`crates/rivets/src/storage/mod.rs:228-255`; `crates/rivets/src/storage/in_memory/trait_impl.rs:441-475`) | determine readiness from unresolved Blocking Dependencies only, then reuse the same filter/sort contract | **medium**: canonical readiness must replace legacy parent propagation without changing ordering |
| 5 | Atomic label add/remove, idempotent on duplicates/missing labels (`crates/rivets/src/storage/mod.rs:262-282`) | transaction-scoped read/update or normalized label rows with uniqueness | low |
| 6 | Resources preserve stable per-Issue IDs, insertion order, no reuse, and reject duplicate target-role pairs (`crates/rivets/src/storage/mod.rs:284-325`; `CONTEXT.md`) | `resources` table with `position`, unique stable ID, target-role uniqueness, and `next_resource_id` on Issue | medium |
| 7 | Notes are append-only with immutable timestamps and empty-content rejection (`CONTEXT.md` "Note"; `test_notes_create_append_validate_and_survive_context_restart`, `crates/rivets-mcp/tests/integration.rs:826-909`) | `notes` table, append operation only | low |
| 8 | JSONL resilient partial load leaves reads available while writes are blocked (`crates/rivets/src/storage/mod.rs:438-481`; `crates/rivets/tests/in_memory_resilient_loading.rs`) | no direct analogue: transactions prevent partial commits; corruption/open failures are explicit errors; JSONL migration retains the partial-load guard | **high intentional behavior change** |
| 9 | JSONL mutates in memory, then `save()` can fail and trigger `reload()` (`crates/rivets/src/cli/execute.rs:190-191,407-413`; `crates/rivets-mcp/src/tools.rs:98-104`) | each SQLite mutation commits its own transaction and returns any error immediately; `save()`/`reload()` are no-ops | medium: callers and error-path tests must be split by adapter lifecycle |
| 10 | Legacy fields migrate on JSONL load: `issue_type`, string `notes`, `external_ref` (`crates/rivets/src/storage/in_memory/issue_record.rs:13-32,315-434`) | relocate and reuse the same compatibility adapter; no SQL-side legacy columns | low after the module move |
| 11 | ID generation: hash-based, `register_id` prevents collisions on load (`crates/rivets/src/id_generation.rs`; `in_memory/jsonl.rs` doc "ID Registration") | re-register on migration; PRIMARY KEY enforces uniqueness | low |
| 12 | CLI `--json` output shapes and MCP/CLI shape parity (`cli_tests.rs`; `cli_and_mcp_issue_json_shapes_match`, `crates/rivets-mcp/tests/integration.rs:638-659`) | backend-agnostic by construction (adapter seam) | low |
| 13 | MCP multi-workspace cache, isolation, per-call `workspace_root` (`context.rs:37-56`; `test_multi_workspace_context_switching`, `integration.rs:1081-1145`) | per-workspace DB path in `database_paths` (`context.rs:47-49`); mixed backends per workspace must be supported (S6) | medium |
| 14 | Concurrency: single-process `Arc<Mutex>` serialization; cross-process writes not atomic today (`AGENTS.md` "Quick Reference", rivets-8rj9) | WAL: one writer at a time, readers concurrent (`https://www.sqlite.org/wal.html#_2_2_concurrency`); `busy_timeout` + `BEGIN IMMEDIATE`; `SQLITE_BUSY` → typed error | **medium**: first real cross-process story; daemon+CLI+MCP contention needs stress tests |

---

## 8. Quantified Level of Effort

Assumptions restated: one engineer already familiar with this codebase; tests written within each slice; excludes review/CI iteration; day = focused 6-7h workday. The base ranges assume the accepted relationship/readiness model in `CONTEXT.md` and ADR-0002 is implemented before SQLite. If this migration must also align the legacy generic relationship interface and in-memory behavior, add **4-7 engineer-days** (D0).

### Option A — hard cutover: **23-33 engineer-days**

| Slice | Days | Notes |
|-------|------|-------|
| S1 Spike (binding, schema, canonical round-trip, migration dry-run on the 252-issue real file) | 2-3 | de-risks relationship mapping and WAL/DELETE choice |
| S2 SQLite module + schema + compatibility-adapter relocation + migration runner | 3-4 | Tethys is a proven binding/connection template |
| S3 Storage interface implementation | 6-9 | transactions, relationships, queries, resources, notes |
| S4 Contract harness + SQLite-specific failure/concurrency tests | 4-6 | shared behavior plus adapter-specific lifecycle tests |
| S5 Migration + rollback tooling | 3-4 | legacy conversion, verification, canonical export |
| S6 Config/init/CLI/MCP wiring + typed error mapping | 3-4 | includes save/reload lifecycle split |
| S7 Hardening, docs, ADR, backup/WAL guidance | 2-3 | format decision and recovery workflow |
| **Migration subtotal** | **23-33** | assumes D0 already complete |
| **If D0 is bundled** | **27-40** | adds 4-7 days to align the accepted relationship model |

### Option B — dual backend (Option A + S8): **25-37 engineer-days**

Adds backend-selection UX, mixed-backend MCP handling, and running the parity matrix twice: **+2-4 days** over A. Long-term drag (every future domain change ×2 implementations) is not quantifiable in days; it is the main strategic cost.

### Option C — derived SQLite index over JSONL: **5-8 engineer-days**

Rebuild/refresh command or on-open rebuild into `.rivets/index/rivets.db` (location already gitignored, `.gitignore:37-38`), query surface for the read paths that matter (list/ready/blocked/search), staleness check on mtime/hash, tests against the real JSONL. No trait changes, no migration, no format risk. Tethys' `index_file_atomic` transaction pattern (`.worktrees/tethys-staleness/crates/tethys/src/db/files.rs:91-143`) is a direct template for rebuild-in-transaction.

### What drives the range (explicit)

1. **D0 — canonical relationship alignment** (**+4-7 days if not already complete**): current storage exposes one generic dependency graph and still propagates Parentage blockedness (`crates/rivets/src/storage/in_memory/graph.rs:105-199`), while `CONTEXT.md:103-123` and ADR-0002 require four distinct relationship roles and non-blocking Parentage. The SQLite schema and contract tests must target the accepted model, not fossilize implementation debt.
2. **D1 — query implementation strategy** (±2-3 days in S3+S4): direct SQL gives one source of truth and transactional locality; a petgraph projection reuses algorithms but creates a synchronization invariant. The canonical Ready predicate is simpler than the legacy parent-propagating BFS, but dependency-tree traversal, cycle rules, ordering, and filter precedence still need contract tests.
3. **Redefining the partial-load contract** (±1-2 days, S4/S6): today's JSONL read-only mode after skipped records (`crates/rivets/src/storage/mod.rs:438-481`) has no database equivalent. SQLite should fail open explicitly on corruption; migration from JSONL retains the existing guard.
4. **`save()`/`reload()` lifecycle split** (±1 day, S6): the trait anticipates database no-ops (`crates/rivets/src/storage/mod.rs:340-384`), but CLI/MCP error paths are JSONL-shaped.
5. **Concurrency, journal, and durability decisions** (±1-2 days, S2/S7): WAL vs DELETE, busy timeout, checkpoint policy, and `synchronous` level affect network filesystems, Git tracking, and power-loss guarantees (`https://www.sqlite.org/wal.html`).
6. **Legacy-field fidelity** (±1 day, S5): migration must preserve canonical exports through the relocated compatibility adapter and the existing resilient-loading fixtures.

---

## 9. Risks and Unknowns

1. **Format-property regression (strategic)**: JSONL's diff/merge/grep/edit properties are stated product differentiators (`README.md:8,14,20`). SQLite is not text-tool-accessible (`https://www.sqlite.org/appfileformat.html#_3_4_accessible_content`), while Git's default binary diff and merge behavior does not provide line-level review or automatic content merging (`https://git-scm.com/docs/gitattributes`, "Generating diff text" and "Built-in merge drivers"). This is the strongest reason not to make SQLite the default without an explicit product decision.
2. **WAL companion files in a git-tracked tree**: `-wal`/`-shm` are quasi-persistent extras (`https://www.sqlite.org/wal.html#_1_overview`, disadvantage 6). A long-running MCP connection can leave committed transactions in a gitignored WAL rather than the main DB file until checkpoint; a tracked authoritative DB therefore needs an enforced checkpoint/close workflow or rollback-journal mode.
3. **Canonical-model drift (D0)**: readiness is load-bearing for agent work selection (`AGENTS.md` "Quick Reference"), but the current graph contradicts `CONTEXT.md` and ADR-0002. Porting the current code verbatim would turn known debt into a database schema.
4. **Network filesystems**: WAL "does not work over a network filesystem" (`https://www.sqlite.org/wal.html#_1_overview`, disadvantage 1). Repos on NFS/CIFS need rollback-journal mode and lose WAL's reader/writer concurrency.
5. **Durability must be explicit**: WAL with `synchronous=NORMAL` can lose the most recent committed transactions after power loss (`https://www.sqlite.org/wal.html#_2_3_performance_considerations`). The current JSONL adapter flushes and renames but does not `sync_all` (`crates/rivets/src/storage/in_memory/jsonl.rs:333-373`), so it must not be described as stronger than SQLite. Choose and test the intended guarantee.
6. **Mixed-backend MCP cache**: once workspaces can differ by backend, `Context` (`crates/rivets-mcp/src/context.rs:34-45`) and info/JSON output should surface backend identity.
7. **rusqlite adoption**: the sibling Tethys repo pins `rusqlite` 0.32 with `bundled` and `hooks` ([manifest](https://github.com/dwalleck/tethys/blob/main/Cargo.toml)); latest docs are 0.40.x (`https://docs.rs/rusqlite/latest/rusqlite/`). Rivets needs an explicit version/MSRV/license/build-time check rather than copying the pin blindly.
8. **Unknown: actual concurrent-writer frequency**. The correctness gap is documented (`rivets-8rj9`, `AGENTS.md`), but no primary source measures contention among CLI, MCP, and multiple agents. That measurement affects urgency, not the implementation estimate.
9. **Unknown: git-history ergonomics**. Current tracker changes are reviewed as text; no repository study quantifies the cost of losing line-level issue diffs. This is a product-workflow decision.

---

## 10. Recommendation

1. **Do not make SQLite the repository-facing default without an explicit product decision.** It improves a real cross-process correctness gap, but it removes the text/Git behavior Rivets advertises. The choice is strategic, not a purely technical upgrade.
2. **If the driver is query performance or a read-heavy daemon**: build **Option C** (regenerable SQLite index at `.rivets/index/`) — 5-8 days, no source-format migration, and consistent with `.gitignore:37-38`.
3. **If the driver is concurrent-writer correctness**: choose **Option B** (SQLite as an optional authoritative adapter) or put all writes behind a single-writer daemon. The existing event-log design improves Git diffs and auditability but uses a process-local lock (`docs/design/event-sourcing.md:263-307`); it needs OS-level locking or daemon ownership before it solves the same race.
4. **Before any authoritative SQLite work**: resolve **D0**. Implement the relationship/readiness model from `CONTEXT.md` and ADR-0002 first, or budget the extra 4-7 days. Do not encode `ParentChild` blockedness or the generic `DependencyType` model into the new schema.
5. **When the daemon roadmap lands** (`docs/design/implementation-roadmap.md`): implement Option B behind the existing storage seam, keep JSONL default, and retain canonical JSONL import/export for portability and rollback.
6. **Scope decisions that materially change the estimate**: D0 relationship alignment (+4-7 days); SQL vs projection queries (±2-3 days); SQLite default vs opt-in (+2-4 days); WAL vs DELETE and durability level (±1-2 days); legacy-field fidelity (+1 day); multi-process stress testing (+1-2 days).

---

## 11. Sources

### Repository (primary; stable paths in this checkout)

- `README.md:8,14,20` — product positioning: Git-native, Git-friendly, Human-readable JSONL.
- `CLAUDE.md` §Work Tracking and §Architecture — `.rivets/` tracking and Tethys repository split.
- `AGENTS.md` §Quick Reference and domain-modeling rules — cross-process claim gap; canonical storage and relationship vocabulary.
- `CONTEXT.md` — domain glossary (Note append-only, Blocked derived, Workflow State).
- `.gitignore:31-32,37-38` — `.worktrees/` ignored; `.rivets/index/` "Local SQLite index (regenerated from .rivets/issues.jsonl)".
- `.gitattributes:1-2` — merge-driver precedent (Beads).
- `.rivets/config.yaml` — `storage: {backend: jsonl, data_file: .rivets/issues.jsonl}` (observed).
- `.rivets/issues.jsonl` — 252 issues / 429.6 KB; issues `rivets-d71`, `rivets-x51`, `rivets-yis`, `rivets-014n` (observed 2026-08-09).
- `Cargo.toml:1-6` — workspace members; `Cargo.lock` — no SQLite dependencies.
- `crates/rivets/src/storage/mod.rs:110-385` (`IssueStorage`), `:391-413` (`StorageBackend`, `data_path`), `:421-481` (`JsonlBackedStorage`, partial-load guard), `:485-619` (JSONL adapter implementation), `:648-682` (`create_storage`).
- `crates/rivets/src/storage/in_memory/mod.rs:76-97` (private compatibility module and storage alias); `crates/rivets/src/storage/in_memory/graph.rs:83-199` (cycle detection and legacy Parentage blockedness).
- `crates/rivets/src/storage/in_memory/jsonl.rs:164-187,193-322` (memory profile and resilient three-pass load), `:324-373` (deterministic temp-write/flush/rename save).
- `crates/rivets/src/storage/in_memory/issue_record.rs:13-32` (`MigrationField`), `:276-434` (`IssueRecord` and legacy conversion), `:437-507` (`CanonicalIssueRecord`).
- `crates/rivets/src/commands/init.rs:49-55,67,71-88,100-121,140-144,232-265` (constants, config structs, `to_backend`, `load`, `init` writes).
- `crates/rivets/src/app.rs:55-83,110-114` (`App::from_directory`, `App::save`).
- `crates/rivets/src/cli/execute.rs:190-191,376-377,407-413,680-681,772-775,808-811,1571-1650` (auto-save, save-failure reload, tests).
- `crates/rivets/src/error.rs:73-95,179+` (skipped-record causes, `StorageError`).
- `crates/rivets-mcp/src/context.rs:25-32,37-56,76-153` (workspace cache, backend resolution).
- `crates/rivets-mcp/src/tools.rs:98-104,121-137` (`save_or_reload`, `storage_for`).
- `crates/rivets-mcp/tests/integration.rs:638-659,661-706,826-909,1081-1145` (shape parity, close/reopen, notes persistence, multi-workspace).
- `crates/rivets/tests/in_memory_storage.rs`, `in_memory_resilient_loading.rs`, `cli_tests.rs`, `init_integration.rs`; `crates/rivets-jsonl/tests/roundtrip.rs`, `resilient_loading.rs`.
- `docs/architecture.md` §3, §8, §9, §"Phase 3: Production Multi-User", §"Design principles for extensibility" (JSONL rationale, auto-save, Postgres/migration plan, JSONL stability).
- `docs/storage-architecture.md` (trait hierarchy, JSONL load/save sequence, recovery strategies, cycle detection, ready-work).
- `docs/design/event-sourcing.md` §Benefits (append-only log, Git-friendly), §"JSONL Format".
- `docs/design/implementation-roadmap.md` Phase 1-8 (event store, daemon, client).
- Tethys first-party source: https://github.com/dwalleck/tethys/blob/main/Cargo.toml (`rusqlite` 0.32, `bundled`, `hooks`) and https://github.com/dwalleck/tethys/blob/main/src/db/mod.rs (`Index`, `Index::open`, busy timeout, WAL, foreign keys, `Mutex<Connection>`).

### Git (official primary documentation)

- Git attributes, binary diff, and built-in binary merge behavior: https://git-scm.com/docs/gitattributes ("Generating diff text", "Performing a three-way merge", "Built-in merge drivers").

### SQLite (official primary documentation, sqlite.org)

- Atomic commit / rollback: https://www.sqlite.org/atomiccommit.html (§1: atomic even on OS crash or power failure; rollback vs WAL mechanisms).
- Write-Ahead Logging: https://www.sqlite.org/wal.html (§1 advantages/disadvantages incl. same-host requirement and `-wal`/`-shm` files; §2.2 concurrency: single writer, readers concurrent; §2.3 `synchronous=NORMAL` durability tradeoff; §3 activation and persistence of WAL mode).
- File locking: https://www.sqlite.org/lockingv3.html (§3 five lock states; §5 `SQLITE_BUSY` on write contention).
- PRAGMAs: https://www.sqlite.org/pragma.html (`#pragma_application_id`; `#pragma_user_version`; `#pragma_integrity_check`; `#pragma_quick_check`; `#pragma_busy_timeout`; `#pragma_foreign_keys`; `#pragma_journal_mode`).
- Backup API: https://www.sqlite.org/backup.html (§1 online backup, snapshot semantics; §1.1 `VACUUM INTO`).
- SQLite as an application file format: https://www.sqlite.org/appfileformat.html (§3.4 text tools not useful; §3.6 atomic transactions; §3.7 incremental updates; §3.9 performance; §3.10 multi-process; §5 "not the perfect application file format for every situation").

### Rust binding (official crate documentation)

- rusqlite: https://docs.rs/rusqlite/latest/rusqlite/ (v0.40.2; `Connection`, `Transaction`, `backup` module — "Online SQLite backup API", `serialize` module — "Serialize a database").
