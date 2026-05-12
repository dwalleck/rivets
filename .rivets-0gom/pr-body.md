## Summary

Fixes `rivets-0gom` (P2): tethys's resolver was creating phantom cross-crate `file_deps` edges on workspaces with shared filenames. Discovered by running `tethys coupling` against the rivets repo itself and comparing to an independent oracle (`grep` + `Cargo.toml`) — **149 of 170 cross-crate edges (88%) were phantoms**.

Three-slice fix:

- **Slice 1** — added `Index::search_symbol_by_name_in_path_prefix(name, prefix)`. Scopes lookups to files under a given path prefix.
- **Slice 2** — routed `fallback_symbol_search` through it. The caller's containing crate is derived from `current_file_path`; same-crate match wins; falls back to unscoped only when no same-crate match exists.
- **Slice 3** — renamed `search_symbol_by_name` → `search_unique_symbol_by_name` and changed semantics to return `None` on workspace-wide ambiguity instead of an arbitrary first-match.

Followed by a review-feedback pass (`/pr-review-toolkit:review-pr` × 6 reviewers) verified per-finding before applying: 6 accepted, 4 modified (reviewer's instinct right but fix sub-optimal), 4 separately rejected. Decision log committed at `.rivets-0gom/review-decisions.md`.

## Results on the rivets workspace

| Metric | Before | After | Change |
|---|---|---|---|
| Cross-crate `file_deps` edges (total) | 170 | 73 | −57% |
| Phantom edges (vs grep+Cargo oracle) | 149 | 52 | **−65%** |
| Ambiguity violations (probe Section 3) | 83 | **0** | **−100%** |
| Intra-crate edges, tethys | 220 | 272 | +52 (phantoms converted to correct same-crate refs) |
| Intra-crate edges, rivets | 75 | 82 | +7 |
| Intra-crate edges, rivets-mcp | 11 | 12 | +1 |
| Intra-crate edges, rivets-jsonl | 9 | 13 | +4 |
| Indexing wall-clock (hyperfine, 3 runs) | 52.86s ± 3.59s | ≤ 26s | Well under 58s budget |

Both legitimate cross-crate pairs preserved:
- `rivets → rivets-jsonl`: 21 → 15 edges (the 6 lost were refs to names that exist in BOTH crates; the resolver now correctly picks rivets's same-crate copy)
- `rivets-mcp → rivets`: 15 → 7 edges (same shape: 8 lost were shadowed-name phantoms misclassified as legitimate)

The remaining 11/7 edges target legitimate domain symbols (`create_issue`, `IssueFilter`, `IssueStorage`, …) — verified via `.rivets-0gom/diagnose_drop.py`.

## Scope — honest

**This PR does NOT fully eliminate phantom edges.** 52 residual phantoms remain on the rivets workspace. They are NOT same-crate-fallback failures or ambiguity-fallback failures — they're a *different* bug class:

Tethys's parser records stdlib and external-crate names as workspace-internal symbols when those names appear in impl items, derive macros, or method signatures. Common offenders:
- `Serialize`, `Deserialize` (serde traits)
- `len`, `write`, `display`, `drop`, `flush` (stdlib methods)
- `Tree`, `Parser`, `children` (tree-sitter / external types)

When a cross-crate reference like `<F as Serialize>::serialize(...)` is parsed, the resolver finds a unique workspace match — the wrong one. Filed as `rivets-3d0s` (P2). Drilldown evidence at `.rivets-0gom/diagnose_residual.py`.

## Follow-ups filed during this work

| Issue | Type | Priority | Why |
|---|---|---|---|
| `rivets-lcb6` | bug | P2 | `file_deps` table never cleared between `tethys index` runs. UPSERT-only schema means phantom edges from previous runs persist; required `rm .rivets/index/tethys.db` between slice 2 verification runs. |
| `rivets-ck11` | task | P3 | Slice 1 unit tests passed despite a Windows backslash bug surfaced in slice 2's integration gate. The fixture used forward-slash paths, so platform-divergent prefix shape was never exercised. |
| `rivets-3d0s` | bug | P2 | Tethys's parser records stdlib trait/method names as workspace symbols (probably from impl items). Source of the 52 residual phantoms after this PR. |

## How this was built — gilfoyle skill suite

This PR is also the worked example of a new skill suite (`gilfoyle`, currently local). Every step has artifacts:

- **prove-it-prototype** — `.rivets-0gom/probe.py` + `oracle.sh`. Independent probe (Python+sqlite3 reading `file_deps`) and oracle (`grep` + Cargo.toml). Probe and oracle disagreed by 88% pre-fix; that disagreement IS the bug specification.
- **falsifiable-design** — `.rivets-0gom/design.md`. Six claims, six paired falsifiers, cheapest falsifier run before approval (`.rivets-0gom/cheapest_falsifier.py` confirmed `Error` has 4 workspace candidates, `Result` has 5, `LIMIT 1` was arbitrary).
- **budgeted-plan** — `.rivets-0gom/plan.md`. Three slices with mandatory complexity budgets, scale budgets, adversarial stress fixtures, per-claim verification matrix, hyperfine wall-clock baseline (`.rivets-0gom/index-baseline.md`).
- **checkpointed-build** — Per-slice oracle recheck after each slice. Drift halted slice 2 mid-run (Windows backslash bug). Plan claim 4 revised mid-stream when the integration gate revealed it was over-strict.
- **assessing-review-feedback** — `.rivets-0gom/review-decisions.md`. Each of 10 review findings verified independently before applying; accept/modify/reject decision with documented rationale per finding.

Probe snapshots at every gate: `.rivets-0gom/baseline-pre-fix.txt`, `after-slice2-fixed.txt`, `after-slice3.txt`, `after-review-fixes.txt`. Each preserves a quantitative checkpoint of the fix's progression.

## Test plan

- [x] `cargo nextest run -p tethys` — 582 passing (was 576 pre-fix; +6 new unit tests across slices 1 + 3, no regression)
- [x] `cargo clippy --all-targets --all-features -p tethys -- -D warnings` — clean
- [x] `cargo fmt --check` — clean
- [x] Probe vs oracle: claims 4, 5, 6 fully verified; claim 3 partial with the residual fully attributed to `rivets-3d0s`
- [x] Indexing wall-clock: 25.94s, within 58s budget (mean + 5s margin from baseline)
- [x] PR 60's coupling-metric feature continues to render correctly; the metrics it computes are now based on substantially less polluted `file_deps`
- [x] Dogfooded via `target/release/tethys.exe callers Index::search_unique_symbol_by_name` — confirmed exactly one caller (`Tethys::fallback_symbol_search`) before merging; no surprises

## Reviewer note

If you re-run `/pr-review-toolkit:review-pr` against this branch, please reference `.rivets-0gom/review-decisions.md` for findings already considered. New findings welcome — specify which existing decisions (if any) they supersede or refine.

🤖 Built using the gilfoyle skill suite. Hail Satan.
