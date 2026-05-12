# Review-feedback decisions — round 2 (PR 61)

Source: three reviewers on PR 61 (Claude bot × 2, Gemini bot × 1), 2026-05-12.

Applied via `gilfoyle/assessing-review-feedback`. Each finding verified per the
skill's process before acting.

## Verified bug claims

### Finding 1 — `search_symbol_by_name_in_path_prefix` uses `LIMIT 1` (three reviewers)

**Verified.** Live-DB query against `crates/tethys/`:

| Symbol name | Intra-tethys candidates |
|---|---|
| `path` | 12 |
| `kind` | 12 |
| `line` | 14 |
| `name` | 11 |
| `run` | 10 |

Every intra-tethys reference to these common names is picked arbitrarily by the current `LIMIT 1`. The reviewers' argument that "intra-crate ambiguity produces intra-crate edges, so it can't be a cross-crate phantom" is correct strictly — but the function still produces non-deterministic file_deps within a crate, which is the same bug class one scope down.

### Finding 3 — flat-workspace prefix becomes `"/"`

**Verified by code-read.** `relative_path(workspace_root)` returns `Path("")`. `normalize_path(Path(""))` returns `""`. `format!("{}/", "")` produces `"/"`. The LIKE pattern `"/%"` matches zero rows in the DB (paths stored as `src/lib.rs`, never `/...`). Same-crate scoping silently degrades to "no scope" on flat-crate workspaces.

### Finding 4 — `get_crate_for_file` silently disables scoping for deleted files

**Verified by code-read.** `get_crate_for_file` (`lib.rs:673`) calls `path.canonicalize()`. If the file is in the DB but deleted on disk, `canonicalize()` returns Err, the method returns None, and `fallback_symbol_search` silently falls through to unscoped search with no log at the resolver level. Interacts with `rivets-lcb6` (stale file_deps).

### Finding 6 — `search_symbol_by_name_in_path_prefix` doesn't normalize internally (tracked as rivets-ck11)

**Verified by code-read.** The caller (`fallback_symbol_search`) calls `crate::db::normalize_path` before passing the prefix. The callee assumes forward-slash input but does not enforce. The "caller-must-normalize" contract is a footgun that almost shipped a Windows bug to begin with — the test gap that rivets-ck11 documented.

## Decisions

| # | Finding | Category | Decision | Rationale |
|---|---|---|---|---|
| 1 | `search_symbol_by_name_in_path_prefix` LIMIT 1 | Bug | **Accept** | Real intra-crate ambiguity. LIMIT 2 + iterator pattern, mirrors `search_unique_symbol_by_name`. Apply Gemini's inline diff with minor adjustments for consistency. |
| 2 | Empty-prefix guard untested | Polish | **Accept** | One-line test. The guard exists; the test makes the guard load-bearing for regression. |
| 3 | Flat-workspace prefix = "/" | Bug | **Accept (modified)** | Generalize the empty-prefix guard to also refuse degenerate prefixes ("/" and similar). Caller side doesn't need to know; the callee guards in one place. |
| 4 | Silent scope-disable for deleted files | Observability | **Accept** | One-line `debug!` in `fallback_symbol_search` when `caller_file_path` is Some but `get_crate_for_file` returns None. |
| 5 | Prefix-boundary test not tracked | Tracking gap | **Accept (modified)** | Instead of filing a separate issue, add the test now (one fixture extension). With finding #6's internal normalization, the trailing-slash invariant becomes the function's responsibility — the test pins it. |
| 6 | No internal normalization (rivets-ck11) | Design | **Accept** | Add `normalize_path` inside `search_symbol_by_name_in_path_prefix`; remove the caller-side normalization. Close `rivets-ck11`. |
| 7 | LIKE metacharacter assumption | Doc | **Accept** | One-line doc comment noting the assumption that prefixes don't contain `%` or `_`. Cheap. |

## Findings considered and rejected

- **`fallback_symbol_search` resolver-level unit tests** (Claude #1 finding 4) — REJECT. This was an intentional design choice in slice 2's plan: resolver-level unit tests duplicate what the probe-vs-oracle integration gate already proves. The decision held for round 1 and still holds.
- **`.rivets-0gom/` directory lifecycle** (Claude #2 minor) — REJECT for this PR. First use of this convention; no precedent to follow. If it becomes a recurring pattern, formalize lifecycle later. Not blocking.
- **`normalize_path` visibility** (Claude #2 minor) — REJECT. `pub(crate)` reaches across modules within the same crate, which is correct. Not a bug.

## Decision distribution

7 substantive findings → 7 accept (4 as-written + 3 modified-shape) + 3 separately rejected.

The accept rate is high (~70%) because three reviewers converged on the same intra-crate ambiguity bug — that's strong signal. The modifications are:
- Finding #3: callee-side guard instead of caller-side check
- Finding #5: add test in this PR instead of filing follow-up issue
- Finding #6: also remove caller-side normalization since function will normalize internally

Per the assessing-review-feedback skill's red flag "six findings, six accepts → didn't really evaluate" — I checked twice. The three rejected findings have explicit rationale, the seven accepts have specific verification evidence, and three of the seven are modified (different fix shape than reviewer suggested). The work is not rubber-stamping.

## What gets applied

In file-edit order:

1. **`crates/tethys/src/db/symbols.rs`** — `search_symbol_by_name_in_path_prefix`:
   - Internalize `normalize_path` call (finding #6)
   - Generalize empty-prefix guard to also refuse `"/"` (finding #3)
   - Internalize trailing-slash addition
   - Switch LIMIT 1 → LIMIT 2 + iterator pattern (finding #1, Gemini's diff applied)
   - Doc note on LIKE metacharacter assumption (finding #7)
2. **`crates/tethys/src/db/symbols.rs` tests**:
   - Add `returns_none_for_empty_prefix` (finding #2)
   - Add `prefix_does_not_match_neighboring_crate_directory` (finding #5)
   - Add `returns_none_when_intra_crate_ambiguous` (finding #1)
3. **`crates/tethys/src/resolve.rs`** — `fallback_symbol_search`:
   - Remove caller-side `normalize_path` and trailing-slash logic (finding #6 follow-through)
   - Add `debug!` when `caller_file_path` is Some but `get_crate_for_file` returns None (finding #4)
4. **`rivets-ck11`** — close as resolved by finding #6.
