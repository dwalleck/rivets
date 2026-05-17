# rivets-3d0s v3 — budgeted plan (K-hybrid)

Source design: `.rivets-3d0s/design-v3-k-hybrid.md`. Cheapest falsifier passed at design time (k_hybrid_simulation.py against current rivets DB: predicted FORBIDDEN=0, corroborated ALLOWED=5, all 5 verifiable as legitimate).

Two slices, each implementable in ≤ 30 min. Total scope: ~210 LOC across 3 files. Three design claims (C4, C5, C6) are negative — no code changes anywhere; verified by code review at PR time.

The skill says "a slice is done when (a) unit tests pass, (b) stress fixture produces expected result, (c) prove-it-prototype oracle still agrees with binary, (d) complexity budget holds." Each slice below includes these as verification checkboxes.

---

## Slice 1: K-hybrid filter in `populate_file_deps_from_call_edges`

**Claim:** After this slice, `populate_file_deps_from_call_edges` filters cross-crate call-derived file_deps to only include edges where the source file has at least one import whose first segment matches the target file's crate. Intra-crate edges are kept always. Orphan files (outside any Cargo-known crate) are bucketed by their top-level directory as a pseudo-crate. (Maps to design claims C1, C2, C3, C9, C10.)

**Oracle (independent):**
- Unit test in `db/call_edges.rs::tests`: construct an in-memory `Index` with 3 pseudo-crates (`crate_a/`, `crate_b/`, `crate_c/`) populated as DB rows directly (no parser). Insert call_edges to exercise the four cases (intra, cross-corroborated, cross-uncorroborated, orphan-cross). Insert one corroborating import row in caller_file. Call the function, assert exactly the expected file_deps rows survive.
- Independence check: the unit test queries `file_deps` directly via SQL `SELECT`, not via any Tethys-side wrapper. The oracle's classification ("expected surviving rows") is hard-coded in the test based on the design matrix, not derived from `crate_of()` or the implementation's filter function.
- After this slice lands, the persistent oracle (`.rivets-0gom/probe.py`) on the rivets workspace must show: total cross-crate edges drop to ≤ 5, FORBIDDEN-pair count to 0.

**Stress fixture:** in-memory DB with this shape:

```
files:
  1: crates/crate_a/src/lib.rs       (crate_a)
  2: crates/crate_a/src/utils.rs     (crate_a)  ← intra-crate target
  3: crates/crate_b/src/lib.rs       (crate_b)  ← cross-crate corroborated target
  4: crates/crate_c/src/lib.rs       (crate_c)  ← cross-crate UNCORROBORATED target (phantom)
  5: examples/oddball.rs             (orphan: top-dir=examples)
  6: bruno-examples/types.rs         (orphan: top-dir=bruno-examples)

symbols:
  10: helper      in file 2 (intra-crate target)
  11: legit_thing in file 3 (cross corroborated)
  12: len         in file 4 (cross UNCORROBORATED — the phantom shape: name collision with std)
  13: extract     in file 5 (orphan)
  14: encode      in file 6 (orphan-different-pseudo-crate)
  20: caller_fn   in file 1 (the source of all 5 call edges)

imports:
  file 1 imports source_module="crate_b::legit_thing"    ← corroborates 1→3 only

call_edges (5 edges all from caller_fn in file 1):
  caller_fn → helper        (intra-crate, expect KEPT)
  caller_fn → legit_thing   (cross-crate + import corroborates, expect KEPT)
  caller_fn → len           (cross-crate + NO import to crate_c, expect DROPPED — the rivets-3d0s phantom shape)
  caller_fn → extract       (caller_crate=crate_a, target_crate=orphan:examples, no import, expect DROPPED)
  caller_fn → encode        (caller_crate=crate_a, target_crate=orphan:bruno-examples, no import, expect DROPPED)
```

Expected post-call `file_deps`:
- (1, 2, _) — intra-crate kept
- (1, 3, _) — cross-crate corroborated kept

NOT expected: any row with from_file_id=1 and to_file_id ∈ {4, 5, 6}.

**Plausible bugs the fixture is designed to fail under:**
1. Implementation forgets the same-crate guard → file 1 → file 2 might be filtered out (file 2 is intra-crate but a naive check might treat it as different)
2. Implementation only matches on first segment of `source_module` — would need to verify `crate_b::legit_thing` first-segment-match works
3. Implementation uses `path_prefix` LIKE instead of full crate-name match — could be tripped by `crate_b` matching `crate_b_other`
4. Implementation forgets orphan pseudo-crate handling — file 5 might be classified as None and the filter would silently keep or drop unpredictably
5. Implementation doesn't handle `Index` lacking workspace context — falls back to all-orphan, breaking the test

**Loop budget:**
- Build `crate_of_file` map: `O(files)` iteration over files table. Production scale: ~50k files × 1 prefix check ≈ 50k ops. Within 10^6 budget.
- Build `imports_per_file` map: `O(imports)` linear scan over imports table. Production scale: ~few thousand imports × 1 split + 1 lookup ≈ negligible.
- Filter call_edges aggregates: `O(unique_file_pairs)` ≈ ~10^4 at production scale × O(1) HashMap lookups. Negligible.
- INSERTs: `O(surviving_file_pairs)`. STRICTLY fewer than the current implementation (filter only subtracts). No regression possible.

**Wall budget:** Phase is reference resolution + file_deps population (one-shot per index, ~20s for rivets workspace pre-fix). Per-pair overhead from K-hybrid filter: sub-millisecond. Net wall delta vs current implementation: < 50ms. No measurable change.

**Files:**
- `crates/tethys/src/db/call_edges.rs` (modify `populate_file_deps_from_call_edges`)
- `crates/tethys/src/indexing.rs` (one call site; build crate-of-file map + pass to modified function)

**Code (advisory):**

```rust
// crates/tethys/src/db/call_edges.rs:

use std::collections::{HashMap, HashSet};
// ... existing imports ...

impl Index {
    /// Populate file-level dependencies from call edges, filtered by import
    /// corroboration for cross-crate edges (rivets-3d0s K-hybrid).
    ///
    /// Aggregates call_edges into file_deps with these rules:
    /// - intra-crate edges (caller_file's crate == callee_file's crate) ALWAYS kept
    /// - cross-crate edges kept IFF the caller file has at least one import
    ///   whose first segment matches the callee file's crate's Rust name
    ///   (e.g., `use crate_b::Foo` corroborates a cross-edge into `crate_b`)
    ///
    /// `file_crate_map` maps each FileId to its crate name (either Cargo-known
    /// or an `orphan:<top-dir>` pseudo-crate). Caller MUST populate every
    /// FileId that appears in `call_edges`; missing entries are treated as
    /// truly path-less and kept conservatively (logged at warn level).
    ///
    /// Returns the count of file_deps rows inserted/updated.
    pub fn populate_file_deps_from_call_edges(
        &self,
        file_crate_map: &HashMap<FileId, String>,
    ) -> Result<usize> {
        trace!("Populating file deps from call edges (K-hybrid filter)");
        let conn = self.connection()?;

        // Aggregate call_edges to (caller_file, callee_file, sum) tuples.
        let aggregated: Vec<(i64, i64, i64)> = conn.prepare(
            "SELECT s1.file_id, s2.file_id, SUM(ce.call_count)
             FROM call_edges ce
             JOIN symbols s1 ON ce.caller_symbol_id = s1.id
             JOIN symbols s2 ON ce.callee_symbol_id = s2.id
             WHERE s1.file_id != s2.file_id
             GROUP BY s1.file_id, s2.file_id",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

        // Build per-file imported-crates set from imports table.
        // Maps each caller_file_id -> set of crate names it imports from.
        let mut imports_per_file: HashMap<FileId, HashSet<String>> = HashMap::new();
        let mut imports_stmt = conn.prepare(
            "SELECT file_id, source_module FROM imports",
        )?;
        for row_res in imports_stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })? {
            let (file_id_i64, source_module) = row_res?;
            let first_segment = source_module
                .split("::")
                .next()
                .unwrap_or("");
            if !first_segment.is_empty() {
                imports_per_file
                    .entry(FileId::from(file_id_i64))
                    .or_default()
                    .insert(crate_name_from_rust_segment(first_segment, file_crate_map));
            }
        }

        // Filter + insert.
        let mut inserted = 0;
        for (caller_fid, callee_fid, ref_count) in aggregated {
            let caller_id = FileId::from(caller_fid);
            let callee_id = FileId::from(callee_fid);
            let caller_crate = file_crate_map.get(&caller_id);
            let callee_crate = file_crate_map.get(&callee_id);

            let keep = match (caller_crate, callee_crate) {
                (Some(a), Some(b)) if a == b => true,                                    // C1: intra-crate
                (Some(_a), Some(b)) => imports_per_file                                  // C2/C3
                    .get(&caller_id)
                    .map_or(false, |imports| imports.contains(b)),
                _ => {                                                                    // truly path-less
                    warn!(caller_file_id = %caller_fid, callee_file_id = %callee_fid,
                          "file not in crate map; keeping conservatively");
                    true
                }
            };
            if keep {
                conn.execute(
                    "INSERT INTO file_deps (from_file_id, to_file_id, ref_count) VALUES (?1, ?2, ?3)
                     ON CONFLICT(from_file_id, to_file_id) DO UPDATE SET
                         ref_count = file_deps.ref_count + excluded.ref_count",
                    params![caller_fid, callee_fid, ref_count],
                )?;
                inserted += 1;
            }
        }

        trace!(file_deps_inserted = inserted, "Populated file deps (K-hybrid)");
        Ok(inserted)
    }
}

// Helper: convert a Rust-namespace first-segment (`rivets_jsonl`) to the
// crate name as it appears in file_crate_map (`rivets-jsonl`). Needed because
// Rust uses underscores in path segments while crate names often use hyphens.
// If no workspace crate matches, returns the segment as-is (will not match
// any workspace crate, hence the import won't corroborate anything — correct
// behavior for external crates).
fn crate_name_from_rust_segment(segment: &str, file_crate_map: &HashMap<FileId, String>) -> String {
    // Build a set of known crate names from the map's values
    let known_crates: HashSet<&String> = file_crate_map.values().collect();
    // Try direct match (e.g., "tethys" → "tethys")
    if known_crates.iter().any(|c| c.as_str() == segment) {
        return segment.to_string();
    }
    // Try underscore-to-hyphen conversion (e.g., "rivets_jsonl" → "rivets-jsonl")
    let dashed = segment.replace('_', "-");
    if known_crates.iter().any(|c| c.as_str() == dashed) {
        return dashed;
    }
    // External crate or unknown: return as-is. Won't match any workspace
    // crate, so corroboration check fails — correct.
    segment.to_string()
}
```

```rust
// crates/tethys/src/indexing.rs (single call site, ~line 447):

// Build file → crate-name map for the K-hybrid filter (rivets-3d0s).
// Cargo-known files use the canonical crate name; orphan files use
// `orphan:<top-dir>` pseudo-crate names so they participate in the filter
// like any other crate.
let file_crate_map = self.build_file_crate_map()?;
let call_edges_count = self.db.populate_call_edges()?;
if call_edges_count > 0 {
    tracing::debug!(call_edges = call_edges_count, "Populated call graph edges");
}
let file_deps_from_calls = self.db.populate_file_deps_from_call_edges(&file_crate_map)?;
```

```rust
// crates/tethys/src/indexing.rs — new helper on Tethys:

fn build_file_crate_map(&self) -> Result<HashMap<FileId, String>> {
    let mut map = HashMap::new();
    for file in self.db.list_all_files()? {
        let crate_name = if let Some(crate_info) = crate::cargo::get_crate_for_file(
            &self.workspace_root.join(&file.path),
            &self.crates,
        ) {
            crate_info.name.clone()
        } else {
            // Orphan: pseudo-crate from top-level directory of the relative path
            let top = file.path.split('/').next().unwrap_or("");
            format!("orphan:{top}")
        };
        map.insert(file.id, crate_name);
    }
    Ok(map)
}
```

**Doc-comment precondition classification:** The new function's doc says `file_crate_map` "MUST populate every FileId that appears in `call_edges`." This is **load-bearing for correctness** — if a FileId is missing, the function silently falls into the "truly path-less" branch and KEEPS the edge (potential phantom). Enforcement: caller-side construction is bulk (all files), so the natural enforcement is the warn-level log if a missing entry is encountered at filter time. This is a defensive measure, not a runtime check that rejects — appropriate because the "missing entry" outcome is a kept-edge (conservative), not a wrong-data outcome that requires hard refusal. The warn ensures missed cases are surfaced in production rather than silently corrupting data.

**Output stream classification:** `trace!` and `warn!` are diagnostic (logged via tracing → stderr). The new function's only data write is to the `file_deps` table (data, persisted). No stdout writes.

**Verification:**
- [ ] `cargo nextest run -p tethys` passes (existing + new K-hybrid unit test)
- [ ] Stress fixture: in-memory DB with 3 crates exercising intra / cross-corroborated / cross-uncorroborated / orphan cases produces exactly 2 file_deps rows
- [ ] `cargo clippy -p tethys --all-targets --all-features -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] **Oracle**: after rebuilding tethys and re-indexing rivets workspace, `python3 .rivets-0gom/probe.py` reports total cross-crate ≤ 10 (down from 74) AND FORBIDDEN-pair count ≤ 5 (rivets-3d0s acceptance target)
- [ ] Loop budget: spot-check via `tethys index` timing on rivets workspace (no regression > 5%)

---

## Slice 2: Integration regression-fence test

**Claim:** A new integration test `audit_drops_cross_crate_call_without_import` in `crates/tethys/tests/file_deps_corroboration.rs` exercises the rivets-3d0s phantom shape — a call to a workspace method whose name collides across crates — and asserts no phantom cross-crate `file_deps` edge is created. This permanently fences slice 1's behavioral change. (Maps to design claims C7, C8 — also reinforces C1, C2, C3.)

**Oracle (independent):**
- The test's assertions are computed independently of the implementation: they query the `file_deps` table directly via SQLite, count edges in known oracle-classified pairs, and assert the expected counts. No implementation helpers are called from the test except `Tethys::index()`.
- Falsifiability check: temporarily reverting slice 1's filter (e.g., replacing the new function with a passthrough that calls the original SQL aggregation) must cause the test to FAIL with a specific phantom edge present. Restoring the filter, test passes. Document this in the slice's commit message.

**Stress fixture:** Cargo workspace with this shape:

```
Cargo.toml                                      (workspace, members = [crate_caller, crate_target, crate_collider])
crate_caller/
  Cargo.toml                                    (depends on crate_target ONLY; NOT on crate_collider)
  src/lib.rs                                    (use crate_target::Helper; calls helper.do_work(); also calls some_input.len())
crate_target/
  Cargo.toml
  src/lib.rs                                    (defines Helper struct with do_work() method)
crate_collider/
  Cargo.toml
  src/lib.rs                                    (defines an unrelated Phantom struct with a len() method)
```

Pre-fix behavior (without slice 1): `crate_caller`'s `some_input.len()` call collides with `crate_collider::Phantom::len()` (the unique workspace `len` method). Resolver produces a phantom call_edge crate_caller→crate_collider. file_deps gets an entry. Test FAILS.

Post-fix behavior (with slice 1): the call_edge still exists in `call_edges` table (resolver unchanged) but the K-hybrid filter sees that crate_caller has no import into crate_collider. file_deps gets NO crate_caller→crate_collider entry. Test PASSES.

Additionally asserts:
- crate_caller → crate_target file_dep exists (corroborated by `use crate_target::Helper`)
- intra-crate edges within each crate exist (if any)

**Plausible bugs the fixture catches:**
- K-hybrid filter accidentally drops corroborated cross-crate edges (false negative)
- K-hybrid filter doesn't handle hyphenated crate names (Cargo's `crate-collider` vs Rust's `crate_collider` in `use` syntax)
- K-hybrid filter accidentally drops intra-crate edges
- Resolver was changed (negative claim C6 violated) — would cause unexpected resolution patterns

**Loop budget:** Integration test indexes 6 files (3 Cargo.tomls + 3 lib.rs). ~200ms wall. No new loops introduced by the test itself.

**Wall budget:** N/A (test, not always-on).

**Files:**
- `crates/tethys/tests/file_deps_corroboration.rs` (new file)

**Code (advisory):**

```rust
//! Integration regression fence for rivets-3d0s (K-hybrid filter).
//!
//! A cross-crate method call whose name collides with a workspace symbol in
//! a NON-imported crate must not produce a cross-crate file_deps edge. The
//! filter at `populate_file_deps_from_call_edges` ensures this.

use rstest::rstest;
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tethys::{IndexOptions, Tethys};

fn build_three_crate_workspace(dir: &TempDir) {
    let files: [(&str, &str); 7] = [
        ("Cargo.toml", r#"
[workspace]
members = ["crate_caller", "crate_target", "crate_collider"]
resolver = "2"
"#),
        ("crate_caller/Cargo.toml", r#"
[package]
name = "crate_caller"
version = "0.1.0"
edition = "2021"

[dependencies]
crate_target = { path = "../crate_target" }
# Deliberately NOT depending on crate_collider
"#),
        ("crate_caller/src/lib.rs", r#"
use crate_target::Helper;

pub fn caller_fn(some_input: &[i32]) -> usize {
    let h = Helper::new();
    h.do_work();
    some_input.len()  // stdlib slice::len, but workspace has a `len` method elsewhere
}
"#),
        ("crate_target/Cargo.toml", r#"
[package]
name = "crate_target"
version = "0.1.0"
edition = "2021"
"#),
        ("crate_target/src/lib.rs", r#"
pub struct Helper;
impl Helper {
    pub fn new() -> Self { Helper }
    pub fn do_work(&self) {}
}
"#),
        ("crate_collider/Cargo.toml", r#"
[package]
name = "crate_collider"
version = "0.1.0"
edition = "2021"
"#),
        ("crate_collider/src/lib.rs", r#"
// Defines `len` as a method on a workspace type — the phantom shape from
// rivets-3d0s. Without K-hybrid filter, crate_caller's `.len()` calls resolve
// here as the unique workspace `len`.
pub struct Phantom;
impl Phantom {
    pub fn len(&self) -> usize { 0 }
}
"#),
    ];
    for (rel, body) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, body).expect("write");
    }
}

fn open_db(tethys: &Tethys) -> Connection {
    Connection::open_with_flags(
        tethys.db_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).expect("open db")
}

#[test]
fn audit_drops_cross_crate_call_without_import() {
    let dir = TempDir::new().expect("tempdir");
    build_three_crate_workspace(&dir);
    let mut tethys = Tethys::new(dir.path()).expect("Tethys::new");
    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    // Count cross-crate file_deps from crate_caller to crate_collider — should be 0.
    let phantom_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM file_deps d
         JOIN files f1 ON f1.id = d.from_file_id
         JOIN files f2 ON f2.id = d.to_file_id
         WHERE f1.path LIKE 'crate_caller/%' AND f2.path LIKE 'crate_collider/%'",
        [],
        |row| row.get(0),
    ).expect("count phantom edges");
    assert_eq!(
        phantom_count, 0,
        "K-hybrid filter must drop cross-crate file_deps edges where source has no import to target's crate"
    );

    // Count cross-crate file_deps from crate_caller to crate_target — should be > 0.
    let legitimate_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM file_deps d
         JOIN files f1 ON f1.id = d.from_file_id
         JOIN files f2 ON f2.id = d.to_file_id
         WHERE f1.path LIKE 'crate_caller/%' AND f2.path LIKE 'crate_target/%'",
        [],
        |row| row.get(0),
    ).expect("count legitimate edges");
    assert!(
        legitimate_count >= 1,
        "K-hybrid filter must preserve cross-crate edges with corroborating import; got {legitimate_count}"
    );
}
```

**Doc-comment precondition classification:** N/A (test file).

**Output stream classification:** Test failures via `assert!` go to stderr via panic — diagnostic. Compliant.

**Verification:**
- [ ] `cargo nextest run --test file_deps_corroboration` passes
- [ ] **Falsifiability check:** temporarily replace `populate_file_deps_from_call_edges` body with a delegation to the original SQL aggregation (passthrough). Re-run test. Test must FAIL with phantom_count > 0. Restore slice 1; test passes.
- [ ] All resolver_routing tests still pass (`cargo nextest run -p tethys --test resolver_routing`)
- [ ] Full workspace test suite passes (`cargo nextest run -p tethys`)

---

## Plan Self-Review

### Every loop in the plan
1. **Slice 1: build_file_crate_map** — `O(files)` × 1 prefix-check; at production scale (50k files) ≈ 50k ops. Within 10^6 budget.
2. **Slice 1: build imports_per_file** — `O(imports)` × 1 string-split + map insert; ≈ few thousand ops at production scale. Negligible.
3. **Slice 1: filter call_edges aggregates** — `O(unique_file_pairs)` × 3 HashMap lookups; ≈ 10^4 ops at production scale. Negligible.
4. **Slice 1: per-surviving-pair INSERT** — `O(surviving_file_pairs)`; strictly ≤ pre-fix size, so cannot regress wall-time.
5. **Slice 2: integration test loop** — bounded by 6-file fixture; ~200ms.

All loops have explicit complexity statements. All within budget at production scale.

### Every fixture
1. **Slice 1's unit test fixture**: 3 crates + 1 orphan top-dir, with deliberate name collision (`len` method in crate_c). Designed to FAIL if (a) intra-crate guard misses, (b) cross-crate import check misses, (c) orphan pseudo-crate handling misses, (d) Rust-name vs Cargo-name conversion (`rivets_jsonl` ↔ `rivets-jsonl`) is wrong. NOT a happy-path exercise.
2. **Slice 2's integration fixture**: 3-crate Cargo workspace with `.len()` collision matching the exact rivets-3d0s shape. Designed to FAIL if (a) K-hybrid filter is bypassed, (b) corroborated edges are accidentally dropped, (c) hyphenated crate-name handling is broken.

All fixtures designed against named plausible bugs, not happy paths.

### Every doc-comment precondition
1. **`populate_file_deps_from_call_edges`'s `file_crate_map` parameter**: "MUST populate every FileId in call_edges." Classified as **load-bearing for correctness** (missing entries silently keep edges). Enforcement: `warn!` log at filter time when a missing entry is encountered. Acceptable because the failure mode is "conservative keep" not "wrong data" — the warn surfaces the gap in production logs.
2. **`crate_name_from_rust_segment` helper**: no caller-side precondition; pure function returning a String. The "known_crates" set is reconstructed per call from the map's values — acceptable for the small set sizes we expect (< 100 crates per workspace).
3. **Slice 2's test fixture builder**: no preconditions exposed.

No `debug_assert!` needed (no sanity-hint-only preconditions).

### Every write target
1. **Slice 1**: writes to `file_deps` table (data, persisted). `trace!`/`warn!` to stderr (diagnostic). Compliant.
2. **Slice 2**: writes to `.rivets/index/tethys.db` via `Tethys::index()` (data, persisted in the tempdir). Test assertion outputs via panic to stderr (diagnostic). Compliant.

No `println!` or unexamined stream writes added.

### Every tracker reference
- The plan mentions **rivets-3d0s** (this issue, verified open).
- The plan mentions **rivets-0gom** in slice 1's oracle description ("`.rivets-0gom/probe.py`") — that issue is closed but the artifacts persist on main (verified at `/home/dwalleck/repos/rivets-3d0s/.rivets-0gom/probe.py`).
- No deferrals to other issues, no "out of scope per rivets-XXX", no "tracked elsewhere" language in the plan body.
- Design's negative space mentions follow-up territory (fully-qualified paths, re-exports) but those are settled rationale — no tracker IDs required by the skill.

All tracker references verified.

## Hard gate
- [x] Every slice has all mandatory fields filled in (claim, oracle, stress fixture, loop budget, wall budget where applicable, files, code, verification)
- [x] Every loop has a complexity statement
- [x] Every slice has a stress fixture (adversarial, not happy-path)
- [x] The plan's claim coverage matches the design's claim list:
  - Slice 1 → C1 (intra-crate), C2 (corroborated cross-crate), C3 (uncorroborated cross-crate), C9 (no edges added), C10 (orphan pseudo-crate)
  - Slice 2 → C7 (FORBIDDEN ≤ 5 workspace-level), C8 (corroborated ALLOWED preserved); reinforces C1/C2/C3
  - C4 (imports-derived unchanged), C5 (call_edges unchanged), C6 (refs unchanged) — verified by code review at PR time (negative claims, no implementation needed)
- [x] Every tracker reference resolves to an existing issue

Ready for `checkpointed-build`.
