# Review decisions — round 1 (PR #117)

Source: `/code-review xhigh PR 117` (code-review agent), 14 findings, against
branch head `66965bf`. Route is Local (`route.md`: no `plan.md`), so repairs use
the repository's reproduce → fix → focused-verification process.

Landing order: blocking rows F1, F2 first; the rest are non-blocking.

| finding-id | finding | reviewer | evidence-state | evidence | decision | fix | note |
|---|---|---|---|---|---|---|---|
| F1 | `Start` on an In Progress Issue succeeds, bumps `updated_at` and (since this PR) clears legacy `closed_at`; fix: add a "must be Open" guard | code-review | Verified | Built `66965bf` binary: `rivets start X` twice → second exits 0, `updated_at` 04:36:01.961 → 04:36:01.973. Code: `validate_transition` `_ => Ok(())` admits `(InProgress, InProgress)` | Modify | Rejected in the domain, not with a storage-only guard: `IssueStatus::validate_transition` gains `(InProgress, InProgress) => AlreadyInProgress` (new `StatusTransitionError::AlreadyInProgress`), so every adapter gets it. Fences: `transition_matrix::in_progress_to_in_progress`, `rejected_status_transition_leaves_issue_unchanged::in_progress_to_in_progress` (domain/mod.rs), `workflow_transition_assignment_matrix` repeated-Start snapshot (tests/in_memory_storage.rs). ADR-0005 and MCP `start` description state "Start requires Open" | blocking — the PR widened the pre-existing defect (it now also destroys legacy closure data). Rejection matches Close-on-Closed and Reopen-on-Open, and `LifecycleAction::Start`'s documented "Move an Open Issue" contract |
| F2 | Removing the `closed_at.is_none()` guard has no test; restoring it passes everything | code-review | Verified | Mutant (guard restored) on `66965bf`: `cargo nextest run --workspace` → 1195 passed | Accept | `apply_status_transition_sets_current_closed_period::{stale_open_closes_fresh, stale_in_progress_closes_fresh}` (domain/mod.rs) | blocking — the core fix was unfenced |
| F3 | MCP test split dropped the restart persistence check for update/claim/start/return_to_open | code-review | Verified | Read `66965bf` `registered_mcp_requests_enforce_update_and_dedicated_intent_contract`: ends at `add_note`, no post-stop JSONL comparison (on `main` it had one) | Accept | Restored via shared `assert_persisted_after_restart` (reporting_parity.rs), used by both the update and lifecycle tests | non-blocking — coverage regression, no behavior defect |
| F4 | No domain unit test for `apply_status_transition`'s `closed_at` behavior | code-review | Verified | `status_transition_tests` at `66965bf` tests `validate_transition`/`validate_reopen` only | Accept | `apply_status_transition_sets_current_closed_period` (6 cases incl. In Progress→Open) and `rejected_status_transition_leaves_issue_unchanged` (3 cases) | non-blocking |
| F5 | Parentage rejection test accepts any rejection | code-review | Verified | Read test: asserts exit 1 / `-32602` only | Accept | Converted to rstest cases with `expected_error`; asserts CLI stderr and MCP message contain the `ActiveChildren` / `ClosedParent` text | non-blocking |
| F6 | Timeline test creates at wall-clock now then transitions at fixed earlier instants (closed before created); `> thursday` depends on host clock | code-review | Verified | Read test: `create` uses `Utc::now()` (2026-09-27) vs events 2026-09-21..24 | Accept | `lifecycle_transition_at_records_current_closed_period_and_shared_event_time` pins Created/Updated to Sunday 2025-09-21 under the lock; events moved to 2025-09-22/23/25 | non-blocking — dates moved a year back so the final edit's wall-clock Updated is far after Thursday |
| F7 | CLI/MCP Closed-edit checks use `assert_ne!` on strings, not strict advance | code-review | Verified | Read `cli_tests.rs` and `reporting_parity.rs` Closed-edit asserts | Accept | Parse RFC 3339 and assert `>` in `test_cli_closed_period_reopens_recloses_and_preserves_closed_edits` and `registered_mcp_lifecycle_records_current_closed_period` | non-blocking |
| F8 | `close`/`reopen` MCP descriptions lost "when to use" guidance | code-review | Verified | Diff of `server.rs` vs `main` | Accept | Restored guidance, kept timestamp semantics (server.rs `close`, `reopen`) | non-blocking |
| F9 | `storage-architecture.md` says closure clears only when leaving Closed; Start/ReturnToOpen also clear it | code-review | Verified | Doc text vs `apply_status_transition` and the legacy loading test | Accept | Reworded bullet in `docs/storage-architecture.md` | non-blocking |
| F10 | Rollout status ("rivets-1nhb only; rest under rivets-ffqs") copied into six docs | code-review | Verified | `grep rivets-ffqs` over README/docs | Modify | Removed from README, architecture.md, data-flow.md, cli-mcp-parity.json (+ regenerated .md); kept the single point-in-time reference in ADR-0005, which is a decision record | non-blocking |
| F11 | New tests use bare `.unwrap()` (CLAUDE.md) | code-review | Verified | Read cited lines | Accept | Replaced with `.expect(..)` in the new/moved code of the three test files; pre-existing unwraps outside this PR's diff untouched | non-blocking |
| F12 | Timestamp test uses nested loops + string match + `unreachable!()` instead of rstest | code-review | Verified | Read test | Accept | `required_timestamps_reject_missing_null_and_malformed_records` is now `#[rstest]` `#[values]` over field × `TimestampDefect` enum; the parentage loop too (F5) | non-blocking |
| F13 | `closed_at = None` repeated across arms; rule is `(target == Closed).then_some(changed_at)` | code-review | Not-applicable | N/A — maintainability preference; F2/F4 fences prove behavior is unchanged | Accept | `apply_status_transition` sets `closed_at` in one expression, assignee clearing separate | non-blocking |
| F14 | Duplicate canonical JSON record builder; `NewIssue` spelled out in full | code-review | Verified | Read `in_memory_resilient_loading.rs` and `trait_impl.rs` test | Accept | Module-level `canonical_issue_record` + `LEGACY_*` constants; `NewIssue { title, ..Default::default() }` | non-blocking |

## Review errors

- F1's anchor says Start "has no 'must be Open' guard (unlike ReturnToOpen/Reopen)";
  the right home for the rule is the domain transition matrix, not a storage
  guard like ReturnToOpen's — see F1 fix.
- F11 cites `in_memory_resilient_loading.rs:452,458` among others; the line set
  mixes new code with the fixture helper, but all cited unwraps were in PR-added code.

## Fence mutation receipts

Baseline for red: working tree on `66965bf` + this round's repairs, with the
single named fault injected; restored green on the same tree without it.

| Fence | Injected fault | Red | Restored |
|---|---|---|---|
| `apply_status_transition_sets_current_closed_period::stale_{open,in_progress}_closes_fresh` | `closed_at = if Closed { closed_at.or(Some(changed_at)) }` (old guard) | 2 failed / 25 | 25/25 pass |
| `transition_matrix::in_progress_to_in_progress`, `rejected_status_transition_leaves_issue_unchanged::in_progress_to_in_progress` | removed `(InProgress, InProgress)` arm | 2 failed / 25 | 25/25 pass |
| `reason_bearing_parentage_rejections_…::{case_1,case_2}` | `ActiveChildren`/`ClosedParent` replaced by `AlreadyClosed`/`NotClosed` | 2 failed / 2 | 2/2 pass |
| F3 restored persistence check | N/A — same assertion that ran on `main`; logic unchanged, only moved into a helper |  |  |

## Verification

- `cargo nextest run --workspace`: 1211 passed, 10 skipped.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo fmt --all --check`: PASS. `cargo test --doc --workspace`: PASS.
- `python scripts/render-cli-mcp-parity.py --check`: PASS.
- CLI smoke (rebuilt binary): second `rivets start` → exit 1,
  "Issue is already in progress (status: in_progress)", JSONL bytes unchanged.

## Repair re-review

Independent `rust-code-reviewer` subagent, given only the uncommitted repair diff
and this log (not the original findings), no write access. Scope: equivalence of
the simplified `apply_status_transition` over all valid pairs, callers affected
by `AlreadyInProgress`, MCP error mapping, test assertions vs names, docs vs code.
Result: PASS — no defects at or above confidence threshold; reran workspace
suite (1211 passed, 10 skipped). Confirmed the only divergent pair
(Closed→In Progress assignee) is unreachable behind `MustReopenToOpen`, and
`Error::InvalidStatusTransition(_)` maps the new variant to -32602.

Nits and disposition:
- Use `assert_eq!(issue, before)` in the rejected-transition test — Refuted:
  `Issue` does not implement `PartialEq`; the test compares every field a
  transition can touch (status, assignee, closed_at, updated_at).
- No CLI/MCP-level repeated-`start` test — Reject: the variant is mapped as a
  group; the CLI smoke above exercised exit 1 and unchanged bytes.
- ADR-0005 line over the file's width — Accept: reflowed.
