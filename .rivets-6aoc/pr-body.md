## Summary

Closes **rivets-6aoc** and **rivets-34tv**, and ships the `src_root()` portion of **rivets-i8qn**. Fixes tethys's resolver pipeline at four sites that all hardcoded `workspace_root.join("src")` (or `target.path.join("src")` in the fourth instance, introduced by the just-merged rivets-v465). The hardcode is silently broken for any multi-crate workspace where the root has no `src/` directory — which is the typical shape of every Cargo workspace, including this one.

**Empirical impact on the rivets workspace:** +110 newly-resolved references post-fix (6,456 → 6,566), +171 specifically intra-crate. Wall-clock change: +1.3% (well under the 10% threshold). Full measurement details in [`measurement-results.md`](.rivets-6aoc/measurement-results.md).

**Key finding the prove-it-prototype phase surfaced:** The hardcoded `workspace_root.join("src")` directory **doesn't exist at all** in the rivets repo (the workspace root is a manifest-only directory, no `src/`). So pre-fix, Pass-2-imports for `crate::*` paths was **100% non-functional** — every cross-file `crate::*` reference either fell through to Pass-2-fallback (whole-workspace symbol-name search) or remained unresolved. This reframes rivets-6aoc from "broken for sub-crate files" to "broken for all files, universally."

## What this PR contains

The fix is one new accessor on `CrateInfo` (slice 1) plus four call-site updates (slices 2-4) plus measurement (slice 5):

- **Slice 1 (`607680c`)**: `CrateInfo::src_root(&self) -> PathBuf` — encapsulates "what is this crate's source directory" with a three-step derivation: `lib_path.parent()` → first `bin_paths.first().parent()` → fallback `<path>/src`. 6 unit tests including stress fixtures for non-standard `lib_path` (`custom/nested/lib.rs`) and bin-only crates with non-standard layouts (`src/bin/X.rs`).
- **Slice 2 (`9bd4efd`)**: `resolve.rs::resolve_cross_file_references` — replaces the hardcoded `crate_root` (computed once outside the file loop) with a per-file lookup via `cargo::get_crate_for_file + src_root()`. Files outside any known crate are skipped entirely (no `workspace_root/src` fallback — that path is the broken assumption).
- **Slice 3 (`a15ce52`)**: `indexing.rs::compute_dependencies` and `compute_dependencies_from_stored` — same per-file lookup pattern at both dep-graph computation sites.
- **Slice 4 (`e2ffe39`)**: `resolver.rs::resolve_module_path` workspace-crate arm — one-line swap of `target.path.join("src")` → `target.src_root()`. Closes the fourth site introduced by rivets-v465.
- **Slice 5 (`3afccd0`)**: pre-fix and post-fix snapshots from the rivets workspace + hyperfine timing + diagnostic dir artifacts. No production code changes.

### Diagnostic dir (`.rivets-6aoc/`)

Full gilfoyle workflow audit trail:

| Phase | Artifacts |
|---|---|
| prove-it-prototype | `probe.py` (DB-driven), `oracle.sh` (independent grep+filesystem), `diff_sets.py` (debug), `design.md` |
| falsifiable-design | `falsifiable-design.md`, `cheapest_falsifier.py` (claim C3 passed pre-implementation) |
| budgeted-plan | `plan.md` (5 slices, all mandatory fields filled) |
| checkpointed-build | `measure.py`, `measure-pre-fix.json`, `measure-post-fix.json`, `measurement-results.md`, `pr-body.md` (this file) |

## What this PR does NOT fix

- The other two `rivets-i8qn` accessors (`entry_point_file()` and `module_name()`). Only `src_root()` lands here. Tracked at **rivets-i8qn**.
- **rivets-714v** (LSP multi-crate path bug). Separate concern.
- The `get_crate_for_file` linear scan's behavior at >50-crate workspaces. The slice 2 budget admits being over the 10⁶-op budget at extreme scale (50k files × 100 crates = 5×10⁶), justified inline with measured numbers and a documented mitigation (pre-computed `BTreeMap<PathBuf, &CrateInfo>`). Tracked at **rivets-bjdn**.

## Numbers (from `measurement-results.md`)

| Metric | Pre-fix (main, c1af978) | Post-fix (this branch) | Δ |
|---|---|---|---|
| Total refs | 21,481 | 21,627 | +146 |
| Resolved refs | 6,456 | 6,566 | **+110** |
| Intra-crate resolved | 5,985 | 6,156 | **+171** |
| Cross-crate resolved | 305 | 296 | -9 |
| `file_deps` total | 374 | 383 | +9 |
| `file_deps` intra-crate | 285 | 307 | +22 |
| Index wall-clock (hyperfine ±σ post-fix vs single-run pre) | 23.15s | 23.444s ± 0.454s | +1.3% |

## Test plan

- [x] `cargo nextest run -p tethys` — **604 pass**, 6 skipped (was 596 pre-fix; +6 from slice 1 src_root tests, +1 slice 2 multi-crate stress fixture, +1 slice 4 non-standard-lib_path stress fixture)
- [x] `cargo clippy -p tethys --all-targets --all-features -- -D warnings` — clean
- [x] `cargo fmt --check` — clean
- [x] **Stress fixtures pass at every slice:**
  - Slice 1: `src_root_follows_non_standard_lib_path_layout` (`Some("custom/nested/lib.rs")` → `<path>/custom/nested`), `src_root_follows_non_standard_bin_path_layout` (`src/bin/myapp.rs` → `<path>/src/bin`)
  - Slice 2: `pass2_imports_resolve_per_crate_in_multi_crate_workspace` (2-crate workspace, both `use crate::module` resolve to their own crate's `src/`)
  - Slice 4: `workspace_crate_arm_uses_src_root_not_hardcoded_src` (non-standard `lib_path` target crate resolves through `src_root()`)
- [x] **All 7 falsifiable-design claims verified:**
  - C1, C2 (slice 1 unit tests)
  - C3 (design-phase `cheapest_falsifier.py`)
  - C4 (pre-existing tests on `cargo::get_crate_for_file`)
  - C5, C6, C7 (slice 5 measurement)
- [x] **Probe + oracle still agree:** 104-pair intersection unchanged after the fix (both measure workspace structure, not resolver behavior).
- [x] **Reviewer-checkable:** read `.rivets-6aoc/measurement-results.md` for the C5/C6/C7 verdicts and the +110 ref delta interpretation.

## Process notes — drift caught by checkpointed-build

The gilfoyle workflow's per-slice gates caught two classes of drift that would have been catastrophic if discovered at slice 5:

1. **Slice 1 — bin-only crate fallback.** Initial `src_root()` impl used `None => self.path.join("src")` for bin-only crates. User flagged it: *"how do we know src is a directory that exists?"* Fixed by deriving from `bin_paths.first().parent()` before falling back. Two new stress fixtures lock the bin-only branches down.

2. **Slices 2 + 3 — eight (then four more) test fixtures had `Cargo.toml`-less workspaces.** Those fixtures relied on the broken `workspace_root.join("src")` hardcode happening to match their `tempdir/src/` directory. With the fix removing that hardcode, the fixtures' files are no longer in any crate. Three independent copies of `workspace_with_files` (in `tests/common/mod.rs`, `tests/indexing.rs`, `tests/test_topology.rs`) and four inline test bodies were updated to auto-write a default `Cargo.toml`. The "three independent copies" tech debt is real but out of scope here.

3. **`workspace_root.join("src")` as a fallback — same bug class, second instance.** Slice 2's initial impl preserved the workspace_root/src fallback for files outside any crate. User flagged again: *"We just did self.workspace_root.join("src") again. Are we sure src exists?"* Changed to skip Pass-2-imports entirely for files outside any known crate. Honest behavior matching the broken-anyway pre-fix outcome.

4. **Post-completion: documented-but-untracked deferral.** Slice 2's loop budget justification described the get_crate_for_file linear scan and a BTreeMap mitigation with a precise trigger condition (>50 crates AND profiling shows non-trivial wall-time), but never landed as a tracker issue. User caught it post-slice-5: *"do we have a rivets issue tracking that?"* Filed as **rivets-bjdn**, saved as a feedback memory for future workflow runs.

Each of these would have been a slice-5-or-later surprise without the per-slice gates and the user's pattern recognition. Drift at slice 1 is cheap; drift at slice 5 is the whole feature.

## Related issues

| Issue | Status | Relationship |
|---|---|---|
| **rivets-6aoc** | open → closed by this PR | The canonical "hardcoded `workspace_root.join("src")` in `resolve.rs`" issue. All four sites of the same bug class now fixed. |
| **rivets-34tv** | open → closed by this PR | Sibling issue covering the `indexing.rs` sites of the same bug class. Both sites fixed in slice 3. |
| **rivets-i8qn** | open (partially addressed) | The accessor refactor issue. `src_root()` lands here; `entry_point_file()` and `module_name()` remain for a follow-up PR. |
| **rivets-bjdn** | filed during this PR | Deferred BTreeMap optimization for the per-file crate lookup at extreme workspace scales. P4. |
| **rivets-714v** | open | LSP multi-crate path bug. Separate concern; not addressed by this fix. |
| **rivets-v465** | closed in #62 | Predecessor PR. Introduced the fourth instance of the bug class (`resolver.rs::resolve_module_path` workspace-crate arm at line 61) — closed by slice 4 here. |
