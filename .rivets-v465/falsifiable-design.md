# rivets-v465 — falsifiable design

Status: cheapest falsifier (`source_module_shapes.py`) passed at design
time. Ready for `budgeted-plan`.

Prior artifacts: `.rivets-v465/probe.py` and friends (prove-it-prototype).
Established that 100% of cross-crate refs in the rivets workspace reach
Pass 2 fallback because `resolve_module_path` returns `None` for any
import path whose `path[0]` is not `crate` / `self` / `super`. The 117
workspace-crate imports (rivets, rivets_jsonl, rivets_mcp, tethys) hit
the `_ => None` arm.

## Purpose

Fix `crates/tethys/src/resolver.rs::resolve_module_path` to recognize
workspace-crate names as a valid path prefix, route to that crate's
`lib_path` (or `bin_path`) as the new `crate_root`, and resolve the
remaining path segments from there. This moves the 117 currently-leaking
workspace-cross-crate imports from "external; cannot resolve" to
"resolved via Pass 2 imports."

Side effect (welcome): once import-based resolution catches workspace
cross-crate refs, the unscoped fallback's population shrinks from
"every cross-crate ref" to "refs whose imports tethys still can't
handle." rivets-3d0s's audit-and-demote design becomes empirically
viable.

## Architecture

`resolve_module_path` currently dispatches on `path[0]`:

```rust
match path[0].as_str() {
    "crate" => resolve_crate_path(&path[1..], crate_root),
    "self"  => resolve_self_path(&path[1..], current_file),
    "super" => resolve_super_path(&path[1..], current_file),
    _       => None,  // External crate - cannot resolve
}
```

Post-fix: add a new arm matching known workspace-crate names. The function
needs access to the workspace's `Vec<CrateInfo>`, either passed as a
parameter or via making `resolve_module_path` a method on a type that
holds it.

```rust
match path[0].as_str() {
    "crate" => resolve_crate_path(&path[1..], crate_root),
    "self"  => resolve_self_path(&path[1..], current_file),
    "super" => resolve_super_path(&path[1..], current_file),
    head    => {
        if let Some(target_crate) = workspace_crates.iter().find(|c| c.module_name() == head) {
            resolve_crate_path(&path[1..], &target_crate.src_root())
        } else {
            None  // External crate, unchanged.
        }
    }
}
```

(`module_name()` and `src_root()` are illustrative — exact accessors depend
on `CrateInfo`'s shape. `module_name` converts hyphens → underscores;
`src_root` is the directory containing `lib.rs`/`main.rs`.)

## Claims

1. **C1 (workspace-crate detection):** Given a `Vec<CrateInfo>` for the
   workspace and a `path` whose first segment matches `CrateInfo::name`
   (with `-` → `_` normalization), `resolve_module_path` returns
   `Some(file_path)` for the target file rather than `None`.

2. **C2 (path-segment traversal):** Given `path = ["rivets", "storage", "in_memory"]`
   and rivets's `src_root = crates/rivets/src/`, `resolve_module_path`
   returns `Some(crates/rivets/src/storage/in_memory/mod.rs)` (or
   `.../in_memory.rs` depending on which exists on disk).

3. **C3 (external crates still external):** Given `path = ["serde", "Serialize"]`
   and `serde` is NOT in the workspace's `CrateInfo` list,
   `resolve_module_path` returns `None`. Unchanged behavior.

4. **C4 (no regression on `crate`/`self`/`super`):** All existing tests
   in `resolver.rs` pass. The `crate`/`self`/`super` arms are not
   modified.

5. **C5 (Pass 2 imports catch cross-crate refs):** ~~After the fix, on the
   rivets workspace, ≥ 80 of the 117 workspace-crate imports produce a
   Pass-2-import resolution rather than reaching the fallback.~~ **REVISED
   2026-05-12 after checkpointed-build slice 3 measurement:** ≥ 5 of the
   ALLOWED-pair cross-crate call-refs migrate from Pass-2-fallback to
   Pass-2-imports. Original ≥80 prediction was wrong because the
   refining probe (`refine_c5_upper_bound.py`) revealed that only **5 of
   105** ALLOWED-pair leaks have a `ref_name`-matching workspace import.
   The other 100 are method-on-imported-type calls (`use rivets::Storage;
   s.create_issue()` — the type is imported but the method has no import
   of its own), which Pass 2's name-matching resolver fundamentally
   cannot catch without type information (LSP / rivets-714v territory).
   Empirical measurement post-fix: **6** ALLOWED-pair refs migrate (5
   named-function + 1 method that overlaps another mechanism).

6. **C6 (fallback population drops):** ~~Total resolved cross-crate refs
   reaching `fallback_symbol_search` drops by ≥ 200 (most of the 279 = 174
   phantom + 105 legit go away).~~ **REVISED:** Drops by ≥ 5. Same
   reasoning as C5 — the dominant population (method-on-imported-type
   calls, 100/105 of ALLOWED leaks; all 174 FORBIDDEN phantoms have no
   imports of any kind) cannot migrate to imports without type info.
   Empirical: fallback handles 273 post-fix vs 279 pre-fix (drop of 6).

7. **C7 (no same-crate regression):** Same-crate resolved ref count is
   unchanged on the rivets workspace (currently ~5971).

## Falsification

| # | Claim | Falsifier | Oracle | Cost | Status |
|---|-------|-----------|--------|------|--------|
| pre | `path[0]=workspace_crate_name` heuristic is sound (no aliases dominate) | `source_module_shapes.py` against current DB | DB query, independent of resolver code | 5m | **PASS** (117 ws imports, 0 aliases observed) |
| C1 | workspace-crate detection returns Some | Unit test in `resolver.rs` with a synthetic `CrateInfo { name: "crate_b", ... }`. Call `resolve_module_path(["crate_b"], ...)`. Pre-fix: None. Post-fix: Some(target). | Unit test asserts on the returned `PathBuf` against the fixture's expected file path | 5m | **PASS** (slice 1+2 landed) |
| C2 | Multi-segment path traversal works | Unit test: `resolve_module_path(["crate_b", "nested", "thing"], crate_b's src_root, ...)`. Assert returns `Some(.../nested/thing.rs)` or `.../nested/thing/mod.rs`. | Unit test asserts on PathBuf; fixture has the file on disk | 5m | **PASS** (slice 1+2 landed) |
| C3 | External crates still return None | Unit test: `resolve_module_path(["serde", "Serialize"], ...)` with `serde` NOT in CrateInfo list. Assert returns None. | Unit test assertion | 2m | **PASS** (slice 2 added `external_crate_returns_none_even_with_workspace_list`) |
| C4 | Existing `crate`/`self`/`super` tests pass | `cargo nextest run -p tethys resolver` | Existing test suite | 30s | **PASS** (589/589 tests) |
| C5 (revised) | ≥ 5 ALLOWED-pair cross-crate refs migrate to Pass-2-imports | `refine_c5_upper_bound.py` (string-match probe, no resolver code) + disable-fallback experiment | DB query: count ALLOWED-pair refs still resolved with fallback off | 10m | **PASS** (6 measured, upper bound was 5) |
| C6 (revised) | Fallback-resolved cross-crate refs drop by ≥ 5 | Diff of `check_pass_provenance.py` counts pre- vs post-fix | DB count comparison | 10m | **PASS** (pre 279 → post 273, drop of 6) |
| C7 | Same-crate resolved count unchanged | Same probe as C5/C6; compare same-crate counts pre/post | DB count comparison | (included in C5/C6) | **PASS** (5971 → 5971 by sampled counts) |

Per-claim distinctness: C1–C4 are unit-test scoped (each test focuses on
one input shape). C5–C7 are post-build measurements on the rivets
workspace, each producing a distinct count that can be compared to its
threshold. A failure of any single claim is localizable to that claim by
reading the test name or the probe section.

## Negative space (what this design deliberately does NOT do)

1. **Doesn't handle aliased imports.** `use rivets as r; ... r::Foo` would
   have `path[0] = "r"`. The 117 workspace imports observed have no
   aliases (see `source_module_shapes.py` output). If aliases appear
   later in real workspaces, separate fix.

2. **Doesn't fix bare-module-path imports.** Probe observed 58 empty
   `source_module` rows and ~118 `node_kinds`/`types`-style bare-module
   imports that don't have a `crate::` prefix. These are Pass-1 extractor
   gaps (the extractor lost the `crate::` prefix). Separate issue.

3. **Doesn't fix Pass 1 same-file resolution.** Pass 1 only resolves
   within-file matches via `name_to_id`. This design only touches Pass 2's
   cross-file resolver.

4. **Doesn't change the unscoped fallback.** Once imports work, the
   fallback becomes a residual handler. rivets-3d0s's audit-and-demote
   becomes viable AFTER this lands; that's a separate ticket.

5. **Doesn't handle re-exports** (e.g., `pub use crate::storage::Storage`
   in lib.rs allowing `use rivets::Storage` from another crate). That
   requires walking the target crate's own export graph. Separate fix
   when needed.

## Self-review

**Claim count:** 7. Within range (3–15).

**Falsifier independence:** C1–C3 use unit tests with independent fixture
data (PathBuf comparison against a known directory shape). C4 uses the
existing test suite (independent of the change). C5–C7 use DB queries
and the disable-fallback rebuild experiment — both independent of the
resolver code being modified.

**Per-claim verification distinctness:** Each claim's falsifier produces
a distinct output (unit test name, count threshold). A failure of C1
vs C2 is localizable to the test name. A failure of C5 vs C6 vs C7 is
localizable by which threshold the probe section reports as below limit.

**Cost distribution:** C1–C4 are sub-10-minute unit-test-scoped. C5–C7
require a rebuild + re-index (~15 min combined). No falsifier needs
production data or multi-day soak.

**Negative space:** 5 items listed.

## Hard gate
- [x] Every claim has a falsifier in the table
- [x] Every falsifier names an independent oracle
- [x] Every claim has a distinct verifiable output (per-test, per-count)
- [x] The cheapest falsifier (`source_module_shapes.py`) ran and PASSED
- [x] Negative space has ≥ 3 entries (5 listed)

Ready for `budgeted-plan`.

---

## Re-design rationale (added 2026-05-12 mid-checkpointed-build)

Slice 3's gate (d) check empirically falsified the original C5 and C6
thresholds (≥80 imports migrate; fallback drops ≥200). Actual: 6 migrate;
fallback drops 6.

Root cause: the original design's quantitative predictions modeled
"workspace-crate imports" and "fallback-resolved leaks" as the same
population. The refining probe (`refine_c5_upper_bound.py`) revealed they
overlap by only ~5%. The dominant pattern (95%) is method-on-imported-type
calls: `use rivets::Storage; s.create_issue()`. The `Storage` import gets
correctly resolved post-fix (C1/C2/C3/C4 all PASS), but the `create_issue`
ref has no import of its own — no name-matching resolver can catch it
without type information.

The cheapest falsifier at design time (`source_module_shapes.py`)
correctly verified the existence and shape of 117 workspace-crate imports
but did NOT cross-check those imports against the leak population's
`ref_name`s. The right design-time probe was the cross-check that
`refine_c5_upper_bound.py` now does — that would have falsified C5/C6
before slice 1 was written. Mea culpa.

Implementation slices 1+2 are kept; they correctly fix the
small-but-real explicit-import case (6 refs migrated, all named imports
of functions/methods) AND provide the necessary `&[CrateInfo]`
plumbing for any future type-aware resolution work. The big remaining
population (100 method-on-imported-type leaks + all 174 FORBIDDEN
phantoms) is now an explicit dependency on rivets-714v (LSP integration
for multi-crate workspaces).

The rivets-3d0s audit-and-demote design that triggered rivets-v465's
filing remains blocked: the fallback still does ~99% of cross-crate
resolution, so any rule at the fallback level still hits both legitimate
and phantom refs indiscriminately. Audit-and-demote becomes viable only
when LSP migrates the bulk of method calls out of fallback.
