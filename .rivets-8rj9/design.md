# Falsifiable design: atomic Assignment claims

## Route and inputs

- Route: **Structural**, from [`route.md`](./route.md). Public storage, CLI, MCP, and error interfaces change; same-Workspace process concurrency is load-bearing.
- Behavior source: `route.md` T4, which records G1–G8: compare-and-set Claim, expected-owner Release, lifecycle/Assignment coupling, create-time readiness, removal of blind general update, restart persistence, and synchronized-process winner/retry semantics.
- Governing domain sources: `rivets-8rj9`, parent specification `rivets-5mlg`, `CONTEXT.md` Work coordination, `docs/adr/0002-issue-relationships-and-readiness.md`, and the lifecycle ownership seam in ADR-0005.
- `spec.md`: **N/A — behavior is explicit** in the route T4 contract and governing tracker specification.
- Empirical premises and `evidence.md`: **N/A — Structural route with T1=no**. All premises are repository-owned and current.
- Baseline observation: two synchronized `rivets update ISSUE --assignee NAME` processes produced one success and one retryable Workspace Busy; retrying the loser then blindly replaced the durable winner. The existing lock serializes writes, but the mutation behind it is not compare-and-set.

## Input shapes

| Input | Production-reachable shapes | Status |
|---|---|---|
| Claim target existence | Existing Issue; missing Issue | Covered by C1, C6, C7 |
| Claim Workflow State | Open; In Progress; Closed | Covered by C1 |
| Claim blockedness | No prerequisites; one Closed prerequisite; one unresolved prerequisite; multiple all resolved; multiple with at least one unresolved | Covered by C1 and C4 |
| Claim current Assignment | Unassigned; assigned to exact claimant; assigned to a different claimant | Covered by C1 |
| Claimant text | Empty, ASCII, Unicode, embedded spaces, control characters | Covered by C1, C6, C7. Existing Assignee text validation remains authoritative: control characters reject; other shapes compare exactly. |
| Release target and expected claimant | Missing Issue; Open/In Progress/Closed; unassigned/same/different Assignee; blocked and unblocked Open Issue; empty/ASCII/Unicode/spaced expected value | Covered by C2, C6, C7 |
| Workflow transition matrix | Open/In Progress/Closed crossed with Open/In Progress/Closed, with Assignment absent/present | Covered by C3 |
| Transition payload presence | Status alone; status plus one or multiple ordinary fields; close/reopen reason absent, valid, or invalid | Covered by C3; rejected transitions and invalid Notes must leave every field unchanged. |
| Creation Assignment | Assignee absent/present crossed with zero, one, multiple, duplicate, missing, cyclic, all-resolved, and at-least-one-unresolved initial prerequisites | Covered by C4. Endpoint, duplicate, and cycle validation precede Assignment readiness. |
| General update Assignment presence | Assignment absent is the only reachable shape after cutover; CLI `--assignee`/`--no-assignee`, MCP `assignee`, and `IssueUpdate.assignee` are unreachable | Covered by C5 |
| CLI Claim/Release cardinality | Exactly one Issue ID and one explicit `--assignee` per invocation | Covered by C6. Batch Claim/Release is N/A — one compare-and-set result avoids partial batch ownership semantics; callers may invoke once per Issue. |
| MCP Workspace selection | Explicit workspace root; omitted root with context; missing/invalid Workspace | Covered by C7; existing `mutation_storage_for` path semantics remain authoritative. |
| Workspace path spelling | Relative and absolute caller paths resolving to one canonical Workspace; distinct canonical Workspaces | Covered by C7, C8, C9 through the existing canonical lock seam. No new path parser is introduced. |
| Concurrent claims | Same Workspace/same Issue with same claimant; same Workspace/same Issue with different claimants; different Workspaces; first attempt acquires or receives Workspace Busy; retry after release | Covered by C8 |
| Concurrent non-Claim mutation | Claim racing CLI update, lifecycle mutation, or MCP mutation against the same Workspace | Covered by C9 |
| Persistence generation | Same process; fresh CLI process; recreated MCP context; canonical reload after save | Covered by C6, C7, C8, C9 |
| Compatibility records | Open unassigned; Open assigned; In Progress assigned; In Progress unassigned; Closed unassigned; Closed assigned | Covered by C10 |
| Direct import integrity | Empty/single/multi Issue collections; valid shapes; unassigned In Progress; assigned Closed; mixed valid/invalid batch | Covered by C10; invalid domain shapes reject atomically rather than bypassing canonical lifecycle/Assignment invariants. |
| Numeric inputs | N/A — Claim and Release introduce no numeric input; existing priority and limit behavior is untouched. |
| Claim/Release collections | N/A — the new intents accept one Issue and one expected claimant, not collections. Initial prerequisite collection shapes are covered by C4. |

### Subtractive-invariant sweep

The core move strengthens Assignment, but one lifecycle constraint is deliberately removed: `IssueStatus::validate_transition` currently prevents every non-Closed Issue from targeting Open.

- That rule made `In Progress → Open` impossible. Removing only that cell can now return active work to the Ready predicate.
- The retained Assignment prevents that returned Open Issue from entering the default unassigned frontier; it remains visible only to its exact Assignee or administrative all-Assignment queries. C3 fences this chain.
- `Open → Open` remains rejected, `Closed → Closed` remains rejected, and a Closed Issue reopens only to unassigned Open. C3 records these still-safe cells.
- Removing Assignment from general update removes a capability, not a safety constraint. Claim and Release become the only intentional mutation path; C5 mechanically fences the removed surface.

## Placement

### Atomic Claim and Release

- **Owner:** the existing storage module at the `IssueStorage` seam. `claim(&IssueId, &str)` and `release(&IssueId, &str)` can inspect the Issue and direct Blocking Dependencies while holding the in-memory storage lock, so check and mutation are one operation.
- **New seam:** N/A — `IssueStorage` already has two real adapters (`InMemoryStorage` and `JsonlBackedStorage`) and is the shared mutation interface. Adding intent-named methods deepens that module; a separate Assignment repository would duplicate Issue lookup, graph access, timestamps, and persistence coordination.
- **Forbidden:** adapters may not implement read-then-update Claim/Release; generic `update` may not accept Assignment; storage may not add a second Workspace file lock; Assignment errors may not be inferred from display strings.

### Lifecycle/Assignment coupling

- **Owner:** the domain `Issue` implementation and typed transition errors, invoked once from the storage update application site. Closing clears Assignment, reopening clears Assignment, entering In Progress requires Assignment, and returning In Progress to Open retains it.
- **New seam:** N/A — ADR-0005 already places transition rules at this seam. Private or crate-visible intent methods deepen the existing domain module without exposing another public mutation surface.
- **Forbidden:** CLI, MCP, JSONL, or individual command handlers may not revalidate or reproduce the transition matrix; adapters may not clear Assignment as a post-update patch.

### Create-time Assignment readiness

- **Owner:** the in-memory storage creation implementation, because initial Blocking prerequisites and their current states are storage-owned. The readiness check runs after endpoint/duplicate/cycle validation and before insertion becomes observable.
- **New seam:** N/A — this is another atomic behavior behind `IssueStorage::create`.
- **Forbidden:** CLI-only prerequisite checks, create-then-rollback persistence, or a second readiness definition separate from direct unresolved Blocking Dependencies.

### CLI and MCP adapters

- **Owner:** existing CLI `Commands`/args/execute modules and MCP models/tools/server modules. They parse role-named inputs, acquire the existing Workspace mutation guard, call `IssueStorage`, save, and translate typed errors.
- **New seam:** N/A — both adapters already use `App::from_directory_for_mutation` or `Tools::mutation_storage_for`.
- **Forbidden:** no unlocked query before mutation; no Claim/Release-specific lock file; no adapter-local Already Claimed comparison; no direct `Issue.assignee` mutation.

### Compatibility loading

- **Owner:** the JSONL compatibility adapter in `storage/in_memory/issue_record.rs` normalizes persisted Assignment/lifecycle combinations before the canonical domain Issue reaches core code; `IssueStorage::import_issues` enforces the same canonical invariant for direct imports after compatibility conversion.
- **New seam:** N/A — compatibility weakness already belongs at the JSONL adapter seam, while the existing storage import interface owns atomic insertion of canonical Issues.
- **Forbidden:** invalid persisted In Progress/Closed Assignment combinations may not leak into the core; migration may not silently erase the repair rationale; direct imports may not bypass canonical validation or partially insert a mixed batch; canonical writes may not reintroduce repaired shapes.

### Mechanical placement fences

C0, C5, C6, C7, and C9 drive public adapters or generated interfaces. C1–C4 test through `IssueStorage`; C10 drives both the loader and public import interface. No test reaches private helpers to prove a public claim.

## Claims

- **C0:** The existing Workspace mutation module is the single process-level serialization point required by Claim and Release; no second lock is needed.
- **C1:** `IssueStorage::claim` atomically implements the complete Open/unblocked/Assignment compare-and-set matrix and changes only Assignee plus `updated_at` on the first successful claim.
- **C2:** `IssueStorage::release` atomically requires the exact current Assignee and Open Workflow State, permits blocked Open release, and changes only Assignee plus `updated_at`.
- **C3:** The domain transition application enforces the complete Workflow State/Assignment matrix once: entering In Progress requires Assignment, In Progress to Open retains it, closing clears it, and reopening yields unassigned Open.
- **C4:** Assigned creation succeeds exactly when the new Open Issue has no unresolved direct Blocking prerequisite, after normal relationship validation; unassigned creation is unchanged.
- **C5:** General update has no production-reachable Assignment mutation surface in the domain request, CLI parser, MCP schema, or adapter implementation.
- **C6:** The real CLI exposes single-Issue Claim/Release intents, classifies both as Workspace mutations, preserves typed failures, and persists results across process restart.
- **C7:** MCP Claim/Release uses the same storage intents and durable mutation guard as CLI, with equivalent results and errors across context recreation and explicit/context Workspace selection.
- **C8:** Two synchronized process claims against one unassigned Ready Issue produce one durable winner; retry distinguishes Workspace Busy from idempotent same-claimant success or Already Claimed for the loser.
- **C9:** A Claim racing any other CLI or MCP mutation cannot be overwritten from a stale snapshot because every adapter mutation reloads under the same durable Workspace lock.
- **C10:** The compatibility loader visibly and idempotently repairs legacy unassigned In Progress and assigned Closed records, and direct import atomically rejects those invalid canonical shapes, while both paths preserve valid Open and assigned In Progress records.
- **C11:** Assignment failures remain typed through core and MCP translation, and Workspace Busy is the only retryable contention result; Already Claimed is terminal until responsibility changes.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C0 | One existing Workspace lock seam is sufficient. | Same canonical Workspace, two processes; distinct Workspaces. | Hold/run concurrent mutations and require same-Workspace contention plus retry without lost writes, while distinct Workspaces proceed. A lock implementation bug could mimic adapter failure, so the test uses an explicit external holder and raw persisted records to isolate adapter classification and lock ownership. | Child exit codes and byte/JSONL state observed by the parent test, independent of the CLI's `App` result. | In `crates/rivets/src/cli/mod.rs`, classify an existing mutating command as non-mutating; `workspace_mutation_lock_retry_preserves_both_cli_writes` loses or overwrites a write. | `crates/rivets/tests/workspace_mutation_lock.rs::workspace_mutation_lock_retry_preserves_both_cli_writes` | <1 minute | PASS |
| C1 | Storage Claim is one atomic compare-and-set with exact mutation scope. | Missing; all states; blocked variants; unassigned/same/other; claimant text variants. | Drive `IssueStorage::claim` through the full matrix; snapshot every Issue field and direct blockers before/after. Falsify on wrong success/error, changed non-Assignment field, timestamp change on same-claimant retry, or partial mutation. An upstream parser cannot explain results because the storage interface is driven directly. | A table-driven expected-state matrix plus structural field-by-field snapshots, not the production Claim implementation. | In `storage/in_memory/trait_impl.rs::claim`, replace the different-Assignee branch with assignment overwrite; the different-claimant row must report C1 and fail. | `crates/rivets/tests/in_memory_storage.rs::claim_compare_and_set_matrix_changes_only_assignment` | 1–2 minutes | PENDING — checkpointed-build, storage slice gate |
| C2 | Storage Release requires exact owner and Open state. | Open blocked/unblocked; In Progress; Closed; unassigned/same/other expected claimant. | Drive `IssueStorage::release`; falsify if mismatch/unassigned/active succeeds, blocked Open fails, or any failed case changes bytes/fields. A generic status failure cannot mask ownership because owner and state axes are varied independently. | Independent matrix and before/after snapshots of the full Issue. | In `trait_impl.rs::release`, delete the expected-Assignee equality check; mismatch row fails with C2. | `crates/rivets/tests/in_memory_storage.rs::release_compare_and_set_matrix_changes_only_assignment` | 1–2 minutes | PENDING — checkpointed-build, storage slice gate |
| C3 | Lifecycle transitions preserve the Assignment matrix at one domain seam. | Full 3×3 state matrix; assigned/unassigned; status plus other fields; optional reason. | Apply transitions through storage and real close/reopen adapters; falsify if In Progress starts unassigned, active-to-Open clears owner, close retains owner, reopen retains owner, or rejection partially changes another field. Separate valid-transition controls prove the application site ran. | Explicit transition truth table and complete pre/post Issue snapshots, independent of `validate_transition` matches. | In the domain transition application, remove the Assignee-required guard for target In Progress; the unassigned Open→In Progress row fails C3. | `crates/rivets/tests/in_memory_storage.rs::workflow_transition_assignment_matrix`; existing CLI/MCP lifecycle tests extended for Assignment | 2–3 minutes | PENDING — checkpointed-build, lifecycle slice gate |
| C4 | Assigned create uses direct Blocking readiness after relationship validation. | Assignee absent/present; prerequisite collection empty/single/multi/duplicate/missing/cyclic/resolved/unresolved. | Create through storage; falsify if an assigned blocked Issue appears, a resolved prerequisite rejects, unassigned behavior changes, or readiness hides a relationship error. Other validation errors are distinguished by typed variants and unchanged storage counts. | Expected result table computed from seeded prerequisite states and raw relationship records. | In `trait_impl.rs::create`, skip unresolved-prerequisite checking when `assignee.is_some()`; assigned+open-prerequisite row fails C4. | `crates/rivets/tests/in_memory_storage.rs::create_assignment_follows_claim_readiness_after_relationship_validation` | 1–2 minutes | PENDING — checkpointed-build, storage slice gate |
| C5 | Blind Assignment update is mechanically absent. | General update with/without historical Assignment fields across Rust, CLI, MCP. | Assert CLI rejects `--assignee` and `--no-assignee`, generated MCP update schema omits `assignee`, and the positive-control Claim surfaces accept and persist Assignee. Absence is decisive because positive controls prove parser/router/schema inspection can observe Assignment. | Clap command model, generated MCP tool schema, and a real positive Claim—not source-text inspection. | Re-add `assignee` to `UpdateArgs` or `UpdateParams`; the corresponding generated-interface assertion fails C5. | `cli::tests::general_update_rejects_assignment_flags`; `rivets-mcp::server::tests::update_schema_excludes_assignment_and_claim_schema_includes_it` | <1 minute | PENDING — checkpointed-build, adapter slice gate |
| C6 | CLI Claim/Release is locked, typed, and restart-durable. | One ID; text variants; success/idempotent/mismatch/blocked/active/missing; fresh process. | Drive the real binary, parse exit/error output, restart for show/ready/release, and compare raw JSONL. Falsify on adapter divergence or non-persistence. Direct storage correctness cannot mask a missing CLI hop because the binary is the entry point. | Parent process parses canonical JSONL and runs fresh binaries; expected outcomes come from the C1/C2 matrix. | In `Commands::mutates_workspace`, omit `Claim`; a held-lock Claim reaches storage instead of Workspace Busy and the CLI test fails C6. | `crates/rivets/tests/cli_tests.rs::claim_release_cli_contract_survives_restart` plus mutation-classification fixture | 2–3 minutes | PENDING — checkpointed-build, CLI slice gate |
| C7 | MCP Claim/Release shares storage and lock semantics with CLI. | Explicit/context Workspace; missing context/path; C1/C2 outcome matrix; recreated context. | Invoke public MCP tools, recreate `Tools`, inspect returned typed errors and raw JSONL. Falsify on different state/error or cache-stale result. A direct-storage pass cannot hide tool/lock/schema faults because the public tool is driven. | Fresh context plus raw JSONL and expected C1/C2 table. | In `Tools::claim`, call `storage_for` instead of `mutation_storage_for`; held-lock test fails C7. | `crates/rivets-mcp/tests/integration.rs::claim_release_contract_survives_context_restart`; `workspace_lock.rs::claim_and_release_require_workspace_lock` | 2–3 minutes | PENDING — checkpointed-build, MCP slice gate |
| C8 | Synchronized claims yield one durable winner and correct retry classification. | Same/different claimant; first winner/Workspace Busy; post-lock retry; separate process generations. | Barrier-start two real CLI Claim processes. Require exactly one durable Assignee. Retry both identities after lock release: durable owner succeeds unchanged; other receives Already Claimed unchanged. A scheduler that serializes both is still decisive because the second must then be Already Claimed, never overwrite. | Parent-owned barrier, child exits, timestamp snapshot, and raw JSONL winner computation. | In `trait_impl.rs::claim`, assign whenever target is Open/unblocked; synchronized or retry loser overwrites and C8 fails. | `crates/rivets/tests/workspace_mutation_lock.rs::synchronized_claims_have_one_durable_winner_and_terminal_retry` | 2–3 minutes | PENDING — checkpointed-build, concurrency slice gate |
| C9 | Claim cannot be lost to another adapter mutation's stale snapshot. | Claim racing CLI update/close and MCP update/close in same Workspace; Workspace Busy then retry. | Start one mutation while another owns the durable lock, release/retry, and require final state to contain both permitted changes with lifecycle Assignment effects applied. Falsify on stale overwrite or partial file. Existing lock-only behavior cannot mask a stale cache because MCP context recreation and raw JSONL are checked. | Operation log maintained by the test parent and canonical JSONL reduced independently in lock-acquisition order. | In MCP `mutation_storage_for`, remove `storage.reload()` under the lock; cached stale MCP mutation overwrites the CLI Claim and C9 fails. | `crates/rivets-mcp/tests/workspace_lock.rs::mixed_cli_mcp_mutation_preserves_atomic_claim` | 3–5 minutes | PENDING — checkpointed-build, concurrency slice gate |
| C10 | Compatibility loading repairs invalid Assignment/state combinations visibly and idempotently, and direct import cannot bypass them. | Six canonical/legacy state×Assignment shapes; empty/single/mixed import batches. | Seed raw JSONL records, load, assert repaired state/Assignment plus migration Note/warning, save/reload/save, and require stable canonical bytes; then drive `IssueStorage::import_issues` with valid and mixed-invalid batches and require atomic rejection. Valid-shape positive controls prove both paths can retain Assignment. Falsify on leaked invalid state, silent repair, second-save drift, or partial import. | Hand-authored input corpus, expected normalization table, warnings, byte comparison, and pre/post storage counts; not the loader conversion or import validation helper. | In `issue_record.rs` conversion, preserve Assignee on Closed records, or in `trait_impl.rs::import_issues` skip canonical validation; the corresponding C10 row fails. | `crates/rivets/tests/in_memory_resilient_loading.rs::assignment_state_migration_is_visible_and_idempotent`; `crates/rivets/tests/in_memory_storage.rs::import_rejects_invalid_assignment_state_atomically` | 2–3 minutes | PENDING — checkpointed-build, compatibility slice gate |
| C11 | Typed error translation preserves retry meaning. | Workspace Busy; Already Claimed; mismatched/unassigned/active release; Assignee required; not found. | Pattern-match core and MCP variants, then inspect MCP protocol code/data: only Workspace Busy has `retryable: true`; Already Claimed names current Assignee and remains non-retryable. String wording cannot produce a false pass because tests match variants before envelopes. | Exhaustive variant table and protocol envelope expectations independent of `Display`. | In `rivets-mcp/src/error.rs`, map `AlreadyClaimed` to `WorkspaceBusy`; retryable-data row fails C11. | Core error conversion unit tests and `rivets-mcp::error::tests::assignment_errors_preserve_retry_classification` | <1 minute | PENDING — checkpointed-build, error slice gate |

## Non-goals and future work

- Multiple or collaborative Assignees are a permanent non-goal for this change: the canonical Assignment contract is one exclusive next-action claimant.
- Authentication, authorization, administrative claim stealing, leases, expiration, and heartbeats are permanent non-goals for this change: identity is caller-supplied and the requested contract contains no authority or time model.
- Network-distributed locking is a permanent non-goal: the accepted Workspace lock is host-local and filesystem-backed.
- Cross-process coordination for direct library consumers of `IssueStorage` is a permanent non-goal: durable Workspace ownership belongs to CLI/MCP adapters; the storage interface guarantees atomicity within one loaded adapter.
- Batch Claim/Release is a permanent non-goal for this interface: one invocation carries one compare-and-set outcome and callers can sequence multiple Issues explicitly.
- No intended future work is introduced by this design; no tracker issue is required.

## Falsifier run log

- `2026-08-30 | cargo test -p rivets --test workspace_mutation_lock workspace_mutation_lock_retry_preserves_both_cli_writes -- --exact | PASS` — C0 survived: one same-Workspace writer received Workspace Busy, retry preserved both serialized writes, and the persisted JSONL contained no lost update.
- Baseline-only observation (not a claim gate): a synchronized blind Assignment update produced durable `alice`, loser `bob` received Workspace Busy, and retrying `bob` succeeded and replaced `alice`. This localizes the missing invariant behind the already-working lock.

## Approval

Status: **APPROVED**

Requester approval: “i approve this design”
Date: 2026-08-30
Approved risk acceptances: None.
