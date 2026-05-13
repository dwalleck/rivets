# rivets-6aoc / rivets-34tv / partial rivets-i8qn — falsifiable design

## Purpose

Make tethys's Pass-2-imports resolution functional for `crate::*` paths in the rivets workspace. The current `crate_root = workspace_root.join("src")` hardcoding at four call sites in tethys points at a directory that doesn't exist (the rivets workspace root has no `src/`), so Pass-2-imports for `crate::*` is currently a complete no-op — 327 of 330 per-symbol `crate::*` imports leak through to Pass-2-fallback or remain unresolved.

## Architecture

### New components

**`CrateInfo::src_root(&self) -> PathBuf`** — encapsulates "what is this crate's source root directory?" Derivation:

```
src_root = match self.lib_path {
    Some(rel) => self.path.join(rel.parent().unwrap_or(Path::new(""))),
    None      => self.path.join("src"),
}
```

This is the partial-`rivets-i8qn` deliverable. The other two accessors from that issue (`entry_point_file`, `module_name`) stay out of scope.

**Per-file crate lookup** — given a file path, find which `CrateInfo` it belongs to. Path-prefix search through `self.crates`. The existing private helper `compute_module_path_for_file` (in `lib.rs:152`) already does this internally; this design factors out the lookup into a shared accessor.

### Call-site changes

All four hardcode sites switch from "workspace-wide" to "per-file" crate root:

| # | Site | Pre-fix | Post-fix |
|---|------|---------|----------|
| 1 | `resolve.rs::resolve_cross_file_references:66` | `let crate_root = self.workspace_root.join("src");` (computed once outside file loop) | crate_root computed per file via per-file lookup → `crate.src_root()` |
| 2 | `indexing.rs::compute_dependencies:~857` | Same pattern | Same fix |
| 3 | `indexing.rs::compute_dependencies_from_stored:~1023` | Same pattern | Same fix |
| 4 | `resolver.rs::resolve_module_path:61` (workspace-crate arm) | `target.path.join("src")` | `target.src_root()` |

## Claims

| # | Claim |
|---|---|
| C1 | `CrateInfo::src_root()` returns `crate.path.join(lib_path.parent())` when `lib_path = Some(rel)` and `rel.parent()` is non-empty. |
| C2 | `CrateInfo::src_root()` returns `crate.path.join("src")` when `lib_path = None`. |
| C3 | For every crate in the rivets workspace, the proposed `src_root()` logic produces a directory that exists on disk. |
| C4 | The per-file crate lookup helper produces exactly one matching `CrateInfo` for every `.rs` file under any `crates/X/src/` path; zero matches for files outside any crate's `src/` (e.g., `bruno-examples/`); never two matches. |
| C5 | After the fix, with Pass-2-fallback disabled, the number of `crate::*`-derived references resolved by Pass-2-imports is ≥ the probe∩oracle baseline (104 distinct (file, first-segment) migrating pairs) scaled by avg refs/import. |
| C6 | The fix does not reduce the total resolved-ref count (`SELECT COUNT(*) FROM refs WHERE symbol_id IS NOT NULL`). |
| C7 | `tethys index` wall-clock time on the rivets workspace does not increase by more than 10% post-fix. |

## Falsification

| # | Falsifier | Independent oracle | Cost | Status |
|---|-----------|--------------------|------|--------|
| C1 | Synthetic `CrateInfo { path: PathBuf::from("/foo"), lib_path: Some("src/lib.rs".into()), bin_paths: vec![] }`. Assert `.src_root() == PathBuf::from("/foo/src")`. | Hand-computed expected path. | 5m | pending |
| C2 | Same as C1 but `lib_path: None`. Assert `.src_root() == PathBuf::from("/foo/src")`. | Hand-computed expected path. | 5m | pending |
| C3 | Parse each rivets-workspace `Cargo.toml`, apply proposed `src_root()` logic, check directory exists on disk. | Filesystem `ls` of each crate's `src/` (mechanism: glob, not Cargo.toml parse). | 5m | **passed** (`.rivets-6aoc/cheapest_falsifier.py`) |
| C4 | For each of the 86 `.rs` files in `crates/*/src/`, run prefix-match against `self.crates` (simulated in Python). Assert exactly one match. Also test bruno-examples/parser.rs returns zero matches. | Path-prefix lookup in Python (mirrors prove-it-prototype probe's `crate_of`). | 10m | pending |
| C5 | (a) Disable Pass-2-fallback via temporary `if false &&` branch (per CLAUDE.md provenance technique). (b) Index pre-fix: `count_pre = SELECT COUNT(*) FROM refs WHERE symbol_id IS NOT NULL AND <ref source_module starts with crate::>`. (c) Apply fix. (d) Re-index. (e) `count_post`. (f) Assert `count_post - count_pre ≥ T` where T ≥ 100. | SQL query on `references` table grouped by source_module pattern. | 45m | pending |
| C6 | Pre-fix snapshot `total_pre = SELECT COUNT(*) FROM refs WHERE symbol_id IS NOT NULL`. Apply fix. Re-index. Snapshot `total_post`. Assert `total_post ≥ total_pre`. | SQL count (mechanism distinct from any resolver code path). | 30m | pending |
| C7 | `hyperfine --warmup 1 'tethys index'` pre-fix and post-fix (5 runs each). Compare median. Assert `post_median / pre_median < 1.10`. | hyperfine — independent process-level timing. | 20m | pending |

## Negative space

The fix deliberately does NOT:

1. **Change tree-sitter import extraction.** The `imports` table contents stay identical; only resolution behavior changes.
2. **Introduce LSP-based resolution.** Pass 3 (`--lsp`) is unchanged. The fix only touches Pass 2 import-driven resolution and dep-graph construction.
3. **Change Pass-2-fallback behavior.** Fallback still searches the whole workspace by symbol name as a last resort. The fix just makes Pass-2-imports do work, *reducing* fallback's load — but fallback's algorithm isn't touched.
4. **Change the `crate::`/`self::`/`super::` arms of `resolve_module_path`.** Those still operate on whatever `crate_root` is passed in; the fix changes what `crate_root` *value* gets passed, not the resolver logic itself.
5. **Introduce the other rivets-i8qn accessors.** `entry_point_file()` and `module_name()` stay out of scope. Only `src_root()` lands here.
6. **Fix non-`crate::` cross-crate imports.** Those are partly handled by the just-merged rivets-v465 work. This PR doesn't touch external-crate or `self::`/`super::` semantics.
7. **Rewrite `compute_module_path_for_file`.** The new per-file crate lookup may share code with that function (or factor out a helper), but the function's existing behavior and call sites are preserved.

## Out-of-scope tracking

These tracker items are NOT closed by this fix:

- **rivets-i8qn (full):** the `entry_point_file()` and `module_name()` accessors remain open after this PR. Only the `src_root()` portion lands.
- **rivets-714v:** LSP multi-crate path bug, separate concern.
- **rivets-53zq:** test polish on PR #62, unrelated to resolver fix.

## Gate checklist (`budgeted-plan` will refuse to run until)

- [x] Every claim has a falsifier in the table
- [x] Every falsifier names an independent oracle
- [x] Every claim has a distinct verifiable output
- [x] The cheapest falsifier (C3) has been run and passed
- [x] "Negative space" lists ≥3 entries (7 entries listed)

## Plan-phase preview (for the next skill)

The `budgeted-plan` skill will slice this into checkpointed-build slices. Likely shape:

1. **Slice 1: `CrateInfo::src_root()` accessor + unit tests.** Falsifies C1, C2. ~10 lines, 2 tests.
2. **Slice 2: Per-file crate lookup helper.** Falsifies C4. Either factor from `compute_module_path_for_file` or duplicate. ~15 lines, 1 test.
3. **Slice 3: Wire site #1 (`resolve.rs:66`) to use per-file crate root.** Run pre/post probe. Falsifies C5 partially. Empirical impact: ~half of the leaks at this site.
4. **Slice 4: Wire sites #2 + #3 (`indexing.rs` dep computation).** Closes rivets-34tv.
5. **Slice 5: Wire site #4 (`resolver.rs:61` workspace-crate arm).** Closes the rivets-v465-introduced fourth instance.
6. **Slice 6: Falsify C5, C6, C7 on the full fix.** Pre/post measurement with fallback-disabled toggle for C5.

Each slice will get a complexity budget, scale budget, stress fixture, and oracle per the `budgeted-plan` skill's hard requirements.
