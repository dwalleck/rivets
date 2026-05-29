# rivets-wsix design: cascade-correctness regression fences

## Purpose

The wsix audit found **zero bugs** (see `what-i-learned.md`). The schema's
`ON DELETE CASCADE` chain plus per-file `DELETE FROM symbols WHERE file_id`
quietly handles re-index correctness for refs, attributes, and other tables —
not the `clear_all_X` pattern lcb6 established. The remaining risk is **schema
drift**: a future change to the cascade FKs, or a new INSERT site added without
a paired clear or cascade, would silently re-introduce the bug class wsix
worried about.

This design defines integration-test fences that lock in the current
correctness so CI catches schema drift before it merges.

## Approach

**One new integration test file**: `crates/tethys/tests/reindex_cascade.rs`.

Each test exercises one cascade or per-file-clear invariant against a tiny
tempdir fixture, using `Tethys::index()` (the production entry point) and
direct SQLite queries (independent of the indexing pipeline's internal state).
Pattern is the same as `pass2_qualified_paths.rs` from PR #74: `tempfile` +
`workspace_with_files` helper + `rusqlite` for the oracle.

## Falsification

| # | Claim | Falsifier | Oracle | Cost | Status | Regression fence |
|---|-------|-----------|--------|------|--------|------------------|
| 1 | After a file's source is mutated to remove a function-body call, re-indexing without DB reset removes the corresponding row from `refs` (the `in_symbol_id`-cascade chain works). | Construct a fixture with `fn entry() { helper::a(); helper::b(); }`. Index. Mutate source to remove `helper::b()`. Re-index. Count refs in entry's file. Expected: count drops by exactly 1. If unchanged or higher, claim false. | Manually count call expressions in the new source via inspection; SQL `COUNT(*)` against the post-mutation DB. | 5 min | **passed** (probe_refs_bug.sh observed 2 → 3 → 1) | New integration test `refs_cascade_on_call_removal` in `reindex_cascade.rs`. |
| 2 | After a file's source is mutated to remove an entire function definition that carried `#[some_attr]`, re-indexing without DB reset removes BOTH the symbol row AND its `attributes` row (`attributes` cascades via `symbols(id) ON DELETE CASCADE`). | Fixture with `#[allow(dead_code)] fn target() {}`. Index. Capture `(symbol_count, attribute_count)`. Remove the `target` fn from source. Re-index. Capture again. Expected: both decrease by exactly 1. | Direct SQL count: `SELECT COUNT(*) FROM symbols WHERE name = 'target'` and `SELECT COUNT(*) FROM attributes a JOIN symbols s ON s.id = a.symbol_id WHERE s.name = 'target'`. | 5 min | pending | New integration test `attributes_cascade_on_symbol_removal` in `reindex_cascade.rs`. |
| 3 | Re-indexing an unchanged workspace produces stable `call_edges` and `file_deps` counts (the existing `clear_all_X` discipline works; lcb6 + sibling). | Fixture with two files, one calling the other (so call_edges and file_deps have rows). Index twice with no source change. Compare counts. Expected: equal across runs. If second run is greater, the clear_all is missing or not called. | Direct SQL count: `SELECT COUNT(*) FROM call_edges` and `SELECT COUNT(*) FROM file_deps`. | 5 min | pending | New integration test `clear_all_tables_stable_under_reindex` in `reindex_cascade.rs`. Already partially covered by `file_deps_idempotency.rs` (rivets-lcb6) — this test adds the call_edges twin and the joint stability assertion. |

## Self-review (per the v1.0.3 falsifiable-design checklist)

### 1. Claim count
3 claims. In the 3-15 healthy band. Each defends a distinct invariant.

### 2. Falsifier independence
- Claim 1's oracle is direct SQL `COUNT(*)` and tree-sitter-independent manual call enumeration. Probe used Bash + sqlite3 CLI; the test will use Rust + rusqlite. Different mechanisms.
- Claim 2's oracle is direct SQL queries against `symbols` and `attributes`. Indexer is the SUT; the SQL is independent.
- Claim 3's oracle is direct SQL `COUNT(*)`. Independent of indexer.

### 3. Falsifier non-vacuity
For each claim, a concrete buggy implementation that would make the fence fail:

- **Claim 1**: if `refs.in_symbol_id REFERENCES symbols(id) ON DELETE CASCADE` were silently relaxed to `... ON DELETE SET NULL` (or the FK dropped entirely) in a future migration, the cascade wouldn't fire and removed refs would persist. The test asserts a specific count decrease — would fail directly.
- **Claim 2**: if a future schema migration changed `attributes.symbol_id REFERENCES symbols(id) ON DELETE CASCADE` to `ON DELETE NO ACTION`, removed symbols would leave orphan attribute rows. Test would observe `attribute_count` unchanged after symbol removal.
- **Claim 3**: if a future change to `index_with_options` accidentally removed the `clear_all_file_deps()` call at `indexing.rs:139`, file_deps would accumulate across runs and the test's count-stability assertion would fail.

Vacuity checks (per the v1.0.3 anti-pattern list):
- No `column LIKE '%...%' AND symbol_id IS NOT NULL` filters (the I1 shape from PR #74).
- No disjunctive assertions where one disjunct is also asserted standalone (the R2-1 shape).
- All assertions are direct count equalities. TDD inversion: each test would fail under exactly one specific code mutation (the cascade FK removal for Claim 1/2, the `clear_all` removal for Claim 3). None passes-when-bug-present.

### 4. Per-claim verification distinctness
- Claim 1's failure message: "refs count for src/lib.rs did not decrease after removing helper::b()". Localizes to the in_symbol_id cascade.
- Claim 2's failure message: "attributes count for symbol 'target' did not decrease after removing the function definition". Localizes to the symbols→attributes cascade.
- Claim 3's failure message: "call_edges count or file_deps count grew across unchanged-source re-index runs". Localizes to the clear_all discipline.
Each test has a distinct failure mode that names the failing invariant.

### 5. Cost distribution
All three falsifiers are ≤ 5 min each. Cheap, deterministic, CI-friendly. No expensive falsifiers in the design.

### 6. Negative space (what this design deliberately does NOT do)

- **Does not fix any bug.** The audit found zero. This is a regression-fence-only design.
- **Does not cover the orphan-file-from-disk case.** Files deleted from disk leave `files` rows behind; their cascade-dependent rows persist. Tracked separately as **rivets-dhxo**. This design's fixtures all retain the original file paths to isolate the per-file re-index path from the orphan-file path.
- **Does not test the streaming-mode (`IndexOptions::with_streaming()`) indexing variant.** Default full-index path only. Streaming has divergent behavior per dhxo's analysis.
- **Does not add an architectural meta-fence** (e.g., "every new INSERT site in `db/*.rs` must have a paired clear path"). That would be a more invasive test (parsing source code at test time) and is properly its own follow-up issue. If a future PR introduces a new bug-class table, the dhxo-style review process should catch it — but a meta-fence would be belt-and-braces. Filed considerations:
  - Could be a separate rivets task post-merge if the team wants it.
  - As written, the three fences here are surface-only: they catch cascade or clear_all *regressions* on the audited tables. They do not catch new bug-class tables added in the future.

### 7. Tracker references

- This design references **rivets-wsix** (this issue) and **rivets-dhxo** (orphan-file boundary) and **rivets-lcb6** (the file_deps fix that established the precedent). All three are real tracker entries verified by `rivets show`.
- No "defer to follow-up" language in the design body. The architectural meta-fence consideration in #6 is explicitly out of scope, not deferred — meaning it can be picked up later if desired, but is not a tracked follow-up obligation. If the user decides post-review to file it, it becomes a new issue ID; otherwise it lives in this design as out-of-scope rationale.

## What's NOT in this design (consolidated)

1. No production code changes — fence-only.
2. No coverage of `IndexOptions::with_streaming()` mode (dhxo territory).
3. No architectural meta-fence for new INSERT sites (see #6).
4. No coverage of cross-crate or `--lsp` Pass-3 cascade behavior (Pass 2 and earlier only).

## Output of falsifiable-design (hard gate)

- [x] Probe and oracle agreed on the smallest slice (see `what-i-learned.md`)
- [x] Each claim has a falsifier
- [x] At least one falsifier has been run (Claim 1 via probe_refs_bug.sh — passed)
- [x] Self-review checklist applied (sections 1-7 above)
- [x] All claims falsifiable
- [x] All tracker references verified to exist

Ready for budgeted-plan.
