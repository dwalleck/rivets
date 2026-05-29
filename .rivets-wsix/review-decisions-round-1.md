# PR #75 review-feedback decisions — round 1

## Reviewers
- **claude-review** (GitHub Action bot, run 26074529893)
- **gemini-code-assist** (GitHub bot)

## Per-finding table

| # | Finding (one line) | Reviewer | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|---|
| 1 | `refs_cascade_on_call_removal` doesn't explicitly assert `c_refs == 1` (only `a_refs == 1` and `b_refs == 0`) — surviving-ref coverage is asymmetric | claude-review | Polish | Yes (read test at `reindex_cascade.rs:101-117`; `c_refs` derivable from `refs_post − a_refs − b_refs = 2 − 1 − 0 = 1` but not asserted) | **Accept** | 4-line addition; matches the existing `a_refs`/`b_refs` pattern. Strengthens against a future-mutation bug class where someone adds a name to the IN-clause and breaks the arithmetic. |
| 2 | `count_lib_refs_by_target_names` joins on `r.symbol_id` (callee), but the cascade fires on `r.in_symbol_id` (container) — indirect oracle | claude-review | Design (informational) | Yes (read SQL at `reindex_cascade.rs:17-29`; reviewer themselves notes this is fine in practice because the fixture resolves cleanly) | **Reject** | The reviewer explicitly classified this as a future-maintainer note, not actionable. The indirection is *the point* — the test verifies the *effect* of the cascade (refs disappear), not the trigger mechanism, which is the right level for a regression fence. Changing to `r.in_symbol_id` would actually weaken the fence by testing the trigger instead of the consequence. |
| 3 | `plan.md` mentions `filetime` / `std::thread::sleep` but the impl uses content-hash change detection (no `filetime` dep) | claude-review | Polish (doc drift) | Yes (`.rivets-wsix/plan.md` does reference filetime; implementation in `reindex_cascade.rs` writes new content, no filetime usage) | **Reject (intentional)** | Per CLAUDE.md "Issue diagnostic directories" convention: "Point-in-time, not maintained. Probe scripts query whatever schema/state was current when the fix was developed; they will go stale and that's expected." The plan documents the design *as drafted*; the implementation legitimately took a simpler path during build, which is exactly what `checkpointed-build` permits ("the implementer is permitted to deviate if the deviation keeps the slice within budget and the oracle passing"). Leaving the plan as-is preserves the historical record of "we considered filetime, picked content-hash instead." |
| 4 | (no findings) | gemini-code-assist | n/a | n/a | n/a | "I have no feedback to provide as there were no review comments." |

## Statistics

- Findings: 3 actionable + 1 explicit no-comment = 4 total
- Accept: 1
- Modify: 0
- Reject: 2 (with rationale, no deferral — both are conscious design choices, not "do later")

Per the skill's red-flag check on "six findings, six accepts": healthy reject rate
(2/3 of actionable findings rejected with explicit rationale). Indicates per-finding
verification actually happened.

## Outcome

- Applied: claim 1 (`c_refs == 1` assertion).
- Documentation: this file.
- No new tracker issues filed — no deferred work in this round.
