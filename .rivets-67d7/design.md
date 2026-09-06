# Design: Canonical Update and dedicated mutation intents

## Route and inputs

Route: Structural, from route.md. Behavior source: spec.md, approved verbatim on 2026-09-06: "yes, I agree". Priority decision: "retain priority in general update". The complete behavior set is spec.md's Behavior and Resolved field behavior sections: six allowed fields, omitted/null unchanged, explicit empty multiline text accepted, empty title rejected, supplied equal values remain updates, empty requests change neither timestamps nor bytes, dedicated ownership, and ordered loop-equivalent batches.

Empirical premises, evidence.md, probe: N/A — no external or unverified system premise. Existing candidate-before-commit mutation, Workspace locking/reload/save, domain transition matrix, and Parentage checks are retained, not replaced.

Current evidence: IssueUpdate in domain/mod.rs:1127-1155 exposes status/note/labels alongside descriptive fields. InMemoryStorage::update in storage/in_memory/trait_impl.rs:232-325 always advances updated_at. Dedicated CLI/MCP Close, Reopen and Append Note currently call generic Update. MCP already rejects an empty request; its stale_cache regression passed during investigation, despite stale registry prose. No dedicated Start or Return to Open surface exists.

## Input shapes

| Shape partition | Coverage |
|---|---|
| Six optional fields: all 64 presence masks | C2/C3; mask zero rejected, all nonzero masks accepted when values valid; supplied unchanged values remain nonempty |
| Optional MCP fields missing/null | C2/C3; both mean omitted; only null values is empty |
| Title empty/whitespace, trimmed byte-length 199/200/201, ASCII/Unicode, internal spaces, forbidden controls | C2; retain current byte-length and control grammar through shared validators, do not invent normalization |
| Description/design/acceptance empty, ordinary ASCII, Unicode, multiline/tab, forbidden terminal controls | C2; explicit empty remains supplied; omitted fields unchanged |
| Priority P0/P1/P2/P3/P4, 5/255, negative, noninteger, above u8 representation | C1/C2; domain range 0-4, malformed representation rejected at adapter seam |
| Kind Bug/Feature/Task/Epic/Chore, invalid wire spelling | C2/C5; Epic with and without children when reclassified |
| Removed keys status, labels, notes/note, assignee, relationships/resources, historical issue_type, unknown keys; present with a value or null; alone or with valid title | C3; strict MCP request decoding, CLI parser rejection; no silently ignored legacy keys |
| Lifecycle current Open/InProgress/Closed × action Start/ReturnToOpen/Close/Reopen × assigned/unassigned canonical combinations | C4; invalid persisted combinations N/A — compatibility loader remains owner |
| Close/Reopen optional reason absent/present, empty/whitespace, controls, Unicode/multiline | C4; existing NoteContent validation and lifecycle prefix semantics retained |
| Parentage absent/present; parent Open/InProgress/Closed; Epic children empty/single/multiple, active/all-Closed | C4/C5; direct graph checks retained |
| Notes empty/single/multiple prior entries; append same content twice | C4; append-only history, not deduplication |
| Targets empty/single/multiple/distinct/duplicates, valid/malformed/missing IDs, mixed success/failure | C6; empty CLI target list rejected; valid batch order and duplicates preserved |
| Workspace access failure, external file changes, lock contention, separate Workspaces | C6/C7; existing locking/reload and error propagation retained |
| 10,000 direct children and 100-target batch in a 10,000-Issue Workspace | C5/C7; bounded candidate update and existing persistence costs |
| Paths/time-zone/replication changes | N/A — no new path normalization, calendar arithmetic, or replication mechanism; preserve existing context selection and UTC timestamps |

## Removed-invariant sweep

This is a restrictive interface cutover, not removal of validation or serialization. Removing status/note branches from Update nevertheless relocates hidden guarantees: candidate validation before replacement, one lock covering graph checks and state commit, atomic reason Note with lifecycle state, closed-parent reopening restriction, active-child close restriction, and Assignment side effects. C4/C5/C7 explicitly defend these guarantees. General Update keeps the existing Epic reclassification restriction. Reopen remains Closed-only; Return to Open must not be an alternate way to reopen a Closed Issue and bypass parent checks.

## Placement

### Validated general Update

**Owner:** existing domain module, with constrained update construction/application housed in `crates/rivets/src/domain/update.rs`; keep the existing public IssueUpdate name, re-export its single canonical definition from domain/mod.rs. This removes the current data bag rather than layering a wrapper around it.

**Interface:** private-field IssueUpdate, no Default, constructed via an IssueUpdateBuilder with private fields. Builder collects optional values; `build()` consumes it and returns a validated, nonempty IssueUpdate or a typed cause. Shared title/text validation helpers are reused (extracted within domain only if necessary); no copied grammar. Priority remains 0-4. Consuming application updates only the six allowed fields. State-dependent Epic reclassification validation remains in storage under the lock.

**New seam alternatives:** (A) retain public optional fields and guard inside every storage implementation — smaller initial diff, but invalid construction and adapter validation duplication remain possible; (B) private validated value with a fallible builder — compiler prevents empty/unvalidated values from crossing storage and gives CLI/MCP one parsing seam. Choose B because this interface has multiple real callers and the repository requires smart construction. Builder storage is an input-construction detail, not another accepted domain value.

**Forbidden:** raw DTOs passed to IssueStorage::update, public mutable Update fields, fallback to the old generic status/note/label fields, per-adapter field grammar, hypothetical storage traits. Mechanical fence: private fields/no Default plus consumer compile-fail examples and owning-domain behavior tests.

### Lifecycle and Note intents

**Owner:** existing domain transition methods own the matrix and Assignment effects; existing InMemoryStorage owns graph-aware checks and atomic commit. Add `IssueStorage::transition(id, LifecycleAction)` and `IssueStorage::append_note(id, NoteContent)` to the existing trait and all implementations. The new action type lives in `domain/lifecycle.rs` and is re-exported once from domain/mod.rs.

**Interface alternatives:** (A) four trait methods for Start, Return to Open, Close and Reopen; (B) one intent enum `LifecycleAction::{Start, ReturnToOpen, Close { reason }, Reopen { reason }}` and one transition method. Choose B: exhaustive action dispatch keeps graph checks, candidate mutation, reason append and commit in one place; it is not a generic target-status setter. Close/Reopen alone carry optional validated Note content and retain existing lifecycle reason prefix construction. Append Note is separate and cannot update descriptive fields.

**Behavior:** Start targets In Progress under the existing matrix (requires Assignment; repeated Start on In Progress preserves current successful transition behavior). ReturnToOpen accepts only In Progress and retains Assignment. Reopen accepts only Closed and produces unassigned Open, including the existing Closed-parent check. Close clears Assignment and rejects active Epic children or already Closed state. State and optional reason Note commit together after all checks. Existing closed_at semantics are intentionally unchanged.

**Forbidden:** adapters reading state to decide transitions, lifecycle through Update, multiple saves for state plus reason, duplicated graph traversal, unlocked check-then-write. Mechanical fences: exhaustive typed action match and storage-level transition/Parentage truth tables.

### CLI and MCP adapters

**Owner:** existing CLI args/dispatch and MCP models/server/Tools. Add focused `cli/lifecycle.rs` for lifecycle handlers including migrated Close/Reopen handlers, and `cli/notes.rs` for Append Note. Do not grow execute.rs with new lifecycle algorithms; retain its existing batch/save helper via crate-private reuse or move that exact helper to the existing CLI batch module if one exists. No broad command-family refactor.

**Public surfaces:** retain `update`, `close`, `reopen`, `claim`, `release`, Label and relationship operations. Add CLI `start <ids>...`, `return-to-open <ids>...`, `note append <ids>... --content <text>`. Add MCP `start` and `return_to_open` with Issue ID/context parameter objects; retain existing `add_note`, `close`, and `reopen`. Remove CLI Update `--status` and `--notes`; no compatibility aliases. All new CLI commands use existing partial-success mechanics.

UpdateParams directly declares the canonical Kind field instead of flattening the migration-only IssueKindInput wrapper; deny unknown fields, including obsolete fields supplied alongside a valid update. Remove its legacy-assignee presence marker. Create's compatibility shape is not touched by this Update cutover. Advertise the Update constraints in its schema without pretending null optional fields count as changes; runtime construction remains authoritative for cross-field nonempty validation.

**Forbidden:** adapter transition matrices, silent obsolete-field acceptance, hidden issue_type compatibility in Update, positional parameter expansion, a new MCP batch tool. Mechanical fences: real parser and registered-server requests; inventory registry checked against public surfaces.

### Documentation

Update docs/cli-mcp-parity.json and its rendered reference, touched examples and library docs, ADR-0001/0005 descriptions of mutation routing, and AGENTS.md instructions currently using update --status. Keep domain meaning unchanged; CONTEXT.md needs no semantic change. Tracker writes remain in the main Workspace until integration; do not overwrite the dirty main tracker with a worktree snapshot.

## Claims

C1. General Update preserves Priority P0-P4 and rejects Priority 5 with its typed cause.
C2. One validated Update value enforces the six-field contract and existing text semantics before mutation.
C3. Empty and obsolete-field requests fail through CLI/MCP without mutation, even when an obsolete field accompanies a valid field.
C4. Dedicated lifecycle and Note operations preserve the existing state/Assignment/Note contracts atomically.
C5. Lifecycle extraction and Kind updates preserve Parentage invariants and the existing 10,000-child bound.
C6. CLI batches produce the same ordered per-target semantic effects and errors as repeated MCP calls.
C7. Dedicated mutations preserve Workspace locking, stale-cache reload, and durable unrelated records.

## Falsification

All PENDING rows are discharged by Main at checkpointed-build in the implementing slice; budgeted-plan will assign exact slice IDs. Oracles are explicit expected records/tables, not another adapter's output alone. Absence assertions always include successful mutation controls on the same fixture. Fixtures identify C1-C7 in failure diagnostics.

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Priority retention and typed bounds | Valid Priority 1 and invalid 5 | Run existing test_update_issue and test_update_rejects_invalid_priority; wrong stored priority or wrong rejection falsifies. Valid update control excludes missing-target/all-reject explanations. Re-run migrated versions after cutover. | Literal Priority 1 and explicit Error::InvalidPriority for 5, independent of production validation | In domain/update.rs builder, lower the accepted maximum to 0; valid Priority 1 fence must fail | Existing in_memory_storage tests named in Falsifier, migrated to validated construction without weakening assertions | Small focused cargo test | PASS — baseline, re-discharge after cutover |
| C2 | Shared validated six-field value | 64 masks, all Kinds, numeric/text partitions, any canonical state | Domain construction/application table plus CLI/MCP update_contract cases assert complete expected records; zero mask rejected, valid nonzero accepted; invalid strings and range errors preserve before-state. Successful empty-description update distinguishes missing vs supplied-empty guards. | Hand-authored field application over fixture copies and explicit grammar boundary examples | In domain/update.rs build, treat Some(empty description) as absent; explicit-empty-only fence must fail | domain::update tests and focused crates/rivets-mcp/tests/update_contract.rs; private-field/Default compile-fail examples | Focused domain and cross-adapter tests | PENDING — Main/checkpointed-build |
| C3 | No empty or obsolete mutation | Missing/null-only; each forbidden key valued/null, alone/with title; CLI removed flags | Real CLI/registered MCP rejection plus direct constructor rejection; compare exact Issue including timestamp and JSONL bytes; valid title control must persist. Mixed forbidden+valid cases exclude an empty guard as the explanation. | Raw before bytes and complete expected unchanged record, not reconstructed serializer output | In models.rs remove UpdateParams deny_unknown_fields; title+status:null request will succeed and fence must fail | update_contract::rejected_update_requests_preserve_state and registered MCP request checks; retain stale_cache empty regression | CLI/server fixture run | PENDING — Main/checkpointed-build |
| C4 | Dedicated intent state and history | Complete state/action/Assignment table, optional reasons and Note history shapes | Call each dedicated storage and adapter operation; compare state, assignee, closed_at semantics and ordered existing/new Notes. Invalid reason/state leaves all fields unchanged; successful append and Close controls prove mutability. | Explicit transition table derived from ADR-0005 and recorded initial Notes, not IssueStatus::validate_transition | In lifecycle transition commit omit reason-note append; Close-with-reason fence must report missing final Note. Separately remove ReturnToOpen source-state guard; Closed case must fail | update_contract::dedicated_intents_preserve_state_and_history and owning storage lifecycle matrix | Focused storage and adapter tests | PENDING — Main/checkpointed-build |
| C5 | Parentage invariants and scale | Epic with active/all-Closed children, child of Closed parent, reclassification with children, 10,000 direct children | Migrate existing Epic close/reopen truth tables and ignored scale test to transition seam; illegal state commit, wrong blocking child IDs, or >100ms measured close check falsifies. Allowed close/reopen controls prevent blanket-rejection explanations. | Explicit fixture child IDs and statuses; existing 100ms bound measured by Instant | In trait_impl.rs transition remove ActiveChildren rejection; existing epic_close_reports_active_direct_children_without_cascade must fail | Existing in_memory_storage Parentage truth tables and epic_close_10k_direct_children_budget | Focused tests plus explicit ignored scale run | PENDING — Main/checkpointed-build |
| C6 | Loop-equivalent partial success | Existing/missing/malformed IDs, duplicate targets, state-dependent failures, valid and invalid shared fields | Run mixed ordered CLI batches and sequential registered MCP calls against equivalent Workspaces; independently assert expected persisted successful targets and error categories for failures; duplicate Note append creates two entries. All-success control prevents a dead mutation path passing. | Literal expected per-target outcomes and final records based on fixture, comparing adapters only after each matches the oracle | In CLI batch loop change error handling to return at first failure; fixture valid/missing/valid must miss final successful change and fail | update_contract::batches_match_ordered_single_target_calls | Real subprocess/protocol fixture run | PENDING — Main/checkpointed-build |
| C7 | Preserved persistence and concurrency | Stale loaded cache after external write; competing writers; 100 targets in 10,000-Issue Workspace | Extend existing stale_cache mutation exercise to new intents and run existing lock regressions; external unrelated record must survive while the target mutation persists. Exercise a 100-target batch in a 10,000-Issue fixture and compare all target and unrelated records after restart. Successful target mutation is the positive control excluding a skipped save. Performance budget is the deterministic C5 bound, not a second ungrounded batch-latency promise. | External JSONL edit marker and independently parsed complete expected final records | In JsonlBackedStorage::transition omit prepare_mutation; external marker preservation fence must fail | Existing stale_cache_mutations_preserve_external_jsonl_changes extended for lifecycle/append; existing Workspace lock fences; update_contract large-Workspace batch case | Focused integration and explicit stress fixture runs | PENDING — Main/checkpointed-build |

## Non-goals and future work

Permanent non-goals for this change: new persistence backend, changed Workspace lock policy, Priority scale/sorting changes, altered text trimming/length/clearing grammar, closed_at cleanup, all-or-nothing batches, new authentication, generic lifecycle status setter, compatibility aliases, broad execute.rs splitting. They are unrelated behavior changes or undermine dedicated ownership.

Intended separate work, verified in tracker: rivets-mmqc owns inventory-wide MCP schema modernization; rivets-y4vw owns inventory-wide semantic parity harness. Neither defers any acceptance criterion of rivets-67d7: this cutover updates its own schemas, callers, registry and behavior fences.

Review-size risk: public Update removal touches many tests. budgeted-plan must inventory cumulative churn plus margin before implementation and split into independently mergeable increments if >4,000 lines; do not retain a production compatibility shim to manufacture green slices.

## Falsifier run log

2026-09-06, worktree /home/dwalleck/repos/rivets-67d7 at upstream 487d6b2:
`cargo test -q -p rivets --test in_memory_storage test_update_`
Result: 2 passed, 0 failed, 52 filtered out. C1 PASS: valid Priority update succeeds and invalid Priority is rejected with the expected typed error. This is a baseline survival check, not proof of the unimplemented cutover.

Earlier baseline: `cargo test -q -p rivets-mcp --test stale_cache empty`: 1 passed. C3 remains PENDING because obsolete mixed-field requests and shared constrained construction do not yet have the target behavior.

LSP cross-worktree reference lookup returned no references despite known uses; reported through xd://report_issue. Before public-symbol implementation, repeat references in the correct worktree server and use compiler-driven migration plus scoped inventory if unavailable.

## Approval

Requester approval (verbatim): "yes, I approve"
Date: 2026-09-06
Risk acceptances: None. No regression fence is waived.
