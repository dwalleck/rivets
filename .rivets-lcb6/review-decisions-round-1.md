# PR #65 review decisions — round 1

Two automated reviewers commented: **claude-review** (1 PR-level comment with 3 sub-findings) and **gemini-code-assist** (2 inline review comments). All five findings triaged below with per-finding verdict and rationale.

## Claude #1 — Drop issue-ID refs from source comments (ACCEPT)

**Where:** `crates/tethys/src/db/file_deps.rs` (clear doc comment); `crates/tethys/src/indexing.rs` (inline comment at the new clear call site).

**Finding:** Both comments referenced `(rivets-lcb6)`. CLAUDE.md says "don't reference the current task/fix/issue #123 in code comments" because those rot once the issue is closed and the audit dir is archival.

**Decision: accept, applied.** Stripped the ID from both comments; kept the substantive WHY (UPSERT mechanism + mirror-of-call_edges rationale).

## Claude #2 — Tighten `file_deps_removed` assertion to `assert_eq!` (ACCEPT)

**Where:** `crates/tethys/tests/file_deps_idempotency.rs:154` (was `assert!(after.row_count < before.row_count, ...)`).

**Finding:** The fixture creates exactly one cross-crate edge; after removing the `use`, the expected delta is exactly `-1`. The looser `<` would still pass if 2 unrelated edges were also removed.

**Decision: accept, applied.** Changed to `assert_eq!(after.row_count, before.row_count - 1, ...)` with a failure message that distinguishes the "stale persists" case (`after == before`) from the "removed too many" case (`after < before - 1`).

## Claude #3 — Non-transactional clear risk (DEFER, FILE)

**Where:** Both `clear_all_call_edges` (indexing.rs:424, pre-existing) and `clear_all_file_deps` (indexing.rs:138, added in this PR).

**Finding:** If the indexer panics between clear and re-population, the affected table is left empty until the next complete run. Pre-existing pattern shared with call_edges, not introduced by this PR — but worth tracking.

**Decision: defer, filed as rivets-ml05** (P4 bug). The fix shape (wrap the relevant indexing section in a transaction) requires design work around the streaming batch_writer's chunked-commit pattern and is out of scope for rivets-lcb6. Adopting the same pattern as call_edges is the strictly-better choice here (no-fix-now would leave file_deps stale, which is the very bug we're fixing).

## Gemini #4 — orphan-file handling in `compute_all_dependencies` (DEFER, FILE)

**Where:** Comment cited `crates/tethys/src/indexing.rs:139`. Function `compute_all_dependencies` exists at line 943 (gemini was right about the name; my initial doubt was wrong).

**Finding:** "`compute_all_dependencies` in streaming mode may re-calculate dependencies for stale entries (files in DB but deleted from disk). Consider eagerly loading all indexed items into a map for comparison against the disk."

**Decision: defer, filed as rivets-dhxo.**

### Round-1 verdict was wrong

I initially rejected this on cascade-cleanup grounds: "`db/files.rs:145-146` cascades deletes when a file is removed from indexing; deleted-from-disk files don't typically linger after a re-index." That reasoning is incorrect.

Pressure-tested by reading the actual code path:

1. The `DELETE FROM symbols / imports` in `db/files.rs:145-146` lives inside `index_file_atomic` and only fires when an EXISTING file is RE-indexed (the `updated > 0` branch). It does NOT fire for files **deleted from disk** because the indexer never processes them.
2. `FileChange::Deleted` IS detected in `reindex.rs:122`, but only by `get_stale_files()` — an observation API that is NOT called from `index_with_options`.
3. Nowhere in `index_with_options` is there any `DELETE FROM files` for orphan entries. So orphan files linger.
4. `compute_all_dependencies` iterates `self.db.list_all_files()` (line 944) which includes those orphans. For each, it loads the stale stored imports + refs and calls `compute_dependencies_from_stored`, which re-inserts `file_deps` rows with the orphan as `from_file_id`.

### Path asymmetry

| Path | Affected? |
|---|---|
| `tethys index --rebuild` (any mode) | No — `db.reset()` wipes the DB; no orphans |
| `tethys index` non-streaming (default) | No — `compute_dependencies` runs per disk-file in the parse loop |
| `tethys index` streaming | **Yes** — `compute_all_dependencies` iterates `list_all_files()` |

### Why still defer (rather than fix in this PR)

Three reasons stand from round-1:

1. **Different bug class from rivets-lcb6** (UPSERT accumulation across runs vs. orphan-file handling in streaming).
2. **Not introduced by this PR.** Pre-existing in streaming mode. `clear_all_file_deps` even partially mitigates by wiping `file_deps` each run, but `compute_all_dependencies` immediately re-inserts from orphans.
3. **Fix needs its own design** — likely add an `cleanup_orphan_files` pass before resolver passes run, using the same staleness check as `reindex.rs::classify_indexed_file`. Filed as **rivets-dhxo** (P3 bug).

### Correction lesson

My round-1 rejection conflated **scope** (legitimate reason to defer) with **validity** (which I asserted incorrectly). The cascade-cleanup claim was a guess from the symbol/import DELETEs without verifying that those DELETEs fire on orphan-file deletion specifically. They don't. Verifying the actual code path took 3 greps and would have caught this in round-1.

## Gemini #5 — Streaming-mode coverage gap (ACCEPT)

**Where:** `crates/tethys/tests/file_deps_idempotency.rs`.

**Finding:** Both new tests used `tethys.index()`, which calls `index_with_options(IndexOptions::default())` (= `use_streaming: false`). Streaming mode has a different `file_deps` population path (post-parse via `compute_all_dependencies` instead of per-file inside the parse loop). The clear runs before the streaming/non-streaming branch, so the fix applies to both — but the test didn't verify that.

**Decision: accept, applied.** Refactored both tests with `#[rstest]` + `#[case::batch(IndexOptions::default)]` / `#[case::streaming(IndexOptions::with_streaming)]`. 4 cases total. Verified all 4 fail pre-fix (reverted the clear, ran tests, both batch and streaming variants reported the `ref_count_sum` drift / stale-edge persistence). Restored the fix; all 4 pass.

The clear's position before the streaming/non-streaming branch is now a meaningful design choice that the test matrix would catch regressing.

## Summary

| Finding | Source | Decision |
|---|---|---|
| 1. Drop issue-ID refs from comments | claude | Accept, applied |
| 2. Tighten `file_deps_removed` assertion | claude | Accept, applied |
| 3. Non-transactional clear | claude | Defer → rivets-ml05 |
| 4. Orphan-file handling in `compute_all_dependencies` | gemini | Defer → rivets-dhxo (corrected from initial reject) |
| 5. Streaming-mode test coverage | gemini | Accept, applied (rstest parameterization, 4 cases) |

**Net diff vs. round-0:** ~30 lines of test additions/changes, 8 lines of comment cleanup, 2 new tracker issues (rivets-ml05 atomicity, rivets-dhxo orphan-file streaming).

## Correction note (post round-1, pre round-2)

Gemini #4 was initially marked "reject" on the basis that `db/files.rs:145-146`'s cascade DELETEs would clean up orphan files. Verification showed those DELETEs only fire on RE-index of an existing file, not on file deletion from disk. Updated verdict above; new tracker rivets-dhxo filed.
