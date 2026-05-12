# rivets-3d0s — prove-it-prototype findings

## Smallest question

> For the ~52 residual cross-crate file_deps edges (174 individual refs)
> remaining after rivets-0gom, what is the resolved-target SymbolKind
> distribution and reference-site kind, and does the workspace actually
> contain real definitions matching those kinds?

This question gates the fix shape:
- If kinds are mostly `method` and there's a real impl block → filter at extraction
- If kinds are mixed and no workspace defs exist → tethys fabricates symbols
- If kinds are mixed but ALL have real defs → bug is in the resolver, not the extractor

## Probe

`.rivets-3d0s/probe.py` — stdlib `sqlite3`, queries tethys's DB directly. ~60
lines. Identifies the 174 phantom cross-crate resolved refs by classifying
file pairs against the FORBIDDEN ordered-pair set from rivets-0gom's
`diagnose_residual.py`. Aggregates by (sym_name, sym_kind, ref_kind).

## Oracle

`.rivets-3d0s/oracle.py` — independent: walks workspace source via stdlib
`os.walk` + `re`. Does NOT touch tethys's DB or extractor. Classifies each
top phantom name by its real workspace definition context.

## Probe vs oracle: agreement on 8/8 slices

| Name | Probe says | Oracle finds | Verdict |
|---|---|---|---|
| `len` | `method` (49 calls) | `pub fn len(&self)` in `rivets-jsonl/src/warning.rs:277` (WarningCollector::len) | **AGREE** |
| `children` | `struct_field` (44 calls) | `pub children: Vec<DepTreeNode>` in `rivets/src/output/tree.rs:24` | **AGREE** |
| `display` | `module` (20 calls) | `mod display;` in `tethys/src/cli/mod.rs:3` | **AGREE** |
| `Tree` | `enum_variant` (17 types) | `enum DepAction { Tree { ... }, ... }` in `rivets/src/cli/args.rs:329` | **AGREE** |
| `write` | `method` (9 calls) | `pub async fn write<T: Serialize>(...)` in `rivets-jsonl/src/writer.rs:148` | **AGREE** |
| `Serialize` | `enum_variant` (8 types) | `LspError::Serialize(#[source] serde_json::Error)` in `tethys/src/lsp/error.rs:35` | **AGREE** |
| `Deserialize` | `enum_variant` (1 type) | `LspError::Deserialize` at same file:39 | **AGREE** |
| `Parser` | `enum_variant` (2 types) | `Error::Parser(String)` in `tethys/src/error.rs:43` | **AGREE** |

The "tethys fabricates symbols" framing in the original issue was a wrong
hypothesis. Every top phantom-target symbol exists as a legitimate
workspace definition matching the kind tethys recorded.

## What I learned (was not obvious before the probe)

**The bug is kind-mismatch in the resolver, not pollution in the extractor.**
The fallback resolver `search_unique_symbol_by_name` returns a workspace-unique
match without checking that the resolved symbol's *kind* is compatible with
the *syntactic role* of the reference. So:

- `T: Serialize` (a generic-bound type reference, `ref_kind=type`) gets
  resolved to `LspError::Serialize` (an `enum_variant` — which can never
  legitimately occupy a type position; `LspError::Serialize` is a value,
  not a type)
- `tree_node.children` (a field access, `ref_kind=call` per tethys's
  extraction) gets resolved to `DepTreeNode::children` (a `struct_field`
  on a completely unrelated type)
- `vec.len()` (a method call, `ref_kind=call`) gets resolved to
  `WarningCollector::len` (a method on an unrelated type)

The unifying pattern: **the resolver doesn't know the receiver/binding type
and just picks the workspace-unique candidate by name.** It happens to be
correct sometimes (legitimate cross-crate calls) and wrong most of the
time for stdlib-trait/method/external-type names.

## Bug-class split (174 refs)

| ref_kind | Count | What happens |
|---|---|---|
| `call` | 145 | Method/field/module/function calls. Receiver type unknown to tree-sitter; resolver picks workspace-unique by name. |
| `type` | 29 | Type-position references (`T: X`, `Vec<X>`, `fn(x: X)`). Resolver picks workspace-unique by name, including non-type kinds like enum_variant. |

| sym_kind of resolved target | Count |
|---|---|
| `method` | 65 |
| `struct_field` | 47 |
| `enum_variant` | 32 |
| `module` | 23 |
| `function` | 6 |
| `struct` | 1 |

`enum_variant` and `struct_field` resolution into type-position is **always
wrong** (enum variants and fields are values, not types). That's a free
~29 phantom-edge reduction with a single ref_kind/sym_kind check.

`method`/`field`/`module` resolution into call-position is **wrong without
type info**: tree-sitter can't determine `tree_node.children`'s receiver
type, so any name-based resolution to a workspace `children` field is a
guess. That's ~145 phantom refs requiring either:
- a type-inference layer (out of scope for tethys),
- or a stricter "don't resolve method-position calls via unscoped fallback"
  policy (probably correct; we'd accept that some legitimate cross-crate
  method calls go unresolved, but tethys already accepts this for stdlib).

## Implication for next skill (falsifiable-design)

The fix design needs to answer:
1. **Where does ref_kind get extracted?** Already in DB (column `refs.kind`).
   So the filter can live in `fallback_symbol_search` or in
   `search_unique_symbol_by_name` (whichever accepts the ref_kind param).
2. **What's the kind-compatibility matrix?**
   - `ref_kind=type` should only match sym_kind in `{struct, enum, trait, type_alias, union}`.
   - `ref_kind=call` is harder — without type info, the only safe answer
     is "don't resolve unscoped at all for call-position refs to
     method/field/module" (refuse rather than guess). Legitimate same-crate
     calls go through the rivets-0gom scoping path before this.
3. **What does "same-crate fallback" look like under this rule?** The
   round-3 integration test (`fallback_routes_unqualified_ref_to_same_crate_not_cross_crate`)
   currently relies on workspace-wide for `shared_helper`. If we tighten the
   workspace-wide fallback, that test fixture needs to use a `function`-kind
   target so it stays valid under the new rule.

## Artifacts

- `probe.py` — committed
- `oracle.py` — committed (portable, stdlib-only; replaces non-portable `oracle.sh`)
- `investigate.py` — committed (one-off drill-down on suspected fabricated symbols)
- `design.md` — this file
