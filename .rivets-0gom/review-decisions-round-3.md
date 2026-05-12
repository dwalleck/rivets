# Review-feedback decisions — round 3 (PR 61)

Source: Claude bot review at 2026-05-12T02:48 (run #25710142369), posted after
round-2 push (commit 96deeaa). Applied via `gilfoyle/assessing-review-feedback`.

This round contains exactly one substantive finding worth re-deciding. Three
other findings in the same review are restatements or low-priority nits — see
below.

## Verified claims

### Finding 1 — No CI regression gate for `fallback_symbol_search` routing

**Verified by inspection.** The probe at `.rivets-0gom/probe.py` is excellent
manual verification but is not invoked by `cargo nextest run`. A regression
that drops the same-crate scoping branch — or reintroduces `LIMIT 1` on
ambiguity — would not be caught by the existing 586 tests, all of which test
the DB layer in isolation. The bug class is one wire-up assumption away from
returning.

## Decisions

| # | Finding | Category | Decision | Rationale |
|---|---|---|---|---|
| 1 | No CI gate for `fallback_symbol_search` routing | Test gap | **Accept (re-decision)** | New framing makes the rejection in round 2 stale. Probe-vs-oracle is documentation, not enforcement. One synthetic two-crate integration test gates the bug class in CI without duplicating the probe's diagnostic richness. |

### Re-decision rationale

Round 2 rejected resolver-level unit tests with the reasoning:

> This was an intentional design choice in slice 2's plan: resolver-level unit
> tests duplicate what the probe-vs-oracle integration gate already proves.

That argument addressed *unit tests* (which would mock `Index` / DB). The
post-round-2 reviewer's argument is structurally different: not "add more
unit tests" but "the probe is the gate, but it's not in CI." Per the
`assessing-review-feedback` skill: "Two reviewers can share the same blind
spot. Verify the underlying claim, not the count." The underlying claim
here — no CI enforcement of the routing — is verifiable and true. The
re-decision changes shape: **integration test, not unit test**.

The new test exercises the full indexing pipeline against a synthetic
two-crate workspace where the bug would manifest, and asserts directly on
`file_deps` (the same table the probe checks). That is the probe-vs-oracle
methodology in a CI-runnable form.

## Findings considered and rejected (or noted only)

- **`get_crate_for_file` canonicalization for deleted files** — Already
  addressed in round 2 (debug log added). Reviewer acknowledged this in
  the same review.
- **LIKE wildcard injection from `path_prefix`** — Already documented in
  the round-2 doc comment; reviewer marked "no action required."
- **`prepare_cached` vs `prepare`** — Low-priority perf nit. Defer; not
  worth scope creep on this PR.

## What gets applied

In file-edit order:

1. **`crates/tethys/tests/resolver_routing.rs`** (new) — Single integration
   test `fallback_routes_unqualified_ref_to_same_crate_not_cross_crate`:
   - Two-crate synthetic workspace
   - `crate_a/src/lib.rs` calls `shared_helper(42)` unqualified
   - Both `crate_a/src/target_module.rs` and `crate_b/src/lib.rs` define `shared_helper`
   - Negative assertion: no `crate_a -> crate_b` `file_deps` edge
   - Positive assertion: `crate_a/src/lib.rs -> crate_a/src/target_module.rs`
     edge present (proves fallback resolution ran)

2. **Falsifiability check (not committed):** temporarily bypassed the
   same-crate branch in `fallback_symbol_search` with `if false && ...` and
   confirmed the positive assertion fails. Test is a real regression gate.

## Surprise finding (not a review item, but worth noting)

During fixture development discovered that `resolve_refs_for_file` short-circuits
on `imports.is_empty()` — files with no `use` statements bypass Pass-2
resolution entirely (including the fallback path). The fixture deliberately
adds an unrelated `use crate::imports_module::imported_fn` to force the
resolver to run on `lib.rs`. This is not a bug per se (it's a Pass-1 vs Pass-2
optimization), but it interacts with `rivets-3d0s` (stdlib trait pollution):
references in import-less files don't go through the rivets-0gom fix at all.
Worth investigating during `rivets-3d0s` work.
