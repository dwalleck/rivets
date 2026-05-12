# Plan: fix tethys resolver phantom cross-crate edges (rivets-0gom)

> Following gilfoyle/budgeted-plan. Three slices, each with mandatory complexity
> budget, scale budget, and stress fixture. The persistent oracle from
> prove-it-prototype (`.rivets-0gom/probe.py` + `oracle.sh`) is the integration
> gate for every slice.

## Pre-fix baselines (captured 2026-05-11)

- **Cross-crate edges**: 170 pairs, 149 phantom (88%). See `.rivets-0gom/baseline-pre-fix.txt`.
- **Intra-crate edges**: tethys=220, rivets=75, rivets-mcp=11, rivets-jsonl=9 (total 315).
- **Ambiguity violations** (probe section 3): 83 (top: `is_empty` x42 from rivets, `path` x34 from rivets-jsonl).
- **Indexing wall clock**: 52.856 ± 3.586 s mean (hyperfine, 3 runs). Captured in `.rivets-0gom/index-baseline.md`.

The probe and oracle re-run at every slice's integration gate. Each claim from the design has an *explicit* verification mechanism, listed below.

## Per-claim verification matrix

| Design claim | Verified by |
|---|---|
| 1 — bug at db/symbols.rs:244 | `cheapest_falsifier.py` (already passed pre-implementation) |
| 2 — fallback prefers same-crate | Slice 1 unit tests on the new function (controlled fixture) |
| 3 — FORBIDDEN pairs → 0 edges | Probe **Section 1**: all 10 FORBIDDEN ordered pairs show 0 cross-crate edges |
| 4 — ALLOWED pairs preserved | Probe **Section 1**: rivets→rivets-jsonl and rivets-mcp→rivets both remain non-zero. Counts MAY decrease when the source-crate shadows a name from the target-crate (the same-crate symbol is then the correct resolution, and the prior cross-crate edge was a phantom misclassified as ALLOWED). The check is "non-zero and the remaining edges resolve to legitimate target-crate domain symbols," verified by `diagnose_drop.py`. |
| 5 — no intra-crate edge lost | Probe **Section 2**: every pre-fix intra-crate count is ≤ post-fix count (monotonically non-decreasing; new ones may appear when phantoms convert to same-crate edges) |
| 6 — genuine ambiguity → None | Probe **Section 3**: ambiguity violation count drops from 83 to 0 |

If any of these fails, the responsible slice's `checkpointed-build` gate fails and execution stops.

---

## Slice 1: add `search_symbol_by_name_in_path_prefix`

**Claim verified by this slice:** 2 (fallback prefers same-crate match, when one exists).

**Oracle:** unit tests on the new function with seeded fixtures. No probe-level effect at this slice (resolver still uses the old path).

**Stress fixture:**
- Two synthetic files, one at `crate_a/src/lib.rs` and one at `crate_b/src/lib.rs`, each defining a symbol named `Foo`.
- Test 1: call new function with prefix `"crate_a/"`. Assert: returned symbol's `file_id` corresponds to `crate_a/src/lib.rs`, NOT `crate_b/src/lib.rs`.
- Test 2: same setup, prefix `"crate_b/"`. Assert: returned symbol's file is in crate_b.
- Test 3: same setup, prefix `"crate_c/"` (no files). Assert: returns `None`.

Adversarial bug class: an implementation that ignores the prefix and returns the workspace-first match fails Tests 1 and 2 simultaneously. An implementation that hard-codes a prefix or returns the same symbol regardless fails Test 3.

**Loop budget:** One SQL query per call. Uses index on `symbols(name)`; JOIN on `files.id` is PK-indexed; the `LIKE prefix || '%'` filter runs over the per-name candidate set (≤ ~10 per common name at rivets scale, ≤ ~100 in worst case). Sub-millisecond per call.

**Wall budget:** N/A — no caller in this slice; the function is added but unused.

**Files:**
- Modify: `crates/tethys/src/db/symbols.rs`

**Code (advisory):**

```rust
/// Search for a symbol by name, restricted to files whose path begins with `path_prefix`.
///
/// Used by `fallback_symbol_search` to prefer same-crate matches before
/// falling back to the unscoped `search_symbol_by_name`. The prefix is
/// typically the caller's containing crate path (e.g. `"crates/tethys/"`).
///
/// Returns `None` if no symbol with that name exists under the given prefix.
pub fn search_symbol_by_name_in_path_prefix(
    &self,
    name: &str,
    path_prefix: &str,
) -> Result<Option<Symbol>> {
    debug_assert!(!path_prefix.is_empty(), "path_prefix must not be empty");
    let conn = self.connection()?;
    let like_pattern = format!("{path_prefix}%");
    conn.query_row(
        &format!(
            "SELECT {SYMBOLS_COLUMNS} FROM symbols s
             JOIN files f ON f.id = s.file_id
             WHERE s.name = ?1 AND f.path LIKE ?2
             LIMIT 1"
        ),
        params![name, like_pattern],
        row_to_symbol,
    )
    .optional()
    .map_err(Into::into)
}
```

**Verification:**
- [ ] Three unit tests pass (one per stress-fixture case)
- [ ] `cargo nextest run -p tethys` clean (no regression)
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo fmt --check` clean

---

## Slice 2: route `fallback_symbol_search` through the new function

**Claims verified by this slice:** 3 (FORBIDDEN → 0), 4 (ALLOWED preserved), 5 (intra-crate non-decreasing).

**Oracle:**
- **Primary**: `.rivets-0gom/probe.py` against current branch + `.rivets-0gom/oracle.sh`. Three sections compared to baseline.
- **Resolver-level unit tests are intentionally NOT added in this slice.** Building resolver-scaffolding tests duplicates what the probe-vs-oracle integration gate already proves. If the probe agrees with the oracle, slice 2 works. If it doesn't, the unit tests wouldn't catch what the integration gate misses. Drop the duplication.

**Stress fixture:** the rivets workspace itself.

Why: the workspace has 4 crates, all of which have a file named `error.rs`. It has `Error`, `Result`, `Warning`, `FileId`, `is_empty`, `path`, `sort`, `kind` as symbol names that collide across crates. It has 83 ambiguity violations. It has 149 phantom edges. It is, by accident, the perfect stress fixture for this slice. If the fix works on the rivets workspace, it works.

Adversarial bug class: an implementation that forgets to call the new function leaves the phantom count at 149 (the probe catches this). An implementation that only calls the new function and never falls back drops the ALLOWED counts to zero (the probe catches this too). An implementation that passes the wrong prefix fails on a per-pair basis (the probe shows which pair).

**Loop budget:** One extra SQL query per fallback resolution. At rivets scale: ~21k references; not all reach fallback. Conservative estimate: 10% reach fallback = 2k extra queries. At sub-millisecond per query: < 2s added.

**Wall budget:** Indexing baseline is **52.856 ± 3.586 s** mean (hyperfine, 3 runs, `.rivets-0gom/index-baseline.md`). Post-slice-2 mean must be ≤ **58 s** (baseline mean + 5s margin). Measurement command, run at slice 2's integration gate:

```bash
hyperfine --warmup 0 --runs 3 --export-markdown .rivets-0gom/index-after-slice2.md 'target\release\tethys.exe index'
```

If post-slice-2 mean > 58s, STOP. Slice 2's per-fallback query is too expensive in practice — either the LIKE pattern is hitting a sequential scan, or the query count is higher than estimated.

**Files:**
- Modify: `crates/tethys/src/resolve.rs` (signature of `fallback_symbol_search` + the call site at line 202)

**Code (advisory):**

```rust
fn fallback_symbol_search(
    &self,
    ref_name: &str,
    is_qualified: bool,
    caller_file_path: Option<&Path>,
) -> Result<Option<Symbol>> {
    if is_qualified {
        return self.db.get_symbol_by_qualified_name(ref_name);
    }

    // Try same-crate first.
    if let Some(path) = caller_file_path
        && let Some(crate_info) = self.get_crate_for_file(path)
    {
        let relative = self.relative_path(&crate_info.path);
        let prefix = format!("{}/", relative.to_string_lossy());
        if let Some(symbol) =
            self.db.search_symbol_by_name_in_path_prefix(ref_name, &prefix)?
            && self.db.get_file_by_id(symbol.file_id)?.is_some()
        {
            return Ok(Some(symbol));
        }
    }

    // Fall back to unscoped (slice 3 will harden this against ambiguity).
    let Some(symbol) = self.db.search_symbol_by_name(ref_name)? else {
        return Ok(None);
    };
    if self.db.get_file_by_id(symbol.file_id)?.is_some() {
        Ok(Some(symbol))
    } else {
        Ok(None)
    }
}
```

Call site at resolve.rs:202:
```rust
if let Some(symbol) = self.fallback_symbol_search(ref_name, is_qualified, ctx.current_file_path)? {
```

**Verification (integration gate):**
- [ ] `cargo nextest run -p tethys` clean (no regression in the existing 576+ tests)
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] Re-index: `target\release\tethys.exe index` succeeds with no warnings
- [ ] **Probe Section 1 (claims 3, 4)**: re-run `.rivets-0gom/probe.py`. Most FORBIDDEN ordered pairs trend toward 0; the residual is slice 3's domain. Both ALLOWED pairs remain non-zero, with magnitude possibly *lower* than baseline when the source crate shadows target-crate names (those phantom-on-allowed edges are correctly retired by same-crate-first). If an ALLOWED pair reaches 0, STOP — that would mean we lost every legitimate cross-crate ref to that target.
- [ ] **Probe Section 2 (claim 5)**: each crate's intra-crate edge count is ≥ baseline (tethys ≥ 220, rivets ≥ 75, rivets-mcp ≥ 11, rivets-jsonl ≥ 9). Lower → STOP, slice 2 has lost legitimate edges.
- [ ] **Probe Section 3 (claim 6 progress)**: record post-slice-2 ambiguity violation count. If 0 → slice 3 is unnecessary (see slice 3). If > 0 → slice 3 is required.
- [ ] **Wall budget**: hyperfine mean ≤ 58s. If higher → STOP.

---

## Slice 3 (conditional): detect genuine ambiguity, return None

**Run this slice ONLY IF** Slice 2's integration gate showed probe Section 3 ambiguity violation count > 0. If it dropped to 0 (no caller has a missing same-crate match alongside multiple cross-crate candidates), this slice is unnecessary and is skipped.

**Rationale for being conditional:** the rivets workspace's ambiguity violations (83 pre-fix) are driven by names like `is_empty`, `path`, `sort`, `kind` referenced from one crate when *that* crate has no same-named symbol. After slice 2, these might all resolve to the correct same-crate symbol if the caller's crate does in fact define the name. The probe's Section 3 is the authoritative answer. We don't speculate — we measure, then decide.

**Claim verified by this slice (if run):** 6 (genuine ambiguity → None).

**Oracle:**
- Unit tests on `search_symbol_by_name` with seeded multi-crate name collisions.
- Probe Section 3 post-slice-3: ambiguity violation count = 0.

**Stress fixture (if slice runs):**
- Three synthetic files, one per "crate": `crate_a/src/lib.rs`, `crate_b/src/lib.rs`, `crate_c/src/lib.rs`.
- `crate_a` and `crate_b` each define a symbol `Bar`. `crate_c` defines nothing called `Bar`.
- Test 1: call `search_symbol_by_name("Bar")`. Assert: returns `None` (ambiguous across `crate_a` and `crate_b`).
- Test 2: delete `crate_b`'s `Bar` and re-index. Call again. Assert: returns `crate_a`'s `Bar` (unique cross-crate match).

Adversarial bug class: a `LIMIT 1` implementation fails Test 1. An over-aggressive `None`-returning implementation fails Test 2.

**Loop budget:** Same SQL query, `LIMIT 2` instead of `LIMIT 1`. Negligible delta.

**Wall budget:** Measure with hyperfine post-slice-3. Combined slice 2 + slice 3 mean must be ≤ 58s (same budget).

**Files:**
- Modify: `crates/tethys/src/db/symbols.rs` (change `search_symbol_by_name` semantics from "first match" to "unique match")

**Code (advisory):**

```rust
/// Search for a symbol by name across all files.
///
/// Returns the unique match if exactly one symbol exists with that name across
/// the entire indexed workspace. Returns `None` if zero matches OR multiple
/// matches (genuine ambiguity — caller should not guess).
///
/// Callers that want a crate-scoped lookup should use
/// `search_symbol_by_name_in_path_prefix` first; this is the unscoped
/// last-resort fallback.
pub fn search_symbol_by_name(&self, name: &str) -> Result<Option<Symbol>> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare(
        &format!("SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE name = ?1 LIMIT 2")
    )?;
    let rows: Vec<Symbol> = stmt
        .query_map([name], row_to_symbol)?
        .collect::<std::result::Result<_, _>>()?;
    match rows.len() {
        1 => Ok(Some(rows.into_iter().next().unwrap())),
        _ => Ok(None),
    }
}
```

**Verification:**
- [ ] Two unit tests pass
- [ ] `cargo nextest run -p tethys` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] **Probe Section 3 (claim 6)**: ambiguity violation count = 0
- [ ] **Probe Section 1 (claims 3, 4)**: still passing (FORBIDDEN = 0, ALLOWED preserved)
- [ ] **Probe Section 2 (claim 5)**: intra-crate counts still ≥ baseline
- [ ] **Wall budget**: hyperfine mean ≤ 58s

---

## Plan self-review

### 1. Every loop in the plan — complexity and scale

| Slice | Loop | Complexity | Scale at rivets | Within budget? |
|---|---|---|---|---|
| 1 | One SQL query (name + LIKE prefix join) | O(symbols matching name) with `symbols(name)` index | ≤ 10 hits per name | Yes, sub-ms |
| 2 | One extra SQL query per fallback resolution | ~10% of 21k refs reach fallback = ~2k queries | ≤ 2s added to 52.8s baseline | Yes, ≤ 58s budget |
| 3 | Same SQL query, LIMIT 2 instead of LIMIT 1 | O(1) row delta | Negligible | Yes |

### 2. Every stress fixture — what bug class is it designed to fail under?

| Slice | Fixture | Bug class it surfaces |
|---|---|---|
| 1 | Two synthetic crates, same `Foo` in each | Implementation ignores prefix |
| 1 | Same setup, prefix matching neither | Implementation returns non-None despite no path match |
| 2 | The rivets workspace itself | Implementation forgot to call new fn, or passes wrong prefix, or only calls new fn (breaks legitimate cross-crate) |
| 3 | Three crates, two with `Bar`, none in caller's | `LIMIT 1` returns arbitrary; or over-aggressive `None` |

### 3. Every doc-comment precondition — `debug_assert`?

| Slice | Doc comment | Precondition | `debug_assert` |
|---|---|---|---|
| 1 | "prefix is typically the caller's containing crate path" | `!path_prefix.is_empty()` | `debug_assert!(!path_prefix.is_empty(), "path_prefix must not be empty")` |
| 2 | no new doc-comment preconditions (type-enforced) | n/a | n/a |
| 3 | no new doc-comment preconditions | n/a | n/a |

### 4. Every write target — data or diagnostic?

No slice writes to stdout or stderr. All changes are inside the library (SQL queries + resolver logic). `tracing::*` calls are unchanged. The probe (`probe.py`) writes to stdout — test harness, not production. No stream-classification concerns.

---

## Plan output: gates for checkpointed-build

The next skill — `checkpointed-build` — refuses to run until:

- [x] Every slice has all mandatory fields filled in
- [x] Every loop has a complexity statement
- [x] Every slice has a stress fixture designed to fail under a specific bug class
- [x] Plan's claim coverage matches design's claim list (1-6 mapped to verification matrix)
- [x] Wall-budget baseline captured (`.rivets-0gom/index-baseline.md`)
- [x] Probe extended to cover all six claims (Section 1, 2, 3)
- [x] Conditional slice (3) has explicit decision criterion (probe Section 3 > 0)

## Final done-state

The fix is done when:

1. **Probe Section 1**: 10 FORBIDDEN pairs = 0 edges; 2 ALLOWED pairs ≥ baseline counts.
2. **Probe Section 2**: intra-crate counts ≥ baseline per crate.
3. **Probe Section 3**: 0 ambiguity violations (achieved after slice 2 or 3 depending on outcome).
4. **Wall clock**: hyperfine mean ≤ 58s (baseline + 5s margin).
5. **Existing tests**: 576+ unit/integration tests still pass.
6. **Lints**: clippy clean, fmt clean.

Each criterion is independently checkable. None is "the test suite passed." All six must hold.
