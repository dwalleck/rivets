# PR #75 review-feedback decisions — round 2

## Reviewers

`/pr-review-toolkit:review-pr` multi-agent pass (4 agents, all reviewers):

- **pr-test-analyzer** — test non-vacuity / coverage
- **code-reviewer** — general quality / CLAUDE.md compliance
- **comment-analyzer** — comment accuracy vs production code
- **type-design-analyzer** — `ClearAllSnapshot` struct

(silent-failure-hunter and code-simplifier were assessed N/A and not dispatched: the
file has no error-handling/fallback logic — all fallible calls are `.expect()`, correct
by test convention — and all four reviewers independently judged the code clean, leaving
nothing to simplify.)

## Verification methodology

Every actionable finding was checked empirically against the real indexer, not accepted
on the reviewer's word. Built `tethys --release`, indexed the exact C3 fixture
(`mod helper;` + `helper::do_thing()`, no `use`), and ran TDD-style inversions by
mutating production code, rebuilding, and re-indexing. All probes use a throwaway temp
workspace; production code was restored and `git diff crates/tethys/src/` confirmed empty
after each.

Key empirical results:

| Probe | Result |
|---|---|
| Index C3 fixture, inspect counts | `imports=0`, `call_edges` count=1 / `SUM(call_count)=1`, `file_deps` count=1 / `SUM(ref_count)=1` |
| Disable `clear_all_file_deps`, re-index 3× | row count holds at **1**; `SUM(ref_count)` grows **1 → 2 → 3** |
| Disable `clear_all_call_edges`, re-index 3× | `call_edges` count **and** `SUM(call_count)` both hold at **1** |
| Schema FK inspection | `file_deps` → `files(id)` (files row UPDATEd, not deleted on re-index ⇒ stable); `call_edges` → `symbols(id) ON DELETE CASCADE` (symbols deleted+recreated ⇒ cascade clears); `attributes` → `symbols(id) ON DELETE CASCADE` (per-symbol) |

## Per-finding table

| # | Finding (one line) | Reviewer | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|---|
| 1 | C3 comment (`:263-273`) cites the wrong UPSERT clause — `insert_file_dependency`'s `ref_count = ref_count + 1` — for a fixture whose `file_deps` row actually comes from `populate_file_deps_from_call_edges` | comment-analyzer | Bug (comment accuracy) | **Yes** — probed: `imports=0`, so `insert_file_dependency` is never called; the row is produced by the call-edge aggregation path | **Modify** | Reviewer's bug claim correct; fix refined. Rewrote the comment to cite the actual path + clause, explain the stable-`file_id` vs volatile-`symbol_id` asymmetry, and stop overclaiming `call_edges` fencing. Doc-only, no behavior change. |
| 2 | Add `SUM(call_count)` assertion to `ClearAllSnapshot` to mirror the `file_deps` SUM fence (symmetric UPSERT-growth shape) | pr-test-analyzer | Design (coverage) | **Yes — REFUTED** | **Reject** | Empirically vacuous. Disabling `clear_all_call_edges` does **not** grow `call_count` (stays at 1 across 3 re-indexes): `call_edges` keys on `symbol_id`s that the per-file `DELETE FROM symbols` cascade-clears every re-index, so the clear is defense-in-depth, not the active mechanism. A `SUM(call_count)` assertion would pass even with the discipline removed — a fence that can't fail. The reviewer's symmetry argument missed that `call_edges` (symbol-keyed) and `file_deps` (file-keyed) behave differently under re-index. No deferred work ⇒ no tracker. |
| 3 | Test 2's `keep` guard is identity-blind (both symbols carry identical `#[allow(dead_code)]`); give `keep` a distinct attribute | pr-test-analyzer | Design (coverage) | **Yes** — schema: `attributes.symbol_id → symbols(id) ON DELETE CASCADE` is per-symbol | **Reject** | The defended bug class ("cascade too aggressive, deletes a sibling's attributes") is structurally near-unreachable: the FK cascades per individual `symbol_id` and cannot reach another symbol's rows. Also, full-file re-index deletes+re-inserts `keep` regardless, so the surviving row is a fresh insert, not a cascade-spared one — the distinct-attribute change adds no real discriminating power. Existing assertion kept (cheap, harmless). No tracker (no real deferred work). |
| 4 | `count_lib_refs_by_target_names` empty-slice → `IN ()` invalid SQL; add `debug_assert!(!names.is_empty())` | code-reviewer | Polish (robustness) | Yes — all 4 call sites pass non-empty literals | **Reject** | Reviewer themselves flagged it "not worth changing for a test-only helper with controlled inputs." Adding defensive guards to test infrastructure for a hypothetical future misuse is over-engineering. Inputs are non-empty by construction. |
| 5 | Derive `Debug` on `ClearAllSnapshot` for future `dbg!` convenience | type-design-analyzer | Polish | Yes — nothing currently consumes a `Debug` impl | **Reject** | Analyzer rated it "take-it-or-leave-it." YAGNI — nothing uses it, and an unused derive is noise the project's pedantic-clippy posture discourages. Per-field `assert_eq!` (which the analyzer endorsed keeping over derived `PartialEq`) already gives targeted failure messages. |

## Byproduct observation (documented, not a finding)

Verifying #2 surfaced that the **existing** `call_edges` row-count assertion
(`reindex_cascade.rs:318-321`) cannot actually fence `clear_all_call_edges` removal —
the `symbols(id) ON DELETE CASCADE` chain clears `call_edges` on every re-index
regardless of whether the explicit clear runs. No single-workspace re-index fixture can
make `clear_all_call_edges` observable (any re-processed file's symbols are deleted; an
un-reprocessed file's edges aren't touched anyway). The clear is therefore
defense-in-depth for non-re-index paths (e.g. `--rebuild`, where `db.reset()` also wipes
it). This is not a defect and not fixable by this test's approach, so it is recorded here
rather than filed. The corrected C3 comment now states this honestly ("weaker companion
check"). Adjacent `file_deps`/rebuild test-coverage expansion already lives at
**rivets-zoi3** (open); `rivets-wsix` (this PR's issue) scoped the UPSERT-only audit.

## Statistics

- Findings: 5 actionable (1 Critical-equiv, 2 Important, 2 Suggestion)
- **Modify: 1** (finding #1 — applied)
- **Reject: 4** (#2 empirically refuted; #3 structurally near-unreachable; #4/#5 polish-not-worth-it)
- No deferred work this round ⇒ no new tracker issues filed.

The high reject rate is empirically grounded, not contrarian: #2 is the headline catch —
a plausible, confidently-argued reviewer finding ("add the symmetric SUM check") that the
inversion **refuted** by showing the proposed fence cannot fail. Applying it would have
added false-confidence test coverage. This is exactly the failure mode
`assessing-review-feedback` exists to prevent.

## Outcome

- **Applied:** finding #1 — corrected C3 doc comment (`reindex_cascade.rs:258-279`).
- **Verification re-run after the edit:** `cargo nextest run -p tethys --test reindex_cascade` → 3/3 pass; `cargo clippy -p tethys --tests --all-features -- -D warnings` → clean; `cargo fmt --check` → clean.
- **Documentation:** this file.
