# Falsifiable design: single-Epic Parentage

## Route and inputs

- **Route:** Structural, from `.rivets-qcje/route.md`.
- **Behavior source:** `.rivets-qcje/route.md` T4; `spec.md` is N/A because `rivets-qcje`, parent Feature `rivets-5mlg`, `CONTEXT.md`, and ADR-0002 fully specify the observable behavior.
- **Behavior set:** set, clear, move, and show one child-to-Epic Parentage through equivalent CLI/MCP intents; allow nested Epics; reject self-parentage, Parentage-only cycles, a second parent, non-Epic parents, active children under Closed parents, child reopen beneath a Closed parent, and Epic close with active direct children; validate a move before replacing the old edge; never derive Blocked/Ready from Parentage; preserve valid Parentage through save/restart and structured output.
- **Empirical premises:** N/A — Structural route; all premises are current repository behavior.
- **Current evidence:** `crates/rivets/src/domain/relationship.rs` contains the typed Blocking pattern; `storage/in_memory/{trait_impl.rs,graph.rs}` owns graph mutation and readiness; `app.rs` and MCP mutation storage own durable Workspace locking; JSONL currently preserves compatibility `DependencyType::ParentChild` edges; real CLI/MCP restart suites are established adapter seams.

## Input shapes

| Input family | Production-reachable shapes | Status |
|---|---|---|
| Parentage value | distinct child/parent; equal IDs | Covered by C1 |
| Endpoint existence | both exist; missing child; missing parent | Covered by C2; missing endpoints reuse typed `IssueNotFound` |
| Issue Kind | every child Kind; candidate parent Bug/Feature/Task/Epic/Chore; Epic reclassified while owning zero/one/many children | Covered by C2 and C6 |
| Workflow State | child Open/In Progress/Closed crossed with parent Open/In Progress/Closed | Covered by C2 and C8 |
| Existing parent | none; same parent; different parent | Covered by C2, C4, and C5. Repeating set/move to the same parent is idempotent; setting a different parent requires `move` |
| Hierarchy shape | empty; one edge; nested Epic chain; direct cycle; deep cycle; parallel Blocking path in either direction | Covered by C3 |
| Direct children | empty; one; many; all Closed; mixed Closed/non-Closed | Covered by C6. Duplicate Parentage edges are unreachable because set is idempotent and graph lookup is kind-specific |
| Operation/result option | set, clear, move, show; show with parent and without parent; clear/move without a parent | Covered by C4 and C5 |
| Adapter shape | CLI text/JSON; MCP tool JSON and JSON-RPC error; omitted MCP Workspace root and explicit root | Covered by C10 and C11 |
| Persistence | valid Parentage alone; Parentage parallel to Blocking; restart after set/move/clear | Covered by C9 |
| Invalid legacy ParentChild data | multiple parents, non-Epic parent, legacy cycle | N/A — intended future migration/repair work is verified as `rivets-vio8`; this change must not silently choose or rewrite ambiguous legacy ownership |
| Numeric inputs | N/A | N/A — Parentage has no numeric interface |
| Free-form strings/paths | empty, Unicode, spaces, relative/absolute Workspace paths | N/A — endpoints use the existing `IssueId` and Workspace context seams; malformed or absent IDs reduce to the endpoint-existence cases above, and this change adds no path interpretation |

## Removed-invariant sweep

This change is partly subtractive: canonical Parentage permanently removes the legacy assumption that a blocked parent makes descendants unavailable.

- The removed propagation previously implied “blocked ancestor ⇒ blocked descendant ⇒ descendant not Ready.” Parentage now forbids that chain. C7 preserves the replacement invariant: only each child’s explicit unresolved Blocking Dependencies affect its Blocked/Ready conditions.
- The generic all-edge cycle rule previously made any reverse path look cyclic. C3 replaces it with a Parentage-only path check and proves that Blocking paths neither create nor excuse Parentage cycles.
- Single-parent cardinality is not removed. C2 and C4 make it explicit and preserve it across retries and failed moves.
- Workspace mutation serialization remains safe: CLI and MCP mutation guards already hold the durable Workspace lock through load, validation, mutation, and save. C4, C10, and C11 exercise Parentage through those existing transaction seams.

## Placement

### Typed Parentage value and errors

- **Owner:** `crates/rivets/src/domain/relationship.rs`, beside `BlockingDependency`. It owns endpoint roles, private fields, validating construction/deserialization, and the typed Parentage invariant errors adapters must surface.
- **New seam:** None — extend the existing typed Issue Relationship domain module and re-export from `domain/mod.rs`.
- **Forbidden:** public fields, raw `(IssueId, IssueId)` arguments, `from`/`to` vocabulary, string-matched errors, or adapter-local construction that bypasses `Parentage::new`.

### Workspace Parentage invariants and lifecycle coupling

- **Owner:** `IssueStorage` and its in-memory implementation. Cardinality, parent Kind, Parentage-only reachability, direct-child state, close/reopen, and atomic move all require the Workspace aggregate; `IssueStatus` and adapters cannot evaluate them.
- **New seam:** Extend `IssueStorage` with four intent-named methods: `set_parent`, `clear_parent`, `move_parent`, and `parent_of`. `set_parent` attaches only an unparented child, with same-parent retries idempotent; `move_parent` requires an existing parent, validates the complete candidate first, then replaces the one old edge under the same storage write guard; `clear_parent` returns the removed Parentage; `parent_of` distinguishes missing child from an existing unparented child.
- **Forbidden:** a second parent map that can drift from the graph; generic all-kind cycle detection; removing the old edge before candidate validation; adapter read-check-write sequences; reclassifying an Epic with children to a non-Epic Kind.

### Blocked/Ready behavior

- **Owner:** `storage/in_memory/graph.rs` and the storage query methods that already derive Blocked and Ready.
- **New seam:** None — Parentage remains absent from the Blocking predicate.
- **Forbidden:** ancestor traversal, synthetic Blocking edges, or adapter filtering based on Parentage.

### CLI adapter

- **Owner:** `cli/{args.rs,mod.rs,execute.rs}` behind one `parent` command with `set`, `clear`, `move`, and `show` actions using explicit `--child`/`--parent` roles.
- **New seam:** None — reuse Clap dispatch, `App::from_directory_for_mutation` for set/clear/move, read-only loading for show, `OutputMode`, and save/reload conventions.
- **Forbidden:** generic `dep` resurrection, positional role ambiguity, adapter validation of Workspace invariants, or a second persistence path.

### MCP adapter

- **Owner:** `rivets-mcp` models, `Tools`, server router, and typed error conversion, with `parent_set`, `parent_clear`, `parent_move`, and `parent_show` tools.
- **New seam:** None — reuse mutation storage guards, shared `IssueStorage`, direct domain serialization, and `invalid_params` classification for client-fixable Parentage errors.
- **Forbidden:** MCP-only semantics, mirrored domain DTOs, cache mutation outside the guarded storage path, or flattened internal error-string matching.

### Persistence

- **Owner:** the existing JSONL compatibility adapter and graph import/export path.
- **New seam:** None — valid Parentage persists as the existing deterministic `parent-child` compatibility edge until `rivets-vio8` performs the canonical `relationships` rewrite.
- **Forbidden:** introducing a second partial canonical schema, silently repairing invalid legacy Parentage, or deleting parallel Blocking edges during move/clear.

## Claims

- **C1.** A Parentage value always has distinct, role-named child and parent IDs and cannot be deserialized around that invariant.
- **C2.** Setting Parentage accepts every child Kind but only an existing Epic parent, preserves at-most-one parent, is idempotent for the same edge, rejects a different existing parent, and prevents an owning Epic from being reclassified to a non-Epic Kind.
- **C3.** Nested Epics are accepted, while self/direct/deep Parentage cycles are rejected using only Parentage edges regardless of Blocking paths.
- **C4.** Moving requires an existing parent, validates the complete candidate before mutation, replaces exactly one Parentage edge atomically, and leaves the old edge intact on every failure.
- **C5.** Clear removes exactly one Parentage edge without touching parallel relationship kinds, and show returns either that one edge or an explicit no-parent result while missing children remain `IssueNotFound`.
- **C6.** Closing an Epic with any non-Closed direct child fails with deterministic child IDs and changes neither parent nor children; no cascade occurs.
- **C7.** Parentage never changes Blocked or Ready; only explicit unresolved Blocking Dependencies do.
- **C8.** A non-Closed child cannot attach or move beneath a Closed Epic, and a child beneath a Closed Epic cannot reopen; attaching an already Closed child remains valid.
- **C9.** Valid Parentage set/move/clear behavior and parallel Blocking edges survive deterministic JSONL save/restart through the compatibility persistence seam.
- **C10.** The real CLI exposes role-safe parent set/clear/move/show text and JSON contracts, uses the mutation lock for writes, and surfaces core Parentage errors unchanged.
- **C11.** MCP exposes the same four intents and serialized Parentage/error semantics through guarded storage and recreated contexts.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Distinct private endpoints survive construction and serde | distinct/self | Construct and deserialize both shapes; acceptance of equal IDs falsifies C1. A serde-only guard could cause the same observation, so both constructor and JSON paths must agree. | Literal endpoint equality independent of production constructors | In `domain/relationship.rs`, remove the equality branch from `Parentage::new`; the self-reference constructor/serde tests must turn red | Domain unit test `parentage_constructor_and_deserialization_reject_self_reference` | <1 min | PENDING — checkpointed-build, domain slice |
| C2 | Epic-only, one-parent ownership and Kind mutation invariant | endpoint/kind/existing-parent matrix | Drive storage across all parent Kinds and none/same/different current parent; any accepted non-Epic/second parent or rejected same-edge retry falsifies C2. Endpoint absence could produce the same error, so every fixture first proves both endpoints exist. | Literal expected map `child -> optional parent` plus created Issue Kinds | In `storage/in_memory/trait_impl.rs`, delete the candidate-parent Epic check; `parentage_cardinality_and_epic_parent_are_enforced` must turn red | Storage integration test `parentage_cardinality_and_epic_parent_are_enforced` | <1 min | PENDING — checkpointed-build, storage slice |
| C3 | Parentage-only acyclicity with nested Epics | acyclic/direct/deep/parallel Blocking | Build a literal parent map and a separate Blocking reverse path; rejecting acyclic nesting or accepting a literal parent path back to the child falsifies C3. A generic cycle check could cause false rejection, so the parallel Blocking control must stay accepted. | Test-local parent-map walk, not petgraph | In `graph.rs`, make `has_parentage_cycle_impl` filter `Blocks` instead of `ParentChild`; `nested_epics_use_parentage_only_cycle_detection` must turn red | Storage integration test `nested_epics_use_parentage_only_cycle_detection` | <1 min | PENDING — checkpointed-build, storage slice |
| C4 | Failed moves preserve old parent; valid moves replace once | none/same/different/invalid candidate | Snapshot `parent_of`, attempt each move, reload, and compare; any gap, second edge, or changed old parent after failure falsifies C4. Save failure can also preserve disk state, so the test additionally queries the same in-memory storage before save. | Before/after literal parent pair observed both in memory and raw persisted record | In `trait_impl.rs`, remove the old ParentChild edge before candidate validation; `parent_move_validates_before_atomic_replacement` must turn red | Storage integration test `parent_move_validates_before_atomic_replacement` | <1 min | PENDING — checkpointed-build, storage slice |
| C5 | Clear/show are kind-specific and distinguish no parent | parent/no-parent/missing child/parallel edge | Clear a child carrying Parentage plus Blocking, then show and query Blocking; removing the Blocking edge, returning a parent, or hiding missing-child failure falsifies C5. | Literal expected Parentage option and separately queried Blocking pair | In `trait_impl.rs`, clear the first outgoing edge without filtering `ParentChild`; `parent_clear_and_show_preserve_parallel_relationships` must turn red | Storage integration test `parent_clear_and_show_preserve_parallel_relationships` | <1 min | PENDING — checkpointed-build, storage slice |
| C6 | Epic close reports active direct children without cascade | empty/single/many/mixed states | Close Epics with each child collection and compare exact sorted IDs and states; successful mixed close or any child state change falsifies C6. A generic transition error could look like rejection, so the assertion matches the Parentage error variant and IDs. | Test-local direct-child/status table | In `trait_impl.rs`, bypass the direct-child guard in `update` when target state is Closed; `epic_close_reports_active_direct_children_without_cascade` must turn red | Storage integration test `epic_close_reports_active_direct_children_without_cascade` | <1 min | PENDING — checkpointed-build, lifecycle slice |
| C7 | Parentage is absent from Blocked/Ready | blocked Epic with child; explicit child blocker positive control | Query Blocked and Ready with only parent blocker, then add an explicit child blocker; child unavailable before the explicit edge or available after it falsifies C7. The explicit-edge positive control proves the query can observe child blockedness. | Literal set of unresolved Blocking endpoint pairs | In `graph.rs`, add ParentChild ancestor propagation to `find_blocked_issues`; `legacy_parentage_never_propagates_blockedness` must turn red | Existing storage integration test `legacy_parentage_never_propagates_blockedness`, extended with explicit-child-blocker positive control in the storage slice | <1 min | PASS |
| C8 | Closed-parent attach/reopen lifecycle invariant | full child/parent state matrix | Attempt set/move/reopen across all state pairs; accepted active-under-Closed or rejected Closed-child attach falsifies C8. Missing endpoints could mask the guard, so all fixtures prove existence first. | Literal allowed-state truth table | In `trait_impl.rs`, remove the Closed-parent check from the Parentage validation helper; `closed_parent_attachment_and_reopen_truth_table` must turn red | Storage integration test `closed_parent_attachment_and_reopen_truth_table` | <1 min | PENDING — checkpointed-build, lifecycle slice |
| C9 | Restart preserves Parentage and parallel kinds | set/move/clear plus Blocking same pair | Save, inspect raw JSON, reload, and repeat queries after every mutation; missing/reordered Parentage or lost Blocking falsifies C9. An in-memory cache could mimic success, so every assertion uses a fresh storage instance. | Raw `serde_json::Value` dependency records plus fresh-loader query | In `storage/in_memory/trait_impl.rs` export, skip `DependencyType::ParentChild`; `parentage_jsonl_restart_round_trip` must turn red | Storage integration test `parentage_jsonl_restart_round_trip` | <1 min | PENDING — checkpointed-build, persistence slice |
| C10 | CLI role-safe commands and wire/errors match core | four actions, text/JSON, restart, invalid states | Run the real binary for each action and exact output/error, including failed move followed by show; wrong roles, envelopes, messages, or post-restart state falsify C10. Storage-only success cannot explain adapter output because the fence drives the process. | Literal stdout/stderr JSON/text plus raw Workspace reload | In `cli/execute.rs`, swap child and parent when constructing set Parentage; `parent_cli_contract_and_restart` must turn red | CLI process integration test `parent_cli_contract_and_restart` | <2 min | PENDING — checkpointed-build, CLI slice |
| C11 | MCP has equivalent guarded intents and wire/errors | four tools, root optional/explicit, recreation | Drive Tools/server add/show/move/clear, exact serialized values and invalid-params errors, recreate context, and retry under held Workspace lock; any semantic or locking divergence falsifies C11. Direct Tools-only behavior could miss router translation, so server schema/error assertions are included. | Literal JSON values, JSON-RPC code, and raw Workspace reload | In `rivets-mcp/src/tools.rs`, implement `parent_move` by calling `set_parent`; `parentage_mcp_contract_context_recreation_and_locking` must turn red | MCP integration test `parentage_mcp_contract_context_recreation_and_locking` plus router schema test | <2 min | PENDING — checkpointed-build, MCP slice |

## Non-goals and future work

- **Intended future work — `rivets-vio8`:** migrate generic legacy dependency records, warn and preserve invalid/ambiguous Parentage in Notes, and write the canonical structured `relationships` schema. This design intentionally persists valid Parentage through the existing compatibility record until that verified Task runs.
- **Permanent non-goal:** cascade closure or automatic child-state mutation. Parentage owns grouping; closing an Epic reports active direct children and stops.
- **Permanent non-goal:** Epic progress rollups or implicit blockers. These conflate ownership with lifecycle/readiness and contradict ADR-0002.
- **Permanent non-goal:** generic relationship mutation or custom relationship kinds. Every relationship keeps its own interface and invariants.

## Falsifier run log

- `2026-08-30 | cargo test -p rivets --test in_memory_storage legacy_parentage_never_propagates_blockedness -- --exact | PASS` — C7 survived: a blocked parent left its child Ready and absent from Blocked; adding an explicit child Blocking Dependency then removed the child from Ready and added it to Blocked.
- Rebase prerequisite check: `cargo check -p rivets -p rivets-mcp | PASS` after integrating the committed Workspace-lock and canonical-readiness stack with merged Blocking cutover fixes.

## Approval

- Requester approval: "Approve as written"
- Date: 2026-08-30
- Approved risk acceptances: None
