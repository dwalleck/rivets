# Budgeted implementation plan: single-Epic Parentage

## Inputs and partition

Approved design: `.rivets-qcje/design.md`, requester approval "Approve as written" on 2026-08-30. Route: Structural. Every design claim C1-C11 is assigned exactly once below; every PENDING falsifier is discharged in its owning slice.

Upstream discovery identified `origin/main` as the repository default. This increment is developed as the next independently-green stack entry above committed `work/brai`, because its lifecycle and mutation paths consume the committed durable-lock and canonical-readiness cutovers; it must be rebased onto the default branch after those prerequisites land.

| Slice | Claims | Diff estimate |
|---|---|---:|
| 1. Typed aggregate and relationship operations | C1-C5, C9 | 950 |
| 2. Lifecycle and readiness invariants | C6-C8 | 550 |
| 3. CLI Parentage contract | C10 | 650 |
| 4. MCP Parentage contract and synchronized guidance | C11 | 850 |
| **Estimated implementation + tests** | **C1-C11** | **3,000** |
| Churn margin | 25% for trait-callsite fanout, four adapter operations, and mutation-fence corrections | 750 |
| **Projected cumulative change** |  | **3,750** |

The projected 3,750 changed lines are below the 4,000-line review-size gate, so the plan has one PR increment.

### PR increment P: Parentage cutover

Slices 1-4 in dependency order. Mergeable definition: the repository exposes typed single-Epic Parentage through storage, CLI, and MCP; all lifecycle, cycle, atomic-move, non-blocking, persistence, restart, and structured-output claims pass without a later increment. Verification: each slice's focused fences, then the final applicable workspace gate. The increment does not write the future canonical `relationships` schema; verified Task `rivets-vio8` owns that migration.

## Slice 1: Add typed Parentage and atomic storage operations

**Claim IDs:** C1, C2, C3, C4, C5, C9

**Expected behavior:** `Parentage` has private role-named endpoints and validating serde; storage set/clear/move/show enforces existing Epic parent, one parent, independent Parentage cycles, idempotent same-parent retries, validation-before-replacement, kind-specific removal, and deterministic JSONL restart without disturbing parallel Blocking edges.

**Oracle:** Literal child-to-parent maps and test-local Parentage path traversal; raw `serde_json::Value` dependency records and fresh-loader queries for restart; separately queried Blocking pairs for kind isolation.

**Stress fixture:** 10,000 Issues with a 5,000-Epic Parentage chain and 50,000 mixed graph edges. Setting an acyclic leaf, rejecting one deep cycle, querying one parent, and moving one child must match the literal map; no second parent or lost parallel edge is allowed.

**Regression fence:** Domain test `parentage_constructor_and_deserialization_reject_self_reference`; storage integration tests `parentage_cardinality_and_epic_parent_are_enforced`, `nested_epics_use_parentage_only_cycle_detection`, `parent_move_validates_before_atomic_replacement`, `parent_clear_and_show_preserve_parallel_relationships`, and `parentage_jsonl_restart_round_trip` in `crates/rivets/tests/in_memory_storage.rs`.

**Named mutation:** C1 remove the equality branch in `Parentage::new`; C2 delete the candidate-parent Epic check; C3 filter `has_parentage_cycle_impl` by `Blocks`; C4 remove the old edge before candidate validation; C5 clear the first outgoing edge without filtering `ParentChild`; C9 omit `ParentChild` during export. Each owning fence must turn red, then return green after restoration.

**Complexity/production scale:** Parent lookup and kind-specific edge replacement are O(out-degree(child)); direct endpoint validation is O(1); Parentage cycle detection is O(V + E_total) because mixed edges are enumerated then filtered; JSONL save remains its existing O(V + E log E) deterministic ordering. At 10,000 Issues and 50,000 mixed edges, each Parentage query/mutation computation must complete within 100 ms. The bound is deliberately twice the existing 50 ms mixed-edge Blocking budget to cover the additional ownership checks without masking an accidental quadratic walk.

**Wall budget/phase:** Always-on for each Parentage storage call; ≤100 ms for each cycle/query/move computation on the 10,000-Issue/50,000-edge fixture, excluding pre-existing JSONL disk-save time.

**Files:** `crates/rivets/src/domain/relationship.rs`, `crates/rivets/src/domain/mod.rs`, `crates/rivets/src/error.rs`, `crates/rivets/src/storage/mod.rs`, `crates/rivets/src/storage/in_memory/graph.rs`, `crates/rivets/src/storage/in_memory/trait_impl.rs`, `crates/rivets/tests/in_memory_storage.rs`.

**Estimate:** 1.5 engineering days.

**Diff estimate:** 950 changed lines including domain/storage tests and the production-scale fixture.

**PR increment:** P

**Commands and expected results:**
- `cargo test -p rivets domain::relationship::tests::parentage_constructor_and_deserialization_reject_self_reference` → constructor and serde accept distinct roles and reject equal IDs against the literal equality oracle; C1 mutation turns red, restoration green.
- `cargo test -p rivets --test in_memory_storage parentage_` → every C2-C5/C9 named storage fence matches the literal map/raw-JSON oracle; each named mutation turns only its owning property red, restoration green.
- `cargo test -p rivets storage::in_memory::graph::tests::parentage_graph_stays_within_scale_budget -- --ignored --exact` → literal-map results agree item by item and every measured Parentage computation is ≤100 ms.
- `cargo check -p rivets -p rivets-mcp` → every `IssueStorage` implementation and caller is migrated in the same slice; no missing trait method or stale signature remains.

## Slice 2: Enforce Epic lifecycle invariants without Parentage blockedness

**Claim IDs:** C6, C7, C8

**Expected behavior:** closing an Epic reports sorted non-Closed direct children and mutates nothing; active children cannot attach/move/reopen beneath Closed Epics while Closed children may attach; parent ownership never changes Blocked/Ready, and an explicit child Blocking Dependency remains the positive control.

**Oracle:** Test-local direct-child/status truth tables and literal unresolved Blocking endpoint sets, independent of graph traversal and transition implementation.

**Stress fixture:** One Epic with 10,000 direct children alternating Open/In Progress/Closed. Close returns exactly the 6,667 non-Closed IDs in deterministic order without changing any child; after all children close, Epic close succeeds. A separate blocked-parent fixture leaves its child Ready until an explicit child Blocking edge is added.

**Regression fence:** Storage integration tests `epic_close_reports_active_direct_children_without_cascade`, `legacy_parentage_never_propagates_blockedness`, and `closed_parent_attachment_and_reopen_truth_table` in `crates/rivets/tests/in_memory_storage.rs`.

**Named mutation:** C6 bypass the direct-child close guard; C7 add ParentChild ancestor propagation to `find_blocked_issues`; C8 remove the Closed-parent validation branch. Each owning fence must turn red, then return green after restoration.

**Complexity/production scale:** Direct-child discovery is O(E_total) with deterministic O(C log C) sorting for C active direct children; blockedness remains the existing O(V + E_total) Blocking-only scan and adds no Parentage walk. At 10,000 children/50,000 mixed edges, active-child discovery and sort must complete within 100 ms; this catches repeated full-graph scans or recursive descendant traversal, neither of which the direct-child rule permits.

**Wall budget/phase:** Always-on for close/reopen/attach validation; ≤100 ms for the 10,000-child direct-child computation, excluding existing JSONL save and user-facing formatting.

**Files:** `crates/rivets/src/domain/relationship.rs`, `crates/rivets/src/error.rs`, `crates/rivets/src/storage/in_memory/trait_impl.rs`, `crates/rivets/src/storage/in_memory/graph.rs`, `crates/rivets/tests/in_memory_storage.rs`.

**Estimate:** 1 engineering day.

**Diff estimate:** 550 changed lines including lifecycle truth-table and stress fixtures.

**PR increment:** P

**Commands and expected results:**
- `cargo test -p rivets --test in_memory_storage epic_close_reports_active_direct_children_without_cascade -- --exact` → exact sorted active-child IDs and unchanged parent/children match the truth table; C6 mutation turns red, restoration green.
- `cargo test -p rivets --test in_memory_storage legacy_parentage_never_propagates_blockedness -- --exact` → parent-only blocker leaves child Ready; explicit child blocker positive control removes it; C7 mutation turns red, restoration green.
- `cargo test -p rivets --test in_memory_storage closed_parent_attachment_and_reopen_truth_table -- --exact` → every child/parent state pair matches the approved truth table; C8 mutation turns red, restoration green.
- `cargo test -p rivets --test in_memory_storage epic_close_10k_direct_children_budget -- --ignored --exact` → all 6,667 active IDs match and computation is ≤100 ms.

## Slice 3: Expose role-safe Parentage through the real CLI

**Claim IDs:** C10

**Expected behavior:** `parent set|clear|move|show` accepts explicit `--child` and `--parent` roles; mutations use `App::from_directory_for_mutation`, save once, and return exact text/JSON; show is read-only; core typed errors are preserved; failed move followed by show proves no mutation; restart returns the same Parentage.

**Oracle:** Literal stdout/stderr text and JSON values plus independent raw Workspace reload after each real process exits.

**Stress fixture:** CLI Workspace with Unicode/spaces in titles, nested Epics, a child carrying parallel Blocking, a failed non-Epic move, and a successful move across process restart. Expected output keys remain `relationship`, `child_id`, `parent_id`, and for move `previous_parent_id`; IDs—not titles—drive roles.

**Regression fence:** Real process integration test `parent_cli_contract_and_restart` in `crates/rivets/tests/cli_tests.rs`, plus Clap shape unit tests for every leaf.

**Named mutation:** Swap child and parent when `execute_parent` constructs set Parentage; the exact role/output/restart fence must turn red, then return green after restoration.

**Complexity/production scale:** N/A — reason: the adapter adds no loop or collection algorithm; it delegates one typed operation to Slice 1/2 storage.

**Wall budget/phase:** N/A — reason: one-off CLI invocation; no new always-on phase beyond the budgeted storage call.

**Files:** `crates/rivets/src/cli/args.rs`, `crates/rivets/src/cli/mod.rs`, `crates/rivets/src/cli/execute.rs`, `crates/rivets/tests/cli_tests.rs`, `README.md`, `docs/README.md`, `docs/agents/issue-tracker.md`.

**Estimate:** 1 engineering day.

**Diff estimate:** 650 changed lines including process fixtures and current CLI guidance.

**PR increment:** P

**Commands and expected results:**
- `cargo test -p rivets --test cli_tests parent_cli_contract_and_restart -- --exact` → every action's text/JSON/error and fresh-process state exactly matches the literal oracle; endpoint-swap mutation turns red, restoration green.
- `cargo test -p rivets cli::tests::parent_` → all four canonical leaves parse only explicit roles; missing/mutually invalid arguments are rejected by Clap.
- `cargo run -p rivets -- parent --help` → help lists exactly set, clear, move, show with child/parent vocabulary and no generic dependency terminology.

## Slice 4: Expose equivalent guarded Parentage through MCP

**Claim IDs:** C11

**Expected behavior:** `parent_set`, `parent_clear`, `parent_move`, and `parent_show` serialize the same role names and core semantics as CLI; mutation tools use the guarded mutation-storage path, classify client-fixable typed errors as `invalid_params`, preserve state across recreated contexts, and return Workspace Busy without mutation under a held lock.

**Oracle:** Literal serialized JSON values and JSON-RPC error codes, raw Workspace reload, and a held `WorkspaceMutationLock` positive contention control.

**Stress fixture:** Real JSONL Workspace exercised once through implicit context and once through explicit `workspace_root`, with same-parent retries, failed move, context recreation, parallel Blocking edge, and held-lock mutation attempts. Query remains readable under lock; every mutator returns Workspace Busy and raw bytes remain unchanged.

**Regression fence:** MCP integration test `parentage_mcp_contract_context_recreation_and_locking` in `crates/rivets-mcp/tests/integration.rs`; server schema/error tests; Parentage additions to `crates/rivets-mcp/tests/workspace_lock.rs` and stale-cache mutation coverage.

**Named mutation:** Implement `Tools::parent_move` by calling `set_parent`; the existing-parent move fixture must turn red, then return green after restoration.

**Complexity/production scale:** N/A — reason: MCP adds constant-shape request translation and delegates all collection/graph work to the budgeted storage interface.

**Wall budget/phase:** N/A — reason: one-off MCP tool request; no new always-on background phase and no adapter loop.

**Files:** `crates/rivets-mcp/src/models.rs`, `crates/rivets-mcp/src/tools.rs`, `crates/rivets-mcp/src/server.rs`, `crates/rivets-mcp/src/error.rs`, `crates/rivets-mcp/tests/integration.rs`, `crates/rivets-mcp/tests/workspace_lock.rs`, `crates/rivets-mcp/tests/stale_cache.rs`, `docs/README.md`, `docs/agents/issue-tracker.md`, `CHANGELOG.md`.

**Estimate:** 1.5 engineering days.

**Diff estimate:** 850 changed lines including router/model, restart, lock, cache, and wire-shape fences.

**PR increment:** P

**Commands and expected results:**
- `cargo test -p rivets-mcp --test integration parentage_mcp_contract_context_recreation_and_locking -- --exact` → implicit/explicit context, exact wire values/errors, failed-move atomicity, and fresh-context state match the literal/raw-file oracle; set-for-move mutation turns red, restoration green.
- `cargo test -p rivets-mcp parentage_tool_` → router lists all four tools, schemas expose role-named fields, and every typed Parentage rejection maps to `invalid_params`.
- `cargo test -p rivets-mcp --test workspace_lock workspace_lock_blocks_every_mcp_mutator_but_not_queries -- --exact` → all three Parentage mutators return Workspace Busy, show remains readable, and persisted bytes remain identical.
- `cargo test -p rivets-mcp --test stale_cache` → Parentage mutation reloads current disk state before applying and cannot overwrite an external change.

## Tracker taxonomy

- **Intended future work — `rivets-vio8`:** invalid legacy Parentage warning/Note preservation and canonical structured `relationships` persistence. The verified Task explicitly depends on `rivets-qcje` and owns that rewrite.
- **Permanent non-goal:** cascade closure, Epic rollups, or Parentage-derived blockers. These contradict ADR-0002's separation of ownership, lifecycle, and readiness.
- **Permanent non-goal:** generic/custom relationship mutation. The clean cutover keeps one intent-named interface per relationship kind.

## Self-review

- [x] C1-C11 are each assigned to exactly one slice; every PENDING falsifier is discharged by the slice implementing its claim, and C7 remains assigned to the lifecycle slice despite its pre-design PASS.
- [x] Every slice has all thirteen mandatory fields; conditional fields carry `N/A — reason`.
- [x] Every claim's permanent fence and exact named mutation land together.
- [x] Every new loop states asymptotic cost, production fixture, explicit maximum cost, and rationale; every always-on computation has a wall budget.
- [x] Partition arithmetic is 3,000 + 750 = 3,750, below 4,000; every slice names single increment P, whose mergeable definition is recorded.
- [x] Every deferral phrase is classified; verified `rivets-vio8` owns intended migration work.
- [x] No slice is marked complete; checkpointed-build exclusively judges completion.
