# rivets-3d0s — budgeted plan

Source design: `.rivets-3d0s/design.md`. Cheapest falsifier passed at design
time (`audit_simulation.py` against current rivets DB: C1-C5 all PASS).

Three slices, each implementable in ≤ 30 min, ≤ 2 files. Total scope:
~80 LOC across 3 files (`db/symbols.rs`, `resolve.rs`, `tests/resolver_routing.rs`).

The skill says "a slice is done when (a) unit tests pass, (b) stress fixture
produces expected result, (c) prove-it-prototype oracle still agrees with
binary, (d) complexity budget holds at the slice's scale." Each slice below
includes these as verification checkboxes.

---

## Slice 1: Add kind-compatibility filter to `search_unique_symbol_by_name`

**Claim:** When called with `ref_kind=Type`, returns only symbols whose
`SymbolKind` is in `{Struct, Class, Enum, Trait, Interface, TypeAlias}`.
When called with `ref_kind=Call`, only `{Function, Method, Macro}`. For
other `ref_kind`s (Import, Inherit, Construct, FieldAccess, Unknown), no
filter is applied — current behavior unchanged. (Maps to design claims
C1 + C2.)

**Oracle:** New unit tests in `db/symbols.rs` test module. Construct an
in-memory DB with mixed-kind symbols all sharing one name, query with
each `ref_kind`, assert filter behavior matches the matrix.

**Stress fixture:** DB containing two symbols both named "Foo": one
`SymbolKind::EnumVariant` and one `SymbolKind::Struct`. Query
`search_unique_symbol_by_name("Foo", ReferenceKind::Type)`. The function
must return `Some(Struct)`, not `None` (because after filter, only one
candidate remains — Struct — and the LIMIT-2 ambiguity refusal sees a
unique match). This catches two plausible bugs at once:
- Filter applied wrong: returns `Some(EnumVariant)` (failure: kind not filtered)
- Filter narrows but ambiguity logic broken: returns `None` (failure:
  filter applied before LIMIT, leaving a single match that should be
  returned, not refused)

**Loop budget:** No new loops introduced. The SQL change adds
`AND kind IN (...)` to the existing query. Indexed lookup on
`idx_symbols_name` (B-tree on `name`) returns matches; the kind filter
acts as a constant-factor in-row check. Asymptotic: `O(log n + k)` where
`n` = symbols workspace-wide, `k` = name matches. Production scale: `n` ≈
3000 (rivets workspace), `k` ≤ 2 (LIMIT 2). Operations per call ≈ 12-15.
Well within 10⁶ budget.

**Wall budget:** Phase is reference resolution during indexing (one-shot,
~20s for rivets workspace pre-fix). Per-query overhead from added filter
clause: sub-millisecond. Called ~1000-2000 times during a full indexing
pass; total added wall ≈ < 100ms. No measurable change.

**Files:**
- `crates/tethys/src/db/symbols.rs`

**Code (advisory):**

```rust
// In Index impl:
pub fn search_unique_symbol_by_name(
    &self,
    name: &str,
    ref_kind: ReferenceKind,
) -> Result<Option<Symbol>> {
    let kind_filter = kind_filter_sql(&ref_kind);
    let conn = self.connection()?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {SYMBOLS_COLUMNS} FROM symbols
         WHERE name = ?1 {kind_filter}
         LIMIT 2"
    ))?;
    let mut iter = stmt.query_map(params![name], row_to_symbol)?;
    let Some(first) = iter.next().transpose()? else {
        return Ok(None);
    };
    if iter.next().transpose()?.is_some() {
        return Ok(None);
    }
    Ok(Some(first))
}

/// SQL fragment for the kind-compatibility filter. Returns an empty
/// string for ref_kinds that don't get filtered (Import, Inherit,
/// Construct, FieldAccess, Unknown).
fn kind_filter_sql(ref_kind: &ReferenceKind) -> &'static str {
    match ref_kind {
        ReferenceKind::Type =>
            " AND kind IN ('struct', 'class', 'enum', 'trait', 'interface', 'type_alias')",
        ReferenceKind::Call =>
            " AND kind IN ('function', 'method', 'macro')",
        _ => "",
    }
}
```

Existing tests in `db/symbols.rs::search_by_name_ambiguity_tests` (3 call
sites at lines 614, 625, 637) get updated to pass a `ref_kind` —
`ReferenceKind::Call` since they use `fn`-defined symbols, which keeps
their semantics unchanged.

**Doc-comment precondition classification:** None added. The function's
contract changes (`ref_kind` is now an input that affects what kinds
match), but no caller-side precondition is introduced — every
`ReferenceKind` value is a valid input.

**Output stream classification:** N/A (read-only DB function, no writes).

**Verification:**
- [ ] Unit tests pass (existing + new kind-filter tests)
- [ ] Stress fixture: 2 symbols named "Foo" (EnumVariant + Struct) → query with `ReferenceKind::Type` returns `Some(Struct)`
- [ ] `cargo nextest run -p tethys` still passes (no other compile breaks)
- [ ] `cargo clippy -p tethys --all-targets -- -D warnings` clean
- [ ] Loop budget: spot-check via `tethys index` timing on rivets workspace (no regression > 5%)

---

## Slice 2: Plumb `ref_kind` from `fallback_symbol_search`

**Claim:** `fallback_symbol_search` forwards the caller's
`ref_.kind: ReferenceKind` to `search_unique_symbol_by_name`. The
unscoped path now uses kind-filtered results. Re-indexing the rivets
workspace reduces FORBIDDEN-pair cross-crate phantom refs from 174 to
≤ 87 (≥ 50% reduction, per design claim C4). Maps to design claims
C3 (same-crate exemption: auto-satisfied because the unscoped path is
the only one touched), C4 (reduction), and C5 (false-positive ceiling).

**Oracle:** Two independent oracles:
1. `audit_simulation.py` run after re-indexing — should report `phantoms
   surviving = 72` (all kind-compatible) and `phantoms demoted = 0`
   (because the binary's filter already prevented them being resolved in
   the first place; the surviving 72 are the ones the simulation
   predicted would survive).
2. `probe.py` run after re-indexing — cross-crate FORBIDDEN-pair refs
   should drop from 174 to ≤ 87 (typically ~72 per the simulation).

**Stress fixture:** Re-index the actual rivets workspace
(`tethys.exe index --rebuild` after wiping `.rivets/index/tethys.db`).
Then run both oracles. This is a stress test by virtue of running
against the largest available real-world input (3000 symbols, 6000+
resolved refs, mixed languages and kinds). Plausible bugs caught:
- Wrong `ref_kind` passed (e.g., always `Type`): probe shows wildly
  different numbers from simulation
- `ref_.kind.clone()` lost on some path: compile error or test failure
- Same-crate path accidentally getting kind-filtered: probe shows
  same-crate resolution count drops too

**Loop budget:** No new loops; parameter threading only.

**Wall budget:** No always-on phase change.

**Files:**
- `crates/tethys/src/resolve.rs`

**Code (advisory):**

```rust
// fallback_symbol_search signature:
fn fallback_symbol_search(
    &self,
    ref_name: &str,
    is_qualified: bool,
    caller_file_path: Option<&Path>,
    ref_kind: ReferenceKind,  // NEW
) -> Result<Option<Symbol>> {
    if is_qualified {
        return self.db.get_symbol_by_qualified_name(ref_name);
    }

    // Same-crate scoping (unchanged — search_symbol_by_name_in_path_prefix
    // is unaffected by this slice).
    if let Some(path) = caller_file_path {
        // ... existing logic ...
    }

    // Unscoped fallback — pass ref_kind for kind-compatibility filtering.
    let Some(symbol) = self.db.search_unique_symbol_by_name(ref_name, ref_kind)? else {
        return Ok(None);
    };
    // ... existing file_id check ...
}

// At the only call site (try_resolve_reference line 205-206):
if let Some(symbol) =
    self.fallback_symbol_search(
        ref_name,
        is_qualified,
        ctx.current_file_path,
        ref_.kind.clone(),  // NEW
    )?
{
    // ...
}
```

**Doc-comment precondition classification:** None added.

**Output stream classification:** N/A.

**Verification:**
- [ ] `cargo nextest run -p tethys` passes (existing resolver tests
      unchanged in behavior)
- [ ] Re-index rivets workspace: `tethys.exe index --rebuild`
- [ ] `probe.py`: FORBIDDEN-pair phantom refs drop from 174 to ≤ 87
- [ ] `audit_simulation.py`: post-re-index, "phantom demoted" count is
      0 (binary did the work; nothing left for the sim to demote);
      "phantom surviving" stays at 72 (the kind-compatible residuals)
- [ ] `cargo clippy -p tethys --all-targets -- -D warnings` clean

---

## Slice 3: Integration test for rivets-3d0s shape

**Claim:** A new regression test in `tests/resolver_routing.rs` exhibits
the rivets-3d0s shape — a type-position reference to a workspace
enum_variant — and asserts no phantom cross-crate `file_deps` edge is
created. Maps to design claim C6 (existing tests pass + new test gates
the bug class going forward).

**Oracle:** `cargo nextest run --test resolver_routing`. Plus an explicit
falsifiability check: revert slice 2's `resolve.rs` change, re-run, new
test must FAIL with a phantom edge; restore, re-run, new test PASSES.

**Stress fixture:** Two-crate workspace where:
- `crate_caller/src/lib.rs` has `pub fn entry<T: Serialize>(t: T) -> Bar { ... }`
  — type-position reference to `Serialize`
- `crate_caller/src/inner.rs` defines `Bar` (unrelated; provides the
  `use` statement to bypass Pass-2 import-less file short-circuit)
- `crate_target/src/lib.rs` has `pub enum Error { Serialize(String) }`
  — same-named enum_variant

Pre-fix (before slice 2 lands): `T: Serialize` falls through to
unscoped fallback, finds `Error::Serialize` as the unique workspace
match, resolves to it. `file_deps` records `crate_caller/src/lib.rs ->
crate_target/src/lib.rs`. Phantom.

Post-fix: `search_unique_symbol_by_name(name=Serialize, ref_kind=Type)`
filters out `EnumVariant`. Returns `None`. Ref stays unresolved. No
phantom edge.

Plausible bugs the fixture catches:
- Type filter doesn't actually exclude enum_variant
- `ref_kind` is hardcoded somewhere instead of forwarded from `ref_.kind`
- Pass-2 short-circuit on import-less files masks the test (this is
  why `crate_caller/src/lib.rs` has the unrelated `use crate::inner::Bar`)

**Loop budget:** Single integration test, indexing 5 files. ~200ms wall.

**Wall budget:** N/A (test, not always-on).

**Files:**
- `crates/tethys/tests/resolver_routing.rs`

**Code (advisory):**

```rust
/// rivets-3d0s regression test. A type-position reference like
/// `T: Serialize` must not resolve to a workspace enum_variant named
/// `Serialize` (e.g., `Error::Serialize` in a sibling crate), because
/// enum variants can never legitimately occupy a type position.
///
/// The audit at `search_unique_symbol_by_name` filters this out by
/// matching only against type-shaped sym_kinds (Struct, Class, Enum,
/// Trait, Interface, TypeAlias). A regression that drops the kind
/// filter for ref_kind=Type would re-introduce the phantom edge below.
#[test]
fn audit_demotes_type_position_ref_to_enum_variant() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("Cargo.toml", r#"
[workspace]
members = ["crate_caller", "crate_target"]
resolver = "2"
"#),
        ("crate_caller/Cargo.toml", r#"
[package]
name = "crate_caller"
version = "0.1.0"
edition = "2021"
"#),
        ("crate_caller/src/lib.rs", r"
mod inner;

use crate::inner::Bar;

pub fn entry<T>(_t: T) -> Bar
where
    T: Serialize,
{
    Bar
}
"),
        ("crate_caller/src/inner.rs", r"
pub struct Bar;
"),
        ("crate_target/Cargo.toml", r#"
[package]
name = "crate_target"
version = "0.1.0"
edition = "2021"
"#),
        ("crate_target/src/lib.rs", r"
pub enum Error {
    Serialize(String),
}
"),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);
    let edges: Vec<(String, String)> = conn
        .prepare(
            "SELECT f1.path, f2.path
             FROM file_deps d
             JOIN files f1 ON f1.id = d.from_file_id
             JOIN files f2 ON f2.id = d.to_file_id",
        )
        .expect("prepare file_deps query")
        .query_map(params![], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");

    let phantom: Vec<&(String, String)> = edges
        .iter()
        .filter(|(from, to)| from.starts_with("crate_caller/") && to.starts_with("crate_target/"))
        .collect();
    assert!(
        phantom.is_empty(),
        "audit should prevent type-position match to enum_variant; \
         got phantom edges: {phantom:?}"
    );
}
```

**Doc-comment precondition classification:** N/A (test file).

**Output stream classification:** Test failures via `assert!` go to
stderr via panic — diagnostic. Compliant.

**Verification:**
- [ ] `cargo nextest run --test resolver_routing audit_demotes_type_position_ref_to_enum_variant`
      passes
- [ ] **Falsifiability check:** temporarily revert slice 2's call-site
      change (or replace `ref_.kind.clone()` with
      `ReferenceKind::Unknown(String::new())`), re-run, test FAILS with
      a phantom edge. Restore.
- [ ] All resolver_routing tests pass together (`cargo nextest run -p
      tethys --test resolver_routing`)

---

## Plan Self-Review

### Every loop in the plan
1. **Slice 1 SQL query:** `O(log n + k)` where n = workspace symbols ≈ 3000, k ≤ 2 (LIMIT). ~15 ops/call. Within budget.
2. **Slice 2:** No new loops.
3. **Slice 3:** Integration test indexes a 5-file fixture; bounded by fixture size, not by any new loop.

All loops have explicit complexity statements. None over budget at production scale.

### Every fixture
1. **Slice 1:** Two name-colliding symbols across kinds (Struct + EnumVariant named "Foo"). Adversarial: catches both "filter doesn't fire" and "filter breaks ambiguity logic." Not a happy-path exercise.
2. **Slice 2:** Real rivets workspace re-index. Adversarial by virtue of scale (3000 symbols, 6000+ refs). Catches parameter-threading bugs that wouldn't show on small fixtures.
3. **Slice 3:** Two-crate fixture with the exact rivets-3d0s shape. Adversarial: catches kind-filter regression specifically. Includes a Pass-2-bypass mitigation (`use crate::inner::Bar`) to avoid the import-less short-circuit surfaced as a side finding in prove-it-prototype.

All three fixtures designed against named plausible bugs, not happy paths.

### Every doc-comment precondition
1. **Slice 1:** Doc comment on `search_unique_symbol_by_name` documents the new `ref_kind` parameter's effect on the filter. No caller-side precondition introduced (any `ReferenceKind` is valid; the function's behavior changes based on input but doesn't refuse any input).
2. **Slice 1's `kind_filter_sql` helper:** Internal function, no public preconditions.
3. **Slice 2:** Doc comment on `fallback_symbol_search` updated to mention the `ref_kind` threading. No preconditions.
4. **Slice 3:** Test doc comment is descriptive (what the test guards against). No preconditions.

No load-bearing preconditions to enforce. No `debug_assert!`s needed.

### Every write target
1. **Slice 1:** Function is read-only DB query. No writes.
2. **Slice 2:** Function is read-only resolution. The DB writes happen elsewhere (Pass 2 calls `db.resolve_reference(...)` which writes `symbol_id` on the ref). Unchanged by this slice.
3. **Slice 3:** Test writes only to assertion macros (which produce stderr via panic on failure). Diagnostic, not data. Compliant.

No `println!` or unexamined stream writes added.

## Hard gate
- [x] Every slice has all mandatory fields filled in (claim, oracle, stress fixture, loop budget, wall budget where applicable, files, code, verification)
- [x] Every loop has a complexity statement
- [x] Every slice has a stress fixture
- [x] The plan's claim coverage matches the design's claim list:
  - Slice 1 → C1, C2 (kind-compatibility rules)
  - Slice 2 → C3 (same-crate exemption is architectural, auto-satisfied because only the unscoped path is touched), C4 (≥ 50% reduction), C5 (≤ 10 ALLOWED-pair false positives — verified via post-build `audit_simulation.py` run)
  - Slice 3 → C6 (existing tests + new test pass)

Ready for `checkpointed-build`.
