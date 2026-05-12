# Review decisions — round 4 (PR #62)

Date: 2026-05-12
Reviewer: User-invoked specialized review agents (type-design-analyzer, comment-analyzer, silent-failure-hunter, pr-test-analyzer). Distinct from rounds 1–3 which were the auto-triggered `claude-review` GitHub Action.

Round 3's PR bot review concluded "ready to merge"; this round is a deeper architectural pass before merging.

---

## Finding R4.1 — `CrateInfo` field-access leakage; accessor refactor

Type-design analyzer recommended three accessors:
- `module_name(&self) -> Cow<str>` (hyphen→underscore normalization)
- `src_root(&self) -> PathBuf` (replace `path.join("src")` hardcode)
- `entry_point_file(&self) -> Option<PathBuf>` (lib_path → first bin chain)

**Decision: DEFER all three to a separate issue.**

**`entry_point_file()`** — accept the design value, but it's a multi-call-site refactor that doesn't fit a "fix" PR. The round-3 test coverage already locks the inline behavior; adding the accessor in a follow-up PR is a clean swap with no behavior change.

**`src_root()`** — implementing as `self.path.join("src")` would relocate the hardcode to the type without fixing it (misleading encapsulation that *looks* like correct abstraction). The correct implementation derives from `lib_path.parent()` — which is precisely the rivets-6aoc fix. So `src_root()` should be introduced *as part of* rivets-6aoc, not separately.

**`module_name()`** — marginal at a single use site. Only worth it if a second caller appears.

**Filed as [rivets-i8qn](rivets-i8qn).** PR scope stays narrow.

---

## Finding R4.2 — Issue-ID references in source comments

**Split:**

### R4.2a — In-this-PR test doc comments

Locations (per current file state, post-round-3):
- `resolver.rs:266` — "rivets-v465 stress fixture: ..." (test doc opener)
- `resolver.rs:356` — "Slice 2 / design claim C2: ..." (test doc prefix)
- `resolver.rs:380` — "Slice 2 / design claim C3 (stronger version): ..." (test doc prefix)
- `resolver.rs:533` — "Slice 2: ..." (test doc prefix)

These reference the budgeted-plan/checkpointed-build slice numbering and the v465 issue ID. Per CLAUDE.md's own guidance ("Don't reference the current task, fix, or callers"), they belong in the diagnostic dir (where they exist), not in the test docs.

**Decision: ACCEPT.** Trim prefixes; keep the substantive description of what each test verifies.

### R4.2b — Pre-existing issue IDs in `resolve.rs:204, 380-381`

Not introduced by this PR. Sweeping them would broaden scope.

**Decision: DEFER.** Could file as cleanup task; not blocking.

---

## Finding R4.3 — Line numbers in CLAUDE.md known-bugs bullet

Current line embeds `resolve.rs:66`, `indexing.rs:857`, `indexing.rs:1023`, `resolver.rs:61`, `db/references.rs:157`. The first refactor that touches any of those files invalidates these silently.

The rest of the "Tethys resolver internals" section already uses function-name references (`resolve.rs::resolve_cross_file_references`, `indexing.rs::store_references`, etc.) — function names rot far slower than line numbers.

**Decision: ACCEPT.** Convert the known-bugs bullet to function-name references.

---

## Finding R4.4 — `trace!` for workspace-crate miss

Silent-failure hunter suggested instrumenting the `find` failure path to distinguish "typo" from "external crate" in logs.

**Decision: REJECT.** The `crate::`/`self::`/`super::` arms don't trace their misses either. Adding observability to one arm creates an inconsistency that's worse than the original gap. If diagnostic granularity becomes important, it belongs at the resolver entry point as a single span, not arm-by-arm.

---

## Finding R4.5 — `.exists()` asymmetry in single-segment path

`resolve_as_module` (used by `crate::`, `self::`, `super::`, multi-segment workspace-crate paths) checks `.exists()` before returning. The single-segment workspace-crate branch does not — it returns `Some(target.path.join(lib_path))` unconditionally if `lib_path` is `Some`.

**Reviewer's own caveat:** end result is correct because downstream `db.get_file_id` returns `None` for nonexistent paths. But intermediate-result phantom paths make trace logs harder to read and break the "resolver only returns paths that exist on disk" invariant that the rest of the file maintains.

**Decision: ACCEPT.** Mirror `resolve_as_module`'s `.exists()` check.

---

## Finding R4.6 — `debug_assert!` for unique workspace-crate names

`discover_crates` returns a `Vec<CrateInfo>`. The new arm's `find(...)` takes the first match. Cargo prevents duplicate `[package].name` at the manifest layer, so duplicates are unreachable in practice — but the invariant is currently implicit.

`debug_assert!` in `Tethys::new` after `discover_crates` is zero-cost in release builds and documents the assumption.

**Decision: ACCEPT.** Add the assertion at `lib.rs:127` (immediately after `discover_crates` returns).

---

## Finding R4.7 — Test brittleness (multi-hyphen, multi-bin)

Reviewer's own framing: "two-line extensions. Not gaps."

- `my-crate` already exercises hyphen normalization; `my-multi-word` is a trivial variation.
- `single_segment_falls_back_to_bin_when_lib_path_absent` uses a one-bin fixture; "first bin" semantics aren't load-bearing for any current caller.

**Decision: DEFER.** Low value; cheap to add later if behavior ever depends on it.

---

## Summary

| # | Finding | Decision |
|---|---------|----------|
| R4.1 | `CrateInfo` accessors | **Defer** — filed as [rivets-i8qn](rivets-i8qn) |
| R4.2a | Trim issue-IDs in resolver.rs test docs | **Accept** |
| R4.2b | Pre-existing IDs in resolve.rs | **Defer** (out of scope) |
| R4.3 | Line numbers → function names in CLAUDE.md | **Accept** |
| R4.4 | `trace!` for workspace-crate miss | **Reject** (consistency) |
| R4.5 | `.exists()` symmetry in single-segment | **Accept** |
| R4.6 | `debug_assert!` for unique crate names | **Accept** |
| R4.7 | Multi-hyphen / multi-bin test extensions | **Defer** (nits) |

**Touches:**
- `crates/tethys/src/resolver.rs` — 4 test doc trims + `.exists()` check
- `crates/tethys/src/lib.rs` — `debug_assert!` in `Tethys::new`
- `CLAUDE.md` — known-bugs bullet rewrite
- `.rivets-v465/review-decisions-round-4.md` — this file

**Test count:** unchanged (596). No test additions; the `.exists()` and `debug_assert!` are covered by existing tests reaching the modified code paths.
