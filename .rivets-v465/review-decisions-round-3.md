# Review decisions — round 3 (PR #62)

Date: 2026-05-12
Reviewer: `claude[bot]` (auto-triggered by round-2 push, comment 4433500382). No Gemini activity this round.
Overall verdict: **"Ready to merge once the test gap is addressed or explicitly deferred."** — explicit approval pending two small test additions.

---

## Finding R3.1 — `src/` hardcode at `resolver.rs:61` acknowledged as deferred

Reviewer is confirming the round-1/2 deferral was intentional and visible in CLAUDE.md / rivets-6aoc. No action.

## Finding R3.2 — `replace('-', "_")` allocation explicitly "not raising as a defect"

Reviewer is acknowledging round-1/2 rejection. No action.

## Finding R3.3 — Bin-only crate path is untested

**Claim:** `single_segment_workspace_crate_resolves_to_entry_point_file` only exercises the `lib_path` branch. The `or_else(|| bin_paths.first().map(|(_, p)| p))` fallback — added precisely to handle bin-only crates — has no test coverage.

**Verification:** Confirmed. The bin fallback was added in round 1 specifically because the function should resolve to the entry-point file regardless of whether a crate is a lib or bin. Without a test, a future refactor could silently drop the bin path without breaking any signal.

**Decision: ACCEPT.** Add a test using the same `workspace_with_crates` pattern (with a new variant that allows specifying `lib_path: None, bin_paths: [...]`).

## Finding R3.4 — Empty entry-point case (`lib_path: None, bin_paths: []`) is untested

**Claim:** The round-2 doc-comment update explicitly documents that single-segment returns `None` when neither entry point is set. That contract has no test.

**Verification:** Confirmed. This is an invariant test — the round-2 doc comment promises a behavior; round 3 locks it down so future code can't silently change it.

**Decision: ACCEPT.** Add a one-screen test asserting `None` for the empty-entry-point case.

## Finding R3.5 — Test count drift in PR body

**Claim:** PR description says "4 new unit tests" but the diff contains 6 (and after round 3 will contain 8). Either the description was written mid-development or the count drifted.

**Verification:** True — PR body was written before round-1 review response added 2 tests (round 1 added `single_segment_...` and `workspace_crate_self_reference_...`). Round 3 will add 2 more.

**Decision: ACCEPT.** One-line edit to PR body to either update the count or rephrase to avoid pinning a specific number.

## Finding R3.6 — `resolver.rs:61` self-annotation in CLAUDE.md known-bugs bullet

Reviewer is confirming the round-2 addition is intentional self-annotation. No action.

---

## Summary

| # | Finding | Decision |
|---|---------|----------|
| R3.1 | `src/` hardcode | No action (acknowledgement of prior deferral) |
| R3.2 | `replace()` alloc | No action (acknowledgement of prior rejection) |
| R3.3 | Bin-only crate test | Accept — add test |
| R3.4 | Empty-entry-point None test | Accept — add test |
| R3.5 | Test count in PR body | Accept — edit body |
| R3.6 | CLAUDE.md self-annotation | No action |

**Touches:**
- `crates/tethys/src/resolver.rs` — extend `workspace_with_crates` fixture (or add a sibling fixture) + 2 new tests.
- PR #62 body — update test count phrasing.
- `.rivets-v465/review-decisions-round-3.md` — this file.

**Test count after round 3:** 596 (was 594).
