# rivets-3d0s — falsifiable design

Status: cheapest falsifier passed 2026-05-12. Ready for `budgeted-plan`.

Prove-it-prototype findings: see git history for `.rivets-3d0s/probe.py`,
`.rivets-3d0s/oracle.py`, `.rivets-3d0s/lsp_probe.py`. Summary: the issue's
"tethys fabricates symbols" hypothesis was wrong. Every top phantom-target
symbol exists as a real workspace definition; the actual bug is **kind-
mismatch in the unscoped fallback resolver**.

## Purpose

Eliminate the bulk of the ~174 phantom cross-crate `file_deps` refs that
survive rivets-0gom by **filtering at the unscoped fallback resolver**:
`search_unique_symbol_by_name` only returns matches whose `sym_kind` is
syntactically compatible with the caller's `ref_kind`.

## Architecture

A focused change at one resolver entry point:

```
fallback_symbol_search                 # crates/tethys/src/resolve.rs
  ├─ same-crate scoping                # rivets-0gom — UNCHANGED
  │    (search_symbol_by_name_in_path_prefix)
  └─ unscoped fallback                 # search_unique_symbol_by_name
        + accept ref_kind parameter        ← NEW
        + filter SQL by sym_kind ∈ ALLOW   ← NEW
```

No new pass, no new column, no post-hoc audit. Just a kind filter on the
existing fallback query. Import-based resolution (steps 1/2 of
`try_resolve_reference`) and same-crate scoping (`search_symbol_by_name_in_path_prefix`)
are unaffected — same-crate variant constructors, field accesses, and
module-qualified calls continue to resolve normally.

## Compatibility matrix

| ref_kind | ALLOW sym_kinds | DEMOTE-by-omission sym_kinds |
|---|---|---|
| `type` (e.g. `T: X`, `Vec<X>`, `fn f(x: X)`) | Struct, Class, Enum, Trait, Interface, TypeAlias | Method, Function, Const, Static, Module, Macro, EnumVariant, StructField |
| `call` (e.g. `x()`, `obj.method()`) | Function, Method, Macro | Struct, Class, Enum, Trait, Interface, TypeAlias, Const, Static, Module, EnumVariant, StructField |
| `import`, `inherit`, `construct`, `field_access`, `unknown` | (no kind filter — current behavior) | — |

## Claims

1. **C1 (type-kind precision):** No `ref_kind=type` phantom ref in current rivets DB survives the audit's rule, *given* that the phantom's `sym_kind` is not in the ALLOW set for type. (i.e., the rule definition catches the population it targets — verifiable empirically.)
2. **C2 (call-kind precision):** No `ref_kind=call` phantom ref with `sym_kind ∈ {StructField, Module, EnumVariant}` survives the audit's rule.
3. **C3 (same-crate exemption):** No same-crate-resolved ref is touched by the rule. The audit applies ONLY to refs returned from the unscoped fallback path, never to same-crate scoping or import-based resolution.
4. **C4 (reduction):** Audit eliminates ≥ 50% of the 174 FORBIDDEN-pair phantom refs in the current rivets workspace.
5. **C5 (false-positive ceiling):** Audit demotes ≤ 10 refs in legitimate ALLOWED-pair cross-crate edges.
6. **C6 (existing-test regression):** `cargo nextest run -p tethys` passes after implementation. Specifically, the round-3/round-4 integration tests in `tests/resolver_routing.rs` still pass.

## Falsification

| # | Claim | Falsifier | Oracle | Cost | Status |
|---|---|---|---|---|---|
| C1 | All `ref_kind=type` non-type-kind phantoms caught | `audit_simulation.py` simulates rule; counts surviving type/non-type phantoms | DB SQL aggregation (independent of resolver code; reads frozen DB state) | 5m | **PASS** (0 survivors) |
| C2 | All `ref_kind=call` non-callable-kind phantoms caught | `audit_simulation.py` counts surviving call phantoms with sym_kind ∈ {StructField, Module, EnumVariant} | DB SQL aggregation | 5m | **PASS** (0 survivors) |
| C3 | No same-crate ref demoted | `audit_simulation.py` walks all same-crate resolved refs, applies rule | DB SQL aggregation | 5m | **PASS** (0 demoted with same-crate exemption; **catastrophic 316 demotions without it — design saved by this falsifier**) |
| C4 | ≥ 50% reduction | `audit_simulation.py` computes (demoted / total) on FORBIDDEN-pair phantoms | DB SQL aggregation | 5m | **PASS** (58.6%, 102/174) |
| C5 | ≤ 10 ALLOWED-pair false-positives | `audit_simulation.py` counts demotions in ALLOWED pairs | DB SQL aggregation | 5m | **PASS** (7 demoted; may include a few unrecognized phantoms in ALLOWED pairs that are themselves legitimate to demote, pending drill-down) |
| C6 | Existing tests unaffected | `cargo nextest run -p tethys` post-implementation | nextest runner | 30s | pending implementation |

All per-claim oracles produce distinct outputs: each claim has its own
section in `audit_simulation.py`'s stdout with a PASS/FAIL verdict and a
specific count or list. A reader can localize a failure to a single claim
by reading the script's output.

**Cheapest falsifier (C1+C2+C3+C4+C5) run against current rivets DB at
commit `b3892ef`. All pass. The earlier draft of the design (no same-crate
exemption) was disproven by C3 — would have demoted 316 legitimate same-
crate refs — and revised before lock-in.**

## Negative space (what this design deliberately does NOT do)

1. **Doesn't catch kind-compatible cross-crate phantoms.** The 72 survivors after audit (65 call/method + 6 call/function + 1 type/struct — the `Metadata` case where workspace `Metadata` collides with `std::fs::Metadata`) are kind-compatible. Resolving them correctly requires type inference (rust-analyzer / LSP, blocked by **rivets-714v**) or a more aggressive policy at the unscoped-fallback level.
2. **Doesn't add type inference.** The audit is a syntactic check on `(ref_kind, sym_kind)` pairs, not a semantic check on receiver/binding types.
3. **Doesn't change the symbol extractor.** Workspace symbols (methods, fields, enum variants) continue to be recorded as before — they just won't get matched in incompatible reference contexts via unscoped fallback.
4. **Doesn't gate qualified-name phantoms.** Qualified-name resolution goes through `get_symbol_by_qualified_name`, not `search_unique_symbol_by_name`. No phantoms observed in qualified-name resolution in current data.
5. **Doesn't fix rivets-714v.** LSP integration on multi-crate workspaces remains broken; rivets-3d0s ships without that dependency met. The audit alone delivers ≥ 50% reduction; the LSP-recoverable refs land later when rivets-714v is resolved.

## Components changed

- `crates/tethys/src/db/symbols.rs::search_unique_symbol_by_name` — add `ref_kind: ReferenceKind` parameter, filter SQL by sym_kind ∈ allowed set.
- `crates/tethys/src/resolve.rs::fallback_symbol_search` — pass `ref_kind` (derived from caller's `ref_.kind` already available in `try_resolve_reference`).
- New test in `crates/tethys/tests/resolver_routing.rs` exercising the rivets-3d0s shape (`.len()` on stdlib type with competing workspace method).

Estimated change footprint: ~30 lines code + 1 integration test.

## Artifacts (in this PR's diagnostic dir)

- `probe.py` — DB-side probe (prove-it-prototype)
- `oracle.py` — independent workspace-source oracle (prove-it-prototype)
- `investigate.py` — drill-down on "fabricated" symbols (prove-it-prototype)
- `lsp_probe.py` — empirical test of `--lsp` non-auditing behavior (prove-it-prototype)
- `audit_simulation.py` — falsifiable-design's cheapest falsifier
- `design.md` — this file
