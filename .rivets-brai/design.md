# Falsifiable design: rivets-brai

## Route and inputs

- **Route:** Structural, from `.rivets-brai/route.md`.
- **Behavior source:** `.rivets-brai/route.md` T4, which records the complete G1-G5 contract: three canonical Workflow States; Blocked derived only from unresolved Blocking Dependencies; non-blocking relationship kinds excluded from eligibility; Ready restricted to Open/unblocked Issues with default, assignee, and explicit all-assignee visibility; and CLI/MCP/text/JSON/restart agreement.
- **Domain sources:** `CONTEXT.md`, ADR-0002, ADR-0004, ADR-0005, ADR-0006, and parent specification `rivets-5mlg`.
- **Code evidence:** `IssueStatus` still has `Blocked`; `find_blocked_issues` propagates Parentage; `ready_to_work` accepts every non-Closed, unblocked Issue and treats an omitted Assignee filter as all assignees; CLI/MCP map directly to that query; status output and statistics still count Blocked as lifecycle.
- **Specification:** N/A — route T4 is explicit.
- **Empirical premises/evidence/probe:** N/A — Structural route with no unverified premise.
- **Schema decision:** ADR-0005 and ADR-0006 govern the later parent-spec wording: `status` remains the code-and-wire field name for Workflow State. This change shrinks its value set; it does not rename the field.

## Input shapes

| Shape | Production-reachable cases | Status |
|---|---|---|
| S1 — canonical Workflow State | `open`, `in_progress`, `closed` through domain serde/FromStr/ValueEnum, CLI list/update/stale inputs, MCP list/update/stale inputs, persisted canonical records, and direct Issue JSON output | Covered by C1 |
| S2 — noncanonical state text | empty, ASCII case variants, whitespace-padded strings, `in-progress` outside clap's documented alias, `blocked`, and arbitrary Unicode/text | Covered by C1 for canonical adapters; legacy persisted `blocked` conversion is N/A — intended future work `rivets-vio8`, while the existing unsafe-partial-load guard preserves rejected records byte-for-byte |
| S3 — dependent lifecycle | Open, In Progress, and Closed dependents | Covered by C2 and C4 |
| S4 — Blocking prerequisite collection | empty; one Open/In Progress/Closed prerequisite; multiple all-Closed prerequisites; multiple mixed Closed/non-Closed prerequisites; duplicate edge N/A — storage rejects duplicate Blocking Dependencies | Covered by C2 |
| S5 — relationship kind | Blocking Dependency, Parentage, Related Association, Discovery Origin, and multiple different kinds on one Issue pair | Covered by C3 |
| S6 — Assignment value | unassigned; assigned to requested assignee; assigned to another assignee; empty, ASCII, Unicode, and embedded-space assignee strings (exact-match behavior is retained; Assignee validation is not introduced here) | Covered by C4 and C5 |
| S7 — Ready assignment selector | omitted/default; one assignee; explicit all-assignees; assignee plus all-assignees simultaneously | Covered by C4 and C5 |
| S8 — optional Ready filters | priority, Issue Kind, and Label each absent/present and jointly present; no status filter because readiness owns lifecycle | Covered by C7 |
| S9 — Ready ordering and limit | Hybrid/Priority/Oldest; zero limit; one; larger than result set | Covered by C7 |
| S10 — Workspace population | empty, single Issue, multiple distinct Issues; duplicate IDs N/A — Workspace identity invariant rejects them | Covered by C2, C4, and C7 |
| S11 — adapter/output lifecycle | CLI text, CLI JSON, MCP JSON, same process/server, and fresh process/server reading the same canonical JSONL Workspace | Covered by C5, C6, and C8 |
| S12 — statistics | no blocked Issues; one/multiple derived blocked Issues; each of the three lifecycle states | Covered by C8 |

## Removed-invariant sweep

This change is subtractive in two places and restrictive in two others.

1. **Remove `IssueStatus::Blocked`.** The variant currently forces every status match, parser, serializer, color/icon renderer, transition matrix, and counter to acknowledge Blocked as lifecycle. Exhaustive matching still protects the remaining three states. Persisted legacy `blocked` no longer reaches the domain; the existing partial-load write guard preserves its bytes until `rivets-vio8` performs the specified lossless conversion and migration Note.
2. **Remove Parentage propagation from `find_blocked_issues`.** The propagation loop silently made a child ineligible when only its parent had a Blocking Dependency. Removing it makes that previously impossible Ready child valid. Direct Blocking cycles remain rejected by the typed relationship operations; relationship-tree depth protection remains independently owned by tree traversal.
3. **Restrict Ready lifecycle from non-Closed to Open.** In Progress was previously offered as Ready. The storage predicate must now compare explicitly with Open so a future lifecycle variant cannot become Ready by default.
4. **Restrict default Ready visibility from all assignees to unassigned.** Assigned Open Issues remain Ready in the domain sense, but query visibility must be explicit: default unassigned, one named assignee, or administrative all-assignees. A sum type preserves mutual exclusion after adapter parsing.

Every now-possible violation is covered by C1-C5. The direct-Blocking close behavior remains safe and is covered by C2.

## Placement

### Canonical Workflow State vocabulary

- **Owner:** `crates/rivets/src/domain/mod.rs`, where `IssueStatus` already owns serde, clap ValueEnum, Display, FromStr, transition rules, and valid-value reporting. The type name and `status` field remain under ADR-0005/0006.
- **New seam:** None — shrink the existing domain interface to Open/InProgress/Closed.
- **Forbidden:** CLI, MCP, output, and persistence must not retain private Blocked string tables or synthesize Blocked as an `IssueStatus`. Exhaustive enum matching and adapter integration fences make stale lifecycle handling fail.

### Derived Blocked predicate

- **Owner:** `crates/rivets/src/storage/in_memory`, behind `IssueStorage::blocked_issues` and `IssueStorage::ready_to_work`. The graph module owns relationship traversal; adapters receive results only.
- **New seam:** None — simplify the existing storage query interface implementation so only outgoing Blocking Dependencies to non-Closed prerequisites contribute.
- **Forbidden:** CLI/MCP may not traverse relationship records or infer blockedness from Parentage, Related, Discovery Origin, text, or assignment. Graph internals remain crate-private; adapter-level fences drive the public query and catch substituted eligibility logic.

### Assignee-aware Ready filtering

- **Owner:** the domain query types plus the existing `IssueStorage::ready_to_work` interface. Add `ReadyAssignmentFilter::{Unassigned, Assignee(String), All}` and a `ReadyFilter` whose default is Unassigned and whose fields cover priority, Issue Kind, Label, and limit.
- **Competing interface A:** add `all_assignees: bool` to generic `IssueFilter`. Small diff, but `IssueFilter` is shared by List where omitted assignment means All; one default cannot represent both List and Ready without caller conventions.
- **Competing interface B:** retain `Option<IssueFilter>` and infer `None` as Unassigned, `Some(default)` as All. No new type, but semantically identical values differ by wrapper presence and callers can combine an assignee field with administrative visibility.
- **Decision:** use a dedicated `ReadyFilter` at the existing Ready query seam. It makes every state valid, gives Ready its canonical default, and prevents status/assignment ambiguity without changing List semantics.
- **New seam:** None — `ReadyFilter` deepens the existing `ready_to_work` interface rather than adding another module or trait.
- **Forbidden:** adapters must not use `IssueFilter` for Ready or post-filter storage results. CLI `--all-assignees` conflicts with `--assignee`; MCP `all_assignees` plus `assignee` returns a typed invalid-argument error before querying.

### CLI, MCP, output, persistence, and documentation adapters

- **Owner:** each existing adapter maps its native inputs once to domain query values and delegates. CLI adds `--all-assignees`; MCP adds `all_assignees`; both keep their existing limit/ordering mechanics in this slice. Output removes Blocked lifecycle rendering/counts while the blocked command and derived statistics remain relationship-driven. Canonical persistence continues to serialize `status` using `IssueStatus`.
- **New seam:** None — all capabilities fit existing adapters and ADR-0004's direct domain serialization seam.
- **Forbidden:** no output DTO mirror, adapter-local readiness predicate, `workflow_state` parallel field, compatibility alias, or deprecated Blocked domain path. `docs/cli-mcp-parity.json` remains the source and its rendered Markdown must be regenerated.

## Claims

- **C1.** Every canonical domain, CLI, MCP, persisted, and emitted Workflow State accepts or emits exactly Open, In Progress, or Closed.
- **C2.** A non-Closed Issue is Blocked exactly while at least one recorded Blocking prerequisite is non-Closed, and closing the final prerequisite preserves the relationship while clearing Blocked immediately.
- **C3.** Parentage, Related Associations, and Discovery Origins never change Blocked or Ready membership, including when they coexist with Blocking Dependencies.
- **C4.** Ready membership is the conjunction of Open, no unresolved Blocking prerequisite, and the selected assignment visibility mode.
- **C5.** CLI and MCP map default, named-assignee, explicit all-assignees, and conflicting selector inputs to the same Ready outcomes and error meaning.
- **C6.** Canonical JSONL reloads preserve lifecycle, Blocking relationships, Blocked groups, and Ready membership across fresh CLI processes and recreated MCP contexts.
- **C7.** Ready priority/kind/label filtering, sorting, and limiting operate only after canonical eligibility has excluded non-Ready Issues.
- **C8.** Text and JSON statistics expose only the three lifecycle counts and a separate derived Blocked count, while Ready and Blocked output serialize canonical Issue states.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Every canonical state surface has exactly three values. | S1, S2 | Drive domain serde/FromStr/ValueEnum, real CLI state inputs, MCP tool inputs, and direct Issue JSON; any accepted/emitted `blocked` or rejection of one of the three canonical values falsifies C1. Positive control: each canonical value succeeds. Other possible cause controlled: parser-only rejection cannot pass because serialization and both adapters are driven separately. | A test-local literal set `{open,in_progress,closed}` copied from ADR-0002, not derived from `IssueStatus::value_variants`. | In `domain/mod.rs` FromStr, add `\"blocked\" => Ok(Self::Open)`; the domain rejection assertion turns red without relying on a non-exhaustive compile failure. | Domain `issue_status_canonical_vocabulary`; CLI process `canonical_workflow_state_inputs`; MCP integration `canonical_workflow_state_inputs`; wire golden assertion. | 1 minute | PENDING — checkpointed-build, per-slice gate assigned in `plan.md` |
| C2 | Direct unresolved Blocking prerequisites exactly determine Blocked. | S3, S4, S10 | Build zero/single/multiple prerequisite matrices, fetch each prerequisite status and recorded edge independently, compare `blocked_issues` and Ready membership before/after closing the final prerequisite, and assert the edge list remains; any mismatch falsifies C2. Other possible cause controlled: relationship deletion is checked separately from eligibility. | Manual predicate over fetched records and role-named edge endpoints: `dependent != Closed && any(prerequisite != Closed)`, computed outside graph traversal. | In `storage/in_memory/trait_impl.rs`, change the prerequisite check from `status != Closed` to unconditional inclusion; `closed_prerequisite_stays_recorded_without_blocking` fails after close. | `crates/rivets/tests/in_memory_storage.rs::closed_prerequisite_stays_recorded_without_blocking` plus blocker-matrix extension. | <1 minute | PASS |
| C3 | Non-blocking relationship kinds never affect eligibility. | S5 | Seed each legacy non-blocking edge kind alone and coexisting with a Blocking edge; any Blocked/Ready change not predicted solely by the Blocking edge falsifies C3. Positive control: add one open Blocking prerequisite and observe exclusion. Other possible cause controlled: identical endpoints are used across kinds. | Expected set built only from fixture Blocking edges; Parentage/Related/Discovery rows are deliberately omitted from the oracle relation. | Restore the ParentChild BFS phase in `storage/in_memory/graph.rs`; the parent-only fixture wrongly removes the child from Ready. | `crates/rivets/tests/in_memory_storage.rs::non_blocking_relationships_never_change_readiness`. | <1 minute | PENDING — checkpointed-build, per-slice gate assigned in `plan.md` |
| C4 | Ready applies lifecycle, blocker, and assignment truth tables together. | S3, S4, S6, S7, S10 | Query a Cartesian fixture through Unassigned, Assignee, and All modes; any ID outside the literal truth-table set or any missing expected ID falsifies C4. Positive controls ensure each mode returns at least one row. Other possible cause controlled: every excluded dimension has a paired Issue differing only in that dimension. | A test-local pure predicate over fixture metadata: `state == Open && blockers_open == 0 && assignment_mode.matches(assignee)`. | In `storage/in_memory/trait_impl.rs`, replace `status == Open` with `status != Closed`; the paired In Progress Issue appears and the matrix fence fails. | `crates/rivets/tests/in_memory_storage.rs::ready_truth_table_covers_state_blocking_and_assignment`. | <1 minute | PENDING — checkpointed-build, per-slice gate assigned in `plan.md` |
| C5 | CLI and MCP assignment selectors have equivalent outcomes and conflicts. | S6, S7, S11 | Run default, named, all, and assignee-plus-all requests through real CLI parsing/process and MCP Tools against equivalent fixtures; compare each to a literal expected ID set and require both conflicts to fail before storage access. Other possible cause controlled: parity is not inferred by comparing adapters to each other. | One adapter-independent fixture manifest with expected IDs for each selector mode and explicit invalid-selector expectation. | In `rivets-mcp/src/tools.rs`, map omitted selector to `All` instead of `Unassigned`; the MCP default expected-ID assertion fails while explicit All remains a positive control. | CLI process `ready_assignment_visibility`; MCP integration `ready_assignment_visibility`; parser/model conflict tests. | 2 minutes | PENDING — checkpointed-build, per-slice gate assigned in `plan.md` |
| C6 | Restart preserves canonical state and eligibility. | S3-S7, S11 | Persist a canonical fixture, record raw relationships and expected sets, run CLI Ready/Blocked, start a fresh process, recreate MCP context twice, and require identical literal outcomes; any restart-only drift falsifies C6. Other possible cause controlled: raw JSONL is inspected so an in-memory cache cannot be the oracle. | Raw JSONL records plus the same manual direct-Blocking/Ready predicate used independently of loader graph reconstruction. | In `storage/in_memory/jsonl.rs`, skip rebuilding `Blocks` edges during load; fresh-process/context results diverge from raw-record oracle. | CLI process restart test and `rivets-mcp/tests/integration.rs::ready_and_blocked_survive_context_recreation`. | 3 minutes | PENDING — checkpointed-build, per-slice gate assigned in `plan.md` |
| C7 | Secondary filters, order, and limit cannot admit ineligible Issues. | S8-S10 | For every sort policy and zero/one/large limits, combine matching and nonmatching eligible/ineligible Issues; any ineligible ID or wrong expected eligible prefix falsifies C7. Positive control: eligible rows with each requested filter exist. Other possible cause controlled: expected ordering uses fixed priorities/timestamps rather than production sorter output. | Literal expected sequences from fixed fixture priority and creation timestamps after a separate test-local eligibility filter. | In `storage/in_memory/trait_impl.rs`, delete the Label comparison from the Ready-specific filter; the eligible but label-mismatched control appears and the exact sequence assertion fails. | `crates/rivets/tests/in_memory_storage.rs::ready_filters_sort_and_limit_after_eligibility`. | 1 minute | PENDING — checkpointed-build, per-slice gate assigned in `plan.md` |
| C8 | Output separates lifecycle counts from derived conditions. | S11, S12 | Run real CLI `stats`, `ready`, and `blocked` in text and JSON on a three-state/direct-block fixture; any lifecycle `blocked` key/value, missing derived count, or noncanonical nested Issue status falsifies C8. Positive control: derived blocked count is nonzero. Other possible cause controlled: fixture also contains an unblocked Open Issue and In Progress Issue. | Manual counts from fixture records and explicit Blocking endpoints, independent of presentation code. | In `cli/execute.rs`, add `\"blocked\": 0` under the JSON `by_status` object; the exact-key assertion turns red. | `crates/rivets/tests/cli_tests.rs::stats_and_frontier_output_separate_lifecycle_from_blocked`. | 2 minutes | PENDING — checkpointed-build, per-slice gate assigned in `plan.md` |

## Non-goals and future work

- **Permanent non-goal:** rename the code/wire field from `status` to `workflow_state`. ADR-0005 and ADR-0006 explicitly retain `status` as the code-and-wire name while using Workflow State as the domain term; introducing a parallel field would violate the one-wire vocabulary.
- **Intended future work — `rivets-vio8`:** lossless migration of legacy Blocked/In Progress/Closed assignment records and legacy relationship records, including migration Notes, warnings, deterministic rewrite, and byte-stable second save. This slice relies on the existing unsafe-partial-load write refusal rather than silently coercing legacy Blocked records.
- **Intended future work — `rivets-8rj9`:** atomic Claim/Release, In Progress assignment requirements, close/reopen assignment transitions, and removal of blind assignment updates. This slice only defines Ready query visibility over the Assignment values that exist.
- **Intended future work — `rivets-qcje` and `rivets-2x2i`:** canonical Parentage, Related Association, and Discovery Origin mutation interfaces and invariants. This slice only removes their effect on Blocked/Ready.
- **Permanent non-goal for this change:** alter Ready sorting policy or CLI/MCP default limits. Those are scheduling/query-shape concerns, not eligibility; `docs/cli-mcp-parity.json` continues to record their independent parity gap.
- **Permanent non-goal:** add a derived `blocked` or `ready` field to serialized Issue records. The existing Blocked/Ready query envelopes expose conditions without duplicating mutable state in ADR-0004's canonical Issue wire shape.

## Falsifier run log

- `2026-08-29 | cargo test -p rivets --test in_memory_storage closed_prerequisite_stays_recorded_without_blocking | PASS` — C2 survived the cheapest decisive falsifier: open prerequisite blocks, closing it unblocks immediately, and the Blocking relationship remains recorded.

## Approval

- **Requester words:** “Approve design”
- **Date:** 2026-08-29
- **Approved risk acceptances:** None; every claim has a deterministic regression fence.
