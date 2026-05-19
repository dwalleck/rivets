# rivets-wsix budgeted plan

Three slices, one per design claim. All slices are pure-test additions (no production code changes). Single new file `crates/tethys/tests/reindex_cascade.rs`. Per-slice budgets reflect test execution time, not production runtime.

## Slice 1: refs cascade on call removal

**Claim:** [design C1] After a file's source is mutated to remove a function-body call, re-indexing without DB reset removes the corresponding row from `refs`.

**Oracle:** Direct SQL `SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id WHERE f.path='src/lib.rs'`. Independent of the indexer: the assertion reads SQLite directly, not through any tethys API layer.

**Stress fixture:**
Starting source: `pub fn entry() { helper::a(); helper::b(); helper::c(); }` → 3 cross-file call refs.
Mutated source: `pub fn entry() { helper::a(); helper::c(); }` → 2 calls (removed the MIDDLE one).
Adversarial intent:
- If the cascade chain were "wipe all refs and re-insert," count goes 3 → 2. ✓ test passes.
- If the cascade missed the middle ref specifically (some weird "first match wins" bug), count goes 3 → 3 or 3 → 1. ✗ test fails.
- If no clearing happened, count goes 3 → 5 (2 new + 3 stale). ✗ test fails.
The exact-count assertion `== 2` distinguishes all three. The middle-removal shape defeats a hypothetical "head-only" or "tail-only" cascade bug.

Also assert: the surviving refs target `helper::a` and `helper::c` specifically (not `helper::b`). Catches the "cascade ran but kept the wrong refs" failure mode.

**Loop budget:** N/A — the slice adds no production loops; only test code.

**Wall budget:** ≤ 5s per test (target for nextest CI). Each `Tethys::index()` call on this fixture is sub-second on the laptop in earlier probes.

**Files:**
- `crates/tethys/tests/reindex_cascade.rs` (new, this slice creates it)

**Code (advisory):**
```rust
//! Regression fences for rivets-wsix: cascade-correctness across re-index runs.
//!
//! The wsix audit (see .rivets-wsix/what-i-learned.md) found that re-index
//! correctness for `refs`, `attributes`, and `symbols` relies on the schema's
//! ON DELETE CASCADE chain, not the `clear_all_X` pattern. These tests lock
//! in that cascade-correctness so a future schema change (e.g., relaxing a
//! cascade FK to SET NULL or NO ACTION) is caught in CI.

use rusqlite::params;

mod common;
use common::{open_db, workspace_with_files};

/// Pin claim 1: removing a function-body call from a file's source produces
/// exactly one row removal from `refs` after re-index, via the
/// `refs.in_symbol_id REFERENCES symbols(id) ON DELETE CASCADE` chain.
#[test]
fn refs_cascade_on_call_removal() {
    let (dir, mut tethys) = workspace_with_files(&[
        ("Cargo.toml", r#"
[package]
name = "wsix_refs"
version = "0.0.0"
edition = "2021"
"#),
        ("src/lib.rs", r"
mod helper;

pub fn entry() {
    helper::a();
    helper::b();
    helper::c();
}
"),
        ("src/helper.rs", r"
pub fn a() {}
pub fn b() {}
pub fn c() {}
"),
    ]);

    tethys.index().expect("initial index");

    let conn = open_db(&tethys);
    let refs_pre: i64 = conn.query_row(
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id JOIN symbols s ON s.id=r.symbol_id
         WHERE f.path='src/lib.rs' AND s.name IN ('a','b','c')",
        params![], |row| row.get(0),
    ).expect("count pre");

    // Sanity: indexer produced what we expected.
    assert_eq!(refs_pre, 3, "fixture should produce 3 cross-file call refs");

    // Mutate source: remove the MIDDLE call. Belt-and-braces mtime bump so
    // tethys doesn't skip-by-hash.
    std::fs::write(dir.path().join("src/lib.rs"), r"
mod helper;

pub fn entry() {
    helper::a();
    helper::c();
}
").expect("rewrite");
    std::thread::sleep(std::time::Duration::from_secs(1));
    filetime::set_file_mtime(dir.path().join("src/lib.rs"),
        filetime::FileTime::now()).expect("touch");

    tethys.index().expect("re-index");

    let conn = open_db(&tethys);
    let refs_post: i64 = conn.query_row(
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id JOIN symbols s ON s.id=r.symbol_id
         WHERE f.path='src/lib.rs' AND s.name IN ('a','b','c')",
        params![], |row| row.get(0),
    ).expect("count post");

    assert_eq!(refs_post, 2, "expected 2 refs (a,c) after removing b()");

    // Stronger: the surviving refs target a and c specifically.
    let b_refs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id JOIN symbols s ON s.id=r.symbol_id
         WHERE f.path='src/lib.rs' AND s.name='b'",
        params![], |row| row.get(0),
    ).expect("count b");
    assert_eq!(b_refs, 0, "ref to helper::b must be gone after source removal");
}
```

If `filetime` is not already a dev-dependency, fall back to deleting + recreating the file (which definitively changes mtime). If even that doesn't trigger re-index, use `Tethys::index_with_options(IndexOptions::rebuild())` — but that defeats the purpose since `--rebuild` does `db.reset()`. The first option (filetime) is the surgical answer.

**Verification:**
- [ ] `cargo nextest run -p tethys --test reindex_cascade refs_cascade_on_call_removal` passes
- [ ] Mutate the assertion to `assert_eq!(refs_post, 3)` (the "bug present" case) and confirm the test fails — TDD-inversion proves non-vacuity
- [ ] `cargo clippy -p tethys --tests -- -D warnings` clean

---

## Slice 2: symbols → attributes cascade on symbol removal

**Claim:** [design C2] Removing an attributed function from source removes both the symbol row AND its attribute rows; cascade `attributes.symbol_id REFERENCES symbols(id) ON DELETE CASCADE` fires.

**Oracle:** Direct SQL count against `symbols` and `attributes` tables, joined by `s.id = a.symbol_id`. Independent of indexer.

**Stress fixture:**
Two attributed symbols. `#[allow(dead_code)] fn target() {}` (the one we'll remove) and `#[allow(dead_code)] fn keep() {}` (the one that stays). Remove `target` from source.

Adversarial intent:
- Correct cascade: target gone, target's attributes gone, keep still there, keep's attributes still there. Final counts: `symbols where name='target'` = 0, `attributes joined on symbols where s.name='target'` = 0, `symbols where name='keep'` = 1, `attributes joined on symbols where s.name='keep'` ≥ 1.
- Cascade too aggressive (clears all attributes for the file, not just for the deleted symbol): keep's attributes also drop to 0. Test catches.
- Cascade too narrow / not firing: target's attribute rows persist with stale `symbol_id`. Test catches via the target-attribute count assertion.

**Loop budget:** N/A.

**Wall budget:** ≤ 5s.

**Files:**
- `crates/tethys/tests/reindex_cascade.rs` (modify — append a new `#[test] fn attributes_cascade_on_symbol_removal`)

**Code (advisory):**
```rust
/// Pin claim 2: removing an attributed symbol from source removes the symbol
/// AND its attribute rows via `attributes.symbol_id ON DELETE CASCADE`. The
/// keep-symbol's attributes must remain.
#[test]
fn attributes_cascade_on_symbol_removal() {
    let (dir, mut tethys) = workspace_with_files(&[
        ("Cargo.toml", /* same as slice 1 */ ),
        ("src/lib.rs", r"
#[allow(dead_code)]
pub fn target() {}

#[allow(dead_code)]
pub fn keep() {}
"),
    ]);

    tethys.index().expect("initial");

    let conn = open_db(&tethys);
    let target_attrs_pre: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attributes a JOIN symbols s ON s.id=a.symbol_id WHERE s.name='target'",
        params![], |row| row.get(0)).expect("count");
    let keep_attrs_pre: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attributes a JOIN symbols s ON s.id=a.symbol_id WHERE s.name='keep'",
        params![], |row| row.get(0)).expect("count");

    assert!(target_attrs_pre >= 1, "fixture should index target's #[allow] attribute");
    assert!(keep_attrs_pre >= 1, "fixture should index keep's #[allow] attribute");

    // Remove target from source.
    std::fs::write(dir.path().join("src/lib.rs"), r"
#[allow(dead_code)]
pub fn keep() {}
").expect("rewrite");
    std::thread::sleep(std::time::Duration::from_secs(1));
    filetime::set_file_mtime(dir.path().join("src/lib.rs"),
        filetime::FileTime::now()).expect("touch");
    tethys.index().expect("re-index");

    let conn = open_db(&tethys);
    let target_attrs_post: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attributes a JOIN symbols s ON s.id=a.symbol_id WHERE s.name='target'",
        params![], |row| row.get(0)).expect("count post");
    let keep_attrs_post: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attributes a JOIN symbols s ON s.id=a.symbol_id WHERE s.name='keep'",
        params![], |row| row.get(0)).expect("count post");
    let target_sym_post: i64 = conn.query_row(
        "SELECT COUNT(*) FROM symbols WHERE name='target'",
        params![], |row| row.get(0)).expect("count");

    assert_eq!(target_sym_post, 0, "target symbol must be gone after source removal");
    assert_eq!(target_attrs_post, 0, "target's attributes must cascade-delete with the symbol");
    assert_eq!(keep_attrs_post, keep_attrs_pre,
        "keep's attributes must NOT cascade-delete (cascade was too aggressive)");
}
```

**Verification:**
- [ ] Test passes on current main
- [ ] TDD-inversion: temporarily change `target_attrs_post` assert to `== keep_attrs_pre` (i.e., expect bug); confirm test fails
- [ ] Clippy clean

---

## Slice 3: clear_all stability on unchanged-source re-index

**Claim:** [design C3] Running `Tethys::index()` twice on an unchanged workspace produces identical row counts in `call_edges` and `file_deps` (the existing `clear_all_X` discipline holds).

**Oracle:** Direct SQL `SELECT COUNT(*) FROM call_edges` and `SELECT COUNT(*) FROM file_deps`. Independent.

**Stress fixture:**
Two-file workspace where lib.rs calls helper.rs's function (producing one call_edge AND one file_dep). Index twice. Assert both counts EQUAL between runs.

Adversarial intent:
- Working `clear_all`: count1 == count2. Test passes.
- `clear_all_file_deps` removed from `index_with_options`: file_deps doubles to 2. Test fails.
- `clear_all_call_edges` removed: call_edges doubles to 2. Test fails.
- A UPSERT bug where ref_count keeps incrementing without new rows: row counts equal, but per-row `ref_count` doubles. Add a third assertion on `SELECT SUM(ref_count) FROM file_deps` to catch this.

**Loop budget:** N/A.

**Wall budget:** ≤ 5s.

**Files:**
- `crates/tethys/tests/reindex_cascade.rs` (modify — append `#[test] fn clear_all_tables_stable_under_reindex`)

**Code (advisory):**
```rust
/// Pin claim 3: re-indexing an unchanged workspace produces stable counts in
/// `call_edges` and `file_deps`. Catches regression of the `clear_all_X`
/// discipline (rivets-lcb6's fix for file_deps, plus call_edges).
#[test]
fn clear_all_tables_stable_under_reindex() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("Cargo.toml", /* ... */ ),
        ("src/lib.rs", r"
mod helper;

pub fn entry() {
    helper::do_thing();
}
"),
        ("src/helper.rs", r"pub fn do_thing() {}"),
    ]);

    tethys.index().expect("first");

    let conn = open_db(&tethys);
    let ce1: i64 = conn.query_row("SELECT COUNT(*) FROM call_edges", params![], |row| row.get(0)).expect("ce1");
    let fd1: i64 = conn.query_row("SELECT COUNT(*) FROM file_deps", params![], |row| row.get(0)).expect("fd1");
    let fd_sum1: i64 = conn.query_row(
        "SELECT COALESCE(SUM(ref_count), 0) FROM file_deps", params![], |row| row.get(0)
    ).expect("fd_sum1");

    assert!(ce1 >= 1, "fixture should produce at least one call_edge");
    assert!(fd1 >= 1, "fixture should produce at least one file_dep");

    // Re-index without source change. Same fixture, same files, no mtime bump.
    // tethys may skip-by-hash, which is FINE for this test — the assertion
    // we care about is that *if* the indexer touches these tables again, the
    // clear_all_X path runs before re-insertion. Force a re-index by calling
    // index() again; under the current implementation, even skipped files'
    // dependencies are recomputed.
    tethys.index().expect("second");

    let conn = open_db(&tethys);
    let ce2: i64 = conn.query_row("SELECT COUNT(*) FROM call_edges", params![], |row| row.get(0)).expect("ce2");
    let fd2: i64 = conn.query_row("SELECT COUNT(*) FROM file_deps", params![], |row| row.get(0)).expect("fd2");
    let fd_sum2: i64 = conn.query_row(
        "SELECT COALESCE(SUM(ref_count), 0) FROM file_deps", params![], |row| row.get(0)
    ).expect("fd_sum2");

    assert_eq!(ce1, ce2, "call_edges count must not grow across unchanged-source re-index");
    assert_eq!(fd1, fd2, "file_deps count must not grow across unchanged-source re-index");
    assert_eq!(fd_sum1, fd_sum2, "file_deps SUM(ref_count) must not grow either (UPSERT-aggregate fence)");
}
```

**Verification:**
- [ ] Test passes on current main
- [ ] TDD-inversion: comment out `self.db.clear_all_file_deps()` in `indexing.rs:139`; confirm `fd_sum1 == fd_sum2` assertion fails (this is the lcb6 regression check)
- [ ] Clippy clean

---

## Plan Self-Review

### 1. Every loop in the plan
- None of the 3 slices adds a new production loop. All slices are test-only. Test-execution loops (over fixture files, over assertions) are bounded by fixture size (≤ 5 files, ≤ 5 assertions per test). No budget concern.

### 2. Every fixture
- Slice 1: middle-removal stress, designed to fail under "wrong-cascade-row removal" bugs. Not happy-path.
- Slice 2: two-symbol-different-attribute-status, designed to fail under both "cascade missed" and "cascade too wide" bugs. Not happy-path.
- Slice 3: cross-file call (covers both call_edges and file_deps) plus a `SUM(ref_count)` assertion, designed to fail under both "clear_all missing" and "UPSERT-aggregate growth without new rows." Not happy-path.

### 3. Every doc-comment precondition
None of the slices introduces a doc-commented precondition. The slices ARE the regression fences for an already-documented (in `what-i-learned.md`) invariant. No new contracts; no enforcement-strength classification needed.

### 4. Every write target
- All asserts go through `panic!` (via the `assert*!` macros) and `nextest` captures them to stderr. That's diagnostic. No `println!`/`eprintln!` introduced.

### 5. Every tracker reference
The plan references:
- **rivets-wsix** (the active issue — verified open and in-progress)
- **rivets-lcb6** (closed, the precedent fix — referenced for context, no deferred work)
- **rivets-dhxo** (open, orphan-file boundary — explicit out-of-scope per design.md, not deferred FROM this work)

No "TODO" or "follow-up" trigger phrases in the plan body. All slices ship in this PR or get rejected; none are deferred.

## Hard gate

- [x] Every slice has all mandatory fields filled in (Claim, Oracle, Stress fixture, Loop budget, Wall budget, Files, Code, Verification)
- [x] Every loop has a complexity statement (N/A noted explicitly for each slice — no new production loops)
- [x] Every slice has a stress fixture
- [x] Plan's claim coverage matches design's claim list (3 slices ↔ 3 design claims, 1:1)
- [x] Every tracker reference resolves to an existing issue

Ready for checkpointed-build.
