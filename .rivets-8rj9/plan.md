# Plan: atomic Assignment claims

Date: 2026-08-30
Route: Structural
Approved design: [`design.md`](./design.md), requester approval “i approve this design”

## Partition arithmetic

| Slice | Diff estimate |
|---|---:|
| Slice 1 — core Assignment and lifecycle invariants | 1,200 lines |
| Slice 2 — CLI cutover and process concurrency | 950 lines |
| Slice 3 — MCP parity, mixed locking, and synchronized docs | 950 lines |
| **Slice sum** | **3,100 lines** |
| Churn margin | **775 lines (25%)** |
| **Projected total** | **3,875 lines** |

During Slice 3, upstream merged the Related/Discovery adapter stack that this branch had already incorporated as its base. The Assignment increment is therefore measured against that accepted review boundary rather than raw `origin/main`: the accepted boundary owns unrelated relationship work, while `work/brai...work/8rj9` owns this change's Assignment, lock, CLI/MCP, test, and documentation increments. That measured Assignment increment is 9,192 changed lines (8,528 insertions, 664 deletions), exceeding both the 3,875-line projection and the 4,000-line review-size threshold.

Partition revision: the upstream relationship merge is the completed boundary, and the remaining Assignment increment ships as one merged PR stack over it. No further safe split exists inside the integrated commit chain because the merge resolution is required for every adapter to compile and test together.

### PR increment: `atomic-assignment`

Slices: 1–3, in order.

Mergeable definition: the public storage seam, CLI, MCP, compatibility loader, tests, and synchronized documentation implement the complete approved Assignment contract. The increment verifies without another increment through storage truth tables, real CLI process tests, real MCP Workspace tests, synchronized mixed-adapter contention, raw JSONL oracles, and focused workspace checks.

## Slice 1: Put Claim, Release, lifecycle coupling, creation readiness, and compatibility integrity behind the storage seam

**Claim IDs:** C1, C2, C3, C4, C10

**Expected behavior:** `IssueStorage` exposes intent-named atomic Claim and Release. Claim accepts only Open, unblocked Issues; is unchanged on same-claimant retry; and rejects other owners. Release requires exact ownership and Open state, including blocked Open. Lifecycle transitions preserve the approved Assignment matrix. Assigned creation rejects unresolved prerequisites after relationship validation. Compatibility loading visibly repairs invalid persisted combinations, while direct import atomically rejects invalid canonical combinations.

**Oracle:** Table-driven expected matrices independent of production branches; complete pre/post Issue snapshots; seeded prerequisite states; hand-authored compatibility JSONL; migration warnings and Notes; raw-byte second-save comparison; storage counts before/after rejected import.

**Stress fixture:** A 10,000-Issue/50,000-edge graph with all 50,000 edges reachable from the claim target, plus multi-prerequisite creation where one late prerequisite remains Open. Claim must detect the unresolved edge without allocating an all-Workspace blocked set; resolved cases succeed. A mixed import batch with valid entries surrounding one invalid lifecycle/Assignment record must insert nothing.

**Regression fence:** `crates/rivets/tests/in_memory_storage.rs::{claim_compare_and_set_matrix_changes_only_assignment,release_compare_and_set_matrix_changes_only_assignment,workflow_transition_assignment_matrix,create_assignment_follows_claim_readiness_after_relationship_validation,import_rejects_invalid_assignment_state_atomically}`; `crates/rivets/tests/in_memory_resilient_loading.rs::assignment_state_migration_is_visible_and_idempotent`

**Named mutation:** C1 — in `storage/in_memory/trait_impl.rs::claim`, overwrite a different Assignee; C2 — remove expected-Assignee equality from `release`; C3 — remove the Assignee-required In Progress guard; C4 — skip unresolved-prerequisite rejection for assigned creation; C10 — preserve Assigned Closed during record conversion or skip canonical validation in `import_issues`. Each corresponding named fence must turn red, then return green after restoration.

**Complexity/production scale:** Claim blockedness is $O(d)$ over the target's outgoing Blocking edges and $O(1)$ extra space; at the repository scale fixture of 10,000 Issues/50,000 total edges, even a 50,000-edge target must complete its graph check within 10 ms, matching the existing edge-query budget. Release and lifecycle mutation are $O(1)$. Assigned creation adds $O(p)$ over initial prerequisites, already bounded by the existing validation/cycle work. Compatibility normalization/import is $O(n+e)$ for one load/import event; 10,000 Issues/50,000 edges must remain within the existing 2-second ready/load-scale envelope.

**Wall budget/phase:** Claim/Release/lifecycle checks are always-on per mutation: maximum 10 ms CPU for the new Assignment/blockedness work at 50,000 edges, excluding existing JSONL persistence; rationale is the existing graph regression budget. Compatibility normalization and import are one-off discrete load/import phases: N/A — one-off phase; no additional wall budget beyond the 2-second scale envelope above.

**Files:** `crates/rivets/src/domain/mod.rs`; `crates/rivets/src/error.rs`; `crates/rivets/src/storage/mod.rs`; `crates/rivets/src/storage/in_memory/trait_impl.rs`; `crates/rivets/src/storage/in_memory/graph.rs`; `crates/rivets/src/storage/in_memory/issue_record.rs`; `crates/rivets/tests/in_memory_storage.rs`; `crates/rivets/tests/in_memory_resilient_loading.rs`; relevant core lifecycle/storage documentation under `docs/`.

**Estimate:** 5–7 focused hours; signal only.

**Diff estimate:** 1,200 changed lines including matrices and migration fixtures.

**PR increment:** `atomic-assignment`

**Commands and expected results:**
- `cargo test -p rivets --test in_memory_storage assignment` → each Claim, Release, lifecycle, create, and import matrix agrees item-by-item with its independent table; rejected operations leave full snapshots/counts unchanged.
- `cargo test -p rivets --test in_memory_resilient_loading assignment_state_migration_is_visible_and_idempotent -- --exact` → every compatibility row normalizes as predeclared, emits visible repair context, and the second canonical save is byte-stable.
- Apply each C1/C2/C3/C4/C10 named mutation and rerun its named fence → only the owning claim fence turns red with its claim-specific assertion; restore and rerun → green.
- Run the 10,000/50,000 claim stress fixture → unresolved edge detected, resolved case accepted, new check ≤10 ms, with no all-Workspace blocked-set allocation.

## Slice 2: Cut Assignment over to real CLI Claim/Release and prove process winner semantics

**Claim IDs:** C0, C5, C6, C8

**Expected behavior:** General update can no longer express Assignment through `IssueUpdate`, CLI flags, or the MCP update model. The real CLI exposes `claim ISSUE --assignee NAME` and `release ISSUE --assignee NAME`, marks both as Workspace mutations, saves through the existing guard, preserves typed failures, and survives restart. Synchronized Claims yield one durable winner; owner retry is unchanged and loser retry is Already Claimed rather than overwrite.

**Oracle:** Clap's generated command model and MCP's generated update schema with positive Claim controls; parent-controlled child-process barrier; exit/error classifications; full timestamp snapshot; independently parsed raw JSONL after each process generation.

**Stress fixture:** Barrier-start 16 CLI processes against one unassigned Ready Issue: eight use claimant `alice`, eight use distinct other names. After contention settles, raw JSONL must contain one owner; all retries for that owner are byte/timestamp-stable and every other retry is terminal Already Claimed. A second Workspace runs a Claim concurrently and must not contend.

**Regression fence:** Existing `crates/rivets/tests/workspace_mutation_lock.rs::workspace_mutation_lock_retry_preserves_both_cli_writes`; `cli::tests::general_update_rejects_assignment_flags`; MCP generated update-schema absence assertion; `crates/rivets/tests/cli_tests.rs::claim_release_cli_contract_survives_restart`; `crates/rivets/tests/workspace_mutation_lock.rs::synchronized_claims_have_one_durable_winner_and_terminal_retry`; exhaustive CLI mutation-classification fixture.

**Named mutation:** C0/C6 — omit Claim or Release from `Commands::mutates_workspace`; C5 — re-add Assignment to `UpdateArgs` or `UpdateParams`; C8 — change core Claim to assign every Open/unblocked claimant. Each named fence must turn red, then green after restoration.

**Complexity/production scale:** No new production loop in CLI parsing or execution; one command invokes one storage compare-and-set and one existing atomic save. The 16-process fixture is test-only concurrency pressure.

**Wall budget/phase:** N/A — no new runtime phase; Claim/Release reuse the existing one-command mutation transaction. Test-only process synchronization is one-off.

**Files:** `crates/rivets/src/domain/mod.rs` (`IssueUpdate` clean cutover); `crates/rivets/src/cli/args.rs`; `crates/rivets/src/cli/mod.rs`; `crates/rivets/src/cli/execute.rs`; `crates/rivets-mcp/src/models.rs` (remove update Assignment); `crates/rivets-mcp/src/tools.rs` (remove generic-update wiring); `crates/rivets/tests/cli_tests.rs`; `crates/rivets/tests/workspace_mutation_lock.rs`; CLI-facing `README.md` and `docs/agents/issue-tracker.md` sections.

**Estimate:** 4–6 focused hours; signal only.

**Diff estimate:** 950 changed lines including process fixtures.

**PR increment:** `atomic-assignment`

**Commands and expected results:**
- `cargo test -p rivets cli::tests::` → update Assignment flags reject, Claim/Release positive controls parse, and both commands classify as mutations.
- `cargo test -p rivets --test cli_tests claim_release_cli_contract_survives_restart -- --exact` → real binaries cover success, idempotence, blocked/active/mismatch/missing failures, Ready visibility, release, close/reopen, and raw JSONL restart persistence.
- `cargo test -p rivets --test workspace_mutation_lock synchronized_claims_have_one_durable_winner_and_terminal_retry -- --exact` → one durable winner, owner retries unchanged, loser retries Already Claimed, and distinct Workspace proceeds.
- Apply C0/C5/C6/C8 named mutations and rerun owning fences → red with claim-specific output; restore and rerun → green.

## Slice 3: Expose MCP parity, mixed-adapter locking, typed retry meaning, and synchronized documentation

**Claim IDs:** C7, C9, C11

**Expected behavior:** MCP Claim/Release schemas, routes, tools, and errors match the storage/CLI contract for explicit and contextual Workspaces. Only Workspace Busy is retryable; Already Claimed and other Assignment errors remain terminal typed failures. Recreated contexts observe durable results. A Claim racing any CLI or MCP mutation cannot be overwritten from a stale snapshot. All public documentation and parity registries describe Claim/Release as implemented and remove blind Assignment guidance.

**Oracle:** Public MCP tool calls, generated router schemas, recreated contexts, protocol error code/data table, a parent operation log reduced independently in lock-acquisition order, and raw canonical JSONL after mixed CLI/MCP mutation.

**Stress fixture:** Keep an MCP context cache warm, perform a CLI Claim under the durable lock, then retry a previously contending MCP ordinary update after lock release. Final raw JSONL must preserve the Claim and the ordinary field mutation. Repeat with MCP Claim and CLI close: close clears Assignment. Explicit and context-selected paths resolving to the same Workspace must contend; a distinct Workspace must proceed.

**Regression fence:** `crates/rivets-mcp/tests/integration.rs::claim_release_contract_survives_context_restart`; `crates/rivets-mcp/tests/workspace_lock.rs::{claim_and_release_require_workspace_lock,mixed_cli_mcp_mutation_preserves_atomic_claim}`; server generated-schema assertion; `rivets-mcp::error::tests::assignment_errors_preserve_retry_classification`; CLI/MCP parity registry test.

**Named mutation:** C7 — use `storage_for` instead of `mutation_storage_for` in MCP Claim; C9 — remove the under-lock `storage.reload()` in `mutation_storage_for`; C11 — map Already Claimed to Workspace Busy/retryable. Each owning fence must turn red, then green after restoration.

**Complexity/production scale:** No new collection loop; MCP adapter work is $O(1)$ outside the storage operation and existing JSONL reload/save. The mixed fixture uses one Workspace at the current 224-Issue production size plus a 10,000-Issue seeded case to ensure cache reload does not alter asymptotic behavior.

**Wall budget/phase:** MCP routing/error translation is always-on but adds only $O(1)$ field parsing and enum translation; maximum accepted adapter-only overhead is 1 ms per tool call, excluding existing lock acquisition, reload, storage, and save. Rationale: no I/O or collection traversal is added in the adapter.

**Files:** `crates/rivets-mcp/src/models.rs`; `crates/rivets-mcp/src/tools.rs`; `crates/rivets-mcp/src/server.rs`; `crates/rivets-mcp/src/error.rs`; `crates/rivets-mcp/tests/integration.rs`; `crates/rivets-mcp/tests/workspace_lock.rs`; `crates/rivets-mcp/README.md`; `AGENTS.md`; `README.md`; `docs/README.md`; `docs/data-flow.md`; `docs/storage-architecture.md`; `docs/module-structure.md`; `docs/agents/issue-tracker.md`; `docs/cli-mcp-parity.json`; generated `docs/cli-mcp-parity.md`.

**Estimate:** 5–7 focused hours; signal only.

**Diff estimate:** 950 changed lines including integration fixtures and docs.

**PR increment:** `atomic-assignment`

**Commands and expected results:**
- `cargo test -p rivets-mcp claim_release` and `cargo test -p rivets-mcp assignment_errors_preserve_retry_classification -- --exact` → tool and protocol outcomes match the C1/C2/C11 tables through context recreation; only Workspace Busy carries retryable data.
- `cargo test -p rivets-mcp --test workspace_lock` → both tools require the shared lock; stale MCP cache cannot erase CLI Claim; close clears Assignment.
- Run the parity registry generator/check used by the repository → Claim/Release are implemented shared intents, update/close/reopen Assignment gaps are removed, and generated Markdown matches JSON.
- Apply C7/C9/C11 named mutations and rerun owning fences → red with claim-specific output; restore and rerun → green.

## Tracker taxonomy

No intended future work is introduced. Every excluded capability is a permanent non-goal already approved in `design.md`: multiple Assignees, authenticated authority, administrative stealing, leases/expiry, distributed locking, direct-library cross-process coordination, and batch Claim/Release. No tracker issue is required.

## Self-review

- [x] Every design claim C0–C11 is assigned exactly once: Slice 1 owns C1–C4/C10, Slice 2 owns C0/C5/C6/C8, Slice 3 owns C7/C9/C11.
- [x] Every slice contains all thirteen mandatory fields; conditional fields carry an explicit N/A reason.
- [x] Every PENDING falsifier is assigned to the implementing slice, with its permanent fence and named mutation in that slice.
- [x] Every new loop records complexity, production input, explicit cost bound, and rationale; every introduced always-on phase has a bound or an explicit no-new-phase rationale.
- [x] Partition arithmetic records the 3,100-line sum, 25%/775-line churn margin, and 3,875-line total; every slice names the single mergeable increment.
- [x] Tracker taxonomy is applied; no untracked deferral phrase remains.
- [x] No slice is declared complete; checkpointed-build exclusively judges completion.
