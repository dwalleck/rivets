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

## Gemini #4 — N+1 in `compute_all_dependencies` for deleted-from-disk files (REJECT)

**Where:** Comment cited `crates/tethys/src/indexing.rs:139`. Function `compute_all_dependencies` does exist at line 943 (I initially doubted this; gemini was right).

**Finding:** "`compute_all_dependencies` in streaming mode may re-calculate dependencies for stale entries (files in DB but deleted from disk). Consider eagerly loading all indexed items into a map for comparison against the disk."

**Decision: reject.** Three reasons:

1. **Not the bug class this PR fixes.** rivets-lcb6 is about `file_deps` accumulating stale rows across runs via UPSERT. The clear-then-repopulate cycle now removes all stale rows regardless of whether the file still exists on disk. If a file was deleted from disk and lingers as an orphan in the DB, that's a separate cleanup concern.
2. **The scenario described is uncommon.** `db/files.rs` cascades deletes when a file is removed from indexing (lines 145-146 in this file remove symbols and imports for the file_id). Deleted-from-disk files don't typically linger in DB after a re-index.
3. **N+1 performance is a separate optimization.** `compute_all_dependencies` does per-file queries by design; whether that's the right architecture is a coupling-graph perf question, not part of the resolver-correctness epic.

If gemini's underlying concern about orphan files is real, it should be filed as its own issue — but it doesn't block rivets-lcb6.

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
| 4. N+1 in `compute_all_dependencies` for stale files | gemini | Reject (not in scope) |
| 5. Streaming-mode test coverage | gemini | Accept, applied (rstest parameterization, 4 cases) |

**Net diff vs. round-0:** ~30 lines of test additions/changes, 8 lines of comment cleanup, 1 new tracker issue (rivets-ml05).
