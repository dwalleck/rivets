# Review-feedback decisions — round 2

Sources received between rounds 1 and 2:
- **Claude code review #1** (against HEAD `ac917e6`, before round-1): 3 findings.
- **Gemini review** (against HEAD `e559eee`, before round-1): 1 inline comment at
  `resolve.rs:334` proposing a Vec<&str>-based rewrite.
- **Claude code review #2** (against HEAD `cc6dd0c`, **after** round-1, reviewing the
  round-1 commit): "Approve with minor nits."

Two of the three pre-round-1 finding sets were already addressed in round-1 (see
`review-decisions-round-1.md`). This round assesses (a) Gemini's inline finding
and (b) Claude #2's three new observations against the round-1 state.

## Findings

| # | Finding (one line) | Reviewer | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|---|
| R2-1 | Vacuous disjunctive assertion in `workspace_crate_prefixed_call_resolves` (round-1 introduced `assert!(unresolved_refs_in_test == 0 \|\| resolved_to_target >= 1)` immediately before `assert!(resolved_to_target >= 1)`). The disjunction is trivially true whenever the second assertion would pass, and is implied-false whenever the second would fail. Cannot catch a bug the second misses. | claude-review #2 | Bug | YES — structural reduction: the disjunction `(A == 0) ∨ (B >= 1)` cannot distinguish "ref resolved correctly" from "ref phantom-resolved to a different symbol." | **Accept** | Replace the disjunctive + the now-unused `unresolved_refs_in_test` query with a dedicated **no-phantoms** query joining `f_target` and asserting `s.name IN ('make_widget_044i','Widget')` only ever binds inside `crate_a/src/lib.rs`. Same anti-pattern as the I1 vacuous-filter fix, just self-inflicted in round-1. |
| R2-2 | `tail = segments[split..].join("::")` allocates even on iterations that fall through to `continue` via the `let Some(file_id) = file_id else { continue }` gate. | claude-review #2 | Polish | YES — `tail` is computed unconditionally at the top of each iteration. | **Reject** | Reviewer explicitly noted "Sub-microsecond in practice." `segments.len()` is bounded by `::`-segment count of a single ref (≤ ~6 in real code). Cost is negligible and deferring would obscure the loop's flow with a delayed-allocation pattern. |
| R2-3 | Depth-1 `super::` test missing. Current `self_and_super_paths_resolve_via_as_written` exercises depth-2 (`src/parent/child.rs`); a depth-1 case (`src/child.rs`) would explicitly lock in tethys's filesystem-walk `super` semantics. | claude-review #2 | Coverage | YES — only depth-2 is exercised. | **Reject** | The fence the new test defends is `qualified_module_fallback`'s `matches!(prefix[0], "crate" \| "self" \| "super")` gate — depth-2 fully exercises that gate. Depth-1 would primarily test `resolver::resolve_super_path`'s filesystem-walk semantics, which is preexisting (and arguably-wrong vs Rust) behavior outside this PR's scope. No tracker entry: not deferred, just out of scope. |
| R2-4 | Gemini's suggested rewrite at `resolve.rs:334`: precompute `path_segments: Vec<&str>` with `std::iter::once("crate").chain(segments.iter().copied()).collect()` and slice it for A and B interpretations. | gemini-code-assist | Design | YES — verified `resolve_module_path` signature at `resolver.rs:31-36` takes `path: &[String]`, not `&[&str]`. Gemini's proposal would not compile. | **Reject** | Compile-breaking proposal. The wider concern (per-iteration allocation) is also rejected per round-1 S7 — three similar lines beats premature abstraction, the allocations are bounded by segment count, and `resolve_module_path`'s `&[String]` API is the actual driver of the cost. |
| R2-5 | Gemini's framing claims single-segment paths "may incorrectly handle submodule shadowing" and "should resolve to the crate entry point file (e.g., lib.rs) rather than the source directory." | gemini-code-assist | Bug | PARTIAL — `resolver::resolve_crate_path` does return `crate_root` (a directory) for an empty tail (`path.is_empty()`), not the entry-point file. BUT: `qualified_module_fallback`'s loop only invokes `resolve_module_path` with `prefix.len() >= 1`. When `prefix = ["crate"]`, the call returns `Some(crate_root)`; `get_file_id` then returns `None` for the directory path; the loop silently falls through. Not exploitable through this code path. Submodule shadowing is also already fenced by the existing `submodule_shadows_workspace_crate` test. | **Reject** | Out of scope. Pre-existing resolver behavior, not introduced by this PR, and not reachable as a phantom-resolution vector through `qualified_module_fallback`. The "submodule shadowing" framing is generic — Gemini cites no concrete fixture and the existing test pins the relevant invariant. |

## Summary

- **Accept:** R2-1 (1 finding).
- **Reject:** R2-2, R2-3, R2-4, R2-5 (4 findings).

5 findings reviewed, 1/4 accept/reject. The single accepted item is the meaningful one — a self-inflicted regression of the same anti-pattern I corrected for I1 in round-1.

## Tracker discipline check

No deferred work. Every Reject above is "concern not real, not blocking, or fix worse than the disease," not "we'll do this later." No new rivets issues filed.

## Application

Single edit to `crates/tethys/tests/pass2_qualified_paths.rs` in `workspace_crate_prefixed_call_resolves`:

- Remove the `unresolved_refs_in_test` query (no longer used).
- Remove the vacuous disjunctive `assert!`.
- Add a dedicated phantom-detection query: count refs in `crate_b/tests/it.rs` resolved to a symbol named `make_widget_044i` or `Widget` whose target file is NOT `crate_a/src/lib.rs`. Assert == 0.
- Keep the positive assertion `resolved_to_target >= 1` unchanged.
