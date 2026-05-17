# Related issues — rivets-0gom cluster

Step 0 artifact for the prove-it-prototype skill: known prior art / sibling
work that bears on the rivets-0gom resolver-phantom-edges fix and its
post-lcb6 re-measurement.

## The resolver-correctness epic chain

```
rivets-ycaq (epic, P2, open)
├── rivets-lcb6 (P2, bug)         file_deps never cleared between runs
│                                  ✓ PR #65 merged (94226ce, 2026-05-17)
│                                  status=in_progress in tracker — needs manual close
│
├── rivets-0gom (P2, bug)         phantom cross-crate edges from shared filenames
│                                  ◐ PR #61 shipped slices 1-3 (search_symbol_by_name_in_path_prefix,
│                                    same-crate preference, ambiguity rejection)
│                                  ◐ Awaiting post-lcb6 verification on a clean DB
│                                  ◐ THIS ITERATION's question: is anything 0gom-class
│                                    still happening, or are all residuals 3d0s-class?
│
├── rivets-3d0s (P2, bug)         residual phantoms from stdlib/external name collisions
│                                  ○ Blocked by 0gom; not started
│                                  ○ Targets ~52 phantom edges remaining post-slice-3
│                                    (mostly `len`, `chi…`, common Rust names that have
│                                     a unique workspace match which is still wrong)
│
└── rivets-dn35 (P3, bug)         Pass 2 short-circuits on import-less files
                                   ○ Independent of the 0gom→3d0s chain
                                   ○ resolve_refs_for_file returns early when
                                     imports.is_empty(), so files with no `use`
                                     statements never get Pass-2 fallback resolution
```

## What this iteration is checking

After PR #65 (lcb6 fix), the `file_deps` table is no longer polluted by
stale rows from prior `tethys index` runs. The previous post-slice-3
snapshot (`after-review-fixes.txt` / `after-slice3.txt`, both 782 bytes,
captured during PR #61 review) was taken on a **polluted DB**. The numbers
in that snapshot:

  - 73 cross-crate (from, to) pairs with edges
  - 0 ambiguity violations
  - ~51 phantom edges in oracle-FORBIDDEN or oracle-MISMATCH pairs

The smallest question for this iteration: **does a fresh post-lcb6 index
produce the same numbers, or do they shift?**

- If **same**: the residuals are genuinely re-created each run by the
  rivets-3d0s class (unique-workspace-match fallback). 0gom is done as
  scoped; close it. The remaining work belongs to rivets-3d0s.
- If **lower**: a portion of those edges were stale cross-run residue
  that lcb6 has now cleared. Sharpens the 3d0s scope.
- If **higher**: surprise; new bug introduced between PR #61 and now.

## Adjacent issues NOT in this cluster

Filed during PR #65 multi-agent review but unrelated to 0gom mechanics:

- **rivets-ml05** (open, P2) — non-transactional clear+repopulate window.
  Pre-existing pattern from lcb6/call_edges; not load-bearing for 0gom.
- **rivets-dhxo** (open, P3) — orphan-file streaming bug surfaced during
  lcb6 review; about deleted-from-disk target files producing stale
  refs, not about resolver scoping.
- **rivets-zoi3** (open, P2) — expanded file_deps test coverage. Future
  fixture work; not load-bearing here.
- **rivets-wsix** (open, P2) — audit other UPSERT-only tables. Process
  follow-up, not 0gom-related.
- **rivets-1v2b** (open, P3) — CancellationToken plumbing. Adjacent.

## Pointer to the persistent probe and oracle

- Probe: `.rivets-0gom/probe.py` — reads `.rivets/index/tethys.db` via
  stdlib `sqlite3`. Outputs cross-crate / intra-crate / ambiguity sections.
- Oracle: `.rivets-0gom/oracle.sh` — reads each crate's `Cargo.toml` +
  greps for `use crate_name::` in source. Outputs ALLOWED / FORBIDDEN /
  MISMATCH per ordered pair.
- Residual classifier: `.rivets-0gom/diagnose_residual.py` — for the 6
  ordered pairs that had residual phantoms post-slice-3, prints the
  per-symbol-name distribution to identify the unique-workspace-match
  signature.
- Historical snapshots: `baseline-pre-fix.txt` (1175 bytes, pre-PR-61),
  `after-slice{1,2,2-clean,2-fixed,3}.txt`, `after-review-fixes.txt`,
  `after-round-2.txt`. All captured on polluted DB.

New snapshot for this iteration will be `after-lcb6-merge.txt`.
