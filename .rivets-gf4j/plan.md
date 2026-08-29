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
| A — Slice 1 | 1,432 |
| B — Slices 2–3 | 1,337 |
| C — Slice 4 | 2,994 |
| **Actual total** | **5,763** |

### Review-fix projection

| Review-fix slice | Diff estimate |
|---|---:|
| 5. Validating Blocking deserialization (F2) | 35 lines |
| 6. MCP persistence and wire fences (F4, F5) | 55 lines |
| 7. Current-reference synchronization (F6, F7, F8) | 35 lines |
| **Projected review-fix sum** | **125 lines** |
| Review-fix churn margin | 25 lines (20%; assertion and wording refinement) |
| **Projected cumulative total** | **5,913 lines** (5,763 actual + 150 review-fix budget) |

The review fixes form a fourth independently green increment because the
original branch already crossed the review-size gate and its three increments
are committed.

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

### PR increment D — Review fixes

- Slices: 5–7.
- Mergeable definition: repairs one invariant bypass, adds the missing C5/C7
  persistence and wire fences, and synchronizes current-reference guidance
  without changing the approved Blocking Dependency contract.
- Independent verification: focused domain and MCP tests, real CLI help/error
  checks, parity rendering, and the final workspace gate pass against increment
  C.

## Slice 1: Add the typed Blocking value and deep storage interface

**Claim IDs:** C0, C1, C2, C3, C4, C5, C9  
**Expected behavior:** Storage accepts only role-safe Blocking values; self/duplicate/Blocking-only cycles fail; same-pair legacy kinds remain; typed prerequisite/dependent/tree queries preserve direction; Closed prerequisites remain recorded but are not active blockers.  
**Oracle:** Literal role-named JSON for C1; raw JSONL tuple counts for C2/C9; test-local adjacency DFS for C3; hand-authored BFS levels for C4; direct state-plus-record blocker scan for C5; LSP references plus Rust privacy for C0.  
**Stress fixture:** One in-memory store containing 10,000 Issues and 50,000 mixed edges, including a long Blocking chain, branches with multiple prerequisites/dependents, Related/Parentage/Discovery records on Blocking endpoint pairs, duplicate/self attempts, and Open/In Progress/Closed prerequisites. Expected: Blocking DFS/tree agree item-by-item with the independent adjacency/BFS tables; legacy tuples are unchanged; measured queries/mutations stay within the recorded budgets.
**Regression fence:** Domain unit test `blocking_dependency_preserves_direction_and_rejects_self`; storage integration tests `blocking_dependency_coexists_with_legacy_kind`, `blocking_cycles_ignore_other_relationship_kinds`, `blocking_tree_preserves_direction_and_depth`, `closed_prerequisite_stays_recorded_without_blocking`; resilient-loader test `legacy_relationships_survive_blocking_mutations`.  
**Named mutation:** (C0) import the existing private graph helper from CLI and require E0603; (C1) swap constructor field assignments; (C2) restore endpoint-only `find_edge` or endpoint-wide retain; (C3) remove the Blocks predicate from reachability; (C4) remove the tree Blocks filter or enqueue source; (C5) treat Closed as active; (C9) serialize only typed Blocking queries. Each mutation must turn its named fence red, then restoration must return green.  
**Complexity/production scale:** Duplicate/removal scans are O(outdegree); Blocking reachability/tree are O(V+E) over Blocking edges only. Current audited scale is 224 Issues/75 Blocking edges; stress scale is 10,000 Issues/50,000 edges. Maximum accepted cost: 50 ms per reachability/tree query and 10 ms per add/remove at stress scale on the development test profile, because ordinary local CLI/MCP operations must remain interaction-latency-negligible.  
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

## Slice 5: Validate deserialized Blocking Dependencies — F2

**Claim IDs:** C1  
**Expected behavior:** Deserializing a role-named Blocking Dependency preserves distinct dependent/prerequisite IDs and rejects a self-reference with a serde error produced from the domain invariant.  
**Oracle:** Deserialize the same literal endpoint pair through a test-only wire struct and compare its IDs to `BlockingDependency::new`; the self-pair oracle is the constructor's typed `SelfReference` rejection.  
**Stress fixture:** N/A — reason: pure two-field domain value with no collection or scale-dependent behavior.  
**Regression fence:** Domain unit tests for valid role-preserving JSON and rejected self-reference JSON.  
**Named mutation:** Restore derived `Deserialize` so serde bypasses `BlockingDependency::new`; the self-reference fence must turn red, then restoration must return green.  
**Complexity/production scale:** N/A — reason: deserialization adds no loop and parses the same two `IssueId` fields once.  
**Wall budget/phase:** N/A — reason: one-off two-field deserialization with no always-on phase.  
**Files:** modify `crates/rivets/src/domain/relationship.rs`.  
**Estimate:** 0.25 engineering day.  
**Diff estimate:** 35 changed lines including tests.  
**PR increment:** D — Review fixes.  
**Commands and expected results:**
- `cargo test -p rivets blocking_dependency_deserialization` → distinct endpoint JSON yields the literal roles; self-reference JSON is rejected; the derive mutation turns the self-reference fence red and restoration returns green.

## Slice 6: Fence MCP persistence and wire output — F4, F5

**Claim IDs:** C5, C7  
**Expected behavior:** Closing a prerequisite leaves the exact Blocking edge queryable after Tools context recreation, and add/list/tree/remove values serialize with only canonical role-named keys and the documented tree envelope.  
**Oracle:** Compare recreated-context queries to the raw JSONL dependent record and compare `serde_json::Value` results to hand-authored literal objects independent of the response structs.  
**Stress fixture:** A real temporary Workspace with two prerequisites, two dependents, one same-pair legacy Related record, a depth-one tree, one Closed prerequisite, and fresh Tools contexts. Expected: exact role-named objects, retained Closed edge, inactive blockedness, and preserved legacy tuple.  
**Regression fence:** MCP integration tests `blocking_dependency_mcp_direction_and_context_recreation` and `test_closing_blocker_unblocks_dependent`, extended with literal serialized values and recreated-context retention.  
**Named mutation:** Rename serialized `prerequisite_id` to legacy `depends_on_id` for C7, and remove incoming Blocking edges when closing the prerequisite for C5; each owning assertion must turn red, then restoration must return green.  
**Complexity/production scale:** N/A — reason: assertions exercise existing constant-size fixtures and introduce no production loop.  
**Wall budget/phase:** N/A — reason: test-only changes introduce no runtime phase.  
**Files:** modify `crates/rivets-mcp/tests/integration.rs`.  
**Estimate:** 0.25 engineering day.  
**Diff estimate:** 55 changed lines including literal wire fixtures.  
**PR increment:** D — Review fixes.  
**Commands and expected results:**
- `cargo test -p rivets-mcp --test integration blocking_dependency_mcp_direction_and_context_recreation` → add/list/tree/remove JSON matches the literal canonical objects before and after context recreation; the key-rename mutation turns red.
- `cargo test -p rivets-mcp --test integration test_closing_blocker_unblocks_dependent` → the dependent becomes Ready while the exact edge remains after context recreation; the close-edge-removal mutation turns red.

## Slice 7: Synchronize current-reference relationship guidance — F6, F7, F8

**Claim IDs:** C6, C7, C8  
**Expected behavior:** Current-reference docs expose only `blocking-dependency`/`--prerequisite`, advertise the actual 24-tool MCP surface, use the canonical Blocking operation in Wayfinder guidance, and identify Parentage as unavailable until verified Task `rivets-qcje`.  
**Oracle:** Compare copy-pastable CLI forms to Clap help/error behavior, MCP count to the router's enumerated tool-name set, and every Parentage deferral to verified Task `rivets-qcje`.  
**Stress fixture:** N/A — reason: documentation synchronization adds no runtime logic; positive controls retain every canonical command/tool while retired forms remain rejected.  
**Regression fence:** Existing CLI/MCP registry and parity tests for canonical presence and generic-surface absence; documentation is checked against their literal accepted/rejected sets.  
**Named mutation:** Re-add `Commands::Dep` or the MCP `dep` tool; the existing registry fence must turn red naming the legacy route, then restoration must return green.  
**Complexity/production scale:** N/A — reason: documentation-only slice.  
**Wall budget/phase:** N/A — reason: no runtime phase is introduced.  
**Files:** modify `README.md`, `docs/README.md`, `docs/agents/issue-tracker.md`, `docs/architecture.md`, `docs/data-flow.md`, and `docs/module-structure.md`.  
**Estimate:** 0.25 engineering day.  
**Diff estimate:** 35 changed lines.  
**PR increment:** D — Review fixes.  
**Commands and expected results:**
- `cargo run -p rivets -- --help` and `cargo run -p rivets -- create --help` → `blocking-dependency` and `--prerequisite` are present; `dep` and `--deps` are absent.
- `cargo test -p rivets-mcp parity_registry_classifies_every_cli_leaf_and_mcp_tool` → the router's 24 current tools exactly match the parity registry.
- `python scripts/render-cli-mcp-parity.py --check` → rendered interface references remain synchronized.

## Tracker taxonomy

- Canonical relationship persistence remains intended work at verified Task `rivets-vio8`; this plan preserves the legacy persistence DTO until that Task.
- Parentage, Related/Discovery, Workflow/Ready, and durable Workspace locking remain intended work at verified Tasks `rivets-qcje`, `rivets-2x2i`, `rivets-brai`, and `rivets-j13o` respectively.
- No untracked deferred work is introduced.

## Self-review

- [x] Original implementation coverage assigns C0–C10 exactly once; review-fix Slices 5–7 explicitly map F2/F4–F8 back to their covering claims and every PENDING falsifier retains its owning implementation slice.
- [x] Every slice contains all thirteen mandatory fields and every conditional field has an explicit `N/A — reason` where applicable.
- [x] Every claim’s regression fence and named mutation are created/applied in the owning slice; no fence-less risk was approved.
- [x] Every new loop records asymptotic complexity, production/stress scale, a maximum accepted cost, and rationale; always-on storage phases have wall budgets.
- [x] Partition arithmetic preserves the original actual 5,763-line total, adds a 20% review-fix churn margin, and places the projected 150-line review budget in independently green increment D.
- [x] Every slice names an increment and each increment has an independent mergeable definition.
- [x] Tracker taxonomy is applied to every intended later Task.
- [x] No slice is declared complete; checkpointed-build owns completion.
