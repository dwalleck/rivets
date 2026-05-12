# rivets-v465 — budgeted plan

Source design: `.rivets-v465/falsifiable-design.md`. Cheapest falsifier
(`source_module_shapes.py`) passed at design time: 117 workspace-crate
imports use `path[0]==crate_name`, no aliases.

Three slices, each ≤ 30 min and ≤ 2 files. Total scope: ~50 LOC
implementation + ~70 LOC tests across 2 files (`resolver.rs`, `resolve.rs`).

---

## Slice 1: New `resolve_module_path` arm for workspace crates

**Claim:** Maps design claims **C1** (workspace-crate detection returns
`Some`) + **C3** (external crates still return `None`) + **C4** (no
regression on `crate`/`self`/`super`). When `path[0]` matches a
`CrateInfo::name` (with `-→_` normalization), the function returns
`Some(file_path)` instead of `None`. Other path heads behave as before.

**Oracle:** Independent of the new code: one stress-fixture unit test
that runs the function against a tempdir-built fixture, asserts on the
returned `PathBuf`. Plus `cargo nextest run -p tethys` to verify
existing `crate`/`self`/`super` tests pass (independent of the new arm
because they don't exercise it).

**Stress fixture:** `tempfile::TempDir` workspace with TWO synthetic
crates:
- `crate_a/src/lib.rs`
- `crate_b/src/storage/in_memory/mod.rs`

Build `CrateInfo` records for both. Call `resolve_module_path(["crate_b", "storage", "in_memory"], crate_a/src/lib.rs, crate_a/src/, &[crate_a_info, crate_b_info])`. Pre-fix: `None`. Post-fix: `Some(crate_b/src/storage/in_memory/mod.rs)`.

Bug class targeted: "match arm fires for wrong inputs" (e.g., matches `"crate"` as a workspace crate name) AND "match arm doesn't fire for the right inputs" (e.g., fails on hyphen-normalized names). The fixture's crate names include both a plain (`crate_b`) and a hyphenated reading (covered in Slice 2).

**Loop budget:** No new loops. The match expression is `O(1)` per dispatch. The added arm does one `iter().find()` over `&[CrateInfo]`. Production scale: workspace crates ≈ 4 (rivets) to ~50 (large workspaces). 50 comparisons per import resolution × ~1000 imports = ~50K ops. Well within `10^6` budget.

**Wall budget:** Phase is indexing (one-shot). Pre-fix `tethys index` takes ~22s on rivets workspace. Adding `iter().find()` per import resolution: sub-millisecond per call × ~1000 calls = < 100ms. Sub-1% perceptible change.

**Files:**
- `crates/tethys/src/resolver.rs` (modify `resolve_module_path` signature + add arm + add stress-fixture test)
- `crates/tethys/src/resolve.rs` (update the one caller, `resolve_module_to_file_id`, to pass `&self.crates`)

**Code (advisory):**

```rust
// resolver.rs:
pub fn resolve_module_path(
    path: &[String],
    current_file: &Path,
    crate_root: &Path,
    workspace_crates: &[CrateInfo],
) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    match path[0].as_str() {
        "crate" => resolve_crate_path(&path[1..], crate_root),
        "self"  => resolve_self_path(&path[1..], current_file),
        "super" => resolve_super_path(&path[1..], current_file),
        head    => {
            // rivets-v465: paths starting with a workspace-crate name
            // (Rust 2018+ idiom: `use other_crate::Module::Item`) route
            // to that crate's src/ directory and recurse.
            let target = workspace_crates
                .iter()
                .find(|c| c.name.replace('-', "_") == head)?;
            let other_src = target.path.join("src");
            resolve_crate_path(&path[1..], &other_src)
        }
    }
}
```

```rust
// resolve.rs::resolve_module_to_file_id — pass workspace crates:
let Some(resolved_file) = resolve_module_path(
    &path_segments,
    current_path,
    crate_root,
    self.crates(),  // existing accessor (lib.rs:660)
) else {
    // ...
};
```

**Doc-comment-as-contract:** `CrateInfo::name` semantics — the doc on the
new arm notes that the match uses `name.replace('-', '_')` to convert
Cargo manifest names to Rust module names. Not a load-bearing precondition
on callers (we accept any `CrateInfo`); just a behavior note.

**Output stream:** N/A (function returns a value; no writes).

**Verification:**
- [ ] `cargo nextest run -p tethys` passes (existing `crate`/`self`/`super` tests unaffected — proves **C4**)
- [ ] New stress-fixture test passes (proves **C1** core path)
- [ ] `cargo clippy -p tethys --all-targets --all-features -- -D warnings` clean
- [ ] `cargo fmt --check` clean

---

## Slice 2: Comprehensive unit tests for the new arm

**Claim:** Maps design claims **C2** (multi-segment traversal) + **C3** (external still None) + the hyphen-normalization edge case.

**Oracle:** Each unit test asserts on a `PathBuf` against an
independently-constructed tempdir fixture. The fixture's files are placed
explicitly; the test's expected output is the path of one of those files.
No cross-dependence on the new code.

**Stress fixture:** Same tempdir workspace pattern as Slice 1, extended:
- Single-crate workspace where path[0] is `"serde"` (NOT in CrateInfo list) — must return None
- Multi-crate workspace with `crate_b/src/nested/deep/thing.rs` to verify 3-segment resolution to a non-mod.rs file
- Crate with hyphenated Cargo name (`my-crate`, `module_name == "my_crate"`) to verify the `-→_` conversion fires
- Empty path edge case (already handled by `if path.is_empty() return None;` — explicit test for the assert)

Bug class targeted: each test catches one specific named-bug:
- "match arm fires for external crates" → external-crate test fails
- "match arm doesn't traverse multi-segment paths" → deep-path test fails
- "match arm doesn't normalize hyphens" → hyphenated-name test fails
- "match arm panics on empty path" → empty-path test fails

**Loop budget:** No new loops introduced (test code).

**Wall budget:** N/A (tests).

**Files:**
- `crates/tethys/src/resolver.rs` (test module only)

**Code (advisory):**

```rust
#[test]
fn resolves_workspace_crate_at_path0() {
    let (_dir, crates, current_file) = two_crate_workspace();
    let result = resolve_module_path(
        &["crate_b".into(), "storage".into()],
        &current_file,
        // crate_root irrelevant; arm doesn't use it for non-crate path[0]
        Path::new("unused"),
        &crates,
    );
    assert!(result.is_some(), "expected Some, got None for workspace-crate path");
    let p = result.unwrap();
    assert!(p.ends_with("crate_b/src/storage.rs") || p.ends_with("crate_b/src/storage/mod.rs"));
}

#[test]
fn external_crate_still_returns_none() {
    let (_dir, crates, current_file) = two_crate_workspace();
    let result = resolve_module_path(
        &["serde".into(), "Serialize".into()],
        &current_file,
        Path::new("unused"),
        &crates,
    );
    assert!(result.is_none(), "serde is not in workspace; expected None");
}

#[test]
fn hyphenated_crate_name_resolves_via_underscore() {
    let (_dir, crates, current_file) = workspace_with_hyphenated_crate("my-crate");
    let result = resolve_module_path(
        &["my_crate".into(), "thing".into()],
        &current_file,
        Path::new("unused"),
        &crates,
    );
    assert!(result.is_some(), "hyphenated 'my-crate' should match 'my_crate' path head");
}

#[test]
fn deep_path_traversal_to_non_mod_rs_file() {
    let (_dir, crates, current_file) = workspace_with_nested_module();
    let result = resolve_module_path(
        &["crate_b".into(), "nested".into(), "deep".into(), "thing".into()],
        &current_file,
        Path::new("unused"),
        &crates,
    );
    assert!(result.is_some());
    assert!(result.unwrap().ends_with("nested/deep/thing.rs"));
}
```

**Doc-comment-as-contract:** N/A (tests).

**Output stream:** Test assertions → stderr via panic. Diagnostic. Compliant.

**Verification:**
- [ ] All four new tests pass
- [ ] `cargo nextest run -p tethys resolver` shows the new test count
- [ ] No new clippy warnings

---

## Slice 3: Empirical verification on the real rivets workspace

**Claim:** Maps design claims **C5** (≥80 workspace imports go Pass-2),
**C6** (fallback drops ≥200), **C7** (same-crate count unchanged).

**Oracle:** The disable-fallback rebuild experiment from
prove-it-prototype. Run twice: once with fallback enabled, once with it
disabled. The DIFFERENCE in resolved cross-crate refs equals "refs
resolved by fallback." Pre-fix: fallback handled 100% of cross-crate
(279 refs). Post-fix expectation: fallback handles ≤ 79.

This is independent of the resolver code being modified: the experiment
TOGGLES the resolver's fallback branch as a switch and measures the
binary's observable output (resolved refs in DB).

**Stress fixture:** The actual rivets workspace at HEAD of this branch.
~3000 symbols, ~6000 resolved refs, mixed languages, real
extractor-emitted imports. Bug classes caught at scale: any
plumbing-related bug that doesn't show up in the unit tests' small
fixtures (e.g., interaction with `get_imports_for_file`'s short-circuit
on empty imports, or with the existing same-crate scoping path).

**Loop budget:** No new loops added by this slice (verification only).
The probe scripts themselves are `O(n)` over resolved refs ≈ 6000.

**Wall budget:** Indexing the rivets workspace: ~22s normal. Three full
re-indexes total (normal + disable-fallback + normal-restore) → ~70s.
Plus ~30s for the rebuild between toggles. ~5 min wall total.

**Files:** None modified persistently. The disable-fallback toggle is a
temporary one-line edit to `crates/tethys/src/resolve.rs` (mirror the
prove-it-prototype experiment), reverted at slice end.

**Code:** Reuse `.rivets-v465/check_pass_provenance.py` and the
disable-fallback procedure documented in `.rivets-v465/design.md`'s
Oracle section. Capture results to a new file
`.rivets-v465/after-fix-counts.txt` (alongside the existing
`after-round-N.txt` convention from rivets-0gom).

**Doc-comment-as-contract:** N/A.

**Output stream:** Probe scripts print to stdout (data: counts that a
downstream pipe might want), warnings during indexing go to stderr
(diagnostic). Both correct under the rule.

**Verification:**
- [ ] Build release: `cargo build --release -p tethys` clean
- [ ] Re-index: `tethys.exe index --rebuild` succeeds
- [ ] `check_pass_provenance.py` shows: cross-crate resolved refs total ≥ 279 (no regression) AND same-crate count ≈ 5971 (proves **C7**)
- [ ] Disable-fallback re-index: cross-crate resolved refs ≥ 80 (proves **C5**, the Pass-2-import-resolved count)
- [ ] Compute fallback population: `(normal_cross_crate - disable_fallback_cross_crate) ≤ 79` (proves **C6**, the fallback drop)
- [ ] Restore: re-enable fallback, rebuild, re-index. `cargo nextest run -p tethys` passes.

---

## Plan Self-Review

### Every loop in the plan
1. **Slice 1** — `workspace_crates.iter().find(|c| ...)`. `O(crates)`. Production scale: 4–50 crates × ~1000 imports per index pass = ~50K ops. Within `10^6`.
2. **Slice 2** — No new runtime loops; tests only.
3. **Slice 3** — `check_pass_provenance.py` iterates resolved refs `O(refs)` ≈ 6000. Off-tree script; not a tethys runtime loop.

All loops have explicit complexity. None over budget.

### Every fixture
1. **Slice 1 stress fixture**: two synthetic workspace crates (`crate_a`, `crate_b`), unit-testable, designed to fail if the match arm misfires or doesn't fire. NOT a happy-path-only exercise; targets named bugs.
2. **Slice 2 fixtures**: each unit test pairs to a distinct named bug class (external-crate match, multi-segment, hyphen-normalization, empty path). Each fixture's existence is justified by a bug it would catch.
3. **Slice 3 fixture**: real rivets workspace at production scale (~6000 resolved refs). Adversarial by virtue of scale — catches plumbing bugs the small unit-test fixtures can't see.

All three slices have adversarial fixtures.

### Every doc-comment precondition
- Slice 1: doc-comment on the new arm describes the hyphen-normalization behavior. Not a precondition on callers (every `&[CrateInfo]` is valid input); behavior note. No enforcement needed.
- Slice 1 `path.is_empty()` check: NOT a doc-comment precondition; the function explicitly returns None for empty paths. Defensive code.
- Slices 2/3: no doc-comment preconditions introduced.

No documented preconditions without enforcement; no `debug_assert!`s required.

### Every write target
- Slice 1, Slice 2: function returns; no writes.
- Slice 3: probe scripts write counts to stdout (data) and tethys index logs to stderr (diagnostic). Compliant.

No rogue `println!` introduced.

---

## Hard gate
- [x] Every slice has all mandatory fields filled in (claim, oracle, stress fixture, loop budget, wall budget where applicable, files, code, verification)
- [x] Every loop has a complexity statement
- [x] Every slice has a stress fixture
- [x] The plan's claim coverage matches the design's claim list:
  - Slice 1 → C1 (workspace-crate detection), C3 (external still None), C4 (no regression)
  - Slice 2 → C2 (multi-segment traversal), C3 (external still None — also covered here with explicit fixture), edge cases
  - Slice 3 → C5 (≥80 ws imports go Pass-2), C6 (fallback drops ≥200), C7 (same-crate unchanged)

Ready for `checkpointed-build`.
