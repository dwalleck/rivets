# Design: fix tethys resolver phantom cross-crate edges (rivets-0gom)

> Following gilfoyle/falsifiable-design. Extends the probe (prove-it-prototype).
> Every claim has a paired falsifier. Cheapest falsifier runs before approval.

## What the probe established

The probe (`.rivets-0gom/probe.py`) and oracle (`.rivets-0gom/oracle.sh`) showed:

- 559 file_deps rows on the rivets workspace post-PR-60
- 170 cross-crate pairs in file_deps
- 149 of 170 (88%) are phantom: oracle says zero cross-crate edges expected, probe says non-zero
- 46 of 72 tethys→rivets phantoms target a single file: `crates/rivets/src/error.rs`
- All phantom target files have names that also exist as files in the *source* crate (`error.rs`, `mod.rs`, `lib.rs`, etc.)

Probe extension to the code: located the mechanism at `crates/tethys/src/resolve.rs:339` → `crates/tethys/src/db/symbols.rs:244`.

```sql
-- crates/tethys/src/db/symbols.rs:244 (search_symbol_by_name)
SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE name = ?1 LIMIT 1
```

No crate scope. When the symbol resolver hits its fallback path (`fallback_symbol_search`), it picks the first symbol of that name in the workspace, regardless of where the reference came from. The pipeline is:

```
unresolved reference  →  fallback_symbol_search
                      →  search_symbol_by_name  (LIMIT 1, unscoped — BUG)
                      →  refs row with wrong target symbol_id
                      →  populate_call_edges aggregates refs → call_edges
                      →  populate_file_deps_from_call_edges aggregates → file_deps
                      →  phantom cross-crate file_deps row
```

## The fix, in one sentence

`search_symbol_by_name` is called from a context that already knows which file (and therefore which crate) the unresolved reference came from. Pass that context in, prefer same-crate matches, and only fall back to other crates when no same-crate match exists.

## Claims

| # | Claim |
|---|-------|
| 1 | The bug lives in the fallback symbol search at `db/symbols.rs:244`: the query has no crate-scope predicate. |
| 2 | After the fix, when a same-crate symbol with matching name exists, the fallback returns it (not a different-crate symbol). |
| 3 | After the fix, on the rivets workspace, every ordered pair of crates that the oracle marks FORBIDDEN has zero file_deps edges. |
| 4 | After the fix, the two legitimate ordered pairs (rivets→rivets-jsonl, rivets-mcp→rivets) still have non-zero file_deps edges. |
| 5 | After the fix, intra-crate file_deps counts are unchanged (the fix is strictly cross-crate). |
| 6 | After the fix, when no same-crate symbol exists for a referenced name and multiple cross-crate symbols do, the fallback either returns the unique cross-crate match (when there is one) or returns None (when there is genuine ambiguity); it does NOT silently pick one. |

## Negative space — what this design will NOT do

- **Will not touch other resolution paths.** LSP-based resolution (Pass 3 in `resolve.rs`), qualified-name resolution (`get_symbol_by_qualified_name`), and direct file-resolver code in `resolver.rs` are out of scope. Only the by-name fallback is being changed.
- **Will not change the schema.** No new columns on `symbols`, no migrations. The crate scope is derivable from `files.path`.
- **Will not deduplicate the multi-crate `Error` situation.** Multiple `Error` types across crates is a valid Rust pattern; the resolver should accept it and pick correctly per call site, not reject it.
- **Will not address other rivets follow-ups.** rivets-c540 (fuzzy suggestions), rivets-ed9y (connection pool), rivets-qawf (SQL sort), rivets-fayv (orphan crate) are unrelated.

## Falsification table

| # | Claim | Falsifier | Oracle | Cost | Status |
|---|---|---|---|---|---|
| 1 | Bug at db/symbols.rs:244 | Inspect query string + count workspace-wide symbol-name collisions on live DB. | `.rivets-0gom/cheapest_falsifier.py` against live index | 10 min | **passed** (see below) |
| 2 | Fallback prefers same-crate match | Seed `symbols` with two `Foo` symbols in different crates. Call new fallback from a file in crate A. Expect: returns crate A's `Foo`, not crate B's. | Unit test on the new fallback function | 15 min | post-implementation |
| 3 | All FORBIDDEN pairs → 0 edges | Re-run `.rivets-0gom/probe.py` after fix. Compare to `.rivets-0gom/oracle.sh`. All 10 FORBIDDEN ordered pairs must show 0 edges. | The persistent oracle from prove-it-prototype | 5 min (after fix is in) | post-implementation |
| 4 | ALLOWED pairs still have edges | Same probe run; rivets→rivets-jsonl > 0, rivets-mcp→rivets > 0. | The persistent oracle | 0 min (same probe run) | post-implementation |
| 5 | Intra-crate edges unchanged | Extend probe to count intra-crate edges. Capture pre-fix snapshot. Re-run post-fix. Diff. | Pre/post comparison; the probe extension is independent of the fix | 20 min | pre-fix snapshot now, post-fix compare later |
| 6 | Unscoped fallback returns None or unique match | Unit test: seed 3 crates with `Bar` symbols, none in caller's crate. Test 1: only one cross-crate `Bar` exists → return it. Test 2: multiple cross-crate `Bar` → return None. | Unit tests on the new fallback | 20 min | post-implementation |

## Cheapest falsifier: run it now — done, result below

**1a. Code inspection.** `crates/tethys/src/db/symbols.rs:244` reads:
```sql
SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE name = ?1 LIMIT 1
```
No crate scope, no file scope, no caller context. Passes by inspection.

**1b. Live DB query** (`.rivets-0gom/cheapest_falsifier.py`):

```
NAME        COUNT  WHAT LIMIT 1 WOULD RETURN
Error           4  crates/rivets/src/error.rs:104
                   ALSO MATCHES (hidden by LIMIT 1): crates/rivets-jsonl/src/error.rs:8
                   ALSO MATCHES (hidden by LIMIT 1): crates/rivets-mcp/src/error.rs:7
                   ALSO MATCHES (hidden by LIMIT 1): crates/tethys/src/error.rs:32
Result          5  crates/rivets/src/error.rs:192
                   ALSO MATCHES (hidden by LIMIT 1): crates/rivets-jsonl/src/error.rs:23
                   ALSO MATCHES (hidden by LIMIT 1): crates/rivets-mcp/src/error.rs:70
                   ALSO MATCHES (hidden by LIMIT 1): crates/tethys/src/error.rs:25
                   ALSO MATCHES (hidden by LIMIT 1): crates/tethys/src/lsp/mod.rs:44
Warning         1  crates/rivets-jsonl/src/warning.rs:44
FileId          1  crates/tethys/src/types.rs:59
```

For `Error` and `Result`, 4 and 5 candidate symbols exist respectively. The `LIMIT 1` query picks the first by `id` (rivets's, indexed earliest). Every workspace reference to `Error` from any crate gets the same target back — including tethys's, where `use crate::error::Error` should resolve to tethys's own `Error`.

**Claim 1 confirmed.** Mechanism is exactly what the probe predicted: unscoped LIMIT 1 collapses common type names across crates to a single arbitrary winner.

## Design — what changes

Three pieces, in slice order:

### Slice 1: introduce crate-scoped variant

Add `search_symbol_by_name_in_crate(name: &str, crate_path_prefix: &str)` next to `search_symbol_by_name`. New function:

```sql
SELECT {SYMBOLS_COLUMNS}
FROM symbols s
JOIN files f ON f.id = s.file_id
WHERE s.name = ?1 AND f.path LIKE ?2 || '%'
LIMIT 1
```

The `LIKE 'crates/<name>/%'` predicate restricts to one crate's source tree. The caller passes the prefix derived from the caller's file path.

Old `search_symbol_by_name` stays. It's still used legitimately by some callers; we'll audit them in slice 2.

**Loop budget:** O(symbols in crate) with index on `symbols(name)`; ≤ ~10^4 symbols per crate on workspaces we care about; well within budget.

**Stress fixture:** two crates each with a `Foo` symbol; call new function with each crate's prefix; assert correct hits.

### Slice 2: route the fallback through crate scope

In `resolve.rs::fallback_symbol_search`, the caller has access to the current file's path (it's tracked at the call site). Derive the crate prefix from that path and pass it to the new function. Fall back to `search_symbol_by_name` (unscoped) only if the crate-scoped query returns None.

**Loop budget:** one extra query per fallback resolution. Bounded by the number of unresolved references. Already exists in this path; we're adding a single SQL call.

**Stress fixture:** two crates, each with `Foo`. Caller in crate A. Pre-fix: caller resolves to crate B's `Foo` (bug). Post-fix: caller resolves to crate A's `Foo`.

### Slice 3: handle the genuine-ambiguity case

If no same-crate match AND multiple cross-crate matches exist, return None instead of `LIMIT 1`. Use `LIMIT 2` and check: if exactly one row, return it; if two, ambiguous → None.

**Loop budget:** one row diff. Negligible.

**Stress fixture:** three crates with `Bar`. Caller has no `Bar`. With one cross-crate `Bar`: returns it. With two cross-crate `Bar`s: returns None.

## Plan output: gates that must pass before checkpointed-build runs

- [x] Probe and oracle agree on disagreement (`rivets-0gom` repro is stable)
- [x] Bug mechanism located (slice 1 site identified)
- [x] Six claims, each with a falsifier
- [x] Falsifiers ranked by cost
- [x] Cheapest falsifier (claim 1b) executed and passed (Error: 4 matches, Result: 5 matches, LIMIT 1 verified arbitrary)
- [x] Negative space enumerated (4 items)
- [ ] User approves design

## Notes for budgeted-plan (the next skill)

- Slice 1 and 2 must land together for any test against the rivets oracle to be meaningful; intra-slice TDD per `tdd-scoped`.
- After slice 2 lands, re-run `.rivets-0gom/probe.py` and compare to oracle — that's the ORACLE gate for the slice.
- Slice 3 is independent and can land after the rivets oracle agreement is confirmed.
- The probe and oracle are the persistent gate for the entire fix. No slice is "done" until the probe agrees with the oracle on the rivets workspace at that slice's point.
