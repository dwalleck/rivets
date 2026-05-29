# What I learned — rivets-wsix prove-it-prototype

## One-sentence summary

The schema's `ON DELETE CASCADE` chain plus per-file `DELETE FROM symbols WHERE file_id` is the actual safety net for re-index correctness across most tables — not the `clear_all_X` pattern lcb6 established for `file_deps` and `call_edges`. The wsix issue's mental model ("UPSERT-only ⇒ needs `clear_all_X`") was a special case, not the general principle.

## Per-table inventory after three probe expansions

| Table | Insert shape | Re-index safety mechanism | Audit result |
|---|---|---|---|
| `call_edges` | UPSERT `ON CONFLICT DO UPDATE` | `clear_all_call_edges` at `indexing.rs:439` | ✓ Safe |
| `file_deps` | UPSERT `ON CONFLICT DO UPDATE` | `clear_all_file_deps` at `indexing.rs:139` (lcb6 fix) | ✓ Safe |
| `imports` | UPSERT `INSERT OR REPLACE` | `DELETE FROM imports WHERE file_id` at `files.rs:146` before per-file re-insert | ✓ Safe |
| `symbols` | Plain `INSERT` | `DELETE FROM symbols WHERE file_id` at `files.rs:145` | ✓ Safe |
| `attributes` | Plain `INSERT` | Cascade from `symbols(id) ON DELETE CASCADE` when symbols are wiped per file | ✓ Safe |
| `refs` | Plain `INSERT`, no explicit clear | Cascade via `refs.in_symbol_id REFERENCES symbols(id) ON DELETE CASCADE` (and the `symbol_id` FK for same-file refs) when the containing file's symbols are wiped | ✓ Safe per probe |
| `files` | `INSERT` after `SELECT id WHERE path = ?` existence check | UPSERT-by-existence-check; row id is reused so cascade-dependent rows keep stable file_id | ✓ Per-file safe |
| `arch_packages` | Plain `INSERT` | `DELETE FROM arch_packages` at start of `repopulate_architecture` (architecture.rs:61); cascade clears the two child tables | ✓ Safe |
| `arch_file_packages` | Plain `INSERT` | Cascade from `arch_packages(id)` | ✓ Safe |
| `arch_package_deps` | Plain `INSERT` | Cascade from `arch_packages(id)` | ✓ Safe |

**Zero new bugs found.** wsix's mental model expected the file_deps shape (monotonic aggregate growth) on other tables. Empirically, none of them exhibit it.

## What surprised me

1. **The `refs` table has no explicit clear and no UPSERT, yet handles re-indexing correctly.** My initial code reading said "this should accumulate." Probe disproved that. The cascade chain via `in_symbol_id → symbols(id)` is wider than I appreciated — when a file's symbols are wiped, *every* ref contained in those symbols cascade-deletes, regardless of whether the ref's target is in the same file.

2. **Tethys's rust extractor doesn't generate refs for `mod X;` declarations.** I expected `mod helper_a; mod helper_b;` to produce 2 file-scope refs (in_symbol_id IS NULL). The probe showed 0. So the in_symbol_id IS NULL edge case — which would have been the only path stale refs could persist — doesn't exist in practice. (This may be worth a separate audit if `extern crate X;` or top-level type aliases produce file-scope refs that mod declarations don't; out of scope for wsix.)

3. **`imports` uses BOTH `INSERT OR REPLACE` AND a per-file `DELETE`.** Belt-and-suspenders. The DELETE alone would be sufficient; the `OR REPLACE` covers a narrower edge case (same-file duplicate `use` statements). Not a bug, just redundant. Worth noting if anyone simplifies.

4. **The wsix issue's classification (a/b/c) missed a fourth class**: tables that are correct *because of schema cascade*, not because of explicit clear-or-UPSERT logic. The `refs`, `attributes`, and `symbols`-via-files-cascade tables all live there.

## Probe / oracle independence

- **Initial probe**: regex grep on `db/*.rs` for `INSERT ... ON CONFLICT DO UPDATE` paired with `fn clear_all_*`. Assumed 1 table per file.
- **Oracle 1**: enumerate `CREATE TABLE` statements from `schema.rs`, then grep `INSERT INTO <table>` across `crates/tethys/src/`. Caught the multi-table `files.rs`, the `refs`-vs-`references.rs` naming mismatch, and the `arch_*` family.
- **Probe 2 (empirical, refs accumulation)**: run `tethys index --workspace .` twice on a tempdir fixture, then thrice with source mutations, comparing `SELECT COUNT(*) FROM refs WHERE file_id = ?`. Disproved my "refs accumulates" hypothesis.
- **Probe 3 (empirical, in_symbol_id IS NULL case)**: same shape, fixture with `mod helper_a; mod helper_b;` at file scope. Showed extractor doesn't produce these refs.

Probe-vs-oracle disagreement on the *table inventory question*: probe under-detected (file-name assumption). Resolved by adopting the oracle's view as canonical for the inventory. Then probes 2 and 3 directly verified empirical behavior on the tables the oracle inventory raised concerns about.

## Implication for the gilfoyle loop

The audit found no bugs to fix. The remaining value is **regression fences** that lock in the audited correctness properties so they survive future schema or indexing changes. Candidates:

- Test: re-index without source change → `refs` count for that file is unchanged (not doubled). Pins the cascade.
- Test: remove a function from a file, re-index → its refs disappear (not orphan). Pins the per-file cascade chain.
- Test: remove `mod X;` then re-index → no orphan refs to X. Pins the (currently empty) file-scope case.
- Test: each non-UPSERT INSERT site (`refs`, `attributes`, `symbols`, `files`) has either a documented cascade path OR a per-file clear. Could be an architecture test that fails CI if a new INSERT is added without a corresponding clear/cascade declaration.

The falsifiable-design step should choose which of these fences to ship.

## What's NOT in this audit

- **Orphan-file behavior** (file deleted from disk, files.id persists): out of scope for wsix per its explicit framing as a sibling of lcb6 (UPSERT growth class). Tracked separately as **rivets-dhxo**.
- **Streaming-mode (`IndexOptions::with_streaming()`) indexing**: only tested the default full-index path. dhxo's analysis suggests streaming has its own divergent behavior here.
- **Concurrent re-index**: only single-threaded `tethys index` runs. SQLite's `busy_timeout` (per CLAUDE.md) protects against concurrent writers but cascade ordering under concurrent reads/writes wasn't probed.
- **Schema migration scenarios**: a future schema change that drops or alters a cascade FK could silently introduce the bug class wsix was looking for. The regression fences should be schema-aware enough to catch that.
