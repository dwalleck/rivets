# rivets-6aoc / rivets-34tv — prove-it-prototype output

## Smallest factual question

> How many `crate::*` imports in the rivets workspace would resolve correctly
> under the proposed fix (`crate_root` = importing file's own crate `src/`)
> but fail to resolve under the current code (`crate_root` = `workspace_root.join("src")`)?

## Probe

`.rivets-6aoc/probe.py` — reads the `imports` table from `.rivets/index/tethys.db`, simulates `resolve_module_path`'s file-existence logic twice (once with the broken root, once with each file's correct per-crate root), and classifies each row.

Output:

```
crate::* imports total (per-symbol rows): 330
   327  a. migrate (fix resolves, bug doesn't)
     3  e. file outside known crate

=== Oracle-comparable unit: distinct (file, first_segment) pairs ===
  All:     112
  Migrate: 112
```

## Oracle

`.rivets-6aoc/oracle.sh` — bash + grep against source `.rs` files (no SQLite, no probe code, no shared interpretation of the DB schema). For each `use crate::X` statement in `crates/*/src/**/*.rs`, extracts the first identifier after `crate::`, checks whether the corresponding module file exists in the same crate's `src/` but not in `workspace_root/src/`, dedupes by `(file, first_segment)`.

Output:

```
Oracle: scanned 86 .rs files in workspace src dirs
Oracle: 111 distinct (file, first_segment) pairs with use crate::
Oracle: 105 of those would migrate under the fix
```

## Agreement

Per `.rivets-6aoc/diff_sets.py` (debugging utility):

| | |
|---|---|
| Probe migrating pairs | 112 |
| Oracle migrating pairs | 105 |
| **Common (set intersection)** | **104** |
| Probe-only | 8 |
| Oracle-only | 1 |

The 9 differing pairs are explained by an extractor-level ambiguity in how tethys encodes `use crate::X;`:

- **All 8 probe-only pairs** are `(file, __CRATE_ROOT__)` — cases where `use crate::Foo` imports a top-level symbol (struct/fn at lib.rs). The DB stores `(source_module='crate', symbol_name='Foo')`; the probe interprets this as "resolve to crate root lib.rs" matching tethys's actual resolver behavior. The oracle's regex captures `Foo` and tries to find `Foo.rs`, which doesn't exist because `Foo` is a symbol, not a module.

- **The 1 oracle-only pair** is `(indexing.rs, lsp)` — `use crate::lsp;` where `lsp` *is* a module. Same DB encoding `(source_module='crate', symbol_name='lsp')`, but here `lsp.rs` exists. The probe's logic interprets the import as crate-root-targeted (matching how the resolver would handle it) and dedupes; the oracle's regex sees `lsp` as the segment.

Both interpretations are coherent — they answer slightly different questions about what `use crate::X` "points at." For the substantive prove-it-prototype claim, **both methods agree that essentially every `crate::*` import in the workspace is affected by the bug** (104 in common, ~95% agreement on the migrating set). The remaining 9 pairs are interpretation drift, not disagreement about the bug.

## What I learned that wasn't obvious before the probe

The rivets workspace root has no `src/` directory at all — `Cargo.toml` lives at the root but the root is a pure workspace manifest, not a crate. So `workspace_root.join("src")` is a path to a directory that doesn't exist. **Every `crate::*` Pass-2-imports resolution fails today**; the bug is universal, not sub-crate-specific as rivets-6aoc's issue body implies. Anything that "works" today does so via Pass-2-fallback (whole-workspace symbol-name search) or remains unresolved.

This reframes the fix:
- Original framing (rivets-6aoc): "wrong for files in sub-crates"
- Empirically correct framing: "wrong for all files; Pass-2-imports is currently a no-op for `crate::*` paths"

## Hard-gate checklist

- [x] Probe written and runs against the real codebase (327 of 330 per-symbol imports migrate)
- [x] Oracle defined and produces output (105 of 111 distinct (file, segment) pairs migrate, independent mechanism)
- [x] Probe and oracle agree on the substantive claim (104-pair intersection; 9-pair drift is interpretation difference, not bug disagreement)
- [x] Wrote down one-sentence learning (workspace root has no `src/`; bug is universal)

Ready for falsifiable-design.

## Scope reminder for downstream phases

The four hardcode sites to fix:
1. `crates/tethys/src/resolve.rs::resolve_cross_file_references` — `self.workspace_root.join("src")` at line 66
2. `crates/tethys/src/indexing.rs::compute_dependencies` — same pattern around line 857
3. `crates/tethys/src/indexing.rs::compute_dependencies_from_stored` — same pattern around line 1023
4. `crates/tethys/src/resolver.rs::resolve_module_path` workspace-crate arm — `target.path.join("src")` at line 61 (introduced by the just-merged rivets-v465 fix)

The fix mechanism (per rivets-i8qn's `src_root()` accessor concept): derive each crate's source root from `CrateInfo::lib_path.parent()` rather than hardcoding `src/`. Combine with a per-file crate lookup helper (or use existing `compute_module_path_for_file`-style logic) so each caller passes its file's own crate's root.
