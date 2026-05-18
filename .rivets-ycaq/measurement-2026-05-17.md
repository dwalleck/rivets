# rivets-ycaq acceptance-criterion #6 measurement

**Date:** 2026-05-17
**Workspace:** `C:\Users\dwall\repos\rivets`
**Commit:** `66ab9fa` (main, post-PR-#67 merge)
**Tethys release binary:** built `2026-05-17 18:59:21`
**Indexed:** 121 files, 2640 symbols, 22145 references, 20.84s (no `--lsp`)

## Acceptance text

> Re-measured baseline on rivets workspace shows:
> 1. workspace cross-crate edges match Cargo dep graph;
> 2. cross-file resolved count is stable across two consecutive index runs;
> 3. phantom-edge rate < 1%.

## Result: all three sub-clauses PASS

### 1. Cross-crate edges match Cargo dep graph

Source: `.rivets-0gom/probe.py` + `.rivets-ycaq/probe_phantom_rate.py`.

| metric | pre-PR-#67 baseline | post-PR-#67 |
|---|---|---|
| total `file_deps` rows | 398 | **330** |
| cross-crate distinct pairs | 8 | **2** |
| cross-crate edges (sum) | 74 | **8** |
| FORBIDDEN-pair (no Cargo dep) edges | 14 | **0** |

The 8 surviving cross-crate edges all live on ALLOWED Cargo-dep pairs:

```
rivets-mcp -> rivets        6 edges
rivets     -> rivets-jsonl  2 edges
```

These match Cargo dep declarations (`rivets-mcp` depends on `rivets`;
`rivets` depends on `rivets-jsonl`). No FORBIDDEN pairs survive.

### 2. Cross-file resolved count stable across two consecutive index runs

Source: re-run of the `--rebuild` path, checked against `file_deps` row +
ref_count sums.

```
after run 1 (rebuild): 330 rows, 2617 ref_count_sum
after run 2 (rebuild): 330 rows, 2617 ref_count_sum
IDEMPOTENT
```

This is rivets-lcb6's clear-before-rebuild fix doing its job. Pre-lcb6,
`file_deps` accumulated stale rows across runs (UPSERT-on-conflict
incremented `ref_count` indefinitely).

### 3. Phantom-edge rate < 1%

Source: `.rivets-ycaq/probe_phantom_rate.py` (this directory).

A cross-crate `file_deps` edge is "phantom" iff the source file has no
`use` import into the target file's crate. The K-hybrid filter at
`crates/tethys/src/db/call_edges.rs::populate_file_deps_from_call_edges`
drops uncorroborated cross-crate edges at the aggregation step.

| metric | value |
|---|---|
| cross-crate edges | 8 |
| corroborated by `use` import | 8 |
| phantom (no `use` import) | **0** |
| **phantom rate** | **0.00%** |

Far below the 1% threshold. PR-#67 over-delivered on the bar ycaq set.

## What this measurement does NOT claim

The K-hybrid design contains phantoms at the `file_deps` boundary, not in
the `refs` table. Re-running `.rivets-3d0s/probe.py` (which measures the
**`refs`** table, not `file_deps`):

```
Phantom cross-crate resolved refs (FORBIDDEN pairs): 202
  93 method, 47 struct_field, 29 enum_variant, 25 module, 7 function, 1 struct
```

This is **by design** (claims C5/C6 in `.rivets-3d0s/design-v3-k-hybrid.md`:
"`call_edges` table is unchanged. `refs` table is unchanged"). Pre-PR-#67's
attempt to clean refs (slice 2 of the original kind-filter design) was
reverted because of the un-ambiguation dynamic documented in
`.rivets-3d0s/slice2-drift-evidence-2026-05-17.md`.

The ycaq epic's correctness frame is "downstream consumers see clean
data", measured at the `file_deps` boundary that coupling/dependencies
read from. Per-ref correctness in the resolver is a separate (and harder)
problem outside ycaq's scope.

Anyone re-running the probes and seeing "202 phantom refs" should not
conclude the fix failed — they should read this section and
design-v3-k-hybrid.md C5/C6.

## Reproduction

```powershell
git checkout main
git pull --ff-only
cargo build --release --bin tethys
./target/release/tethys.exe index --rebuild
python .rivets-ycaq/probe_phantom_rate.py
```

For the idempotency check:

```powershell
./target/release/tethys.exe index --rebuild
sqlite3 .rivets/index/tethys.db "SELECT COUNT(*), SUM(ref_count) FROM file_deps;"
./target/release/tethys.exe index --rebuild
sqlite3 .rivets/index/tethys.db "SELECT COUNT(*), SUM(ref_count) FROM file_deps;"
# Both queries should print identical results.
```

## Outstanding ycaq acceptance items (not satisfied by this measurement)

ycaq has 6 acceptance criteria. This measurement satisfies #6. The other
five gate the epic's closure:

- [x] rivets-lcb6 (closed in PR #65)
- [x] rivets-0gom (closed in PR #66)
- [x] rivets-3d0s (closed in PR #67)
- [ ] rivets-dn35 (Pass-2 short-circuit on import-less files) — still open
- [ ] rivets-i8qn (CrateInfo accessors) — still open
- [ ] rivets-6jxv (dedupe `crate_root_for_file` helper) — still open
- [x] **#6: re-measured baseline meets the three sub-clauses — done by this artifact**

The three remaining issues are intentionally narrow (one resolver bug, two
refactor-only cleanups). They don't change the measurement above — the
correctness incident is empirically contained.
