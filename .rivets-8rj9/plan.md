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

Review re-entry adds four independently green correction slices: Slice 4 (700 lines), Slice 5 (800 lines), Slice 6 (650 lines), and Slice 7 (400 lines), for 2,550 lines plus a 25%/638-line review churn margin, or 3,188 projected review-fix lines. The revised cumulative projection is 7,063 lines. These corrections modify the already-open integrated PR because each fixes its current public/storage/concurrency contract; splitting them behind the known-broken PR would leave PR 102 unmergeable.

### PR increment: `atomic-assignment`

Slices: 1–7, in order.

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

## Slice 4: Repair persistence compatibility, canonical Assignee text, and storage wrapper integrity — F1, F5, F8, F9, F13, F17, F18, F21

**Claim IDs:** C1, C2, C4, C10, C12, C17

**Expected behavior:** Legacy Blocked and blank-Assignment records migrate visibly to canonical Open/unassigned shapes; blank create/Claim/Release input rejects; read-only snapshot loads tolerate atomic replacement while every mutator, including Related/Discovery, reloads iff changed; MockStorage returns typed unsupported errors; shared filter/revision helpers preserve output; production-scale timing fixtures are explicit checkpoints.

**Oracle:** Hand-authored JSONL and byte-stable second save; text-shape/state matrices; old/new complete snapshot sets; typed MockStorage variants; independent Ready/list truth tables; read-back digest comparison.

**Stress fixture:** Existing 10,000-Issue/50,000-edge Claim/Ready fixtures run explicitly with `--ignored`; multi-buffer JSONL revision comparison catches hash-domain drift.

**Regression fence:** `crates/rivets/tests/in_memory_resilient_loading.rs` legacy workflow/Assignment migration; storage source-revision and MockStorage tests; relationship stale-source tests; existing Ready/list truth tables; exact ignored scale tests.

**Named mutation:** Decode status directly as `IssueStatus`; remove blank validation; restore `ensure_writable` on one relationship mutator; make MockStorage Claim panic; use a different revision hasher update domain. Each owning fence must turn red, then green after restoration.

**Complexity/production scale:** Blank validation is O(m) over bounded Assignee text without allocation. Revision hashing and JSONL load remain O(file bytes). Unchanged-source mutations avoid a full O(n+e) parse/graph rebuild. Ready/list predicates remain O(1) per Issue.

**Wall budget/phase:** Always-on unchanged-source mutation performs two revision hashes plus atomic save but no unconditional reload; explicit 10k/50k timing phases are one-off ignored checkpoints with their existing 10 ms/2 s budgets.

**Files:** `crates/rivets/src/domain/mod.rs`; `crates/rivets/src/storage/mod.rs`; `crates/rivets/src/storage/in_memory/{issue_record.rs,jsonl.rs,trait_impl.rs,graph.rs}`; `crates/rivets/tests/{in_memory_resilient_loading.rs,in_memory_storage.rs}`.

**Estimate:** 6 focused hours; signal only.

**Diff estimate:** 700 changed lines including migration and revision fences.

**PR increment:** `atomic-assignment`

**Commands and expected results:**
- `cargo test -p rivets legacy_blocked` and `cargo test -p rivets assignment` → compatibility rows migrate once, blank identities reject without mutation, and canonical Claim/Release matrices remain exact.
- `cargo test -p rivets storage::tests` plus focused stale relationship tests → read snapshots remain complete, every mutation reloads changed source, MockStorage returns typed errors, and revision digests agree.
- Run the two exact ignored 10k/50k checkpoints → existing performance bounds pass without entering the default suite.

## Slice 5: Correct MCP Update, Reopen, same-server serialization, async locking, and cache ownership — F2, F6, F7, F10, F12, F20, F22, F24
**Claim IDs:** C3, C5, C7, C9, C13, C15, C16

**Expected behavior:** Historical MCP `assignee` and empty Update reject before mutation; Reopen accepts only Closed; concurrent same-server mutations serialize and both succeed; external processes remain fail-fast Busy; cached freshness comes from `prepare_mutation`; lock/path filesystem work does not block async workers; eviction removes test lock-bypass metadata; Claim/Release share one private transaction helper and advertise Open-only idempotency.

**Oracle:** Public MCP protocol envelopes, full pre/post Issue/JSONL snapshots, parent barriers and operation logs, scheduler sentinel, cache membership, and generated schemas.

**Stress fixture:** Two simultaneous creates against one server plus an external lock holder; stale Related/Discovery mutation after atomic source replacement; current-thread sentinel while asynchronous lock acquisition is delayed.

**Regression fence:** MCP integration Update/Reopen/Claim/Release tests; `tests/workspace_lock.rs`; `tests/stale_cache.rs`; Context eviction unit test; router schema/description tests.

**Named mutation:** Drop legacy-assignee detection; remove Reopen Closed guard; acquire flock before storage write guard; re-add unconditional reload; call synchronous lock acquisition from the async handler; retain evicted test marker. Each owning fence must turn red, then green after restoration.

**Complexity/production scale:** Parameter/state/cache checks are O(1). Path canonicalization/metadata and flock move to Tokio filesystem or blocking pool. Cached unchanged-source mutations avoid O(n+e) reload; persistence remains O(n+e) save.

**Wall budget/phase:** Adapter-only checks remain below 1 ms excluding filesystem/storage. Filesystem latency is isolated from async workers; same-server wait duration is bounded only by the owning mutation and does not become `WorkspaceBusy`.

**Files:** `crates/rivets-mcp/src/{context.rs,error.rs,models.rs,server.rs,tools.rs}`; `crates/rivets-mcp/tests/{integration.rs,stale_cache.rs,workspace_lock.rs}`; `crates/rivets/src/workspace_lock.rs`.

**Estimate:** 8 focused hours; signal only.

**Diff estimate:** 800 changed lines including concurrency and responsiveness fences.

**PR increment:** `atomic-assignment`

**Commands and expected results:**
- `cargo test -p rivets-mcp update` and `cargo test -p rivets-mcp reopen` → historical/empty Update and non-Closed Reopen reject with unchanged bytes while canonical controls succeed.
- `cargo test -p rivets-mcp --test workspace_lock` and `cargo test -p rivets-mcp --test stale_cache` → same-server calls both succeed, external contention remains Busy, stale relationship writes are preserved, and async responsiveness advances.
- `cargo test -p rivets-mcp context` → evicted test Workspaces no longer bypass durable locking.

## Slice 6: Move CLI prompts outside lock ownership and make sidecar/classification adoption exhaustive — F3, F4, F11, F15, F19, F22, F23

**Claim IDs:** C0, C6, C14

**Expected behavior:** Title and confirmation prompts hold no durable lock; after input the command acquires the lock, loads authoritative state, mutates, and saves; upgraded Workspaces idempotently ignore the sidecar; every nested action has an exhaustive lock classification; Claim/Release share private transaction/rendering scaffolding; project workflow docs claim before In Progress; tracing config has one stderr writer.

**Oracle:** Parent-owned pipes and child exits, raw JSONL reduction, exact ignore-file entries, compiler-exhaustive action matches, Clap syntax smoke, and documented real CLI sequence.

**Stress fixture:** Hold create, multi-close, multi-reopen, and delete at prompts while another writer completes; then resume and require both serialized results. Start from an old `.rivets/.gitignore` without the sidecar rule and acquire repeatedly.

**Regression fence:** `crates/rivets/tests/workspace_mutation_lock.rs`; Workspace lock upgrade tests; CLI mutation-classification tests; Claim/Release restart contract; documented workflow smoke.

**Named mutation:** Construct mutation App before prompt; skip ignore adoption; replace one nested exhaustive action match with wildcard false; split Claim/Release save paths. Each owning fence must turn red, then green after restoration.

**Complexity/production scale:** Action classification and Assignment dispatch are O(1). Ignore adoption scans one small metadata file; no Issue/edge loop is added.

**Wall budget/phase:** Human prompt is an unbounded one-off phase with no Workspace lock. Post-prompt mutation uses the existing one-command transaction. Ignore adoption is one-off per lock acquisition and bounded by metadata-file size.

**Files:** `CLAUDE.md`; `crates/rivets/src/{app.rs,main.rs,workspace_lock.rs}`; `crates/rivets/src/cli/{args.rs,execute.rs,mod.rs}`; `crates/rivets/tests/{common/mod.rs,workspace_lock.rs,workspace_mutation_lock.rs}`.

**Estimate:** 7 focused hours; signal only.

**Diff estimate:** 650 changed lines including real-process prompt fixtures.

**PR increment:** `atomic-assignment`

**Commands and expected results:**
- `cargo test -p rivets --test workspace_mutation_lock` → every idle prompt permits another writer; resumed commands reload and preserve both results without probe races or leaked children.
- `cargo test -p rivets workspace_lock` and `cargo test -p rivets cli::tests` → pre-upgrade ignore adoption is idempotent and every action variant is explicitly classified.
- Run the documented Claim→In Progress→Close sequence with the real binary → every step succeeds; status-only unassigned control rejects.

## Slice 7: Synchronize parity documentation and PR-added test diagnostics — F14, F16

**Claim IDs:** C10, C18

**Expected behavior:** Authoritative parity text contains no claim that legacy Blocked is accepted or counted; rendered Markdown matches; every bare `unwrap()` introduced by PR 102 has a descriptive failure context.

**Oracle:** Canonical status parser/registry assertions, generated Markdown check, and zero added bare-unwrap diff matches.

**Stress fixture:** N/A — documentation/test-diagnostic cleanup adds no runtime logic.

**Regression fence:** Parity registry contract test; renderer `--check`; affected test targets compile and run.

**Named mutation:** Reinsert one legacy Blocked registry sentence or one PR-added bare unwrap; registry/diff audit turns red, then green after restoration.

**Complexity/production scale:** N/A — no production loop or runtime behavior.

**Wall budget/phase:** N/A — no runtime phase.

**Files:** `docs/cli-mcp-parity.json`; generated `docs/cli-mcp-parity.md`; PR-added/modified Rust test files; `.rivets-8rj9/review-decisions.md`.

**Estimate:** 3 focused hours; signal only.

**Diff estimate:** 400 changed lines, primarily diagnostic substitutions.

**PR increment:** `atomic-assignment`

**Commands and expected results:**
- `python3 scripts/render-cli-mcp-parity.py && python3 scripts/render-cli-mcp-parity.py --check` → generated reference exactly matches canonical status vocabulary.
- Diff audit for PR-added test lines containing bare `.unwrap()` → zero; affected tests compile and pass with descriptive failure contexts.

## Review correction execution

- [x] Slice 4: persistence compatibility, Assignee validation, storage wrappers, shared filters/revisions, and opt-in scale fences.
- [x] Slice 5: MCP Update/Reopen contract, same-server serialization, freshness ownership, async filesystem isolation, cache eviction, and Assignment helpers.
- [x] Slice 6: pre-lock CLI prompts, sidecar ignore adoption, exhaustive mutation classification, documented claim order, and shared CLI Assignment helpers.
- [x] Slice 7: authoritative parity regeneration and zero PR-added bare test unwraps.

## Tracker taxonomy

No intended future work is introduced. Every excluded capability is a permanent non-goal already approved in `design.md`: multiple Assignees, authenticated authority, administrative stealing, leases/expiry, distributed locking, direct-library cross-process coordination, and batch Claim/Release. No tracker issue is required.

## Self-review

- [x] Original claims C0–C11 retain their owning implementation slices; review-fix slices name the original root-cause claims they correct. New claims C12–C18 are each owned exactly once: Slice 4 owns C12/C17, Slice 5 owns C13/C15/C16, Slice 6 owns C14, and Slice 7 owns C18.
- [x] Every slice contains all thirteen mandatory fields; conditional fields carry an explicit N/A reason.
- [x] Every review falsifier was assigned to and passed in the review-fix slice implementing its claim, with its permanent fence and named mutation recorded in that slice.
- [x] Every new loop records complexity, production input, explicit cost bound, and rationale; every introduced always-on phase has a bound or an explicit no-new-phase rationale.
- [x] Partition arithmetic records the original 3,875-line projection and the 3,188-line review-fix projection for a revised cumulative 7,063 lines; every slice names the existing integrated PR increment.
- [x] Tracker taxonomy is applied; no untracked deferral phrase remains.
- [x] Every slice is complete under the final checkpointed-build gate recorded in `design.md`.
