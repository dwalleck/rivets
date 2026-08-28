# Plan: canonical Blocking Dependencies

## Inputs

- Route: `.rivets-gf4j/route.md` — Structural.
- Approved design: `.rivets-gf4j/design.md`, approved 2026-08-28 with no risk acceptances.
- Specification artifact: N/A — behavior is fully explicit in route T4.
- Empirical evidence artifact: N/A — Structural route.

## Partition arithmetic

| Slice | Diff estimate |
|---|---:|
| 1. Typed domain and storage seam | 1,250 lines |
| 2. CLI and create cutover | 950 lines |
| 3. MCP cutover | 750 lines |
| 4. Generic-surface retirement and synchronized docs | 850 lines |
| **Projected sum** | **3,800 lines** |
| Churn margin | 760 lines (20%; broad exported-trait/test callsite migration and generated parity documentation are the main uncertainty) |
| **Projected total** | **4,560 lines** |

The projected total exceeded the 4,000-line review-size gate. The initial
two-increment partition was revised at the final size tripwire because actual
increment B also crossed 4,000 changed lines.

| Actual increment | Changed lines |
|---|---:|
| A — Slice 1 plus PR feedback | 1,462 |
| B — Slices 2–3 plus carried prerequisite fix | 1,338 |
| C — Slice 4 plus review fixes/log | 3,170 |
| **Summed increment diffs** | **5,970** |
| **Final cumulative base diff** | **5,284** |

### PR increment A — Typed Blocking storage foundation

- Slices: 1.
- Mergeable definition: adds the role-safe domain value and dedicated storage queries/mutations alongside the still-working legacy adapter surfaces. Existing CLI and MCP remain green.
- Independent verification: domain/storage/resilient-loader fences and the full `rivets` crate tests pass without increments B or C.

### PR increment B — Canonical CLI and MCP adapters

- Slices: 2–3.
- Mergeable definition: migrates create, CLI, MCP, output, and adapter tests to the approved Blocking interface while the generic routes remain available only until increment C.
- Independent verification: real CLI process tests and MCP context-recreation/schema tests pass against increment A.

### PR increment C — Generic-surface retirement

- Slices: 4.
- Mergeable definition: removes generic CLI/MCP/storage mutation/query surfaces and synchronizes current-reference documentation.
- Independent verification: registry absence fences, current-reference documentation audit, parity rendering, and the full workspace gate pass against increment B.

## Slice 1: Add the typed Blocking value and deep storage interface

**Claim IDs:** C0, C1, C2, C3, C4, C5, C9  
**Expected behavior:** Storage accepts only role-safe Blocking values; self/duplicate/Blocking-only cycles fail; same-pair legacy kinds remain; typed prerequisite/dependent/tree queries preserve direction; Closed prerequisites remain recorded but are not active blockers.  
**Oracle:** Literal role-named JSON for C1; raw JSONL tuple counts for C2/C9; test-local adjacency DFS for C3; hand-authored BFS levels for C4; direct state-plus-record blocker scan for C5; LSP references plus Rust privacy for C0.  
**Stress fixture:** One in-memory store containing 10,000 Issues and 50,000 mixed edges, including a long Blocking chain, branches with multiple prerequisites/dependents, Related/Parentage/Discovery records on Blocking endpoint pairs, duplicate/self attempts, and Open/In Progress/Closed prerequisites. Expected: Blocking DFS/tree agree item-by-item with the independent adjacency/BFS tables; legacy tuples are unchanged; measured queries/mutations stay within the recorded budgets.
**Regression fence:** Domain unit test `blocking_dependency_preserves_direction_and_rejects_self`; storage integration tests `blocking_dependency_coexists_with_legacy_kind`, `blocking_cycles_ignore_other_relationship_kinds`, `blocking_tree_preserves_direction_and_depth`, `closed_prerequisite_stays_recorded_without_blocking`; resilient-loader test `legacy_relationships_survive_blocking_mutations`.  
**Named mutation:** (C0) import the existing private graph helper from CLI and require E0603; (C1) swap constructor field assignments; (C2) restore endpoint-only `find_edge` or endpoint-wide retain; (C3) remove the Blocks predicate from reachability; (C4) remove the tree Blocks filter or enqueue source; (C5) treat Closed as active; (C9) serialize only typed Blocking queries. Each mutation must turn its named fence red, then restoration must return green.  
**Complexity/production scale:** Duplicate/removal scans are O(outdegree); Blocking reachability/tree are O(V + E_total) because petgraph enumerates outgoing edges before the implementation filters to Blocking semantics. Current audited scale is 224 Issues/75 Blocking edges; stress scale is 10,000 Issues/50,000 mixed edges. Maximum accepted cost: 50 ms per reachability/tree query and 10 ms per add/remove at stress scale on the development test profile, because ordinary local CLI/MCP operations must remain interaction-latency-negligible.
**Wall budget/phase:** always-on storage request phase; 50 ms maximum for graph queries and 10 ms for mutation validation at the stated stress scale.  
**Files:** create `crates/rivets/src/domain/relationship.rs`; modify `crates/rivets/src/domain/mod.rs`, `crates/rivets/src/error.rs`, `crates/rivets/src/storage/mod.rs`, `crates/rivets/src/storage/in_memory/{graph.rs,inner.rs,mod.rs,trait_impl.rs,jsonl.rs}`, `crates/rivets/tests/{in_memory_storage.rs,in_memory_resilient_loading.rs}`.  
**Estimate:** 1.5 engineering days.  
**Diff estimate:** 1,250 changed lines including tests and the mixed legacy fixture.  
**PR increment:** A — Typed Blocking storage foundation.  
**Commands and expected results:**
- `cargo test -p rivets blocking_dependency` → C1–C5 fences agree item-by-item with their literal/DFS/BFS/state-scan oracles; under each named mutation its owning fence turns red and identifies the claim, then returns green after restoration.
- `cargo test -p rivets --test in_memory_resilient_loading legacy_relationships_survive_blocking_mutations` → all original non-blocking tuples and order survive add/remove/save/reload while the Blocking tuple changes independently.
- `cargo test -p rivets` → increment A is independently green with legacy adapters still operating.
- `cargo check --workspace` → adapters compile through `IssueStorage`; the C0 privacy mutation turns this command red with E0603.

## Slice 2: Cut Issue creation and CLI/output over to canonical roles

**Claim IDs:** C6, C10  
**Expected behavior:** `NewIssue` and CLI create accept explicit prerequisites atomically; `blocking-dependency add/remove/list/tree` expose correct role words and JSON from both endpoint perspectives; restart preserves identifiers and direction.  
**Oracle:** Raw before/after JSONL edges and records plus the literal phrase “DEPENDENT depends on PREREQUISITE”; hand-authored expected JSON objects use `dependent_id`/`prerequisite_id`.  
**Stress fixture:** A real temporary Workspace with one dependent, multiple prerequisites/dependents, one same-pair legacy Related record, a branching tree, one missing final prerequisite, invalid CLI IDs, and depth 0/1/N. Expected: valid create stores every prerequisite; invalid create leaves byte-identical JSONL; all list/tree rows match the literal endpoint/depth table after process restart.  
**Regression fence:** CLI process tests `blocking_dependency_cli_direction_and_restart` and `create_with_prerequisites_is_atomic`; CLI parser/output unit tests for add/remove/list/tree and generic create-argument rejection.  
**Named mutation:** (C6) swap CLI arguments or print reversed wording; (C10) insert the Issue before validating the final prerequisite. Each mutation must turn the named process fence red with the offending endpoints or leaked Issue, then restoration must return green.  
**Complexity/production scale:** Create prerequisite validation is O(P), where P is requested prerequisites. Production expectation is fewer than 20; stress input is 1,000. Maximum accepted validation cost: 20 ms at 1,000 prerequisites, because that is fifty times expected use and validation is in-memory ID/duplicate lookup before one save. CLI tree delegates the O(V+E) work and budget to slice 1.
**Wall budget/phase:** N/A — reason: each CLI command is a one-off process phase; storage always-on budgets are owned by slice 1.  
**Files:** modify `crates/rivets/src/domain/mod.rs`, `crates/rivets/src/cli/{args.rs,execute.rs,mod.rs,types.rs}`, `crates/rivets/src/output/{mod.rs,json.rs,tree.rs}`, `crates/rivets-mcp/src/tools.rs` only for the `NewIssue` caller migration, and `crates/rivets/tests/{cli_tests.rs,in_memory_storage.rs,init_integration.rs}` plus affected inline domain/CLI/output tests.  
**Estimate:** 1 engineering day.  
**Diff estimate:** 950 changed lines including process fixtures.  
**PR increment:** B — Canonical adapter cutover.  
**Commands and expected results:**
- `cargo test -p rivets --test cli_tests blocking_dependency_cli_direction_and_restart` → exact human phrase and role-named JSON agree with raw JSONL before/after restart; list-by-dependent and list-by-prerequisite return the expected edge sets; tree rows match the literal depth table.
- `cargo test -p rivets create_with_prerequisites_is_atomic` → empty/single/multi creates are complete; duplicate/missing prerequisites leave no Issue and byte-identical storage; named mutation exposes the leaked Issue and turns red.
- `cargo test -p rivets cli` → `blocking-dependency` parser/help/output remain green and `--deps` is rejected with `--prerequisite` shown as the replacement.

## Slice 3: Add canonical MCP Blocking tools and context persistence

**Claim IDs:** C7  
**Expected behavior:** MCP add/remove/list/tree carry explicit endpoint roles, the list query cannot represent neither/both role states, and a fresh Tools context reloads the same directed edges/tree.  
**Oracle:** Raw JSONL endpoint tuples plus a literal tool-name/parameter-schema set and the same hand-authored tree table used independently from the MCP implementation.  
**Stress fixture:** A real JSONL Workspace with multiple endpoint roles, a branch, a same-pair legacy kind, explicit/default workspace contexts, and fresh context recreation. Expected: every response matches raw JSONL and the literal table; invalid tool shapes fail before storage; removal preserves the legacy edge.  
**Regression fence:** MCP integration test `blocking_dependency_mcp_direction_and_context_recreation` plus server schema tests for the tagged list query and all four tool names.  
**Named mutation:** Swap `dependent_id` and `prerequisite_id` in `tools.rs`; the integration fence must turn red with reversed structured fields, then return green after restoration.  
**Complexity/production scale:** N/A — reason: MCP adds no collection traversal; it delegates graph work to the slice-1 storage interface.  
**Wall budget/phase:** N/A — reason: each MCP tool invocation is a discrete one-off phase over storage whose always-on budget is owned by slice 1.  
**Files:** modify `crates/rivets-mcp/src/{models.rs,tools.rs,server.rs,error.rs,lib.rs}` and `crates/rivets-mcp/tests/integration.rs`.  
**Estimate:** 0.75 engineering day.  
**Diff estimate:** 750 changed lines including schemas and real-storage tests.  
**PR increment:** B — Canonical adapter cutover.  
**Commands and expected results:**
- `cargo test -p rivets-mcp --test integration blocking_dependency_mcp_direction_and_context_recreation` → add/list/tree/remove results agree field-for-field with raw JSONL and remain identical after context recreation; the named swap mutation turns the fence red.
- `cargo test -p rivets-mcp blocking_dependency` → tagged list schemas expose only `PrerequisitesOf { dependent_id }` and `DependentsOf { prerequisite_id }`, and all four canonical tools are registered.

## Slice 4: Retire generic mutation surfaces and synchronize documentation

**Claim IDs:** C8  
**Expected behavior:** No public CLI, MCP, or `IssueStorage` mutation/query route accepts a generic relationship kind; positive registries contain the canonical add/remove/list/tree surfaces; all user/agent/storage/parity documentation describes the same contract.  
**Oracle:** Literal accepted/rejected command/tool/method-name sets from the approved design, compared independently against Clap, MCP router schemas, and Rust compile failures at removed callsites.  
**Stress fixture:** N/A — reason: this is deletion and schema/documentation synchronization, not runtime collection logic; positive controls require every canonical command/tool to remain present while each generic route is absent.  
**Regression fence:** CLI/MCP registry tests `generic_dependency_mutation_surfaces_are_absent`, compile-time trait callsite migration, parity registry/render consistency check.  
**Named mutation:** Re-add `Commands::Dep` or server `dep`; the registry fence must turn red naming the legacy route, then return green after restoration.  
**Complexity/production scale:** N/A — reason: this slice removes interfaces and runtime branches; it adds no loop or production-scale work.  
**Wall budget/phase:** N/A — reason: no runtime phase is introduced.  
**Files:** modify remaining generic callers/implementations in `crates/rivets/src/{domain/mod.rs,storage/mod.rs,storage/in_memory/trait_impl.rs,cli/mod.rs,cli/args.rs,cli/execute.rs,output/mod.rs,output/json.rs,output/tree.rs}`; remove MCP `DepParams`/`dep`; update `README.md`, `docs/{README.md,architecture.md,storage-architecture.md,data-flow.md,cli-mcp-parity.md,cli-mcp-parity.json,agents/issue-tracker.md}`, `.agents/summary/{interfaces.md,data_models.md,components.md,workflows.md,architecture.md,review_notes.md}`, and parity-rendering inputs/scripts only where the canonical intent registry requires it. `CONTEXT.md` and ADR-0002 remain unchanged because they already state the target.  
**Estimate:** 1 engineering day.  
**Diff estimate:** 850 changed lines including migrated tests and synchronized documentation.  
**PR increment:** C — Generic-surface retirement.  
**Commands and expected results:**
- `cargo test -p rivets -p rivets-mcp generic_dependency_mutation_surfaces_are_absent` → every canonical positive control exists; `dep`, `--type`, and `--deps` are absent and rejected; each named reintroduction turns red.
- `python scripts/render-cli-mcp-parity.py --check` → rendered Markdown and JSON registry agree and Blocking intents no longer report legacy/future adapter gaps covered by this Task.
- `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test` → the complete workspace gate passes after the final slice.

## Tracker taxonomy

- Canonical relationship persistence remains intended work at verified Task `rivets-vio8`; this plan preserves the legacy persistence DTO until that Task.
- Parentage, Related/Discovery, Workflow/Ready, and durable Workspace locking remain intended work at verified Tasks `rivets-qcje`, `rivets-2x2i`, `rivets-brai`, and `rivets-j13o` respectively.
- No untracked deferred work is introduced.

## Self-review

- [x] C0–C10 are assigned exactly once; every PENDING falsifier is assigned to its implementing slice.
- [x] Every slice contains all thirteen mandatory fields and every conditional field has an explicit `N/A — reason` where applicable.
- [x] Every claim’s regression fence and named mutation are created/applied in the owning slice; no fence-less risk was approved.
- [x] Every new loop records asymptotic complexity, production/stress scale, a maximum accepted cost, and rationale; always-on storage phases have wall budgets.
- [x] Partition arithmetic includes the original 20% churn margin; the exact 5,284-line cumulative diff is split into three independently mergeable increments after the final size tripwire.
- [x] Every slice names an increment and each increment has an independent mergeable definition.
- [x] Tracker taxonomy is applied to every intended later Task.
- [x] No slice is declared complete; checkpointed-build owns completion.
