# Slice 2 drift evidence — design class falsified empirically

**Date:** 2026-05-17 (iteration #2, post-PR #65 substrate)
**Branch:** `fix/rivets-3d0s-kind-filter`
**Reverted at:** slice 2 (uncommitted; slice 1 commit `4419136` retained on branch)

## What was tried

The `audit_simulation.py`-validated design from `.rivets-3d0s/design.md`:
add a `ref_kind` parameter to `search_unique_symbol_by_name`, filter the
candidate set by `SymbolKind` compatibility inside the unscoped fallback
SQL query.

Slice 1 (committed): added `Option<&ReferenceKind>` parameter, filter logic,
4 unit tests. resolve.rs's `fallback_symbol_search` passes `None` so live
behavior is unchanged — proven via byte-identical probe.py output to baseline.

Slice 2 (reverted): threaded `&ref_.kind` from `try_resolve_reference` → 
`fallback_symbol_search` → `search_unique_symbol_by_name`. **Live behavior 
change.** Drift detected on the per-slice oracle gate.

## Drift evidence

| metric | slice 1 (baseline) | post-slice-2 | delta |
|---|---|---|---|
| total file_deps rows | 398 | 417 | **+19** |
| cross-crate ordered pairs | 8 | 7 | -1 |
| **cross-crate edge sum** | **74** | **74** | **0 — unchanged!** |
| FORBIDDEN-pair edges | 14 | 7 | −7 (50% drop, design target met) |
| MISMATCH-pair edges | 38 | 43 | +5 |
| ALLOWED-pair edges | 22 | 24 | +2 |
| intra-tethys edges | 201 | 219 | +18 |
| **ambiguity violations** | **0** | **3** | **+3 — rivets-0gom claim 6 BROKEN** |
| cross-crate refs (Section 3) | 326 | 324 | −2 (~0 demotion) |

### Per-pair shift

| pair | slice 1 | post-slice-2 | delta | oracle |
|---|---|---|---|---|
| tethys → rivets-jsonl | 23 | 28 | +5 | MISMATCH |
| tethys → rivets | 15 | 15 | 0 | MISMATCH |
| rivets → rivets-jsonl | 15 | 16 | +1 | ALLOWED |
| rivets-mcp → rivets | 7 | 8 | +1 | ALLOWED |
| rivets → tethys | 5 | 4 | −1 | FORBIDDEN |
| rivets-mcp → tethys | 4 | **0** | **−4 ✓** | FORBIDDEN |
| rivets-jsonl → tethys | 3 | **0** | **−3 ✓** | FORBIDDEN |
| rivets-mcp → rivets-jsonl | 2 | 2 | 0 | FORBIDDEN |
| **rivets → rivets-mcp** | (none) | **1** | **NEW** | **FORBIDDEN (new phantom!)** |

## Failure mode

**This is the same un-ambiguation drift documented in the issue notes from
the prior revert** (`chore(rivets-3d0s): revert audit slice; file rivets-v465
as root-cause blocker`):

> "the audit narrows the candidate set, which converts previously-ambiguous
> names into unique matches — creating new method-call phantoms via
> un-ambiguation. The falsifiable-design simulation didn't model this dynamic
> coupling."

### The mechanism

Pre-slice-2 for a reference like `rivets/.../some_call.rs:foo.kind(...)`:
1. Same-crate scoping path: no `kind` symbol in `rivets` → falls through
2. Unscoped fallback: `WHERE name='kind' LIMIT 2` finds ≥2 workspace matches
   → returns None (rivets-0gom claim 6 ambiguity refusal)
3. Ref stays unresolved

Post-slice-2 for the same reference (assuming ref_kind=Call):
1. Same-crate scoping path: unchanged, falls through
2. Unscoped fallback with filter: `WHERE name='kind' AND kind IN ('function','method','macro') LIMIT 2`
   → narrows from "2+ candidates" to "exactly 1 candidate (the method `kind` somewhere)"
   → LIMIT 2 sees a single result → returns Some(symbol)
3. Ref resolves to a wrong-crate target. `file_deps` records a NEW edge.

The simulation's model: "audit demotes resolved refs based on (ref_kind,
sym_kind) compatibility." The simulation iterates resolved refs and decides
whether they'd survive an audit rule.

The implementation's reality: "filter narrows candidate set BEFORE
ambiguity check fires." Refs that ambiguity-refusal correctly rejected pre-fix
now slip through as unique-match resolutions.

These are different operators with different fixed-points. The simulation is
not a faithful model of the implementation.

## Why the design's claims appear to pass anyway

The design's claim C4 ("≥ 50% reduction in FORBIDDEN-pair phantom edges") is
in fact met: 14 → 7 (50% drop). FORBIDDEN-pair refs from `rivets-mcp →
tethys`, `rivets-jsonl → tethys`, `rivets → tethys` did get correctly
demoted by the kind filter.

But the SAME mechanism that caused those correct demotions ALSO created new
unique-match resolutions elsewhere:
- +5 in `tethys → rivets-jsonl` (MISMATCH)
- +1 new pair `rivets → rivets-mcp` (FORBIDDEN, oracle says zero edges
  expected — newly created PHANTOM)
- +18 in intra-tethys (within-crate phantoms)
- +3 ambiguity violations breaking rivets-0gom's load-bearing claim

Net cross-crate edge count: **unchanged (74 → 74)**. The design moves
phantoms between buckets rather than eliminating them.

## Path forward

User chose option A: **discard slice 2, re-enter `gilfoyle:falsifiable-design`
with a post-pass audit approach.**

The hypothesis for the new design:

> Apply the kind-compatibility audit AFTER all resolution passes complete.
> Iterate the resolved refs in the `refs` table; for each ref whose
> `(ref_kind, sym_kind)` pair is incompatible, demote it (set `symbol_id =
> NULL` and `target_file_id = NULL`). Then re-run `populate_call_edges` and
> `populate_file_deps_from_call_edges` to rebuild downstream aggregates.

Sidesteps the un-ambiguation dynamic by construction: the audit operates on
the resolver's output, not its inputs, so it cannot create new
unique-resolutions. It can only remove incorrect ones.

Risk: legitimate cross-crate refs that came through imports might also
be kind-incompatible if the import was for the wrong symbol. The
falsifiable-design iteration needs to model and measure this.

## Slice 1's commit (4419136) status

The slice 1 commit adds:
- `Option<&ReferenceKind>` parameter to `search_unique_symbol_by_name`
- 4 unit tests for the kind filter
- doc comment for the new behavior

This is unused infrastructure under the post-pass design (the post-pass audit
doesn't need a kind filter on `search_unique_symbol_by_name`). Decision on
whether to revert slice 1 deferred to the new design iteration — it may
still be useful as the demotion query primitive.

## Artifacts referenced

- `.rivets-3d0s/design.md` — the falsified design
- `.rivets-3d0s/plan.md` — the slice plan that halted
- `.rivets-3d0s/probe.py` — the persistent oracle (re-used)
- `.rivets-3d0s/audit_simulation.py` — the falsified cheapest falsifier
- `.rivets-0gom/probe.py` — the probe.py being used as persistent gate
- `.rivets-0gom/after-lcb6-merge.txt` — the slice 1 / pre-slice-2 baseline

## Files restored to slice 1 state

- `crates/tethys/src/resolve.rs` reverted via `git checkout`
- `crates/tethys/src/db/symbols.rs` retained (slice 1's signature change)
