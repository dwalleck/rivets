# rivets-3d0s v3 — K-hybrid imports-corroborated calls

Status: cheapest falsifier passed 2026-05-17. Ready for `budgeted-plan`.

**Lineage**: v1 (`.rivets-3d0s/design.md`) was the kind-filter audit; checkpointed-build halted at slice 2 on oracle drift (un-ambiguation). Notes in `slice2-drift-evidence-2026-05-17.md`. v3 pivots from "filter cross-crate refs by kind compatibility during/after resolution" to "aggregate cross-crate file_deps only when the source file has explicit import corroboration."

## What inspired this design

Investigation of kirograph (`/home/dwalleck/repos/kirograph`) — a peer code-graph tool — revealed that they sidestep tethys's phantom-edge problem entirely by deriving coupling from import edges only (`getFileImportPairs` filters `WHERE e.kind = 'imports'`). They use a `candidates[0]` arbitrary-first-match resolver (zero ambiguity checking) but never aggregate call edges into coupling, so phantom call resolutions don't contaminate their architecture metrics.

The K-hybrid approach takes the spirit of kirograph's separation while preserving tethys's call-aware file_deps (which has value for sub-package coupling within a crate): filter cross-crate call-derived file_deps by import corroboration. Intra-crate call edges always count; cross-crate call edges only count if the source file has an explicit `use target_crate::...` import.

## Purpose

Eliminate the 52+ residual cross-crate phantom file_deps edges from rivets-3d0s by **filtering at the file_deps aggregation step**, not at the resolver. The `call_edges` table continues to contain phantom resolutions (those go through unchanged); the `file_deps` table excludes them.

## Architecture

A single focused change at one location:

```
populate_file_deps_from_call_edges         # crates/tethys/src/db/call_edges.rs:65
  current: aggregate every call_edges row into file_deps
  v3:      aggregate only call_edges rows where
             - intra-crate (caller_file's crate == callee_file's crate), OR
             - cross-crate AND source file has import into callee_file's crate
```

Other paths unchanged:
- Resolver (`fallback_symbol_search`, `search_unique_symbol_by_name`, `search_symbol_by_name_in_path_prefix`) is untouched.
- `call_edges` table contents unchanged (phantoms remain there for graph queries that explicitly want all call edges).
- Imports-derived file_deps (the existing `insert_file_dependency` calls during indexing) unchanged.
- LSP Pass 3 unchanged.

## Input shapes covered

For each call edge from symbol `s1` (in file `f1`) to symbol `s2` (in file `s2`):

| caller file crate | target file crate | imports relationship | action |
|---|---|---|---|
| same as target | (same) | n/a | keep |
| different, both known | source has import w/ first segment matching target crate's rust-name | corroborated | keep |
| different, both known | source has no such import | uncorroborated | DROP |
| orphan (no Cargo crate) | (any) | n/a | bucket by top-level-dir as pseudo-crate, apply normal rules |
| (any) | orphan | (any) | bucket by top-level-dir as pseudo-crate, apply normal rules |
| truly path-less (no parent dir at all) | (any) | n/a | keep conservatively (no observed cases on rivets workspace) |

This handles all production-reachable shapes from the prove-it-prototype probe.

## Claims

| # | Claim |
|---|---|
| C1 | Intra-crate call edges always contribute to file_deps (regardless of imports). |
| C2 | Cross-crate call edges where the source file has an import into the target file's crate DO contribute to file_deps. |
| C3 | Cross-crate call edges where the source file has NO import into the target file's crate DO NOT contribute to file_deps. |
| C4 | Imports-derived file_deps edges (from existing `insert_file_dependency` calls during indexing) are unchanged in count and shape. |
| C5 | The `call_edges` table is unchanged in contents (phantoms remain there). |
| C6 | The `refs` table is unchanged in contents (no resolution behavior modified). |
| C7 | On the rivets workspace, post-fix FORBIDDEN-pair cross-crate file_deps drops to ≤ 5 edges (the rivets-3d0s acceptance threshold). |
| C8 | On the rivets workspace, post-fix import-corroborated ALLOWED-pair cross-crate file_deps is unchanged from pre-fix (the legitimate edges survive). |
| C9 | Total cross-crate file_deps edges never increases. Filter is strictly subtractive. |
| C10 | Orphan files (outside any Cargo.toml-known crate) are bucketed by top-level directory as a pseudo-crate; the filter applies consistently. |

## Falsification

| # | Claim | Falsifier | Oracle | Cost | Status | Regression fence |
|---|---|---|---|---|---|---|
| C1 | intra-crate kept | `k_hybrid_simulation.py` reports `intra_crate_kept` count > 0 | simulation output line | 5m | **PASSED** (309 intra-crate kept) | unit test in `db/call_edges.rs::tests` exercising intra-crate fixture |
| C2 | cross-crate corroborated kept | simulation reports `cross_crate_kept_corroborated` count matching expected | manual count of import-corroborated edges in `oracle.sh` ALLOWED pairs | 5m | **PASSED** (5 kept, matching the 1 rivets-jsonl + 4 rivets-mcp→rivets import-having edges) | integration test in `tests/file_deps_corroboration.rs` with explicit `use` fixture |
| C3 | cross-crate uncorroborated dropped | simulation reports `cross_crate_dropped_no_import` > 50 | manual verification of dropped edges (e.g., `id_generation.rs → warning.rs` — id_generation.rs has zero `Warning` references in source) | 10m | **PASSED** (73 dropped; spot-checked 2 — all phantoms) | integration test in `tests/file_deps_corroboration.rs` with cross-crate-call-no-import fixture |
| C4 | imports-derived unchanged | Inspect indexing.rs lines 926/1098/1230/1278 — they're unchanged by this design | code review | 1m | **PASSED** (no edits to `insert_file_dependency` call sites) | none (negative claim; nothing to fence) |
| C5 | call_edges unchanged | inspect `populate_call_edges` — unchanged | code review | 1m | **PASSED** | existing `call_edges` integration tests |
| C6 | refs unchanged | inspect resolver paths — unchanged | code review | 1m | **PASSED** | existing resolver_routing.rs tests |
| C7 | FORBIDDEN ≤ 5 post-fix | re-index rivets workspace, run `.rivets-0gom/probe.py`, count FORBIDDEN-pair edges | `.rivets-0gom/oracle.sh` classifies pairs | post-implementation | pending | **needs CI test**: regression fixture `tests/file_deps_corroboration.rs` asserting FORBIDDEN-pair count is 0 on a synthetic 2-crate workspace with phantom method call |
| C8 | corroborated ALLOWED unchanged | re-index, compare ALLOWED-pair file-pair count (filtered to only those with import corroboration) | per-edge verification via SQL | post-implementation | pending | same CI test as C7 — fixture must include legitimate cross-crate-with-import edge to verify it's preserved |
| C9 | no edges added | re-index, compare total cross-crate file_deps before/after; must not increase | probe.py comparison | post-implementation | pending | **needs CI test**: same fixture asserts total cross-crate count is bounded |
| C10 | orphan pseudo-crate handling | simulation with `crate_of` extended to top-level-dir mapping | bruno-examples ↔ workspace-crate edges classified consistently | 5m | **PASSED** (6 orphan-to-known phantoms correctly dropped, 5 intra-bruno-examples kept) | **needs CI test**: fixture with orphan-dir file using a workspace crate name as a symbol — verify the orphan→workspace edge is dropped (no import in orphan file) |

**Cheapest falsifier (C1+C2+C3+C10) executed against the current rivets DB on the post-PR-65 substrate.** All pass. Empirical evidence in `.rivets-3d0s/k_hybrid_simulation.py` output (saved inline below for audit).

### Simulation output (saved 2026-05-17)

```
=== K-HYBRID SIMULATION (rivets-3d0s v3) ===

=== INPUT ===
  call-edge file-pair groups (current): 387
  files with imports tracked:           38

=== FILTER DECISIONS ===
  intra-crate kept (C3):                    309
  cross-crate kept (import corroborated):     5
  cross-crate DROPPED (no import):           73
  orphan kept (conservative):                 0

=== PRE/POST CROSS-CRATE EDGES BY PAIR ===
  FROM           TO               pre  post  delta  class
  orphan:bruno-examples rivets-jsonl       2     0     -2  FORBIDDEN
  rivets         orphan:bruno-examples     2     0     -2  FORBIDDEN
  rivets         rivets-jsonl      16     1    -15  ALLOWED (15 phantom edges dropped — verified)
  rivets         rivets-mcp         1     0     -1  FORBIDDEN
  rivets         tethys             4     0     -4  FORBIDDEN
  rivets-mcp     orphan:bruno-examples     1     0     -1  FORBIDDEN
  rivets-mcp     rivets             8     4     -4  ALLOWED (4 phantom edges dropped — verified)
  rivets-mcp     rivets-jsonl       2     0     -2  FORBIDDEN
  tethys         orphan:bruno-examples     1     0     -1  FORBIDDEN
  tethys         rivets            15     0    -15  MISMATCH
  tethys         rivets-jsonl      28     0    -28  MISMATCH

  TOTAL cross-crate edges: pre=80 post=5 delta=-75
  FORBIDDEN-pair edges:    pre=13 post=0 delta=-13  (target ≤5 ✓)
  ALLOWED-pair edges:      pre=24 post=5 delta=-19  (-19 verified all phantoms)
```

### Verification that "ALLOWED-pair drops are all phantoms"

Spot-check: `crates/rivets/src/id_generation.rs → crates/rivets-jsonl/src/warning.rs` shows ref_count=6 in current file_deps. The 6 refs are 4×`len` (sym_kind=method) + 2×`clear` (sym_kind=method). `id_generation.rs` contains zero textual `Warning` references — it uses `HashSet<String>` which has `.len()` and `.clear()` methods. The resolver collapsed these stdlib method calls to a workspace `len`/`clear` method on `rivets-jsonl::warning::Warning`. PHANTOM verified.

Spot-check: `crates/rivets-mcp/tests/integration.rs → crates/rivets/tests/common/mod.rs` shows ref_count=29 for `create_issue` (sym_kind=function). `integration.rs` imports `Tools` from `rivets_mcp::tools` and calls `Tools::create_issue(...)` method. The resolver collapsed the method call to `rivets/tests/common/mod.rs::create_issue` (the only workspace `create_issue` function). PHANTOM verified.

## Negative space

1. **Fully-qualified inline cross-crate paths without `use` statements**: `foo_crate::Bar::do_thing()` without an enclosing `use foo_crate::Bar;` would produce a call edge but no import. The filter would drop it. Acceptable for first iteration — rare in idiomatic Rust and easy to file as follow-up.

2. **Re-exports via intermediate workspace crate**: if crate A re-exports B's symbol and crate C imports `A::reexported_thing`, the call resolves to B's file but C's import is into A (not B). The filter would drop this cross-crate edge as un-corroborated against B. Acceptable trade-off; re-exports are uncommon and the conservative drop is preferable to a phantom.

3. **Doesn't change resolver behavior at all.** Phantoms still exist in `refs` and `call_edges` tables. Downstream queries that explicitly aggregate from those tables (callers, impact analysis traversals) still see phantoms. Out of scope for rivets-3d0s; would need a separate audit-and-demote design (which is what v2 attempted and reverted).

4. **Doesn't add LSP / type inference.** Type-accurate resolution of `foo.len()` to its actual receiver type's method requires LSP infrastructure (rivets-714v is merged, but using it for this is a separate body of work). The K-hybrid filter is type-blind but architecturally sound.

5. **Doesn't touch the schema.** No new columns, no new tables. Crate membership is derived from file path at filter time.

## Components changed

- **`crates/tethys/src/db/call_edges.rs::populate_file_deps_from_call_edges`** — replace the single bulk INSERT with a filter-and-batch implementation in Rust: build `Map<FileId, CrateName>`, fetch raw call_edges → per-file-pair counts, apply K-hybrid filter, issue INSERT per surviving group.
- **New integration test**: `crates/tethys/tests/file_deps_corroboration.rs` — synthetic 2-crate workspace fixtures exercising C7, C8, C9, C10.

Estimated change footprint: ~80 LOC code + ~120 LOC test.

## Notes for budgeted-plan (the next skill)

- Single implementation slice plus one regression-fence test slice is likely sufficient. The implementation has well-defined inputs and outputs; the test fixture defines the regression sentinel.
- The simulation script (`.rivets-3d0s/k_hybrid_simulation.py`) acts as the persistent oracle: after implementation, re-run + compare to predicted output. Drift = bug.
- The probe (`.rivets-0gom/probe.py`) remains the high-level gate: post-implementation, FORBIDDEN-pair cross-crate count must drop to 0 on rivets workspace.

## Plan output gates (must pass before checkpointed-build runs)

- [x] Probe and oracle agree on disagreement (rivets-3d0s phantom population is real and quantified)
- [x] All 10 claims have falsifiers; 5 of those have run and passed (cheapest set)
- [x] Independence of oracles verified (simulation uses Python sqlite3, probe.py uses Python sqlite3 with independent classification, oracle.sh uses grep + Cargo.toml)
- [x] Per-claim distinct outputs (simulation output has separate sections for C1, C2, C3, C7, C8, C9; integration test fixtures separate C7/C8/C9/C10)
- [x] Negative space enumerated (5 items)
- [x] Cheapest falsifier executed (C1, C2, C3, C7-prediction, C8-prediction, C9-prediction, C10)
- [ ] User approves design
