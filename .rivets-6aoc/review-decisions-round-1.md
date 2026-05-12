# Review decisions — round 1 (PR #63)

Date: 2026-05-12
Reviewer: User-invoked specialized review agents (5 in parallel: code-reviewer, type-design-analyzer, comment-analyzer, silent-failure-hunter, pr-test-analyzer). Consolidated review with C1-C3 marked CRITICAL by silent-failure-hunter; code-reviewer independently rated the PR approve-worthy with no findings ≥80 confidence.

The two views are both legitimate. The silent-failure-hunter view is: "observability and qualified-name fallback for crate-less files should land in this PR for the fix to be honest." The code-reviewer view is: "core fix correct, follow up on the rest." I'm taking the silent-failure-hunter view on C1-C3 + I6 (cheap and matches the PR's discipline) and deferring the rest with explicit tracker entries.

---

## Critical findings

### C1 — Silent-skip sites use `trace!` instead of `debug!`

**Sites:** `resolve.rs:105-113`, `indexing.rs:850-856`, `indexing.rs:1035-1041`

**Verified:** `lib.rs:175` (`compute_module_path_for_file`) uses `debug!` for the same "file not within any known crate" condition. My new code uses `trace!`. Default tracing-subscriber filters trace out, so production runs silently drop these files with no operator visibility.

**Decision: ACCEPT.** Raise all three to `debug!`. Matches existing convention.

### C2 — `resolve_refs_for_file` silently drops qualified-name fallback for crate-less files

**Site:** `resolve.rs:105-113`

**Verified:** `fallback_symbol_search(ref_name, is_qualified, ctx.current_file_path)` signature doesn't take `crate_root`. Pre-fix, files outside any crate ran through the full pipeline (imports/glob with non-existent `workspace_root/src` → all None) and then fell through to `fallback_symbol_search`, which CAN resolve qualified names path-agnostically via `get_symbol_by_qualified_name`. My slice 2 skip drops this entire path.

**Empirical impact in rivets workspace:** 3 imports in 2 bruno-examples files; the actual ref count affected is small but non-zero. The slice 5 measurement showed net +110 resolved (overwhelming evidence of fix benefit) but didn't isolate the regression from crate-less files.

**Decision: ACCEPT.** Change strategy: don't skip crate-less files; instead use the file's parent directory as a sentinel `crate_root`. The `crate::` arm will resolve to phantom paths (and return None via `.exists()` check); other arms (`self::`/`super::`/workspace-crate/fallback) work unchanged. Coherent semantics ("treat a standalone file as its own single-file 'crate' rooted at its directory") and preserves fallback resolution.

### C3 — Doc comment in `indexing.rs::compute_dependencies` is factually wrong

**Site:** `indexing.rs:836-839`

**Verified:** My doc says "`self::`/`super::`/external arms can't anchor dep edges without a crate context either." `resolver.rs::resolve_self_path` uses only `current_file.parent()`; `resolve_super_path` uses `current_file.parent().parent()`. Neither uses `crate_root`. The justification for skipping crate-less files in dep-graph computation is genuine only for the `crate::*` arm.

**Decision: ACCEPT.** Narrow the doc-comment claim. Coupled with I4's fix (move the lookup inside the import loop), the skip becomes per-arm, not whole-function.

---

## Important findings

### I1 — `src_root()` returns paths that may not exist on disk

**Site:** `types.rs:172-200` (the slice 1 accessor)

**Verified:** Branch 3 of the derivation (`<path>/src` defensive fallback) is reachable when `lib_path = None` AND `bin_paths.is_empty()`. `discover_crates` produces this state when a `Cargo.toml` has `[package]` but no `[lib]`, no `[[bin]]`, and the conventional `src/lib.rs` / `src/main.rs` files don't exist on disk. Cargo's manifest parser doesn't reject this configuration — it produces a warning at build time, not at parse time.

The rustdoc claim that "Cargo rejects such a configuration" overstates Cargo's behavior. The path returned is unverified.

**Decision: ACCEPT MODIFIED.** Take the `tracing::warn!` route (not the `unreachable!`/`debug_assert!` route): `src_root()` is invoked from production code paths, panicking on pathological input is the wrong response. The total function shape (always returns `PathBuf`) is preserved; the anomaly is surfaced via warn.

### I2 — `dir_of` swallows malformed `lib_path`

**Site:** `types.rs::dir_of`

**Verified:** If `lib_path` is absolute (e.g., `/etc/passwd/lib.rs`), `Path::join` replaces the base. `crate_path.join("/etc/passwd")` returns `/etc/passwd`, escaping the workspace. Requires a malicious or malformed checked-in `Cargo.toml`.

**Decision: ACCEPT.** Validate `lib_path` and `bin_paths` are relative and non-empty in `cargo.rs::parse_crate_from_manifest`. Log `warn!` and treat as `None` if invalid. Defense-in-depth at the boundary, not inside `src_root()`.

### I3 — Single-segment arm in `resolver.rs:56-62` still inlines lib_path/bin_path

**Verified:** Real encapsulation leak. The lib_path-or-first-bin chain is exactly the `entry_point_file()` accessor that rivets-i8qn defers.

**Decision: DEFER with TODO.** rivets-i8qn explicitly scopes `entry_point_file()` to a follow-up PR; landing it here expands scope beyond what was agreed in falsifiable-design. Add a `// TODO(rivets-i8qn):` comment at the leak point so the next reader sees the connection.

### I4 — `compute_dependencies` loses `super::`/`self::` dep edges for crate-less files

**Sites:** `indexing.rs:850-856`, `1035-1041`

**Verified:** Same shape as C2 on the dep-graph side. Pre-fix, these computed `self::Foo` and `super::Foo` dep edges (using `current_file.parent()`) for crate-less files; my skip drops those.

**Decision: ACCEPT.** Same fix mechanism as C2: use file-parent sentinel `crate_root` instead of skipping. Then the `crate::*` arm returns None (no anchor) but `self::`/`super::` continue to produce edges.

### I5 — Claims C5/C6/C7 are measurement-only, not CI-fenced

**Verified:** `measurement-results.md` is a one-shot snapshot. No automated test asserts the +110 floor or any related count. A future change re-introducing the hardcode would silently re-break, since my unit/stress tests use multi-crate fixtures that the per-file lookup correctly handles regardless of the bug.

**Decision: ACCEPT.** Add an integration test that indexes a fixed 2-crate fixture and asserts a known minimum resolved-ref floor. Highest single-test ROI for regression protection.

### I6 — Workflow nomenclature ("slice N", "claim CN", "pre-fix impl") in comments

**Sites:** `types.rs:2185`, `types.rs:2210`, `resolver.rs:545-552`, `resolve.rs:99-104`

**Verified:** Per CLAUDE.md: "Don't reference the current task, fix, or callers — those belong in the PR description and rot as the codebase evolves." My slice-1 and slice-4 test docs reference workflow vocabulary.

**Decision: ACCEPT.** Strip "Slice N", "Stress fixture for...", "claim CN", "pre-fix", "post-fix" from doc comments. Test names + assertion messages already carry the documentation load.

---

## Suggestions

| # | Finding | Decision | Tracker |
|---|---|---|---|
| S1 | Distinct trace when workspace-crate matches but sub-path missing | **Reject** — low value | — |
| S2 | Normalize `Cargo.toml` path match via `Path::file_name()` | **Defer (folded into rivets-dzn8)** | [rivets-dzn8](rivets-dzn8) |
| S3 | Inconsistent log message format (prose vs structured fields) | **Defer (filed)** | [rivets-limz](rivets-limz) |
| S4 | Warn at parse time if multi-bin entries have divergent parents | **Defer (filed)** | [rivets-nqbg](rivets-nqbg) |
| S5 | Additional test coverage (3+ crate workspaces, ambient `target/`, streaming mode, `#[ignore]` LSP test for rivets-714v) | **Partial accept** — fold streaming-mode into I5; others already covered (prefix-match via `get_crate_for_file_prefers_longest_prefix_match`), out of resolver scope (target/), or owned by [rivets-714v](rivets-714v) | — |
| S6 | Struct-level doc on `CrateInfo`: "use `src_root()` — see rivets-6aoc" | **Accept** — low-cost defense against a fifth regression site | inline in this PR |
| S7 | `src_root()` doc test + intra-doc links | **Reject** — adds maintenance surface, existing unit tests document behavior fine | — |
| S8 | Three independent copies of `workspace_with_files` | **Defer (filed)** | [rivets-dzn8](rivets-dzn8) |

---

## Action ordering (recommended)

Per the silent-failure-hunter's priorities + my own evaluation:

1. **C1** — `trace!` → `debug!` at 3 sites. Mechanical, ~5 min.
2. **C3** — Fix the inaccurate doc comment in `compute_dependencies`. ~2 min.
3. **C2 + I4** (combined) — File-parent sentinel `crate_root` for crate-less files, in both `resolve.rs` and `indexing.rs`. ~30 min. Preserves pre-fix fallback resolution. Update the doc claims to match.
4. **I6** — Strip workflow nomenclature from test docs. ~10 min.
5. **I1 + I2** — `src_root()` `warn!` on the pathological fallback branch; `lib_path` / `bin_paths` validation in `parse_crate_from_manifest`. ~15 min.
6. **I5** — Integration test with floor assertion (3-crate fixture, asserts ≥N resolved cross-file refs). Fold in streaming-mode coverage (S5 partial). ~30 min.
7. **I3** — Add `// TODO(rivets-i8qn):` comment at `resolver.rs:56`. ~2 min.
8. **S6** — Struct-level doc on `CrateInfo` pointing at `src_root()`. ~3 min.
9. **File trackers** for: S3 (log format consistency), S4 (multi-bin parent divergence), S8 (workspace_with_files dedup). ~10 min.

**Total estimate:** ~2 hours of fixes + ~10 min of tracker filings. All within the existing branch / PR.

---

## Why I'm taking the silent-failure-hunter view on C1-C3

The reviewer noted both views are legitimate. The code-reviewer agent rated the PR approve-worthy with no high-confidence findings. The silent-failure-hunter agent flagged C1/C2 as CRITICAL.

These aren't contradictory — they reflect different bars. Code-reviewer cares about "is this correct" (yes, the four fix sites are correct). Silent-failure-hunter cares about "what does this make undiscoverable" (the trace-level skip + qualified-name fallback drop).

The deciding factor: **the PR's own discipline.** I went through prove-it-prototype → falsifiable-design → budgeted-plan → checkpointed-build to produce verifiable claims. The +110 ref delta in slice 5 is the empirical evidence the design holds. Shipping with silent-skip + lost-fallback weakens that empirical claim — the +110 might be +105 with 5 lost-to-skip; we don't know. Landing C1-C3 in this PR keeps the design's discipline intact.

The follow-up items (I3, S3-S5, S7) are genuine deferrals that don't compromise the core claims.
