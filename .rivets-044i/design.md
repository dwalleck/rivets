# rivets-044i: Qualified refs from import-less files

## Status

Falsifiable design draft. Cheapest falsifier: paper hand-trace against probe
shapes (see Falsification table row 1). Run completed during design (see
"Cheapest falsifier run" section).

## Purpose

Extend `try_resolve_reference` so qualified refs of the form
`segment1::segment2::...::segmentN` resolve correctly *when the first
segment is not an explicit or glob import* — currently the only way these
refs can reach the cross-file resolver. After rivets-dn35, such files reach
Pass 2 but the existing fallback (`get_symbol_by_qualified_name` literal
match against `symbols.qualified_name`) is structurally unable to match
because stored qualified names are module-stripped
(`indexing.rs:627-630`: free fns store `name`; methods store
`parent_name::name`).

## What the probe revealed

(See `.rivets-044i/probe.rs`, `.rivets-044i/oracle.md`,
`.rivets-044i/what-i-learned.md`.)

- Shape #1 (`helper::do_thing_q()` from import-less `crate_a/src/lib.rs`):
  ref extracted, target symbol indexed, ref stays `symbol_id IS NULL`.
- Shape #2 (`crate_a::Widget::make_widget_044i()` from import-less
  `crate_b/tests/it.rs`): same outcome.
- The literal-string fallback (`fallback_symbol_search`'s `is_qualified`
  branch at `resolve.rs:357-359`) cannot succeed because the stored
  qualified name does NOT include the source-text path prefix.

## Input shapes enumerated

Inputs to the new fallback path are `ref_name` strings reaching
`try_resolve_reference` after explicit/glob imports miss. Production-reachable
shapes:

**`ref_name` first-segment classification:**
- (s-crate) `crate::...` — Rust 2018+ explicit crate-relative
- (s-self) `self::...` — sibling within current module
- (s-super) `super::...` — parent module
- (s-wscrate) `<workspace-crate-name>::...` — Rust 2018+ extern-crate
  prefix (also covers `-` → `_` normalization, e.g. `rivets-jsonl` → `rivets_jsonl`)
- (s-submod) `<submodule-of-current-crate>::...` — implicit-crate-relative
  (the **shape #1 case** the probe revealed)
- (s-extern) `<external-crate-name>::...` (`std`, `serde`, `tokio`, etc.) —
  must stay unresolved

**`ref_name` length / tail shape:**
- (t-free) tail names a free function: stored `qualified_name = name`
- (t-method) tail names an `impl` method: stored
  `qualified_name = "Type::method"`
- (t-assoc) tail names an associated const/fn: same shape as method
- (t-type) tail names a type: stored `qualified_name = name`

**`ResolveContext` shape:**
- (c-base) `explicit_imports.is_empty() && glob_imports.is_empty()` — the bug case
- (c-imp-miss) imports non-empty but first segment misses both — must reach
  the new fallback the same way

**Workspace shape:**
- (w-single) single-crate workspace (`[package]` only)
- (w-multi) `[workspace]` with multiple member crates
- (w-hyphen) crate name contains `-` (normalized at lookup)

**Out-of-scope shapes (justified):**
- C# files. The C# extractor lacks namespace-based qualified-name storage
  (rivets-jwf9). Pass 2's C# path remains coverage-bound by that ticket;
  044i does not touch the C# branch.
- LSP (Pass 3) refs. Pass 3 is independent and unaffected.
- Refs whose tail names a re-export (`pub use foo::Bar as Baz`). Re-exports
  are not tracked in `symbols.qualified_name`; covered by rivets-itz7 family,
  not 044i.
- Refs with generic type arguments (`Vec<Foo>::iter`). The extractor strips
  generics before storing `qualified_name`; the qualified-name search is
  unaware of generics either way.

## Architecture

Pure extension of `resolve.rs::try_resolve_reference`. No schema change, no
new tables, no new public API.

After the existing explicit-import / glob-import / `fallback_symbol_search`
attempts return None, the new path activates ONLY when the ref is qualified
(`ref_name.contains("::")`). It performs a **longest-prefix split** on the
`::`-segments, computes a file ID for each prefix via `resolve_module_path`
(with implicit-crate-prepend retry for non-prefixed first segments), then
queries `search_symbol_by_qualified_name_in_file(tail, file_id)`. First
successful (file_id, symbol) pair wins.

### Algorithm

```text
fn qualified_module_fallback(
    ref_name: &str,
    current_file: &Path,
    src_root: &Path,
    workspace_crates: &[CrateInfo],
    db: &Index,
) -> Result<Option<Symbol>> {
    let segments: Vec<&str> = ref_name.split("::").collect();
    if segments.len() < 2 { return Ok(None); }

    for split in (1..segments.len()).rev() {
        let prefix = &segments[..split];   // module path
        let tail   = segments[split..].join("::");  // symbol qualified_name

        // Interpretation A: implicit-crate (only if path[0] not already
        // in {crate, self, super}; this avoids the meaningless "crate::crate::"
        // shape but still tries crate-relative for the s-submod shape).
        let file_id = if !matches!(prefix[0], "crate" | "self" | "super") {
            let mut with_crate = vec!["crate".to_string()];
            with_crate.extend(prefix.iter().map(|s| s.to_string()));
            resolve_module_path(&with_crate, current_file, src_root, workspace_crates)
                .and_then(|p| db.get_file_id(&relative(p)))
        } else { None };

        // Interpretation B: as-written (lets the s-crate/self/super/wscrate
        // arms of resolve_module_path fire).
        let file_id = file_id.or_else(|| {
            let as_owned: Vec<String> = prefix.iter().map(|s| s.to_string()).collect();
            resolve_module_path(&as_owned, current_file, src_root, workspace_crates)
                .and_then(|p| db.get_file_id(&relative(p)))
        });

        if let Some(file_id) = file_id {
            if let Some(sym) = db.search_symbol_by_qualified_name_in_file(&tail, file_id)? {
                return Ok(Some(sym));
            }
        }
    }
    Ok(None)
}
```

### Insertion point

`try_resolve_reference` (`resolve.rs:162-231`). After the existing
`fallback_symbol_search` returns None for a qualified ref, invoke the new
path. If it succeeds, log via `trace!` and call `db.resolve_reference`.

The new path is invoked ONLY when `is_qualified == true`. Unqualified
behavior (rivets-0gom-protected same-crate-first then unscoped-unique) is
not touched.

## Falsification

| # | Claim | Falsifier | Oracle | Cost | Status | Regression fence |
|---|-------|-----------|--------|------|--------|------------------|
| 1 | Longest-prefix split with implicit-crate-first interpretation produces the correct (file_id, symbol) pair for shapes s-crate, s-self, s-super, s-wscrate, s-submod and returns None for s-extern. | Hand-trace algorithm against the 6 first-segment shapes using actual `resolve_module_path` semantics from `resolver.rs:31-65`. | Rust's own scoping rules (independent of tethys code). | 15m | **passed (paper)** — see Cheapest-falsifier run below | unit tests in `resolve.rs#tests` (one `#[test]` per shape, post-impl). |
| 2 | Same-crate qualified ref via implicit-crate-relative resolves to its definition. | Run `probe_044i.rs::probe_044i_qualified_ref_from_import_less_file` against patched resolver. | SQL: `refs.symbol_id` of the lib.rs ref equals `symbols.id` of `do_thing_q` in helper.rs. | 5m | pending impl | The probe test stays in `tests/probe_044i.rs` and becomes the regression fence (renamed to a non-probe name post-fix). |
| 3 | Workspace-crate-prefix ref from import-less integration test resolves. | Run `probe_044i.rs::probe_044i_workspace_crate_prefix_from_import_less_file` against patched resolver. | SQL: `refs.symbol_id` of the `it.rs` ref equals `symbols.id` of `make_widget_044i`. | 5m | pending impl | Same as claim 2 — kept as the regression fence. |
| 4 | External-crate-prefixed refs do NOT phantom-resolve via the new path. | Build a synthetic single-crate workspace with `std::collections::HashMap` referenced from an import-less file. After indexing, count refs with `reference_name LIKE 'std::%'` and `symbol_id IS NOT NULL`. | SQL: count is 0. Independent of resolver internals. | 10m | pending impl | New test `qualified_external_crate_stays_unresolved` in `tests/probe_044i.rs`. |
| 5 | `crate::sub::Item` qualified ref from import-less file resolves. | Synthetic workspace `crate_a/src/lib.rs` with `mod sub;`, ref `crate::sub::Thing`. After indexing, verify ref resolves to `Thing` in `sub.rs`. | SQL: `refs.symbol_id` of the ref equals `symbols.id` of the target struct. | 10m | pending impl | New test `qualified_crate_prefix_resolves` in `tests/probe_044i.rs`. |
| 6 | Unqualified-ref resolution path unchanged: `pass2_no_imports.rs::unqualified_call_in_import_less_file_resolves_via_fallback` continues to pass. | Run that test against patched code. | The test's existing oracle (resolved_to_target ≥ 1 via same-crate prefix search). | 1m | pending impl | The test itself (already present at `tests/pass2_no_imports.rs`). |
| 7 | Cross-crate file_deps phantom rate on the rivets workspace does NOT regress above 0.00%. | Re-index rivets with `cargo run --bin tethys -- index`, run `python .rivets-ycaq/probe_phantom_rate.py`. Compare to baseline. | The probe's own count (independent: reads `imports` + `file_deps` tables directly, classifies each cross-crate edge). | 30m | pending impl | Existing rivets-3d0s regression fence: `crates/tethys/tests/file_deps_corroboration.rs::k_hybrid_drops_cross_crate_call_without_import_corroboration`. Verified present 2026-05-18. |
| 8 | rivets-0gom ambiguity violations on rivets workspace do NOT increase. | Re-index rivets, run `python .rivets-0gom/probe.py`. Compare Section 3 violation count to baseline. | The 0gom probe (independent of resolve.rs). | 15m | pending impl | Existing rivets-0gom regression fence: `crates/tethys/tests/resolver_routing.rs::fallback_routes_unqualified_ref_to_same_crate_not_cross_crate`. Verified present 2026-05-18. |

### Cheapest falsifier run (claim 1, paper hand-trace)

For each of 6 input-shape combinations, trace the algorithm:

| Shape | ref_name | expected outcome | algorithm trace | result |
|-------|----------|------------------|-----------------|--------|
| s-submod (shape #1) | `helper::do_thing_q` | resolve | split=1: implicit-crate prepends `["crate","helper"]` → `crate_a/src/helper.rs` (exists). Lookup `do_thing_q` (qualified_name="do_thing_q"). Hit. | **agrees** |
| s-wscrate (shape #2) | `crate_a::Widget::make_widget_044i` | resolve | split=2: prefix=`crate_a::Widget`. implicit-crate → `crate::crate_a::Widget` → not found. as-written → workspace-crate hit but `crate_a/src/Widget.rs` doesn't exist. None. split=1: prefix=`crate_a`. as-written → workspace-crate single-segment → entry_point_file = `crate_a/src/lib.rs`. Lookup `Widget::make_widget_044i` (qualified_name="Widget::make_widget_044i"). Hit. | **agrees** |
| s-extern | `std::collections::HashMap` | unresolved | All splits: prefix starts with `std` which is not crate/self/super/workspace-crate; implicit-crate `["crate","std",...]` returns None (no such submodule); as-written `["std",...]` returns None (not a workspace crate). All splits exhausted. None. | **agrees** |
| s-crate | `crate::storage::Issue` | resolve | split=2: prefix=`crate::storage`. as-written → `crate_root/storage.rs`. Lookup `Issue`. Hit. | **agrees** |
| s-super | `super::helper::foo` | resolve | split=2: prefix=`super::helper`. as-written → `current_dir/../helper.rs`. Lookup `foo`. Hit. (implicit-crate skipped because path[0] is `super`.) | **agrees** |
| s-self | `self::helper::foo` | resolve | split=2: prefix=`self::helper`. as-written → `current_dir/helper.rs`. Lookup `foo`. Hit. | **agrees** |

All 6 shapes the design must handle produce the algorithmically-traced
outcome the oracle predicts. Claim 1 survives its cheapest-falsifier
attempt. Proceed.

## Negative space

Things this design deliberately does NOT do:

1. **Does not resolve external crate references.** A ref like
   `serde::Deserialize` from a workspace file stays unresolved. (We have no
   index of external symbols and adding one is out of 044i's scope; LSP
   Pass 3 covers this when enabled.)
2. **Does not introduce a new symbol cache.** Each unresolved qualified ref
   pays O(segments) lookups per resolution attempt. For large workspaces
   this could become a hot path; rivets-bjdn is the existing tracker for
   that optimization, not in scope here.
3. **Does not change Pass 1 (tree-sitter same-file) or Pass 3 (LSP)
   behavior.** Strictly additive to the Pass 2 fallback chain.
4. **Does not disambiguate between multiple impl blocks of the same Type
   in the same file with the same method name.** Such overloads are
   `cargo check`-rejected; if present (e.g., via macros), `LIMIT 1`
   semantics in `search_symbol_by_qualified_name_in_file` apply
   non-deterministically. We accept this — same behavior as the
   existing fallback.
5. **Does not affect C# resolution.** The C# Pass 2 path is gated by
   rivets-jwf9; 044i's added branch fires regardless of language but
   `resolve_module_path` itself is Rust-specific and returns None for
   non-Rust file extensions (already true today).

## Deferrals / out-of-scope cross-checks

- "rivets-jwf9 — C# namespace resolution": **verified open**
  (`rivets show rivets-jwf9` → P3 open).
- "rivets-bjdn — pre-compute crate-path lookup map": **verified open**
  (P4 open).
- "rivets-3d0s K-hybrid file_deps regression fence": verified present at
  `crates/tethys/tests/file_deps_corroboration.rs::k_hybrid_drops_cross_crate_call_without_import_corroboration`
  (2026-05-18). No action needed.
- "rivets-0gom ambiguity regression fence": verified present at
  `crates/tethys/tests/resolver_routing.rs::fallback_routes_unqualified_ref_to_same_crate_not_cross_crate`
  (2026-05-18). No action needed.
- "rivets-bjdn deferred perf cache": tracker ID verified; no action needed
  in 044i.
- No other deferrals.

## What changes (concrete diff surface)

- `crates/tethys/src/resolve.rs`:
  - Add `qualified_module_fallback` private method on `Tethys`.
  - Call it from `try_resolve_reference` after `fallback_symbol_search`
    returns None AND `is_qualified == true`.
- `crates/tethys/tests/probe_044i.rs`:
  - Stays in place; both probe tests become passing regression fences
    after the fix. Add claims-4 and claims-5 tests as additional cases.
- No schema changes, no new public APIs, no behavior change for
  unqualified refs.
