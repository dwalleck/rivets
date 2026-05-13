# rivets-lcb6 — Clear file_deps between index runs

## Problem

`file_deps` is written via UPSERT-only across two phases of `index_with_options`:

1. **Per-file dependency computation** — `compute_dependencies` / `compute_dependencies_from_stored` in `crates/tethys/src/indexing.rs` calls `insert_file_dependency` at lines 911, 1083, 1215, 1263. Uses `ON CONFLICT(from_file_id, to_file_id) DO UPDATE SET ref_count = ref_count + 1`.
2. **Call-edge aggregation** — `populate_file_deps_from_call_edges` in `db/call_edges.rs` at line 67 also UPSERTs into `file_deps`, aggregating from the just-populated `call_edges` table. Called from `indexing.rs:432`.

Without an intervening `DELETE FROM file_deps`, every `tethys index` run leaves stale rows from prior runs in the table. The empirical evidence is documented in rivets-0gom: during slice 2, the probe showed identical phantom-edge counts pre- and post-resolver-fix until the DB was manually wiped, because old phantoms persisted.

Compare with `call_edges`, which is cleared at `indexing.rs:424` via `clear_all_call_edges` (`db/call_edges.rs:13`) immediately before `populate_call_edges`. `file_deps` has no analog.

## Fix

Add `clear_all_file_deps()` to `db/file_deps.rs` mirroring `clear_all_call_edges`:

```rust
pub fn clear_all_file_deps(&self) -> Result<()> {
    trace!("Clearing all file deps");
    let conn = self.connection()?;
    conn.execute("DELETE FROM file_deps", [])?;
    Ok(())
}
```

Call it from `index_with_options` **at the start**, after workspace discovery and before any per-file processing. This is earlier than `clear_all_call_edges`'s position (line 424) because `file_deps` is written in phase 1 (`compute_dependencies` per file), whereas `call_edges` is written only in phase 2 (`populate_call_edges` at line 425).

## Input shapes

| Path | Current state | Post-fix |
|---|---|---|
| `tethys index --rebuild` | `db.reset()` deletes DB file first → file_deps already empty when index_with_options runs. Existing behavior is correct by accident. | Clear is a no-op on an already-empty table. Still safe. |
| `tethys index` (no flag, no incremental support yet) | Reuses existing DB. File_deps accumulates stale rows. **Buggy path.** | Clear wipes prior rows before re-population. Fixed. |
| First-ever index (DB doesn't exist yet) | `db.open` creates schema, table empty. | Clear is a no-op. Safe. |
| Future `tethys index --incremental` (rivets-bxom) | Doesn't exist yet. | Wholesale clear is **wrong** for incremental — should be scoped to changed files. rivets-bxom will need to handle this when it lands. Acceptance criterion #4 already captures this. |

## Regression fences

Per acceptance criteria:

1. **Idempotency test**: index the rivets workspace twice. `SELECT COUNT(*) FROM file_deps` must be identical across runs. Pre-fix: increases. Post-fix: stable.
2. **Edge-removal test**: index a 2-file fixture where file A `use`s file B. Verify edge exists. Modify A to remove the `use`. Re-index. Verify edge is gone.

Both go in `crates/tethys/tests/file_deps_idempotency.rs` (new file), mirroring the structure of `lsp_multi_crate.rs`. NOT `#[ignore]`'d — these are cheap and shouldn't require any external tooling.

## Out of scope

- Per-file scoped clear (acceptance criterion #4 — defer to rivets-bxom).
- Migrating other UPSERT-only tables to clear-then-insert if any exist (haven't audited; not the bug being fixed here).
- Changes to the schema or the UPSERT logic itself — the UPSERT is correct *within* a run for handling duplicate edges from multiple references; the bug is purely the missing inter-run clear.
