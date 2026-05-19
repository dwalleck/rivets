# Review-feedback decisions — round 3

Sources received after round-2 push:
- **Claude code review #3** (00:38:00Z, against `cc6dd0c` round-1 state).
- **Claude code review #4** (00:45:54Z, against `d07867f` round-1+fmt state).

Both reviewed the round-1 state, not round-2 — so they re-flagged round-2's
already-fixed R2-1 disjunctive (verified that fix is in `80167b3`, already
applied), the round-2 already-rejected `(*s).to_string()` and `tail` allocation
items, and surfaced two new observations that warrant action.

## Findings

| # | Finding (one line) | Reviewer | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|---|
| R3-1 | `.rivets-044i/verification.md:66` reports `(5 tests, all pass — claims 1, 2, 3, 4, 5)` but the file as-committed in round-1 contains 8 tests. Audit trail is stale relative to current state. | claude #3 | Doc drift | YES — verified by reading `verification.md` and counting tests in `pass2_qualified_paths.rs`. | **Accept** | Per the diagnostic-directory convention these are point-in-time, but the issue hasn't closed and the document is currently misleading reviewers. Update to reflect 8-test reality with a note that the additional 3 were added by the round-1 review-decisions commit. |
| R3-2 | The `super::` filesystem-walk-vs-Rust-spec divergence is documented in the new `self_and_super_paths_resolve_via_as_written` test's docstring but has no tracker entry. | claude #4 | Tracker discipline | YES — `rivets list` returns 52 open issues; grepped for `(resolve_super\|super_path\|super::.*semantic\|filesystem.*walk)` → zero matches. | **Accept** | Per `feedback_tracker_entries_for_deferrals`: documented-only deferrals rot. The divergence is preexisting tethys behavior, not introduced by PR #74, but PR #74 is the first place it's *explicitly documented in code*. File a new rivets issue and reference it from the test docstring. |
| R3-3 | `(*s).to_string()` is unidiomatic — auto-deref via `s.to_string()` is the more conventional form. | claude #3, #4 | Style | Repeated finding (round-1 S6, now also round-3). YES — confirmed clippy doesn't flag it. | **Reject (carryover)** | Two reviewers flagging the same thing is in the skill's red-flags list: "Two reviewers can share the same blind spot. Verify the underlying claim, not the count." The underlying claim is "clippy flags it" — verified false in round-1. Subjective style preferences don't override the linter's machine verdict. Standing by the rejection. |
| R3-4 | Disjunctive assertion in `workspace_crate_prefixed_call_resolves` is vacuous. | claude #3, #4 | Bug | Was a real bug — | **Already fixed** | Round-2 R2-1, commit `80167b3`. Both reviewers were reading the round-1 state and didn't see the fix. No action. |
| R3-5 | `tail = segments[split..].join("::")` allocates before the `file_id` gate. | claude #4 | Polish | Repeat of R2-2. | **Reject (carryover)** | Already rejected in round-2; reviewer explicitly noted "Sub-microsecond in practice." Carryover rejection. |

## Summary

- **Accept:** R3-1, R3-2 (2 findings).
- **Reject (carryover):** R3-3, R3-5 (2 findings — repeat of prior rejections).
- **n/a:** R3-4 (already fixed in prior round).

5 findings reviewed, 2 net new actions.

## Tracker discipline check

R3-2 *files* a new tracker entry rather than deferring without one — exactly the
discipline the rule enforces. R3-1 is in-scope for this branch's next commit.
No silent drops.

## Application

1. Edit `.rivets-044i/verification.md`: update the test-count line and add a brief note.
2. `rivets create` a new issue for the `super::` semantics divergence; capture the ID.
3. Update the docstring of `self_and_super_paths_resolve_via_as_written` to reference the new issue ID.
4. Commit (one bundled commit).
