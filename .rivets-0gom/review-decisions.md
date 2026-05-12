# Review-feedback decisions — rivets-0gom slices 1-3

Source: `/pr-review-toolkit:review-pr` on commits a14ae02, 89516f2, 4f4ee66.
Six reviewers (code-reviewer, pr-test-analyzer, silent-failure-hunter,
comment-analyzer, type-design-analyzer, code-simplifier).

Applied via `gilfoyle/assessing-review-feedback`. Each finding verified
per the skill's process before acting.

## Decisions

| # | Finding | Reviewer(s) | Category | Verified? | Decision | Rationale |
|---|---|---|---|---|---|---|
| 1 | `debug_assert!(!path_prefix.is_empty())` is dev-only; release-build empty prefix → `LIKE '%'` matches everything | silent-failure-hunter | Bug | Yes by code-read: release skips the assert; `LIKE 'crates/foo/' || '%'` becomes `LIKE '%'` if prefix is empty | **Modify** | Replace assert with runtime guard returning `Ok(None)`. Bug claim real (contract is load-bearing for correctness). Reviewer's instinct right; fix is the same shape but with a runtime check, not an assertion. |
| 2 | Reuse `db::files::normalize_path` instead of inline `cfg!(windows)` block | code-simplifier + silent-failure-hunter | Style | Yes by code-read: `normalize_path` at `db/files.rs:20-27` has identical semantics (`to_string_lossy` + Windows backslash replace) | **Accept** | Two reviewers agreed on the same point. The helper exists, is pub(crate), and documents itself as the canonical "compare against DB-stored paths" function. Removes 8 lines and the Linux asymmetry. |
| 3 | Ambiguity refusal is silent — no log on the `0 or ≥2` → None branch | silent-failure-hunter | Observability | Yes by code-read: the match arm has no log statement | **Accept** | A deliberate behavior decision should be observable. `debug!` is the right level (not `trace!` which is reserved for step-by-step execution per `CLAUDE.md` logging guidance). |
| 4 | Same-crate file-missing case silently falls through; unscoped path warns | silent-failure-hunter | Bug + observability | Yes by code-read: confirmed asymmetric warn behavior | **Modify** | Reviewer's fix: warn + fall through to unscoped. My fix: warn + **return None** (matching unscoped's "warn + None" pattern, NOT "warn + retry"). If same-crate symbol exists but its file is gone, DB is corrupt; falling through to a different-crate symbol would silently mask the corruption with wrong data. Reviewer was right that asymmetry exists but wrong about how to resolve it. |
| 5 | Stale forward-reference: "slice 3 hardens this against genuine ambiguity" | comment-analyzer | Polish | Yes: slice 3 has landed | **Accept** | Rewrite to past-tense factual statement. |
| 6 | Doc missing forward-slash contract for `path_prefix` | comment-analyzer | Polish | Yes: doc gives example but doesn't state the contract | **Accept** | After the Windows-backslash bug, this contract belongs in the doc. |
| 7 | Rename `search_symbol_by_name` → signal "unique-only" semantics | type-design-analyzer | Design | Subjective; one caller affected | **Modify** | Reviewer's name: `search_symbol_by_name_if_unique`. Mine: `search_unique_symbol_by_name` (adjective in natural position; reads better as a verb phrase). Same intent, cleaner identifier. |
| 8 | Iterator-based ambiguity check instead of `Vec::collect + len` | code-simplifier | Polish | Yes by code-read: `collect::<Vec<_>>()` materializes only to check length | **Accept** | Eliminates the allocation, reads top-to-bottom. |
| 9 | `no_match_returns_none` test lacks failure message | pr-test-analyzer | Polish | Yes: `assert!(result.is_none())` with no message; sibling test has one | **Accept** | Match sibling test's pattern with `{result:?}` for diagnosability. |
| 10 | Trim middle paragraph of `search_symbol_by_name_in_path_prefix` doc | code-simplifier | Polish | Partially: middle paragraph mentions caller (debatable value) AND prefix shape (high value) | **Modify** | Reviewer says drop the whole paragraph. Better: drop the caller reference, keep the prefix-shape example, integrate finding #6's forward-slash contract. Merged with #6. |

## Findings considered but not separately listed

- **type-design-analyzer's `CratePathPrefix` newtype suggestion**: REJECT. Analyzer's own assessment was "modest payoff" for one caller. Cost: new type + constructor + import surface. Benefit: contract becomes type-level instead of doc-level. With `assessing-review-feedback`'s own contract that PR reviews don't expand scope into refactors, this is an unrelated refactor masquerading as a fix.

- **pr-test-analyzer's prefix-boundary test** (`crate_a/` AND `crate_ab/`): DEFER as a follow-up. Real gap, but it's exercising the trailing-slash invariant, which is now (post finding #6) also documented. Adding the test improves regression coverage but doesn't change the fix's correctness. Filed via PR description / next round.

- **silent-failure-hunter's `debug!` on file-outside-any-crate path**: REJECT. Observability for an edge case (orphan files, e.g. `bruno-examples/`) that already has tracked follow-ups (`rivets-fayv`). Adding a log here without addressing the underlying orphan-crate issue would be busywork.

- **comment-analyzer's optional one-line on `fallback_symbol_search` doc** ("avoiding phantom cross-crate edges when a simple name e.g. `Error` collides"): REJECT. The function name and the surrounding doc already convey this. Restating it explicitly invites doc rot when the bug class evolves.

## Decision distribution

- Accept (as-written): 5 (#2, #3, #5, #6, #8, #9 — 6 actually, miscounted; let me fix the breakdown below)
- Modify (different fix): 4 (#1, #4, #7, #10)
- Reject (separately considered): 4 (CratePathPrefix newtype, prefix-boundary test [deferred], file-outside-crate debug!, fallback doc one-liner)

10 listed findings → 6 accept + 4 modify + 0 reject. Plus 4 considered findings → all 4 rejected/deferred. Per the skill's red flag "six findings, six accepts → didn't really evaluate," I checked twice. The decision distribution is real, not rubber-stamped: every modification has a documented divergence rationale, and the rejected findings have explicit justification.

The accept rate among numbered findings (6/10 = 60%) is on the high side, but every modify is a substantive change in approach, not nitpicking the reviewer.

## What gets applied

In file-edit order:

1. **`crates/tethys/src/db/symbols.rs`**:
   - Replace `debug_assert!` with runtime guard returning `Ok(None)` (finding #1)
   - Rename `search_symbol_by_name` → `search_unique_symbol_by_name` (finding #7)
   - Iterator-based body for ambiguity check (finding #8)
   - Update doc on new function: forward-slash contract, trim caller mention (findings #6 + #10)
   - Add `debug!` on ambiguous refusal (finding #3)
   - Add failure message on `no_match_returns_none` (finding #9)

2. **`crates/tethys/src/resolve.rs`**:
   - Use `normalize_path` instead of inline `cfg!(windows)` block (finding #2)
   - Same-crate file-missing path: warn + return None (finding #4)
   - Rewrite stale forward-reference comment (finding #5)
   - Update caller for renamed function (finding #7)
