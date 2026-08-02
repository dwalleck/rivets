# Falsifiable design — rivets-p1g4: Curate Associated Resources and link Workspace Paths

Status: proposal (awaiting approval)
Probe: `.rivets-p1g4/probe.py` — all sections agree with independent oracles.
Cheapest falsifier: `.rivets-p1g4/falsify-normalization.py` — 432/432 random
cases agree with `realpath -m`. **PASSED before approval.**

## Purpose

Complete the Associated Resource lifecycle from `rivets-yx1h` (which delivered
add + list of Web URL resources): update and remove resources by their stable
identifier without disturbing identity or ordering, and add Workspace-relative
path targets with portable normalized form.

## Architecture

```
CLI (rivets resource add/update/remove/list)          MCP (resource_add/update/remove/list)
        │  parse at the seam                              │  parse at the seam
        ▼                                                 ▼
   domain: Issue::add_resource / update_resource / remove_resource
        │  ResourceTarget::{Web, Path}  WorkspacePath (validated, normalized)
        ▼
   storage: IssueStorage::{add,update,remove}_resource → JSONL adapter
        (ResourceTargetRecord::{Web, Path})  →  rehydrate validates
```

Key property: `WorkspacePath` normalization is **root-free** — the stored form
is the normalized relative path (lexical, no filesystem access, so paths need
not exist). The workspace root is only a *meaning* anchor (portable across
checkouts); it is not needed to validate or store. CLI resolves the root for
help text only; MCP uses the workspace_root it already resolves for storage.

## Input shapes

**ResourceTarget**: `Web{url}` (existing), `Path{path}` (new).
**WorkspacePath raw input**: empty; whitespace-only; control chars; absolute
(`/x`, `//x`); escape (`../x`, `a/../../b`); in-bounds traversal
(`docs/../src`); dot forms (`./x`, `x/.`, `.`, `a/..`); slash collapse
(`a//b`, `src/`); single component; multi-component; Unicode; embedded
spaces; deep nesting.
**Update**: target-only; role-only; label-only; label-clear; all fields;
web→path; path→web; path→path; normalized-duplicate target; nonexistent id;
empty update (no fields); updates of first/middle/last/only resource.
**Remove**: first; middle; last; only remaining; nonexistent id.
**Persistence**: path record round-trip; invalid persisted path; ordering;
`next_resource_id` monotonicity across remove/update; MCP context restart.

## Falsification

| # | Claim | Falsifier | Oracle | Cost | Status | Regression fence |
|---|-------|-----------|--------|------|--------|------------------|
| 1 | `WorkspacePath::new` accepts every string that is non-empty, control-char-free, non-whitespace-only, not starting with `/`, whose component-stack normalization stays in bounds and non-empty; stores the normalized form | random 400-case corpus + boundary prefixes | `realpath -m <root>/<input>` | 1m | **passed** | domain unit test `workspace_path_*` in resource.rs embedding probe case table |
| 2 | Accepted inputs' normalized form equals the relative part of `realpath -m <root>/<input>` | same corpus | `realpath -m` | 1m | **passed** | same unit test |
| 3 | Rejects exactly: empty/whitespace-only, control chars, absolute, escape (`..` past root), empty-after-normalization (`a/..`, `.`) | case table incl. all rejection classes | `realpath -m` (escape/absolute/empty→root) + domain-policy (control/whitespace, probe A note) | 1m | **passed** | same unit test |
| 4 | Duplicate detection uses normalized equality: `docs/../src` + `src` same role → `DuplicateTargetRole`; same target with distinct roles → allowed; web behavior unchanged | domain test on both target kinds | hand-computed from stored normalized forms | 5m | pending | domain test `duplicate_detection_normalizes_paths` |
| 5 | `Issue::update_resource(rid, update)` changes only provided fields; id and position unchanged; label clear via `Some(None)`; empty update → typed error; dup check on post-update state; unknown rid → `ResourceNotFound` | domain test matrix (each field alone, clear, empty, unknown id, dup) | hand-computed expected resource state per case | 10m | pending | domain test `update_resource_*`; CLI fence in cli_tests |
| 6 | `Issue::remove_resource(rid)` removes exactly that resource; remaining ids and positions unchanged; `next_resource_id` unchanged (ids never reused) | remove first/middle/last/only + add-after-remove | hand-count from JSONL (probe C method) | 10m | pending | domain test `remove_resource_*`; storage round-trip test |
| 7 | JSONL `{"type":"path","path":"..."}` records round-trip; invalid persisted path value → typed `InvalidResourceData` load warning, issue skipped (same as Web) | write record by hand, load, mutate, reload | raw JSONL parse + warning type | 10m | pending | `in_memory_storage`/`in_memory_resilient_loading` integration test |
| 8 | CLI `resource add --path` stores path normalized relative to workspace root (`.rivets` parent found from cwd — not cwd), rejecting absolute/escape with typed errors | run CLI from a subdirectory of a scratch workspace | raw JSONL parse of stored record + realpath | 10m | pending | cli_tests integration test |
| 9 | CLI `resource update ISSUE --resource RID …` and `resource remove ISSUE --resource RID` reach the same domain paths; text/JSON output shows post-state; unknown rid and duplicate errors typed | CLI invocations on scratch workspace | JSONL + CLI `--json` cross-check | 10m | pending | cli_tests integration test |
| 10 | MCP `resource_add` accepts `path` xor `url` (exactly one), `resource_update` and `resource_remove` keyed by resource id; normalization identical to CLI for the same input | MCP integration calls against temp workspace | CLI result on same workspace (cross-surface oracle) | 15m | pending | rivets-mcp integration tests |
| 11 | MCP responses distinguish `McpResourceTarget::Path{path}` from `Web{url}`; role/label/order/ids match storage | MCP call + raw JSONL | JSONL parse | 10m | pending | rivets-mcp integration test |
| 12 | Updates, removals, path resources, ordering, and identifiers persist across process and MCP context restart | mutate → drop storage/context → reload | probe B/C method (fresh process, raw JSONL) | 10m | pending | integration tests (CLI + MCP restart) |

## Negative space

1. **No migration changes.** Legacy `external_ref` values that look like
   relative paths stay migration Notes (behavior specified and closed in
   `rivets-yx1h`); this ticket does not reopen it.
2. **No path resolution.** No absolute-form display, no filesystem existence
   or symlink checks, no `resolve` operation — the ACs require storing
   normalized relative targets that need not exist; consumption of path
   resources (e.g. opening the file) is not part of this ticket.
3. **No Windows-specific handling.** `/` is the only separator; drive
   letters/backslashes are ordinary filename characters on this Linux
   codebase. No UNC/`C:` rejection.
4. **No absolute-input relativization.** An absolute path inside the
   workspace is rejected, not rewritten to relative — AC says "cannot be
   absolute" (strictest reading; a forgiving rewrite would make stored
   values machine-dependent).
5. **No batch operations.** Update/remove address one resource per call.

## Deferrals

None. All scope boundaries above are settled rationale (closed behavior,
missing AC mandate, platform boundary, AC text), not deferred work.

## Tracker references

- Parent epic: rivets-wb0q (verified open)
- Blocker (closed, merged PR #86): rivets-yx1h (verified closed)
- This task: rivets-p1g4 (in_progress, assigned)
