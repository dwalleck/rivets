# Review-feedback decisions — round 4 (PR 61)

Source: Claude bot review at 2026-05-12T03:07 (run #25710418596), posted after
round-3 push (commits 06b2b28 + 9bfe4a5). Applied via
`gilfoyle/assessing-review-feedback`.

This round had four substantive findings plus one re-raise of a previously-
decided convention question.

## Decisions

| # | Finding | Category | Decision | Rationale |
|---|---|---|---|---|
| 1 | Empty-imports short-circuit in `resolve_refs_for_file`; add test pinning import-less file behavior | Coverage gap | **Reject** | Already captured: documented as a side finding in `review-decisions-round-3.md` and noted on `rivets-3d0s`. A test pinning "import-less files get zero refs" would document a Pass-2 limitation, not the rivets-0gom fix. Different bug class; scope creep on this PR. |
| 2 | Missing test: `shared_helper` exists only in `crate_b`, must resolve via unscoped fallback | Coverage gap | **Accept** | Real coverage gap. The existing test exercises the priority leg (same-crate wins when present); this test exercises the fallthrough leg (no same-crate → use workspace-wide if unique). Both paths in `fallback_symbol_search` can regress independently. |
| 3 | `debug_assert!` for `%` / `_` in path prefix | Defense in depth | **Reject (was provisionally Accept)** | Initially accepted, then reverted on failure-against-tests evidence: the assert as suggested fires on legitimate snake_case paths. Rust crates routinely use `_` in directory names (e.g. `proc_macro2`); even the existing unit tests use `crate_a`/`crate_b` fixtures. Reviewer's framing "crate directory paths essentially never contain these characters in practice" is false. The underlying bug claim — silent wrong results possible if a same-length path differs only at the wildcard position — is technically true but exceedingly rare. Doc comment continues to assert the limitation in prose. Proper fix (LIKE ESCAPE clause + input escaping) is correct engineering but bigger scope than this PR. |
| 4 | `normalize_path` Unix round-trip allocation | Style/perf nit | **Reject** | Reviewer marked "purely a style note." Negligible cost relative to the SQLite query. Scope creep. |
| – | `.rivets-0gom/` directory lifecycle | Convention | **Reject** | Already decided in round 2. The diagnostic-directory pattern is the established workflow convention; not a per-PR bot decision. |

## What gets applied

In file-edit order:

1. **`crates/tethys/tests/resolver_routing.rs`** — Add second integration test
   `fallback_resolves_via_unscoped_when_no_same_crate_candidate`:
   - Same two-crate workspace pattern as the priority test
   - `shared_helper` defined ONLY in `crate_b/src/lib.rs`
   - Unqualified call from `crate_a/src/lib.rs`
   - Asserts edge `crate_a/src/lib.rs -> crate_b/src/lib.rs` IS present (proves
     fallthrough to unscoped resolution when same-crate scoping misses)

## Re-decision discipline

Finding #3 is documented as a provisional Accept that flipped to Reject after
falsifying evidence (the assert produced 5 false failures on the existing
test suite when run against legitimate `crate_a/`-style paths). This is the
`assessing-review-feedback` skill working as designed:

> If a fix turns out to be wrong during implementation, that's another
> iteration of step 3. Don't ship a fix you no longer believe in just
> because you already started writing it.

The reviewer's underlying concern (silent wrong results possible) is real but
the proposed mechanism doesn't fit the actual input distribution. The right
fix is LIKE escaping; deferring rather than scope-creeping this PR.

## Decision distribution

5 findings → 1 accept + 4 reject. Higher reject rate than round 2 (which had
3 reviewers converging on one strong finding); this round's findings were
more dispersed and several were already-addressed in earlier rounds or
required deeper scope than the reviewer suggested.
