# Review decisions — round 2 (PR #62)

Date: 2026-05-12
Reviewer: `claude[bot]` (auto-triggered by round-1 push). One review, no new Gemini activity.
Overall verdict from reviewer: **approve with minor suggestions** (no blockers).

Per assessing-review-feedback: each finding is a hypothesis. Decisions below.

---

## Finding R2.1 — New `target.path.join("src")` site not cross-referenced

**Claim:** The new arm in `resolver.rs:61` is a third instance of the same `src/` hardcoding tracked by `rivets-6aoc` (`resolve.rs:66`) and `rivets-34tv` (`indexing.rs:857`, `:1023`). When those issues are eventually fixed, the new site could be missed.

**Verification:** Confirmed — `rivets-6aoc`'s body already lists resolve.rs:66 and the indexing.rs sites but does not mention the new resolver.rs:61 site. CLAUDE.md's "Known multi-crate resolver bugs" bullet has the same gap.

**Decision: ACCEPT.** Two-line update:
1. Add `resolver.rs:61` to CLAUDE.md's known-bugs bullet (one new path in the existing list).
2. Add a one-line cross-reference to `rivets-6aoc`'s notes so the umbrella tracker includes all three sites.

No code change — the actual `src/` hardcoding remains deferred to rivets-6aoc/34tv per round-1 decision 2a.

---

## Finding R2.2 — O(n) linear scan with `.replace()` allocation per call

**Claim:** `workspace_crates.iter().find(|c| c.name.replace('-', "_") == head)` is O(n) and allocates per iteration. For large workspaces (hundreds of crates) this could accumulate. Suggest a TODO comment or a follow-up issue.

**Verification:** This is identical to round-1 Finding 3 (Gemini's allocation point) plus a new "what if scale changes" framing. The reviewer explicitly says "Not a blocker at current scale."

**Decision: REJECT.** Same reasoning as round 1: the rivets workspace has 4 crates, the resolver is not on a hot loop (called once per import per file during indexing), and adding a TODO comment doesn't help a future reader who would see the code and reach the same conclusion. The round-1 decision log already records this. If scale ever materially changes, the comment would be added then.

---

## Finding R2.3 — Doc comment doesn't describe entry-point fallback edge case

**Claim:** The function's doc comment now says single-segment paths resolve to "the target crate's entry-point file (`lib_path` or first bin)" but doesn't say what happens when both are absent (`None`).

**Verification:** Confirmed. The current doc comment leaves the `None` case implicit. A reader has to read the implementation to learn that a crate with no `lib_path` and no `bin_paths` returns `None`. This is a real (if small) clarity gap.

**Decision: ACCEPT.** One-line addition to the doc comment making the `None` case explicit.

---

## Summary

| # | Finding | Decision |
|---|---------|----------|
| R2.1 | Cross-reference resolver.rs:61 site | Accept — CLAUDE.md bullet + rivets-6aoc body |
| R2.2 | TODO for linear scan / HashMap | Reject (same as round 1) |
| R2.3 | Doc comment edge case | Accept — one line |

**Touches:**
- `CLAUDE.md` — one line in the known-bugs bullet.
- `crates/tethys/src/resolver.rs` — one line in the function's doc comment.
- `rivets-6aoc` issue body — one cross-reference note.
- `.rivets-v465/review-decisions-round-2.md` — this file.

No code logic changes. No new tests needed (R2.3 is a documentation fix; R2.2 is rejected).
