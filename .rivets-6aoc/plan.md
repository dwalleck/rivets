# rivets-6aoc / rivets-34tv / partial rivets-i8qn — budgeted plan

Five slices. Each ≤30 min, ≤2 files. Slice fields per the `budgeted-plan` skill contract.

## Pre-flight findings (already verified)

- **C3** passed in falsifiable-design (`.rivets-6aoc/cheapest_falsifier.py`): all 4 rivets-workspace crates produce a `src_root()` matching disk.
- **C4** already satisfied by existing tests in `crates/tethys/src/cargo.rs` and `crates/tethys/tests/cargo_discovery.rs`:
  - `get_crate_for_file_finds_matching_crate`
  - `get_crate_for_file_returns_none_for_no_match`
  - `get_crate_for_file_prefers_longest_prefix_match` (catches the `rivets-jsonl` vs `rivets` path-prefix bug class)
  - `get_crate_for_file_finds_correct_crate` (integration on real workspace)

So the per-file crate lookup helper is **out of scope** — `cargo::get_crate_for_file` already exists and works.

---

## Slice 1: `CrateInfo::src_root()` accessor

**Claim:** C1 — `src_root()` returns `crate.path.join(lib_path.parent())` when `lib_path = Some(rel)`; C2 — falls back to `crate.path.join("src")` when `lib_path = None`.

**Oracle:** Hand-computed expected `PathBuf` per test case; `Path::ends_with` checks for cross-platform compatibility (Windows uses `\`, Unix uses `/`). Independent of any tethys resolver code.

**Stress fixture:** A `CrateInfo` with non-standard `lib_path = Some("custom/nonstandard/lib.rs")`. A buggy impl that hardcodes `"src"` regardless of `lib_path` would return `<path>/src` (not `<path>/custom/nonstandard`) and fail. This catches the bug class "impl ignored `lib_path` and just appended `src`."

**Loop budget:** No loops introduced. Pure `PathBuf::join` operations.

**Wall budget:** N/A (no always-on phase touched).

**Files:**
- `crates/tethys/src/types.rs` — add `impl CrateInfo { pub fn src_root(...) }` + 4 tests in the existing tests module.

**Code (advisory):**

```rust
impl CrateInfo {
    /// Returns the directory containing this crate's source modules.
    ///
    /// When `lib_path` is `Some(path)`, returns `self.path.join(path.parent())`.
    /// Falls back to `self.path.join("src")` for crates with no `lib_path`
    /// (bin-only crates with non-standard layouts).
    #[must_use]
    pub fn src_root(&self) -> PathBuf {
        match &self.lib_path {
            Some(rel) => match rel.parent() {
                Some(p) if !p.as_os_str().is_empty() => self.path.join(p),
                _ => self.path.clone(),
            },
            None => self.path.join("src"),
        }
    }
}
```

**Doc-comment-as-contract:** The doc says "falls back to `<path>/src` when `lib_path` is `None`." This is descriptive of behavior (the return value IS the contract; callers can compute the alternative themselves). **Sanity-hint** classification — no enforcement needed beyond the type's return shape (`PathBuf` always returns something useful).

**Verification:**
- [ ] 4 unit tests pass (lib_path=Some("src/lib.rs"), Some("custom/nested/lib.rs"), Some("lib.rs") — parent empty, None)
- [ ] Stress fixture (non-standard lib_path) returns the right path, not `src`
- [ ] `cargo nextest run -p tethys` — full suite still green
- [ ] No new clippy warnings

---

## Slice 2: Wire `resolve.rs::resolve_cross_file_references` to per-file crate root

**Claim:** Partial C5 — site #1 of 4 starts contributing to the Pass-2-imports newly-resolvable count.

**Oracle:** The prove-it-prototype probe (`.rivets-6aoc/probe.py`) — its "112 migrate" count is the target. Post-this-slice, the SQL on the `references` table should show some of those 112 (file, first_segment) pairs starting to resolve via Pass-2-imports for files in this site's call path.

**Stress fixture:** A test workspace with 2 crates where `crate_a/src/lib.rs` has `use crate::module_a;` (resolves to `crate_a/src/module_a.rs`) AND `crate_b/src/lib.rs` has `use crate::module_b;` (resolves to `crate_b/src/module_b.rs`). Pre-fix: neither resolves (workspace_root/src doesn't exist, so module_a.rs and module_b.rs aren't found). Post-fix: both resolve to their respective crates' files. Catches the bug "I forgot to use the per-file crate's src_root and kept the workspace_root one."

**Loop budget:** Existing loop is `O(files_with_unresolved_refs)` iterating `by_file`. The change moves `crate_root` computation from outside the loop to inside, adding `O(crates)` per iteration (the `get_crate_for_file` linear scan).

- New cost: `O(files_with_unresolved_refs × crates)`
- Production scale (rivets): 50 files × 4 crates = 200 ops per Pass-2 invocation
- Large workspace pessimism: 50,000 files × 100 crates = 5×10⁶ ops
- 5×10⁶ exceeds the 10⁶ budget — **justification required**

**Justification:** The inner operation is `Path::starts_with` (microsecond-scale, integer comparison after path-component parsing). Pass 2's per-file inner work is dominated by SQL queries on `imports` and `symbols` (each ref needs multiple round-trips); the added `O(crates)` lookup is a low-single-digit microsecond hit per file. For workspaces with crate-count ≥ ~100, swap to a pre-computed `BTreeMap<PathBuf, &CrateInfo>` for O(log crates) lookup. Pre-computed in a local once-per-Pass-2 step, this drops the cost back to `O(files_with_unresolved_refs × log crates)` ≈ 50,000 × 7 = 3.5×10⁵, well under budget. **Acceptance: ship the simple linear scan; revisit if a workspace appears with >50 crates and Pass-2 wall-time becomes non-trivial.** Tracked at [rivets-bjdn](rivets-bjdn).

**Wall budget:** Pass 2 runs once per `tethys index`. Currently ~hundreds of milliseconds on the rivets workspace. The change adds <1% wall-time. No new always-on phase introduced.

**Files:**
- `crates/tethys/src/resolve.rs` — modify `resolve_cross_file_references` to compute per-file crate root.

**Code (advisory):**

```rust
// In resolve_cross_file_references, replace:
//   let crate_root = self.workspace_root.join("src");
//   for (file_id, refs) in by_file {
//       resolved_count += self.resolve_refs_for_file(file_id, refs, &crate_root)?;
//   }
// with:
for (file_id, refs) in by_file {
    let crate_root = self
        .db
        .get_file_by_id(file_id)?
        .and_then(|f| {
            let abs = self.workspace_root.join(&f.path);
            cargo::get_crate_for_file(&abs, &self.crates).map(CrateInfo::src_root)
        })
        .unwrap_or_else(|| self.workspace_root.join("src")); // fallback for files outside any crate
    resolved_count += self.resolve_refs_for_file(file_id, refs, &crate_root)?;
}
```

**Doc-comment-as-contract:** The fallback to `workspace_root.join("src")` for files outside any crate preserves pre-fix behavior for that edge case (rather than failing). This is **sanity-hint** — files outside any crate shouldn't have `crate::*` imports in the first place; if they do, we don't make their resolution worse. No runtime enforcement needed.

**Verification:**
- [ ] New integration test: 2-crate workspace, both `use crate::module_X` resolve
- [ ] Existing `module_path_integration` test still passes
- [ ] `cargo nextest run -p tethys` — full suite green
- [ ] Probe re-run shows fewer pairs in the "migrate" set (i.e., some have *already* migrated to Pass-2-imports)

---

## Slice 3: Wire `indexing.rs` (sites #2 + #3)

**Claim:** Partial C5 — sites #2 and #3 (the two `compute_dependencies*` functions) start contributing to the migrate count for the dep-graph side.

**Oracle:** The dep-graph table in tethys.db. Pre-fix, `depended_files` entries for files with `use crate::module_X` should be missing (or only the same-crate ones from fallback). Post-fix, they should include sub-crate entries pointing at the correct per-crate `src/`.

**Stress fixture:** Same 2-crate workspace as slice 2. After indexing, query the dep graph: does `crate_a/src/lib.rs` have a dep edge to `crate_a/src/module_a.rs`? Does `crate_b/src/lib.rs` have a dep edge to `crate_b/src/module_b.rs`? Pre-fix both are absent; post-fix both present. Catches "I wired site #1 but forgot the dep-graph sites have the same bug."

**Loop budget:** Same shape as slice 2. Both `compute_dependencies` and `compute_dependencies_from_stored` iterate over imports per file. The per-file crate lookup is O(crates) added per file. Same justification as slice 2 applies.

**Wall budget:** N/A — same Pass-2 / indexing phase; <1% added wall-time.

**Files:**
- `crates/tethys/src/indexing.rs` — two near-identical changes at the two sites (~857, ~1023).

**Code (advisory):** Same pattern as slice 2 — replace `let crate_root = self.workspace_root.join("src");` with per-file lookup that calls `cargo::get_crate_for_file(...).src_root()` with the workspace-root fallback.

**Doc-comment-as-contract:** Same as slice 2.

**Verification:**
- [ ] Dep-graph stress fixture: both expected edges present post-fix
- [ ] `cargo nextest run -p tethys` — full suite green
- [ ] Re-run probe; another chunk of migrating pairs moves to "resolved via imports"

---

## Slice 4: Wire `resolver.rs:61` (workspace-crate arm from rivets-v465)

**Claim:** Partial C5 — site #4 closes the rivets-v465-introduced fourth instance of the same bug.

**Oracle:** A targeted unit test in `resolver.rs::tests`. The fix replaces `target.path.join("src")` with `target.src_root()`. For a workspace-crate target with non-standard `lib_path = Some("custom/path/lib.rs")`, the resolver should look in `<crate>/custom/path/` for module files, not `<crate>/src/`.

**Stress fixture:** Build on the existing `workspace_with_crates` helper. Add a variant (or extend) that allows specifying a non-standard `lib_path`. The test constructs a target crate with `lib_path = Some("custom/lib.rs")` containing a `module.rs` in `<crate>/custom/`. Resolution of `use target_crate::module` from a caller should produce `<target>/custom/module.rs`, not `<target>/src/module.rs` (the latter wouldn't exist). A pre-fix impl using `target.path.join("src")` would return None.

**Loop budget:** No new loop. Single accessor call replacing `Path::join`.

**Wall budget:** N/A.

**Files:**
- `crates/tethys/src/resolver.rs` — one-line replacement at line 61 + one new test.

**Code (advisory):**

```rust
// In the workspace-crate arm of resolve_module_path:
// before:  let other_src = target.path.join("src");
// after:
let other_src = target.src_root();
```

Plus a new test:

```rust
#[test]
fn workspace_crate_arm_uses_src_root_not_hardcoded_src() {
    let dir = tempfile::tempdir().expect("temp dir");
    let target_crate = dir.path().join("target");
    fs::create_dir_all(target_crate.join("custom")).expect("custom dir");
    fs::write(target_crate.join("custom/lib.rs"), "").expect("lib.rs");
    fs::write(target_crate.join("custom/module.rs"), "pub fn x() {}").expect("module.rs");

    let caller_crate = dir.path().join("caller");
    fs::create_dir_all(caller_crate.join("src")).expect("caller src");
    fs::write(caller_crate.join("src/lib.rs"), "").expect("caller lib.rs");

    let crates = vec![
        CrateInfo { name: "caller".into(), path: caller_crate.clone(),
                    lib_path: Some(PathBuf::from("src/lib.rs")), bin_paths: vec![] },
        CrateInfo { name: "target".into(), path: target_crate.clone(),
                    lib_path: Some(PathBuf::from("custom/lib.rs")), bin_paths: vec![] },
    ];

    let result = resolve_module_path(
        &["target".to_string(), "module".to_string()],
        &caller_crate.join("src/lib.rs"),
        &caller_crate.join("src"),
        &crates,
    );
    let resolved = result.expect("non-standard lib_path target should resolve via src_root");
    assert!(
        resolved.ends_with("custom/module.rs") || resolved.ends_with("custom\\module.rs"),
        "expected custom/module.rs (via lib_path.parent()), got {resolved:?}"
    );
}
```

**Doc-comment-as-contract:** N/A — call site change.

**Verification:**
- [ ] New stress test for non-standard lib_path passes
- [ ] All 8 existing resolver tests still pass
- [ ] `cargo clippy -p tethys --all-targets --all-features -- -D warnings` clean

---

## Slice 5: Full-fix measurement gate (C5, C6, C7)

**Claim:** C5 — Pass-2-imports with fallback disabled resolves ≥T newly-resolvable refs (T ≥ 100); C6 — total resolved-ref count doesn't decrease post-fix; C7 — `tethys index` wall-clock doesn't regress >10%.

**Oracle:**
- C5: SQL counts on `references` table grouped by source_module pattern, comparing baseline (fallback-disabled, pre-fix) vs measurement (fallback-disabled, post-fix). Independent of any single slice's correctness.
- C6: `SELECT COUNT(*) FROM refs WHERE symbol_id IS NOT NULL` pre/post.
- C7: `hyperfine --warmup 1 --runs 5 'tethys index ...'` pre/post.

**Stress fixture:** The rivets workspace itself. The probe∩oracle from prove-it-prototype produced 104 pairs that should migrate. Real-workspace measurement is the integration check.

**Loop budget:** N/A — measurement only, no code change.

**Wall budget:** N/A.

**Files:** None modified. Two measurement scripts in `.rivets-6aoc/`:
- `measure_c5_c6.py` — runs SQL queries and prints comparison
- `measure_c7.sh` — wraps `hyperfine` invocations

**Code (advisory):**

C5 requires the toggle-fallback technique from CLAUDE.md. A one-line code change in `resolve.rs` (`if false && let Some(symbol) = fallback_symbol_search(...)`), recompiled, then re-indexed. Snapshot the resolved-count before AND after applying the fix (with toggle off both times). The diff is the C5 number.

**Doc-comment-as-contract:** N/A.

**Output stream:** Measurement scripts write to stdout (data — could be piped to `jq`/`awk` for analysis). Errors and progress to stderr.

**Verification:**
- [ ] C5: Pass-2-imports-resolved cross-crate refs (fallback off) post-fix ≥ pre-fix + 100
- [ ] C6: total resolved-ref count post-fix ≥ pre-fix
- [ ] C7: `hyperfine` post / pre ratio < 1.10
- [ ] Probe re-run: zero remaining "migrate" pairs (all 112 should now resolve via imports)
- [ ] Oracle agrees with binary: re-run `.rivets-6aoc/oracle.sh`, all 105 migrating pairs match what the resolver now does

---

## Plan Self-Review

### 1. Every loop's complexity

| Slice | Loop | Asymptotic | Production scale | Total | Within budget? |
|-------|------|------------|------------------|-------|----------------|
| 1 | (no new loop — pure PathBuf::join) | — | — | — | ✓ |
| 2 | files_with_unresolved_refs × crates (per-file `get_crate_for_file`) | O(F × C) | rivets: 50 × 4 = 200. Large workspace pessimism: 50k × 100 = 5×10⁶ | 5×10⁶ | **Over budget at extreme scale; justified inline** (Path::starts_with is fast, dominated by SQL; swap to BTreeMap if >50 crates appears) |
| 3 | Same shape as slice 2 (2 call sites in indexing.rs) | O(F × C) | Same as slice 2 | Same | Same justification |
| 4 | (no new loop — single accessor call) | — | — | — | ✓ |
| 5 | (measurement only — no new code loops) | — | — | — | ✓ |

### 2. Every fixture

| Slice | Fixture | Bug class it falsifies |
|-------|---------|------------------------|
| 1 | `lib_path = Some("custom/nonstandard/lib.rs")` | "Impl ignores `lib_path` and just appends `src/`" |
| 2 | 2-crate workspace, both with `use crate::module_X` | "Wired site #1 but forgot to actually pass per-file crate_root" |
| 3 | Same 2-crate workspace, query dep-graph edges | "Wired one site but missed the dep-graph sites" |
| 4 | Workspace-crate target with `lib_path = Some("custom/lib.rs")` | "Used `target.path.join('src')` instead of `target.src_root()`" |
| 5 | The rivets workspace itself, measured pre/post | "Slice tests all passed but full-fix didn't actually migrate refs" |

### 3. Every doc-comment precondition

| Slice | Precondition | Class | Enforcement |
|-------|--------------|-------|-------------|
| 1 | "Falls back to `<path>/src` when lib_path is None" | Sanity-hint (descriptive, return type is the contract) | None needed; type signature is the enforcement |
| 2 | Fallback for files-outside-any-crate is `workspace_root.join("src")` | Sanity-hint (preserves pre-fix behavior for an edge case that shouldn't have crate:: imports anyway) | No runtime check; the `unwrap_or_else` IS the documented behavior |
| 3 | Same as slice 2 | Same | Same |
| 4 | N/A (call-site change only) | — | — |
| 5 | N/A (measurement only) | — | — |

### 4. Every write target

| Slice | Writes | Stream | Class |
|-------|--------|--------|-------|
| 1 | (none — pure function) | — | — |
| 2 | `tracing::debug!` / `trace!` for crate-not-found cases | stderr | Diagnostic |
| 3 | Same as slice 2 | stderr | Diagnostic |
| 4 | (none — single expression change) | — | — |
| 5 | Measurement script outputs | stdout (data — for `jq`/`awk` pipes) and stderr (progress) | Both classified explicitly |

### Gate checklist

- [x] Every slice has all mandatory fields filled in
- [x] Every loop has a complexity statement
- [x] Every slice has a stress fixture
- [x] The plan's claim coverage matches the design's claim list:
  - C1, C2 → slice 1
  - C3 → already passed (cheap falsifier in design phase)
  - C4 → already satisfied (existing tests on `get_crate_for_file`)
  - C5 → slice 5 (measurement gate; partial accumulation via slices 2-4)
  - C6 → slice 5
  - C7 → slice 5

All gaps closed. Plan is ready for `checkpointed-build`.
