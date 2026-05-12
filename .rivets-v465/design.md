# rivets-v465 — prove-it-prototype findings

Status: hard gate satisfied. Probe and oracle agree empirically.
Ready for `falsifiable-design`.

## Smallest question (final)

> Where are the ~105 legitimate cross-crate refs and the ~174 FORBIDDEN-pair
> phantoms being resolved — Pass 1 (tree-sitter direct), Pass 2 import-based,
> or Pass 2 unscoped fallback?

This question gates the fix shape:
- If Pass 1: fix the tree-sitter extractor's qualified-name matching
- If Pass 2 imports: fix `resolve_via_explicit_import` / `resolve_via_glob_import`
- If Pass 2 fallback: confirms the rivets-v465 hypothesis; fix is in import path

## Probe path

1. **`probe.py`** — classify each leak by caller's import coverage (NO_TARGET_IMPORT / GLOB / EXPLICIT_NAME_MATCH / PARENT_PATH_OR_OTHER). Result: 74 / 0 / 5 / 26.
2. **`inspect_imports.py`** — dumped the actual `imports` table rows for the implicated caller files. Found that imports ARE correctly extracted (e.g., `crates/rivets-mcp/src/context.rs` has `use rivets::storage::in_memory::new_in_memory_storage`).
3. **`check_same_crate_ambiguity.py`** — for each leak, count same-crate symbols with the same name. Result: 99 have ZERO same-crate match, 6 have ONE (rivets-0gom scoping should have caught those 6).
4. **`drill_one_same_crate.py`** — looked at the 6 "one same-crate" cases. Each is a workspace enum variant name (`Io`, `IssueNotFound`) where both rivets and rivets-mcp define their own `Error::*` variant; tethys picked the wrong crate.
5. **`simulate_rivets_0gom_lookup.py`** — directly ran rivets-0gom's `search_symbol_by_name_in_path_prefix` SQL for those 6 cases. The SQL returns the correct same-crate match UNIQUELY. Conclusion: the SQL works; something upstream is bypassing it.
6. **`check_ref_extraction.py`** — looked at `reference_name` field for those 6 refs. All `NULL`. Hypothesized Pass 1 was resolving them.
7. **`check_pass_provenance.py`** — checked `reference_name` NULL/SET split across all 105 leaks + 174 phantoms. All 100% `reference_name=NULL`. Hypothesized Pass 1 is doing everything.
8. **Tracked the hypothesis to ground truth** — found `db/references.rs:157`: `UPDATE refs SET symbol_id = ?2, reference_name = NULL WHERE id = ?1`. Pass 2 also clears `reference_name` when it resolves. So `reference_name=NULL` does NOT mean "Pass 1 resolved this." The DB doesn't track pass provenance directly.

## Oracle (empirical, independent of resolver code)

**Mechanism:** modify `crates/tethys/src/resolve.rs` to disable the
`fallback_symbol_search` branch (`if false && let Some(symbol) = ...`).
Rebuild release. Wipe DB. Re-index. Count resolved refs.

This is independent of any DB-side probe because it changes the resolver's
behavior at build time, then asks the resolver itself how many refs it
resolves.

**Result with fallback disabled:**

| Population | Pre-disable | Post-disable |
|---|---|---|
| Total legit-cross leaks (ALLOWED pairs) | 105 | **0** |
| FORBIDDEN-pair phantoms | 174 | **0** |
| Same-crate resolved refs | 5971 | (unchanged, ~6000) |

**Verdict: 100% of leaks AND 100% of phantoms go through Pass 2 fallback.
Pass 2's import-based resolution catches ZERO cross-crate refs in the
rivets workspace.**

(Code change reverted; DB re-indexed to restore production state for
future probes.)

## What I learned (single sentence, was not obvious before)

**Pass 2's import-based resolution (`resolve_via_explicit_import` +
`resolve_via_glob_import` paths in `crates/tethys/src/resolve.rs`) does
nothing for cross-crate refs in the rivets workspace** — every cross-crate
ref reaches `fallback_symbol_search` even when the caller file has an
explicit `use rivets::module::Name` matching the ref's name. The import
resolver isn't "leaky" — it's effectively bypassed entirely for cross-crate
resolution.

## Implications for the fix design

Three observations now grounded in evidence:

1. **The hypothesis that filed rivets-v465 was correct** but understated. I
   said "leaks 80% of legitimate refs." Actual: leaks 100%. (The 80% number
   came from how many ALLOWED-pair refs Option A's extended audit would
   demote; the experiment-disable shows ALL of them are reaching fallback.)

2. **The audit-and-demote approach (Option A from rivets-3d0s) is dead** for
   a more fundamental reason than the false-positive rate. With Pass 2
   imports doing nothing, the fallback is the ONLY mechanism resolving
   cross-crate refs — including the legitimate ones. Demoting at the
   fallback level demotes everything.

3. **The actual fix scope is bigger and elsewhere.** The fix needs to be in
   `resolve_via_explicit_import` and/or `resolve_via_glob_import` —
   investigate why those don't match cross-crate ref names against the
   workspace symbol table for any of the 105+174 = 279 cross-crate refs.

The next skill (`falsifiable-design`) needs to:
- Frame the fix as "make Pass 2 import-based resolution actually resolve
  cross-crate refs"
- Identify per-import-shape (named, glob, alias, re-export) what's failing
- Probe at least one specific failure case with a controlled fixture

## Open questions for the design

- For `use rivets::storage::in_memory::new_in_memory_storage`, what does
  `resolve_via_explicit_import` actually do? Walk through the code in a
  debugger or with `trace!` instrumentation on a controlled fixture.
- Does `resolve_symbol_in_module` do path-to-file mapping correctly across
  crate boundaries? Tethys uses `resolver::resolve_module_path` which takes
  a `crate_root`. For cross-crate refs, crate_root is the CALLER's root,
  which would never contain the TARGET's path.
- Why does same-crate scoping work but cross-crate import resolution doesn't?

## Artifacts

- `probe.py` — initial import-coverage classifier
- `check_same_crate_ambiguity.py` — distinguished phantom vs legitimate
- `drill_one_same_crate.py` — examined the 6 rivets-0gom-should-have-caught cases
- `simulate_rivets_0gom_lookup.py` — confirmed rivets-0gom SQL works for those 6
- `check_ref_extraction.py` — checked reference_name field
- `check_pass_provenance.py` — provenance attempt (debunked: Pass 2 also nulls reference_name)
- `inspect_imports.py` — dumped imports table for sample files
- `design.md` — this file

Notable: no `oracle.py` per the convention of earlier diagnostic dirs.
The oracle here is a *code-change-and-reindex experiment*, which is
documented in the "Oracle" section above but is not a standalone script
because it requires modifying the resolver source.
