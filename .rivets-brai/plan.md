# Budgeted plan: rivets-brai

## Inputs and partition

- Approved design: `.rivets-brai/design.md`, requester words “Approve design”, 2026-08-29, no accepted fence risk.
- Route: Structural.
- Slice diff sum: 1,750 changed lines.
- Churn margin: 25% = 438 lines, rounded up from 437.5. Rationale: lifecycle enum removal fans through exhaustive matches and fixtures, while the new Ready query type changes every trait adapter and test helper.
- Projected total: 2,188 changed lines.
- Review-size gate: 2,188 ≤ 4,000, so one PR increment.

### PR increment: canonical-readiness

Slices 1-2 in dependency order. Mergeable definition: the repository accepts and emits only three Workflow States; storage derives Blocked only from explicit unresolved Blocking Dependencies; Ready uses the assignee-aware canonical predicate through CLI and MCP; canonical restart behavior and output contracts pass without later work. Verification: the domain, storage, real CLI process, MCP integration, restart, stress, wire/parity registry, and workspace gates named below all pass within this increment.

## Slice 1: Remove Blocked from lifecycle vocabulary and output

**Claim IDs:** C1, C8

**Expected behavior:** Domain serde/FromStr/ValueEnum, CLI lifecycle inputs, MCP lifecycle inputs, persistence output, and direct Issue JSON expose exactly Open/In Progress/Closed. CLI statistics report those three lifecycle counts plus a separate derived Blocking count; Ready/Blocked outputs contain only canonical Issue statuses.

**Oracle:** The literal ADR-0002 state set `{open,in_progress,closed}` and manually counted fixture records/Blocking endpoints; neither oracle derives from the production enum or statistics renderer.

**Stress fixture:** N/A — pure enum/schema subtraction and presentation of an existing aggregate; no collection algorithm or production-scale path is introduced.

**Regression fence:**
- `crates/rivets/src/domain/mod.rs::issue_status_canonical_vocabulary`
- `crates/rivets/tests/cli_tests.rs::canonical_workflow_state_inputs`
- `crates/rivets-mcp/tests/integration.rs::canonical_workflow_state_inputs`
- existing MCP direct-Issue wire golden assertion updated to the exact value set
- `crates/rivets/tests/cli_tests.rs::stats_and_frontier_output_separate_lifecycle_from_blocked`

**Named mutation:**
- C1: in `crates/rivets/src/domain/mod.rs` FromStr, add `\"blocked\" => Ok(Self::Open)`; the domain rejection fence must turn red without a non-exhaustive compile failure.
- C8: add `\"blocked\": 0` under the JSON `by_status` object in `crates/rivets/src/cli/execute.rs`; the exact-key statistics fence must turn red.

**Complexity/production scale:** N/A — no new loop; enum parsing/serialization remains O(1), and the existing statistics scan is not duplicated.

**Wall budget/phase:** N/A — no new runtime phase; existing parse, render, and statistics phases only lose lifecycle branches.

**Files:**
- `crates/rivets/src/domain/mod.rs`
- `crates/rivets/src/output/color.rs`
- `crates/rivets/src/cli/execute.rs`
- `crates/rivets/src/cli/mod.rs`
- `crates/rivets-mcp/src/tools.rs`
- `crates/rivets-mcp/src/error.rs`
- `crates/rivets-mcp/src/server.rs`
- `crates/rivets/tests/in_memory_storage.rs`
- `crates/rivets/tests/cli_tests.rs`
- `crates/rivets-mcp/tests/integration.rs`

**Estimate:** 2 hours

**Diff estimate:** 450 changed lines: 140 implementation cleanup + 310 test/fixture updates.

**PR increment:** canonical-readiness

**Commands and expected results:**
- `cargo test -p rivets issue_status_canonical_vocabulary` → literal set is exactly three; each canonical value round-trips; Blocked does not parse as a domain value.
- `cargo test -p rivets --test cli_tests canonical_workflow_state_inputs` → real CLI accepts all canonical values, rejects `blocked`, and reports only the three valid values.
- `cargo test -p rivets-mcp --test integration canonical_workflow_state_inputs` → MCP accepts all canonical values, rejects `blocked` with the same valid-value meaning, and emits only canonical state strings.
- `cargo test -p rivets --test cli_tests stats_and_frontier_output_separate_lifecycle_from_blocked` → text/JSON lifecycle counts have exactly Open/In Progress/Closed, derived blocked count is nonzero and separate, and nested Ready/Blocked Issues use canonical states.
- Under each named mutation, rerun its command → the named fence fails for the mutated property; restore the mutation and rerun → the same fence passes.

## Slice 2: Centralize assignee-aware Ready and direct Blocked semantics

**Claim IDs:** C2, C3, C4, C5, C6, C7

**Expected behavior:** `IssueStorage::ready_to_work` accepts a dedicated `ReadyFilter` whose assignment mode is Unassigned by default, one exact Assignee, or explicit All. It returns only Open Issues without unresolved explicit Blocking prerequisites, then applies priority/kind/label filters, sorting, and limit. Parentage, Related, and Discovery edges never affect eligibility. CLI `--all-assignees` and MCP `all_assignees` map identically; selecting both one assignee and all assignees fails. Fresh CLI processes and recreated MCP contexts reproduce the same Ready and Blocked sets from canonical JSONL.

**Oracle:** Test-local truth tables over fixture metadata, raw JSONL records, and role-named relationship endpoints compute `state == Open && no non-Closed Blocking prerequisite && assignment_mode.matches(assignee)` independently of graph traversal, Ready filtering, and adapter result comparison.

**Stress fixture:** 10,000 Issues with 50,000 seeded relationship edges spanning all four legacy kinds, mixed prerequisite states, mixed assignments, and at least one eligible positive control per assignment mode. Expected: only direct `Blocks` edges to non-Closed prerequisites exclude Open Issues; a test-local oracle matches every returned ID; a timed Ready query completes within 2 seconds. This is over 40× the audited 224-Issue Workspace and over 600× its 75 Blocking edges.

**Regression fence:**
- `crates/rivets/tests/in_memory_storage.rs::closed_prerequisite_stays_recorded_without_blocking`
- `crates/rivets/tests/in_memory_storage.rs::non_blocking_relationships_never_change_readiness`
- `crates/rivets/tests/in_memory_storage.rs::ready_truth_table_covers_state_blocking_and_assignment`
- `crates/rivets/tests/in_memory_storage.rs::ready_filters_sort_and_limit_after_eligibility`
- `crates/rivets/tests/in_memory_storage.rs::ready_stress_fixture_matches_oracle_within_budget`
- `crates/rivets/tests/cli_tests.rs::ready_assignment_visibility`
- `crates/rivets-mcp/tests/integration.rs::ready_assignment_visibility`
- CLI parser and MCP model conflict tests
- CLI process restart test and `crates/rivets-mcp/tests/integration.rs::ready_and_blocked_survive_context_recreation`

**Named mutation:**
- C2: in `storage/in_memory/trait_impl.rs`, include Closed prerequisites unconditionally; the close/unblock fence turns red.
- C3: restore ParentChild propagation in `storage/in_memory/graph.rs`; the parent-only child disappears from Ready and the non-blocking relationship fence turns red.
- C4: replace `status == Open` with `status != Closed`; the paired In Progress Issue appears and the truth-table fence turns red.
- C5: map omitted MCP selector to `All`; the MCP default expected-ID fence turns red while explicit All remains the positive control.
- C6: skip rebuilding `Blocks` edges in `storage/in_memory/jsonl.rs`; fresh process/context output disagrees with the raw-record oracle.
- C7: delete the Label comparison from the Ready-specific filter; the label-mismatched eligible control appears in the exact sequence.

**Complexity/production scale:** The changed Ready query performs one direct graph/Issue eligibility pass plus one result filter/sort: O(n + e + r log r), where n is Issues, e is relationship edges, and r ≤ n is eligible results. At the audited n=224/e≈214 total relationships this is negligible. The 10,000-Issue/50,000-edge stress fixture is the explicit production-scale upper fixture; maximum accepted Ready query cost is 2 seconds, chosen to detect accidental per-Issue graph rescans while remaining stable on CI. No per-result allocation beyond the existing returned Issue clones and sort buffer is added.

**Wall budget/phase:** Always-on Ready query: ≤2 seconds at 10,000 Issues/50,000 edges, with the normal 224-Issue Workspace expected far below that bound. Blocked query retains one O(n + e) pass and no new phase. Adapter selector conversion is O(1).

**Files:**
- `crates/rivets/src/domain/mod.rs`
- `crates/rivets/src/storage/mod.rs`
- `crates/rivets/src/storage/in_memory/graph.rs`
- `crates/rivets/src/storage/in_memory/mod.rs`
- `crates/rivets/src/storage/in_memory/trait_impl.rs`
- `crates/rivets/src/cli/args.rs`
- `crates/rivets/src/cli/execute.rs`
- `crates/rivets/src/cli/mod.rs`
- `crates/rivets-mcp/src/models.rs`
- `crates/rivets-mcp/src/tools.rs`
- `crates/rivets-mcp/src/server.rs`
- `crates/rivets/tests/in_memory_storage.rs`
- `crates/rivets/tests/cli_tests.rs`
- `crates/rivets-mcp/tests/integration.rs`
- `docs/cli-mcp-parity.json`
- `docs/cli-mcp-parity.md`
- `docs/architecture.md`
- `docs/storage-architecture.md`
- `README.md`
- `docs/agents/issue-tracker.md`
- `CHANGELOG.md`

**Estimate:** 5 hours

**Diff estimate:** 1,300 changed lines: 420 domain/storage/adapter implementation + 680 behavioral/stress/restart tests + 200 documentation/registry updates.

**PR increment:** canonical-readiness

**Commands and expected results:**
- `cargo test -p rivets --test in_memory_storage` → every storage fence in this slice agrees item-by-item with the direct-edge, lifecycle, assignment, and secondary-filter oracle; closing the last prerequisite preserves its edge and admits the dependent.
- `cargo test -p rivets --test in_memory_storage ready_stress_fixture_matches_oracle_within_budget` → all 10,000-Issue fixture IDs agree with the independent oracle and measured Ready query time is ≤2 seconds.
- `cargo test -p rivets --test cli_tests ready_assignment_visibility` → default returns only unassigned eligible IDs; named returns only that Assignee's eligible IDs; all returns both; conflicting selectors fail; blocked/In Progress/Closed controls never appear.
- `cargo test -p rivets-mcp --test integration ready_assignment_visibility` → the same literal expected-ID sets and conflict meaning hold through MCP Tools.
- `cargo test -p rivets --test cli_tests ready_and_blocked_survive_restart` → two fresh CLI processes return the same literal Ready/Blocked sets derived from raw JSONL.
- `cargo test -p rivets-mcp --test integration ready_and_blocked_survive_context_recreation` → recreated contexts return the same literal Ready/Blocked sets and recorded relationships.
- `python scripts/render-cli-mcp-parity.py && python scripts/render-cli-mcp-parity.py --check` → generated parity Markdown exactly matches the authoritative JSON registry, whose Ready entry records canonical eligibility while retaining the independent limit/order gap.
- Under each C2-C7 named mutation, rerun its owning fence → red for the named property; restore and rerun → green.

## Tracker taxonomy

- Legacy workflow/relationship migration remains intended future work under verified `rivets-vio8`.
- Atomic Assignment transitions remain intended future work under verified `rivets-8rj9`.
- Canonical Parentage and non-blocking relationship mutation interfaces remain intended future work under verified `rivets-qcje` and `rivets-2x2i`.
- Wire-field rename, serialized derived flags, and Ready sort/default-limit changes are permanent non-goals for this change for the reasons recorded in the approved design.

## Self-review

- [x] Every design claim C1-C8 is assigned exactly once; every PENDING falsifier is assigned to its implementing slice.
- [x] Both slices contain all thirteen mandatory fields and every conditional field has an explicit N/A rationale.
- [x] Every claim's deterministic fence is created or updated in its implementing slice and has its approved named mutation.
- [x] The changed Ready loop records O(n + e + r log r), audited/current and stress sizes, a 2-second maximum, and its always-on phase budget.
- [x] Diff arithmetic is 1,750 + 438 = 2,188, below the 4,000-line review gate; every slice belongs to the independently mergeable `canonical-readiness` increment.
- [x] Every future-work statement cites a verified tracker Issue; permanent non-goals carry approved rationales.
- [x] No slice is declared complete; checkpointed-build owns completion.
