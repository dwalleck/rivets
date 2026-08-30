# Plan: symmetric Related Associations and directed Discovery Origins

## Inputs and partition

- Route: Structural (`.rivets-2x2i/route.md`).
- Approved design: `.rivets-2x2i/design.md`, requester approval “I approve this design” on 2026-08-29.
- Upstream default branch: `origin/main`, discovered with `git symbolic-ref --short refs/remotes/origin/HEAD`.
- Estimated changed lines: Slice 1 = 320; Slice 2 = 1,200; Slice 3 = 420; Slice 4 = 780; Slice 5 = 1,100; sum = 3,820.
- Churn margin: 20% = 764 lines. This additive cross-crate change touches exhaustive trait implementations, generated parity output, and integration fixtures whose exact assertion churn is hard to predict.
- Projected total: 4,584 changed lines. Because this exceeds 4,000, the work is partitioned into two independently mergeable PR increments.

### PR increment: core-relationships

Slices 1–3. Mergeable definition: domain values, public storage interface, in-memory semantics, compatibility persistence, and restart tests compile and pass against `origin/main` without any CLI or MCP caller. Existing Blocking interfaces remain unchanged.

### PR increment: relationship-adapters

Slices 4–5. Mergeable definition: dedicated CLI and MCP add/remove/list operations consume the core interface, update parity/documentation, and pass real-process/context, locking, stale-cache, schema, and restart checks. It depends only on `core-relationships`.

## Slice 1: Add invariant-preserving domain relationship values

**Claim IDs:** C1

**Expected behavior:** Constructing Related in either order produces one equal value serialized as `{left_issue_id,right_issue_id}` in lexical order; Discovery preserves `{discovered_issue_id,source_issue_id}`; both constructors and deserializers reject self-reference with typed errors.

**Oracle:** Hand-authored literal JSON and direct `IssueId` ordering, independent of production serde conversion.

**Stress fixture:** N/A — pure two-endpoint value types have no collection or scale-dependent logic.

**Regression fence:** `crates/rivets/src/domain/relationship.rs::tests::relationship_values_preserve_semantics_and_reject_self`.

**Named mutation:** Remove endpoint sorting in `RelatedAssociation::new`; the fence must fail reverse-order equality and literal JSON assertions.

**Complexity/production scale:** Related construction performs one comparison and at most one two-value swap, O(1); Discovery construction is O(1). No collection loop.

**Wall budget/phase:** N/A — reason: constructors are constant-time and do not introduce a runtime phase.

**Files:** `crates/rivets/src/domain/relationship.rs`; `crates/rivets/src/domain/mod.rs`.

**Estimate:** 1 hour.

**Diff estimate:** 320 changed lines including domain tests.

**PR increment:** core-relationships.

**Commands and expected results:**
- `cargo test -p rivets relationship_values_preserve_semantics_and_reject_self` → both Related orders equal the literal left/right object; Discovery matches the literal directed object; both self cases and self-deserialization fail with their typed variants.
- Apply the named mutation, then run `cargo test -p rivets relationship_values_preserve_semantics_and_reject_self` → reverse-order Related equality fails; restore the mutation and the command returns green.

## Slice 2: Implement typed in-memory relationship semantics

**Claim IDs:** C0, C2, C3, C5

**Expected behavior:** Every new production mutation crosses `IssueStorage`; Related add/list/remove is symmetric and idempotent; Discovery is directed, multi-source, duplicate-rejecting, and Discovery-cycle-safe; both coexist with other kinds and leave Ready/Blocked unchanged.

**Oracle:** Test-local normalized unordered-pair sets, a test-local Discovery adjacency DFS, and literal Ready/Blocked ID sets computed only from Workflow State plus Blocking tuples.

**Stress fixture:** Build 300 Issues with 1,400 mixed Related, Discovery, and Blocking edges, including Related rings/triangles, 100 discovered Issues with four sources each, a reverse-order duplicate Related add, and a proposed Discovery cycle. Expected: canonical Related degree, all valid Discovery sources, rejected cycle with no mutation, and each measured typed operation under 2 seconds in optimized test execution. This is 1.3× the audited Issue count and 6.5× the audited relationship count while keeping the permanent suite practical.

**Regression fence:** `crates/rivets/tests/in_memory_storage.rs::{related_association_is_symmetric_idempotent_and_removable_from_either_side,discovery_origin_is_directed_multi_source_and_acyclic,nonblocking_relationships_do_not_change_ready_or_blocked_and_coexist,nonblocking_relationship_operations_stay_within_scale_budget}` plus Rust privacy and `cargo check --workspace` for C0.

**Named mutation:** C0: expose graph mutation to `cli/execute.rs`, which must fail Rust privacy/seam checks. C2: query outgoing Related edges only, which must fail endpoint-B listing. C3: remove the `DiscoveredFrom` filter from reachability, which must fail the non-Discovery control or literal cycle case. C5: broaden blockedness beyond `Blocks` or restore endpoint-only duplicate detection, which must change expected eligibility or tuple counts.

**Complexity/production scale:** Related add/remove inspects at most parallel edges for one pair, O(kinds); Related list scans incoming plus outgoing degree O(degree); Discovery cycle detection is O(V+E); sorted results add O(r log r). The audited Workspace has 224 Issues and 214 relationship edges, yielding fewer than 438 graph visits for cycle detection and at most 214 list rows. Maximum accepted stress cost is 2 seconds per measured operation at 300 Issues/1,400 edges; this deliberately exceeds audited graph size while allowing debug/CI variance and fencing accidental unbounded behavior.

**Wall budget/phase:** Always-on library operations. At the 300-Issue/1,400-edge stress scale each measured add/list/cycle-rejection operation must complete within 2 seconds; rationale: these run inside interactive CLI/MCP requests before JSONL save, and the fixture materially exceeds the audited relationship count.

**Files:** `crates/rivets/src/storage/mod.rs`; `crates/rivets/src/storage/in_memory/graph.rs`; `crates/rivets/src/storage/in_memory/trait_impl.rs`; `crates/rivets/src/error.rs`; `crates/rivets-mcp/src/error.rs` for the exhaustive public-error callsite; `crates/rivets/tests/in_memory_storage.rs`.

**Estimate:** 5 hours.

**Diff estimate:** 1,200 changed lines including trait implementations, typed errors, graph helpers, and behavioral/stress tests.

**PR increment:** core-relationships.

**Commands and expected results:**
- `cargo test -p rivets --test in_memory_storage related_association_is_symmetric_idempotent_and_removable_from_either_side` → reverse adds produce one canonical pair, both endpoint lists agree, and reverse remove clears only Related.
- `cargo test -p rivets --test in_memory_storage discovery_origin_is_directed_multi_source_and_acyclic` → two sources survive, duplicates/self/cycles fail without mutation, and the test-local DFS agrees item by item.
- `cargo test -p rivets --test in_memory_storage nonblocking_relationships_do_not_change_ready_or_blocked_and_coexist` → literal Ready/Blocked sets are byte-for-byte unchanged while all four raw kinds remain.
- `cargo test -p rivets --test in_memory_storage nonblocking_relationship_operations_stay_within_scale_budget` → a 300-Issue/1,400-edge fixture has canonical Related degree, rejects the literal Discovery cycle, and every measured operation is at most 2 seconds.
- Apply each C2/C3/C5 named mutation and rerun its named test → the expected endpoint, cycle-control, or eligibility assertion turns red; restore each mutation and all four commands return green.
- `cargo check --workspace` plus the recorded AST/LSP seam check → every implementation/caller compiles through `IssueStorage`; a deliberate CLI graph call turns red under C0 and restoration returns green.

## Slice 3: Preserve deterministic relationship behavior across JSONL restart

**Claim IDs:** C4, C6

**Expected behavior:** JSONL reconstruction evaluates cycles per kind, accepts Related cycles and mixed-kind paths, rejects Discovery-only cycles deterministically, writes new Related records under canonical left ownership, preserves Discovery direction and unrelated legacy records, and remains byte-stable after a second save.

**Oracle:** A test-local per-kind acceptance matrix and an independent raw JSONL parser over literal `(record_owner,target,kind)` tuples and file bytes.

**Stress fixture:** Two equivalent Workspaces add the same Related pairs in opposite argument order; a mixed fixture adds a Related triangle, Discovery chain plus proposed cycle, Blocking reverse path, legacy Parentage, one-way Related, and reciprocal legacy Related. Expected: equivalent new typed records/bytes, Related triangle accepted, only Discovery-only cycle rejected, legacy reciprocal input queryable symmetrically but not migrated, unrelated tuples retained, and second save byte-identical.

**Regression fence:** `crates/rivets/tests/in_memory_resilient_loading.rs::{relationship_cycles_are_scoped_by_kind,related_and_discovery_persist_deterministically_across_restart}`.

**Named mutation:** C4: restore global `has_cycle_impl` in JSONL loading, which must reject the mixed-kind positive control. C6: store Related under the caller's first endpoint, which must make opposite-order Workspace bytes and raw owners differ.

**Complexity/production scale:** Loader validation performs per-kind reachability O(E(V+E)) in the current three-pass compatibility loader; at 224 Issues/214 edges the upper bound is about 94,000 node/edge visits before JSON serialization. Result sorting is O(E log E). Maximum accepted load/save round trip is 1 second at the audited 224/214 shape; rationale: Workspace load/save is one discrete local-file event, and canonical migration optimization belongs to `rivets-vio8`.

**Wall budget/phase:** N/A — reason: JSONL reconstruction/save is a one-off phase per Workspace load or discrete mutation, not a background or per-loop always-on phase.

**Files:** `crates/rivets/src/storage/in_memory/jsonl.rs`; `crates/rivets/tests/in_memory_resilient_loading.rs`; `crates/rivets/tests/common/mixed_legacy.rs` or a focused inline fixture if the shared fixture would broaden unrelated migration assertions.

**Estimate:** 3 hours.

**Diff estimate:** 420 changed lines including loader logic and restart fixtures.

**PR increment:** core-relationships.

**Commands and expected results:**
- `cargo test -p rivets --test in_memory_resilient_loading relationship_cycles_are_scoped_by_kind` → every row matches the literal per-kind acceptance matrix; Related/mixed paths load and only Discovery-only cycles warn/skip.
- `cargo test -p rivets --test in_memory_resilient_loading related_and_discovery_persist_deterministically_across_restart` → opposite caller orders produce identical new typed tuples/bytes, unrelated legacy tuples survive, reciprocal legacy Related remains compatibility input, and second save is byte-identical within the 1-second fixture bound.
- Apply the C4 global-cycle mutation and rerun its fence → mixed-kind positive control turns red; restore and green. Apply the C6 caller-order mutation and rerun its fence → raw-owner/byte comparison turns red; restore and green.
- `cargo test -p rivets` → the core increment remains independently green with existing Blocking behavior unchanged.

## Slice 4: Expose dedicated real-CLI relationship commands

**Claim IDs:** C7

**Expected behavior:** `related add/remove/list` and `discovery add/remove/list` parse role-named arguments, delegate typed behavior, emit specified text/JSON, classify only add/remove as mutations, and persist across independent real-process invocations.

**Oracle:** Literal text/JSON expectations plus independent raw JSONL relationship tuples read after separate binary generations.

**Stress fixture:** A temporary Workspace with Related reverse add/remove, multiple Related neighbors, Discovery with two sources, self/cycle failures, all relationship kinds on one pair, invalid IDs, empty lists, text/JSON modes, and a fresh process for every command. Expected outputs and unchanged raw bytes on rejected mutations are recorded in the test.

**Regression fence:** `crates/rivets/tests/cli_tests.rs::related_and_discovery_cli_are_structured_symmetric_and_persistent` plus `crates/rivets/src/cli/mod.rs::tests::workspace_mutation_lock_classification_is_exhaustive` and CLI help assertions.

**Named mutation:** Swap Discovery CLI endpoint construction or omit `RelatedAction::Add` from mutation classification; the process test must report reversed JSON/raw fields or the classification test must report a read classification.

**Complexity/production scale:** CLI formatting sorts/serializes at most the selected Issue's relationship degree, O(r log r), with audited r ≤ 214 and resulting output below tens of kilobytes. Maximum accepted formatting overhead is 50 ms excluding existing process startup/storage load; rationale: output construction must remain negligible beside local I/O.

**Wall budget/phase:** N/A — reason: each CLI invocation is a one-off process event; no always-on phase is introduced.

**Files:** `crates/rivets/src/cli/args.rs`; `crates/rivets/src/cli/mod.rs`; `crates/rivets/src/cli/execute.rs`; `crates/rivets/tests/cli_tests.rs`; `README.md`; `docs/cli-mcp-parity.json`; generated `docs/cli-mcp-parity.md`.

**Estimate:** 4 hours.

**Diff estimate:** 780 changed lines including real-process tests and the CLI half of parity documentation.

**PR increment:** relationship-adapters.

**Commands and expected results:**
- `cargo test -p rivets --test cli_tests related_and_discovery_cli_are_structured_symmetric_and_persistent` → both Related endpoint lists return one canonical left/right value; Discovery lists two directed sources; rejected self/cycle mutations leave bytes unchanged; text/JSON and restart observations match literals.
- `cargo test -p rivets workspace_mutation_lock_classification_is_exhaustive` → Related/Discovery add/remove are mutations and list actions are reads.
- Apply the endpoint-swap and mutation-classification named mutations and rerun the corresponding commands → each named assertion turns red; restore and both return green.
- `cargo test -p rivets --test cli_tests test_cli_help_shows_all_commands` → top-level and nested help expose exact role names and no generic `dep`/`--type` surface.

## Slice 5: Expose matching MCP tools and complete parity documentation

**Claim IDs:** C8, C9

**Expected behavior:** Six MCP tools mirror CLI semantics and wire values, use the durable mutation path for add/remove, return typed lists, classify constructor/not-found inputs correctly, survive context recreation/stale-cache scenarios, reject held Workspace locks, and appear exactly in server schemas and parity documentation.

**Oracle:** Raw JSONL tuples, literal tool/parameter schema sets, CLI help operation sets, and parity JSON compared independently of returned storage objects.

**Stress fixture:** Real JSONL Workspace exercised through default and explicit roots, reverse Related add/remove/list, multi-source Discovery, self/cycle/duplicate/not-found errors, context recreation, an external JSONL mutation between calls, and a held Workspace lock for every add/remove tool. Expected: matching direct domain JSON, no lost external records, retryable busy errors on all mutations, and all six tools plus existing Blocking tools in the registry.

**Regression fence:** `crates/rivets-mcp/tests/integration.rs::related_and_discovery_mcp_match_cli_and_persist`; focused stale-cache and Workspace-lock relationship tests; `crates/rivets-mcp/src/server.rs` schema/registry tests; `python scripts/render-cli-mcp-parity.py --check`.

**Named mutation:** C8: use `storage_for` instead of `mutation_storage_for` in `related_add`, which must unexpectedly succeed under held lock and turn the lock fence red. C9: remove `discovery_list` from the router or leave its parity surface empty, which must fail registry/parity checks naming that operation.

**Complexity/production scale:** MCP model conversion/serialization is O(r) after storage returns sorted results; at audited r ≤ 214 the structured payload remains below tens of kilobytes. No new unbounded allocation beyond the returned relationship vector. Maximum accepted adapter overhead including a local JSONL mutation at 224 Issues/214 edges is 1 second; rationale: MCP is interactive and shares the existing local durable-save path.

**Wall budget/phase:** Always-on per MCP request. The real-storage integration fence records add/remove/list completion at audited production scale and requires at most 1 second per tool call; existing Workspace-lock contention fails immediately rather than consuming that budget.

**Files:** `crates/rivets-mcp/src/models.rs`; `crates/rivets-mcp/src/tools.rs`; `crates/rivets-mcp/src/server.rs`; `crates/rivets-mcp/src/error.rs`; `crates/rivets-mcp/src/lib.rs`; `crates/rivets-mcp/README.md`; `crates/rivets-mcp/tests/integration.rs`; `crates/rivets-mcp/tests/stale_cache.rs`; `crates/rivets-mcp/tests/workspace_lock.rs`; `docs/cli-mcp-parity.json`; generated `docs/cli-mcp-parity.md`; `README.md` if command/tool overview needs the final parity state.

**Estimate:** 6 hours.

**Diff estimate:** 1,100 changed lines including tool implementations, error/schema wiring, real-storage, stale-cache, locking, and parity tests.

**PR increment:** relationship-adapters.

**Commands and expected results:**
- `cargo test -p rivets-mcp --test integration related_and_discovery_mcp_match_cli_and_persist` → typed Related/Discovery JSON matches raw tuples, errors preserve categories, context recreation retains data, and every measured tool call is at most 1 second.
- `cargo test -p rivets-mcp --test stale_cache relationship` → each new mutation reloads external records and saves without overwrite.
- `cargo test -p rivets-mcp --test workspace_lock relationship` → each new add/remove tool returns retryable Workspace Busy while the durable lock is held.
- `cargo test -p rivets-mcp server::tests::test_tool_schemas_publish_expected_fields` and `cargo test -p rivets-mcp server::tests::generic_dependency_mcp_tool_is_absent` → all six tools have exact role fields, generic `dep` remains absent, and existing Blocking tools remain present.
- `python scripts/render-cli-mcp-parity.py --check` → source inventory and generated Markdown agree exactly for CLI/MCP surfaces.
- Apply the C8 lock-bypass mutation and rerun the lock test → the Related add case turns red; restore and green. Apply the C9 router/registry mutation and rerun schema/parity checks → `discovery_list` is named missing; restore and green.
- `cargo test -p rivets-mcp` → the adapter increment remains independently green over core-relationships.

## Tracker taxonomy

- Final canonical relationship migration remains assigned to verified Task `rivets-vio8`.
- Parentage remains assigned to verified Task `rivets-qcje`.
- Claim/release remains assigned to verified Task `rivets-8rj9`.
- No new intended future work is introduced by this plan. The permanent non-goals and their local-Git/fixed-vocabulary rationale remain those approved in `design.md`.

## Self-review

- [x] C0–C9 are assigned exactly once; every PENDING falsifier is discharged by its owning slice.
- [x] Every slice has all thirteen mandatory fields and every conditional field has an explicit `N/A — reason`.
- [x] Every claim's permanent fence and named mutation are created in the same slice.
- [x] Every new loop records asymptotic cost, audited/stress sizes, a maximum accepted cost, and every always-on phase has a wall budget.
- [x] The 3,820 + 764 = 4,584 review-size projection is partitioned into two independently mergeable increments.
- [x] Tracker taxonomy is applied with verified IDs.
- [x] No slice is declared complete; checkpointed-build exclusively judges completion.
