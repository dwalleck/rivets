# Review decisions — round 1 (PR #62)

Date: 2026-05-12
Reviewers: 3× `claude` top-level reviews, 1× `gemini-code-assist` review with 3 inline comments.

Per the `assessing-review-feedback` discipline: each finding below is treated as a hypothesis to verify, not an instruction. Decisions are recorded with explicit accept/modify/reject and reasoning.

---

## Finding 1 — Inline comment references issue ID `rivets-v465`

**Source:** Claude review #1, Claude review #2.
**Location:** `crates/tethys/src/resolver.rs:44-49` (inside the new `head =>` arm).

**Claim:** The comment cites `rivets-v465` and narrates pre-fix behavior. CLAUDE.md says: *"Don't reference the current task, fix, or callers — those belong in the PR description and rot as the codebase evolves."*

**Verification:** CLAUDE.md text confirmed; the comment is multi-paragraph and includes the issue ID. The bug history already lives in the PR description and commit message.

**Decision: ACCEPT.** Replace with a one-line comment explaining the *why* (Rust 2018+ idiom routing into another crate's `src/`).

---

## Finding 2 — Hardcoded `src/` ignores `CrateInfo::lib_path` + single-segment returns directory instead of file

**Source:** Claude review #1, Claude review #2, Claude review #3, Gemini inline (`resolver.rs:54`, medium-priority).

This finding has two distinct sub-claims that I'm splitting:

### 2a. Multi-segment case uses hardcoded `src/` instead of deriving from `lib_path`

**Claim:** `target.path.join("src")` always appends `src/`. `CrateInfo.lib_path` is already populated; we could derive `src/` from `lib_path.parent()`.

**Verification:** Confirmed. `CrateInfo.lib_path` is `Option<PathBuf>` (relative to crate path), typically `Some("src/lib.rs")`. Deriving the directory via `.parent()` would handle non-standard layouts.

**However:** The pre-existing callers in `indexing.rs` and `resolve.rs` *also* hardcode `workspace_root.join("src")`. Those are tracked as **rivets-6aoc** (resolve.rs:66) and **rivets-34tv** (indexing.rs:857, 1023). Fixing only the new arm here creates a one-off inconsistency without resolving the broader bug class.

**Decision: REJECT for this PR.** Defer to rivets-6aoc/rivets-34tv where the broader `src/` hardcoding cleanup belongs. Will reply to reviewers acknowledging the point and pointing at those issues.

### 2b. Single-segment path (`["rivets"]`) returns directory instead of entry-point file

**Source:** Gemini inline (resolver.rs:54), point #2.

**Claim:** When `path.len() == 1` (e.g., `use rivets;`), the current code calls `resolve_crate_path(&[], &target.path.join("src"))`, which returns `Some(target.path.join("src"))` — a *directory*, not a file. The correct behavior is to return the crate's entry-point file (`lib.rs`).

**Verification:** Traced all 3 production call sites of `resolve_module_path` (`indexing.rs:881`, `indexing.rs:1042`, `resolve.rs:314`). All three feed the result into a file-level dependency record (`relative_path(&resolved)` used as a dep_path / file ID). Returning a directory there would silently corrupt the dep graph with a non-file edge. So this is a real bug, not a theoretical one — even if `use rivets;` is rare.

**Decision: ACCEPT.** Fix by branching on `path.len() == 1` and using `target.lib_path` (fall back to first `bin_paths` entry, then `lib.rs` as a last resort) to resolve to the entry-point file.

Note: this *does* introduce one use of `lib_path` while leaving the multi-segment case still hardcoded to `src/`. That's a deliberate inconsistency in favor of staying narrowly scoped; rivets-6aoc/34tv will sweep the rest.

---

## Finding 3 — `c.name.replace('-', "_")` allocates per `.find()` iteration

**Source:** Claude review #1, Claude review #3, Gemini inline (resolver.rs:54, point #1).

**Claim:** Allocates a fresh `String` per crate in the workspace, per resolver call.

**Verification:** Confirmed. For the rivets workspace (4 crates), it's noise — ~4 small allocations per resolver call that the allocator handles trivially. The resolver is not on a hot inner loop (called once per import per file during indexing, not per reference).

**Decision: REJECT.** The byte-level comparison alternative Gemini suggested is denser and harder to read. The current code communicates intent clearly. If this ever shows up in a profile, pre-normalize at `CrateInfo` construction time instead.

---

## Finding 4 — Diagnostic directories (`.rivets-3d0s/`, `.rivets-v465/`) committed to repo

**Source:** Claude review #1, Claude review #2, Claude review #3 (all three flagged it).

**Claims (combined):**
- Conflicts with CLAUDE.md's documented `docs/design/` and `docs/plans/YYYY-MM-DD-<feature>.md` convention.
- Python probe scripts hardcode `tethys.db` paths and will rot as the schema evolves.
- Future contributors will see `.rivets-3d0s/` and not know what it is.

**Verification:** Reviewers are correct that the convention is undocumented from their perspective. From mine, this is the established `.<issue-id>/` pattern recorded in personal memory (`feedback_diagnostic_directories.md`) — a deliberate choice to keep probes, oracles, plans, and decision logs co-located with the issue ID for future archaeology.

**Decision: MODIFY.** Don't move the files (the convention is correct), but **document the pattern in CLAUDE.md** so reviewers stop flagging it. Add a short paragraph under "Documentation Conventions" describing what `.<issue-id>/` directories are, when they're created, and that the probe scripts are point-in-time snapshots not maintained tooling.

---

## Finding 5 — `crate_root` hardcoded to workspace root in `indexing.rs:881` and `resolve.rs:314`

**Source:** Gemini inline (high-priority, both files).

**Claim:** `crate_root` callers pass `workspace_root/src` instead of the file's own crate root, breaking sub-crate `crate::` resolution.

**Verification:** This is **rivets-6aoc** and **rivets-34tv** verbatim — already documented in the PR description as deferred. Gemini did not appear to read the PR body.

**Decision: REJECT for this PR.** Reply to both inline comments pointing at the existing issue IDs. No code change.

---

## Finding 6 — Missing self-reference test (`use rivets::Foo` from inside `rivets`)

**Source:** Claude review #3.

**Claim:** The new arm doesn't have a test for the case where the caller imports its *own* crate name (semantically equivalent to `crate::`).

**Verification:** Confirmed missing. The contract here is: the new arm should find the caller's own `CrateInfo`, recurse into its own `src/`, and produce the same path as the `crate::` arm would.

**Decision: ACCEPT.** Add a test that constructs a single-crate workspace where the file uses its own crate name in the import path, and asserts the result matches the `crate::`-form resolution.

---

## Finding 7 — Tests could use `Path::ends_with(Path)` instead of `||`-d string comparisons

**Source:** Claude review #1.

**Claim:** Cosmetic improvement to existing tests.

**Decision: REJECT.** Cosmetic, no behavior change. Not worth churn in round 1.

---

## Summary

| # | Finding | Decision |
|---|---------|----------|
| 1 | Inline comment references issue ID | Accept — trim |
| 2a | Multi-segment uses hardcoded `src/` | Reject (deferred to rivets-6aoc/34tv) |
| 2b | Single-segment returns directory | Accept — fix |
| 3 | `replace()` allocates per find | Reject (noise) |
| 4 | Diagnostic dirs convention | Modify (document in CLAUDE.md, don't move) |
| 5 | Gemini high-pri inline (rivets-6aoc/34tv) | Reject — already filed |
| 6 | Missing self-reference test | Accept — add |
| 7 | `Path::ends_with` style nit | Reject (cosmetic) |

**Round 1 code touches:**
1. `crates/tethys/src/resolver.rs` — trim comment + add single-segment branch + new test.
2. `CLAUDE.md` — document `.<issue-id>/` diagnostic convention.

**Reviewer replies:**
- One round-1 summary PR comment.
- Two inline replies on Gemini's `indexing.rs:881` and `resolve.rs:314` comments pointing at rivets-6aoc/34tv.
