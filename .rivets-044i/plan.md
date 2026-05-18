# rivets-044i: Budgeted plan

## Coverage map

| Design claim | Slice |
|--------------|-------|
| 1 (longest-prefix split algorithm) | 1 |
| 2 (shape #1 same-crate qualified ref resolves) | 1 |
| 3 (shape #2 workspace-crate-prefix ref resolves) | 1 |
| 4 (external-crate refs stay unresolved) | 2 |
| 5 (`crate::` qualified ref resolves) | 2 |
| 6 (unqualified-ref behavior unchanged) | 1 (existing pass2_no_imports test re-run) |
| 7 (no rivets-3d0s phantom-rate regression) | 3 |
| 8 (no rivets-0gom ambiguity regression) | 3 |

---

## Slice 1: Mechanism — implement `qualified_module_fallback` and wire into `try_resolve_reference`.

**Claim:** Longest-prefix split with implicit-crate-first interpretation
produces the correct (file_id, symbol) pair for shapes s-submod and
s-wscrate (covering the probe's two failing tests), and is invoked only
after the existing explicit/glob/fallback paths return None for qualified
refs.

**Oracle:** The two `#[test]` cases in `tests/probe_044i.rs` (renamed to
`tests/pass2_qualified_paths.rs` to drop the probe nomenclature). Each
test independently asserts `refs.symbol_id == symbols.id` for the
expected target via SQL — independent of resolver internals. Same
oracles as the prove-it-prototype run.

**Stress fixture:** The two probe-inherited fixtures defeat the
non-bug-fix-baseline, but the slice ALSO needs an adversarial fixture
that would catch the most plausible bug class: **misordered
interpretation precedence.** Plausible bug: the algorithm tries
as-written before implicit-crate, so a ref where `s1` is *both* a
workspace crate name *and* a submodule of the current crate would
resolve to the wrong file.

Fixture: workspace with two members, `crate_a` and `helper` (which is
ALSO a workspace member). Inside `crate_a`, declare `mod helper;` with
`crate_a/src/helper.rs::pub fn local_thing()`. The workspace crate
`helper` defines `helper/src/lib.rs::pub fn external_thing()`. Inside
`crate_a/src/lib.rs` (import-less), ref `helper::local_thing()`.

Per Rust scoping the submodule shadows the extern crate, so the ref
MUST resolve to `crate_a/src/helper.rs::local_thing`, NOT to
`helper/src/lib.rs`. This fixture is added as
`pass2_qualified_paths::submodule_shadows_workspace_crate`.

**Loop budget:**
- Per ref reaching the new fallback: O(S × (M + L)) where
  - S = `ref_name.split("::").count()` (typically ≤ 5)
  - M = `resolve_module_path` cost: O(S) string ops + filesystem
    `.exists()` syscalls (≤ 2 per call: tries .rs file, then mod.rs)
  - L = `search_symbol_by_qualified_name_in_file` cost: 1 SQL point
    lookup on an indexed column (O(log #symbols))
- Worst case per ref: ≤ 5 splits × (2 interpretations × (5 ops + 2
  syscalls) + 1 SQL lookup) ≈ 50 ops + 20 syscalls + 5 SQL queries.
- Production scale (rivets workspace): refs reaching new fallback ≤ U,
  where U is the count of currently-unresolved qualified refs missing
  all existing paths. From baseline metrics in `.rivets-ycaq`, total
  unresolved refs ≈ 70k workspace-wide; qualified subset ≤ 40k; subset
  reaching the new fallback can't exceed that. Realistic estimate
  (informed by the probe-shape pattern): hundreds to low thousands.
- Total: at most ~5k × (20 syscalls + 5 SQL) ≈ 10^5 syscalls + 25k SQL
  queries.
- **Verdict:** within the 10^6 ops cap. **Above** the 10^3 syscalls
  cap by ~10×, justified: syscalls are filesystem `.exists()` calls
  that are OS-cached after the first access; the dominant cost is SQL
  lookups, which are point queries on `(qualified_name, file_id)`
  index already used by Pass 2 today.

**Wall budget:** Post-slice `cargo run --bin tethys -- index` on the
rivets workspace ≤ 1.5× pre-slice indexing wall time. Measured via
hyperfine in slice 3's verification.

**Files:**
- `crates/tethys/src/resolve.rs` (add private method
  `qualified_module_fallback`, modify `try_resolve_reference` call site)
- `crates/tethys/tests/probe_044i.rs` → renamed to
  `crates/tethys/tests/pass2_qualified_paths.rs` (add the
  submodule-shadowing fixture and drop the "probe" naming since it's
  now a permanent regression fence)

**Code (advisory):**

```rust
// In resolve.rs

/// Attempt to resolve a qualified reference by interpreting its prefix
/// as a module path and querying the resulting file for the tail symbol.
///
/// Activates only after all import-based paths and the existing
/// `fallback_symbol_search` qualified-name lookup return None.
/// Tries each prefix split position from longest to shortest. For each
/// prefix, attempts two interpretations in order: (a) implicit-crate
/// (prepend `crate::`, skipped if path[0] is already `crate`/`self`/
/// `super`); (b) as-written. The first file_id obtained is queried with
/// `search_symbol_by_qualified_name_in_file` on the tail.
///
/// Returns `Ok(None)` for unqualified names, external-crate refs, and
/// any ref whose tail doesn't match a symbol in the resolved file.
fn qualified_module_fallback(
    &self,
    ref_name: &str,
    current_file_path: Option<&Path>,
    src_root: &Path,
) -> Result<Option<Symbol>> {
    let Some(current_file) = current_file_path else {
        return Ok(None);
    };
    let segments: Vec<&str> = ref_name.split("::").collect();
    if segments.len() < 2 {
        return Ok(None);
    }

    let crates = self.crates();
    for split in (1..segments.len()).rev() {
        let prefix = &segments[..split];
        let tail = segments[split..].join("::");

        // Interpretation A: implicit-crate (skip if already prefixed).
        let mut file_id: Option<FileId> = None;
        if !matches!(prefix[0], "crate" | "self" | "super") {
            let mut with_crate: Vec<String> = Vec::with_capacity(prefix.len() + 1);
            with_crate.push("crate".to_string());
            with_crate.extend(prefix.iter().map(|s| (*s).to_string()));
            if let Some(p) = resolve_module_path(&with_crate, current_file, src_root, crates) {
                let rel = self.relative_path(&p);
                file_id = self.db.get_file_id(&rel)?;
            }
        }

        // Interpretation B: as-written.
        if file_id.is_none() {
            let as_owned: Vec<String> = prefix.iter().map(|s| (*s).to_string()).collect();
            if let Some(p) = resolve_module_path(&as_owned, current_file, src_root, crates) {
                let rel = self.relative_path(&p);
                file_id = self.db.get_file_id(&rel)?;
            }
        }

        let Some(file_id) = file_id else { continue };
        if let Some(sym) = self.db.search_symbol_by_qualified_name_in_file(&tail, file_id)? {
            return Ok(Some(sym));
        }
    }
    Ok(None)
}

// In try_resolve_reference, after fallback_symbol_search returns None:
if is_qualified {
    if let Some(symbol) = self.qualified_module_fallback(
        ref_name,
        ctx.current_file_path,
        ctx.src_root,
    )? {
        trace!(
            ref_id = ref_.id,
            ref_name = %ref_name,
            symbol_id = %symbol.id,
            "Resolved reference via qualified module fallback"
        );
        self.db.resolve_reference(ref_.id, symbol.id)?;
        return Ok(true);
    }
}
```

**Verification:**
- [ ] `cargo nextest run -p tethys --test pass2_qualified_paths` — both
  inherited probe tests + the submodule-shadows-workspace-crate fixture
  pass.
- [ ] `cargo nextest run -p tethys --test pass2_no_imports` — passes
  unchanged (claim 6 non-regression).
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` clean.
- [ ] Oracle (prove-it-prototype) still agrees with binary: re-run
  probe tests, check assertions.

---

## Slice 2: Adversarial coverage — claims 4 + 5.

**Claim:**
- (claim 4) External-crate-prefixed refs (`std::collections::HashMap`,
  `serde::Deserialize`) do NOT phantom-resolve via the new path.
- (claim 5) `crate::sub::Item` qualified refs resolve under the
  s-crate first-segment shape.

**Oracle:** SQL queries against the in-memory test DB, independent of
resolver internals — same pattern as slice 1's oracles.

**Stress fixture:**
- **`qualified_external_crate_stays_unresolved`**: import-less file
  containing a ref `std::collections::HashMap`. The DB should have
  ZERO refs in that file with `reference_name LIKE 'std::%'` AND
  `symbol_id IS NOT NULL`. Plausible bug class this fixture defeats:
  algorithm naively trying `["crate","std",...]` and matching a
  same-named submodule. Adversarial twist: ALSO add a `mod std_helper;`
  (sic — looks std-ish) to defeat partial-string-match bugs.
- **`qualified_crate_prefix_resolves`**: import-less `crate_a/src/lib.rs`
  with `mod sub;`, ref `crate::sub::Thing`. `crate_a/src/sub.rs` defines
  `pub struct Thing`. Plausible bug class: the as-written path
  incorrectly fires before implicit-crate, resolving "crate" as a
  workspace-crate-name and failing. The fixture would catch that
  because there's no crate named "crate" — the only path that resolves
  is the `crate::` arm of `resolve_module_path`.

**Loop budget:** Inherits slice 1's `qualified_module_fallback` cost
budget. No new loops introduced.

**Wall budget:** N/A — test-only slice.

**Files:**
- `crates/tethys/tests/pass2_qualified_paths.rs` (add 2 new
  `#[test]` cases).

**Code (advisory):**

```rust
#[test]
fn qualified_external_crate_stays_unresolved() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("Cargo.toml", /* crate_a single-package */),
        ("src/lib.rs", r"
mod std_helper;
pub fn entry() {
    let _ = std::collections::HashMap::<u32, u32>::new();
    let _ = std_helper::do_local();
}
"),
        ("src/std_helper.rs", "pub fn do_local() -> i32 { 1 }"),
    ]);
    tethys.index().expect("index");
    let conn = open_db(&tethys);

    // External-crate prefix MUST NOT resolve.
    let std_resolved: i64 = conn.query_row(
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id = r.file_id
         WHERE f.path = 'src/lib.rs'
           AND r.symbol_id IS NOT NULL
           AND (r.reference_name LIKE 'std::%' OR EXISTS (
               SELECT 1 FROM symbols s WHERE s.id = r.symbol_id
                 AND s.name IN ('HashMap','collections')))",
        params![], |r| r.get(0),
    ).expect("count");
    assert_eq!(std_resolved, 0,
        "std::collections::HashMap must stay unresolved");

    // Local lookalike submodule still resolves (sanity).
    let local_resolved: i64 = conn.query_row(
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id
         JOIN symbols s ON s.id=r.symbol_id
         WHERE f.path='src/lib.rs' AND s.name='do_local'",
        params![], |r| r.get(0),
    ).expect("count");
    assert!(local_resolved >= 1, "std_helper::do_local must resolve locally");
}

#[test]
fn qualified_crate_prefix_resolves() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("Cargo.toml", /* crate_a single-package */),
        ("src/lib.rs", r"
mod sub;
pub fn entry() { let _: crate::sub::Thing; }
"),
        ("src/sub.rs", "pub struct Thing;"),
    ]);
    tethys.index().expect("index");
    let conn = open_db(&tethys);
    let resolved_to_target: i64 = conn.query_row(
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id
         JOIN symbols s ON s.id=r.symbol_id
         WHERE f.path='src/lib.rs' AND s.name='Thing'",
        params![], |r| r.get(0),
    ).expect("count");
    assert!(resolved_to_target >= 1,
        "crate::sub::Thing must resolve to Thing in sub.rs");
}
```

**Verification:**
- [ ] Both new tests pass with the slice-1 mechanism in place.
- [ ] All existing tests still pass.

---

## Slice 3: Workspace verification — claims 7 + 8 + wall-budget check.

**Claim:** The PR introduces no regression in:
- Cross-crate file_deps phantom rate (rivets-3d0s; must stay 0.00%).
- rivets-0gom ambiguity violations (must not increase).
- Indexing wall time (must stay within 1.5× pre-fix baseline).

**Oracle:**
- Phantom-rate: `.rivets-ycaq/probe_phantom_rate.py` (independent SQL
  classification of file_deps edges).
- Ambiguity: `.rivets-0gom/probe.py` Section 3 (independent ref-vs-symbol
  cross-crate classification).
- Wall time: `hyperfine` against `cargo run --bin tethys -- index` with
  warmup.

**Stress fixture:** The rivets workspace itself. This is the production
input the design's measurement-based claims target. No synthetic
fixture; we re-index real code.

**Loop budget:** N/A — verification slice, no new loops introduced.

**Wall budget:**
- Indexing post-fix ≤ 1.5× pre-fix baseline (measured fresh in this
  slice to set a real number; pre-fix snapshot is checked into
  `.rivets-044i/wall-time-baseline.txt`).

**Files:**
- `.rivets-044i/wall-time-baseline.txt` (new — pre-fix indexing time
  from hyperfine).
- `.rivets-044i/wall-time-post.txt` (new — post-fix).
- `.rivets-044i/phantom-rate-post.txt` (new — output of probe).
- `.rivets-044i/ambiguity-post.txt` (new — output of 0gom Section 3
  probe).
- **No production code change in this slice.**

**Code (advisory):** Shell-script-style verification steps:

```sh
# Pre-slice-1: baseline (must be captured BEFORE slice 1 lands).
hyperfine --warmup 1 --runs 3 'cargo run --release --bin tethys -- index' \
    --export-markdown .rivets-044i/wall-time-baseline.txt

# Post-slice-1+2: re-measure.
hyperfine --warmup 1 --runs 3 'cargo run --release --bin tethys -- index' \
    --export-markdown .rivets-044i/wall-time-post.txt

# Phantom-rate (regression fence for claim 7).
python .rivets-ycaq/probe_phantom_rate.py > .rivets-044i/phantom-rate-post.txt
grep '0.00' .rivets-044i/phantom-rate-post.txt   # must show 0.00%

# Ambiguity (regression fence for claim 8).
python .rivets-0gom/probe.py > .rivets-044i/ambiguity-post.txt
# Section 3 violation count must be ≤ baseline (read from .rivets-0gom/measurement-* if extant).
```

**Verification:**
- [ ] `phantom-rate-post.txt` shows cross-crate phantom rate = 0.00%.
- [ ] `ambiguity-post.txt` Section 3 violation count ≤ baseline.
- [ ] `wall-time-post.txt` mean ≤ 1.5× `wall-time-baseline.txt` mean.
- [ ] CI regression fences pass:
  `cargo nextest run -p tethys --test file_deps_corroboration --test resolver_routing`.

---

## Plan self-review

### List 1: every loop, its complexity statement, and budget verdict

| Loop | Complexity | Production scale | Verdict |
|------|-----------|------------------|---------|
| `qualified_module_fallback`'s `for split in (1..segments.len()).rev()` | O(S) per ref where S ≤ 5 typical | Hundreds to low thousands of refs reach this fallback on rivets | within ops cap; syscall cap exceeded ~10× but OS-cached |

No other new loops. Existing loops (`for (file_id, refs)`, `for ref_ in
refs`) unchanged by this PR.

### List 2: every fixture, the bug class it defeats

| Slice | Fixture | Plausible bug it defeats |
|-------|---------|--------------------------|
| 1 | `probe_044i_qualified_ref_from_import_less_file` (inherited) | "qualified ref from import-less file misses fallback" |
| 1 | `probe_044i_workspace_crate_prefix_from_import_less_file` (inherited) | "workspace-crate-prefix path bypasses qualified lookup" |
| 1 | `submodule_shadows_workspace_crate` (new) | "interpretation-precedence inverted: extern crate found before submodule" |
| 2 | `qualified_external_crate_stays_unresolved` | "implicit-crate prepend phantoms an external-crate ref" |
| 2 | `qualified_crate_prefix_resolves` | "implicit-crate retry shadows the explicit `crate::` path" |
| 3 | rivets workspace as a whole | "fix introduces phantom file_deps or 0gom-class ambiguity" |

No happy-path-only fixtures.

### List 3: every doc-comment precondition

| Function | Precondition | Class | Enforcement |
|----------|--------------|-------|-------------|
| `qualified_module_fallback` | "Activates only after import-based paths and `fallback_symbol_search` return None." | Sanity hint (caller contract — fires from one site only). | No assertion needed; single call site in `try_resolve_reference`. |
| `qualified_module_fallback` | Returns `Ok(None)` for unqualified names. | Load-bearing for correctness — wrong output if violated would be a phantom resolution. | Runtime check: `if segments.len() < 2 { return Ok(None); }`. Survives release builds. |
| `qualified_module_fallback` | Returns `Ok(None)` if `current_file_path` is `None`. | Load-bearing — `resolve_module_path` requires a current file path. | Runtime check: `let Some(current_file) = current_file_path else { return Ok(None); }`. |

### List 4: every write target

| Write | Stream | Class |
|-------|--------|-------|
| `trace!(... "Resolved reference via qualified module fallback")` | stderr (tracing default) | Diagnostic |
| `db.resolve_reference(...)` | SQLite refs table | Data (intended persistent output) |

No `println!`/`eprintln!` introduced.

### List 5: tracker references

| Reference | Tracker ID | Verified |
|-----------|------------|----------|
| Perf-cache deferral for `qualified_module_fallback` | `rivets-bjdn` | Open, P4 (verified 2026-05-18) |
| C# Pass-2 namespace gap | `rivets-jwf9` | Open, P3 (verified 2026-05-18) |
| Phantom-rate regression fence | `crates/tethys/tests/file_deps_corroboration.rs::k_hybrid_drops_cross_crate_call_without_import_corroboration` | Present (verified 2026-05-18) |
| Ambiguity regression fence | `crates/tethys/tests/resolver_routing.rs::fallback_routes_unqualified_ref_to_same_crate_not_cross_crate` | Present (verified 2026-05-18) |
| Parent issue | `rivets-044i` | Open, P3 (verified 2026-05-18) |

No un-tracked deferrals.
