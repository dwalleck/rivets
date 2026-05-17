# PR #67 review-feedback decisions

**Date:** 2026-05-17 (post-K-hybrid filter implementation)
**Reviewer:** multi-agent automated review (pr-code-reviewer,
pr-comment-analyzer, pr-silent-failure-hunter, pr-test-analyzer,
pr-type-design-analyzer)
**Methodology:** `gilfoyle/assessing-review-feedback` — every finding
verified before decision; tracker discipline enforced on deferrals.

## Headline

**17 findings → 7 applied, 10 rejected.** ~58% reject rate, driven
primarily by the code-reviewer agent's branch-context confusion:
three of its four "Critical" findings (with 95-98% confidence) reference
commit `e2770d2` (which adds an `AND kind = 'call'` filter to
`populate_call_edges` plus three regression tests plus
`clear_and_rebuild_call_edges`). That commit lives on
`feature/tethys-overview`, never on `main`. The bot reported these as
"removed in this PR" — they were never present to remove.

## Per-finding decisions

| # | Finding (one line) | Agent | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|---|
| 1 | `kind='call'` SQL filter removed from `populate_call_edges` | code-reviewer | Bug | Yes (verified false) | **REJECT** | `e2770d2` exists only on `feature/tethys-overview`. Not on `main`. My PR doesn't touch `populate_call_edges`. |
| 2 | Three regression tests deleted | code-reviewer | Bug | Yes (verified false) | **REJECT** | Same root cause as #1 — tests live in `e2770d2` on the parallel branch. Never on main. |
| 3 | `clear_and_rebuild_call_edges` public API removed | code-reviewer | Bug | Yes (verified false) | **REJECT** | Same — function exists only on `feature/tethys-overview`. `git show origin/main` confirms absence. |
| 4a | `_corroborated` suffix used in references | comment-analyzer | Bug | Yes (partial) | **MODIFY** | Only in `plan-v3-k-hybrid.md` (advisory pre-implementation). Updated plan to use actual function name. |
| 4b | "Test fixture has the import" | comment-analyzer | Bug | Yes (verified false) | **REJECT** | Fixture has `use crate::imports_module::imported_fn` (intra-crate). No cross-crate `use`. Docstring claim is accurate. |
| 4c | "Eliminates phantom edges" wording ambiguity | comment-analyzer | Style | Yes | **MODIFY** | Reworded `indexing.rs` comment to "Filters phantom edges out at the file_deps aggregation step" with explicit note about resolver behavior unchanged. |
| 5 | C# imports use `.` not `::` separator | silent-failure-hunter | Bug | Yes (partial) | **MODIFY** | Real for future `.csproj` discovery; behaviorally inert today (tethys treats all C# files as one pseudo-crate per top-level directory). Added `first_path_segment` helper handling both `::` and `.`. Cheap future-proofing. |
| 6 | Missing-entry fallback branch untested | test-analyzer | Test gap | Yes | **ACCEPT** | Added `k_hybrid_keeps_edge_conservatively_when_file_missing_from_crate_map` unit test. |
| 7 | Silent edge dropping — no counter/log | silent-failure-hunter | Observability | Yes | **MODIFY** | Added `dropped: usize` counter; included in summary `trace!`. Per-edge tracing would be too noisy at workspace scale. |
| 8 | `warn!` uses raw IDs, doesn't distinguish missing side | silent-failure-hunter | Observability | Yes | **MODIFY** | Added `from_crate_missing` and `to_crate_missing` booleans to the warn fields. Cheap; no DB roundtrip needed. |
| 9 | `_` → `-` normalization one-directional | test-analyzer | Bug | Yes (verified false) | **REJECT** | Reverse case (`use foo-bar`) is invalid Rust syntax. Current one-direction normalization handles every legal case. |
| 10 | Integration test uses `!is_empty()` not exact match | test-analyzer | Test quality | Yes | **ACCEPT** | Tightened to `legitimate_edges.contains(&expected)` with explicit expected tuple. Catches partial regressions. |
| 11 | Introduce `CrateName` enum + `FileCrateMap` newtype | type-design-analyzer | Design | n/a | **REJECT** | CLAUDE.md: "Don't add abstractions beyond what the task requires." Stringly-typed `HashMap<FileId, String>` is honest and sufficient at current scale. Worth considering if the K-hybrid filter spreads to more code paths. |
| 12 | Extract `should_keep_edge()` pure predicate | type-design-analyzer | Design | n/a | **REJECT** | The match logic is 7 lines, used once. Three existing unit tests already cover it via the public function. Extraction would be for testability we already have. |
| 13 | Add `return_type: None` to test `InsertSymbolParams` | silent-failure-hunter | Polish | Yes (verified false) | **REJECT** | `InsertSymbolParams` has no `return_type` field. Bot hallucination. |
| 14 | Filter `orphan:*` entries from `known_crates` | silent-failure-hunter | Polish | Yes | **REJECT** | `orphan:*` strings cannot appear as Rust import first-segments by construction. Lookups for them would just not match. Memory savings negligible; premature optimization. |
| 15 | Replace `C1`/`C2`/`C3` claim refs with descriptive labels | comment-analyzer | Polish | n/a | **REJECT** | The numeric refs are deliberate audit trail from `design-v3-k-hybrid.md` claims to code. Removing weakens traceability — these are not "volatile design-doc references", they're stable identifiers in a committed design doc. |
| 16 | Remove "redundant" comments | comment-analyzer | Polish | Yes | **REJECT** | The flagged comments document the Mutex re-entrancy avoidance pattern (`// Acquires and releases the connection guard internally.` and `// Re-acquire the connection for the inserts.`). Load-bearing rationale — removing them leaves the next maintainer vulnerable to re-introducing the deadlock that was caught and fixed during this slice's implementation. |
| 17 | Empty imports table test | test-analyzer | Test gap | Yes | **ACCEPT** | Added `k_hybrid_empty_imports_table_drops_all_cross_crate_keeps_intra` for the pure-data-crate edge case. |

## Categorical breakdown

| outcome | count | findings |
|---|---|---|
| ACCEPT | 3 | #6, #10, #17 |
| MODIFY | 5 | #4a, #4c, #5, #7, #8 |
| REJECT | 9 | #1, #2, #3, #4b, #9, #11, #12, #13, #14, #15, #16 (counted as 11 above; #11/#12 share a "design out-of-scope" rationale) |

Of the 11 rejects:
- 4 are verified-false bug claims (#1, #2, #3, #4b, #9, #13 — 6 actually)
- 2 are design-out-of-scope (#11, #12)
- 3 are premature/non-load-bearing polish (#14, #15, #16)

## Why no deferral tracker IDs

The skill's tracker discipline requires every "Reject (defer)" or
"Modify (deferred work)" to cite a verified tracker ID. None of this
log's rejections are deferrals — they are either:

- **Verified-false claims** (#1, #2, #3, #4b, #9, #13): not "we'll do
  this later", but "this isn't a real bug." No follow-up work needed.
- **Design alternatives declined** (#11, #12): not deferred — actively
  decided against. The current implementation is judged sufficient at
  current scale. If future scale demands abstractions, that's a new
  decision at that time, not a deferred-from-now decision.
- **Polish declined** (#14, #15, #16): same as above — not "later",
  but "not at all unless conditions change."

If any of these later prove worth doing, they get filed as new issues
at that time with fresh rationale, not as "carried over from PR #67
review."

## What the review caught that's real

Even with the high false-positive rate, the review surfaced 7 real
improvements: a C# future-proofing gap (#5), two test coverage gaps
(#6, #17), two observability improvements (#7, #8), one wording
ambiguity (#4c), and one test-assertion tightening (#10). Plus one
documentation hygiene fix in the plan doc (#4a).

The methodology paid off: applying all 17 findings without verification
would have introduced unrelated changes to `populate_call_edges` (per
#1), undone the deliberate audit trail (#15), reintroduced the
deadlock-prevention comments as "redundant" (#16), and added at least
one abstraction (#11 or #12) the project guidelines explicitly
discourage.

## Implementation

Applied in commit `7d3504c` (refactor) + this commit (plan doc rename
+ decision log).
