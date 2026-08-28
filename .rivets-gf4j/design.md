# Design: canonical Blocking Dependencies

## Route and inputs

- Route: **Structural**, from `.rivets-gf4j/route.md`.
- Behavior source: `.rivets-gf4j/route.md` T4; `spec.md` is `N/A — behavior was fully explicit`.
- Governing domain sources: `CONTEXT.md`, ADR-0002, parent `rivets-5mlg`, and Task `rivets-gf4j`.
- Empirical premises: `N/A — Structural route; current repository code and the accepted domain decisions cover every premise`.
- Behavior set:
  1. Given distinct existing Issues, adding a Blocking Dependency records one directed edge from dependent to prerequisite and every interface uses those role names.
  2. Given an existing Blocking Dependency, add/remove/list/tree, human output, JSON, MCP, and restart persistence preserve the same endpoint roles.
  3. Given a self-edge, duplicate Blocking Dependency, or cycle consisting only of Blocking Dependencies, addition fails without mutation.
  4. Given several prerequisites or several dependents, all directed edges are retained and queryable.
  5. Given another legacy relationship kind on the same endpoint pair, the Blocking Dependency can coexist with it and removing either kind does not remove the other.
  6. Given an Open or In Progress prerequisite, the dependent is blocked; once that prerequisite is Closed, the edge remains but ceases to be an active blocker.

## Input shapes

| Shape | Status |
|---|---|
| Existing distinct dependent and prerequisite | Covered by C1, C2 |
| Missing dependent; missing prerequisite; both missing | Covered by C2 |
| Dependent equals prerequisite | Covered by C1, C3 |
| No Blocking Dependencies; one; several distinct prerequisites | Covered by C2, C4 |
| No dependents; one; several distinct dependents | Covered by C2, C6, C7 |
| Repeated identical Blocking Dependency | Covered by C2 |
| Same endpoints carrying Blocking plus Related, Parentage, or Discovery Origin legacy records | Covered by C2, C9 |
| Two-node and multi-hop Blocking cycles | Covered by C3 |
| Non-blocking paths between the same nodes | Covered by C3, C9 |
| Dependent in Open, In Progress, or Closed state | Covered by C5 |
| Prerequisite in Open, In Progress, or Closed state | Covered by C5 |
| Remove existing and absent Blocking Dependencies | Covered by C2 |
| List by dependent and list by prerequisite | Covered by C6, C7 |
| CLI list with neither or both endpoint roles | Covered by C6 |
| Tree with empty, chain, and branching graphs | Covered by C4, C6, C7 |
| Tree depth zero (unlimited), one, and greater than one | Covered by C4, C6, C7 |
| CLI text and JSON modes | Covered by C6 |
| MCP default context and explicit `workspace_root`; fresh context after mutation | Covered by C7 |
| Create with zero, one, several, and duplicate prerequisites | Covered by C10 |
| Syntactically invalid, empty, spaced, Unicode, and absolute-path-like Issue IDs | Covered by C6 and C7: CLI rejects invalid syntax at its parser seam; MCP/storage return typed not-found outcomes without creating a relationship |
| Negative tree depth | N/A — Clap parses depth as unsigned, so this shape is unreachable before Blocking Dependency behavior |
| Generic relationship persistence records with all four legacy kinds | Covered by C9 |

## Removed-invariant sweep

This change is subtractive in three places.

1. **Endpoint-only uniqueness is removed.** It previously guaranteed at most one edge for an endpoint pair. Once different relationship kinds may coexist, duplicate detection, removal, queries, and serialization must select by relationship kind; C2 and C9 guard those facts.
2. **The one global cycle graph is removed from Blocking decisions.** It previously made every relationship kind participate in every cycle. Blocking cycle detection must still reject self, two-node, and multi-hop Blocking cycles while ignoring non-blocking paths; C3 guards both halves.
3. **The generic adapter mutation surface is removed.** It previously let callers choose semantics with a string/enum at runtime. CLI and MCP must expose only role-named Blocking operations in this slice; C6, C7, and C8 guard the clean cutover.

Still safe: Issue existence checks and JSONL atomic save remain owned by the existing storage seam and are exercised through C2, C6, and C7.

## Placement

### Domain relationship value

- **Owner:** `rivets::domain`, in a focused relationship module re-exported by `domain`; it owns role names and the self-reference invariant.
- **New seam:** `BlockingDependency::new(dependent_id, prerequisite_id) -> Result<BlockingDependency, BlockingDependencyError>`, private fields, role-named accessors, canonical serde fields `dependent_id` and `prerequisite_id`.
- **Competing shape A:** pass two `IssueId` parameters to every method. It avoids one value construction but repeats ordering knowledge at every caller and keeps swaps representable.
- **Competing shape B — chosen:** carry one `BlockingDependency` value across domain, storage, output, and MCP. One construction excludes self-reference and gives every adapter the same serializable direction.
- **Forbidden:** generic `from`/`to`, `depends_on_id`, or `DependencyType::Blocks` in a canonical Blocking interface; adapter-owned self/cycle rules.

### Storage and graph behavior

- **Owner:** the existing `IssueStorage` seam, with the in-memory adapter owning graph implementation and the JSONL adapter owning persistence.
- **New seam:** dedicated `add_blocking_dependency`, `remove_blocking_dependency`, `blocking_prerequisites`, `blocking_dependents`, and `blocking_dependency_tree` methods. The generic storage mutation/query methods leave the public trait; legacy relationship records remain loadable and serializable until `rivets-vio8`.
- **Competing shape A:** expose petgraph edges and let adapters filter. Rejected: it exports implementation and duplicates direction/cycle rules.
- **Competing shape B — chosen:** filter edge kind, detect Blocking-only cycles, preserve parallel legacy kinds, and build trees behind `IssueStorage`. This deepens the existing seam and keeps both adapters thin.
- **Forbidden:** CLI/MCP imports from `storage::in_memory`; endpoint-only `find_edge`; removing every edge to a prerequisite; using non-blocking edges in Blocking cycle/tree queries.

### Issue creation

- **Owner:** `NewIssue` and `IssueStorage::create`.
- **New seam:** replace generic `NewIssue.dependencies` with role-specific `prerequisites: Vec<IssueId>`; CLI replaces generic `--deps` syntax with repeatable `--prerequisite`.
- **Forbidden:** type-prefixed create arguments or caller-selected relationship kinds.

### CLI adapter and output

- **Owner:** `rivets::cli` for orchestration and `rivets::output` for presentation.
- **New seam:** `blocking-dependency add/remove/list/tree`. Add/remove use required `--dependent` and `--prerequisite`; list accepts exactly one of those roles; tree requires `--dependent`. JSON serializes role-named domain values/tree entries. Human text uses “depends on,” “prerequisite,” or “blocked by.”
- **Forbidden:** the generic `dep` command, `--type`, ambiguous arrows without prose, or reimplementation of graph rules.

### MCP adapter

- **Owner:** `rivets-mcp` models/tools/server, delegating to `IssueStorage`.
- **New seam:** `blocking_dependency_add`, `blocking_dependency_remove`, `blocking_dependency_list`, and `blocking_dependency_tree`; parameter schemas carry explicit dependent/prerequisite roles and responses serialize `BlockingDependency` or role-named tree entries.
- **Competing list shape A:** one flat object with two optional IDs. Rejected because neither/both states are representable.
- **Competing list shape B — chosen:** a tagged query enum with `PrerequisitesOf { dependent_id }` and `DependentsOf { prerequisite_id }`, so invalid combinations do not cross the tool seam.
- **Forbidden:** a generic type string, direct graph access, or MCP-only wording/validation.

### Persistence and documentation

- **Owner:** existing Issue record/JSONL adapter for the legacy `dependencies` field during this slice; canonical relationship record migration belongs to verified Task `rivets-vio8`.
- **New seam:** none. Typed operations map Blocking values to legacy `blocks` records without changing their dependent-to-prerequisite direction; other kinds round-trip untouched.
- **Forbidden:** emitting a partially canonical `relationships` schema before all relationship kinds can migrate losslessly, or dropping legacy non-blocking records.
- Documentation describing commands, tools, storage direction, outputs, and parity status changes atomically with implementation; `CONTEXT.md` and ADR-0002 already state the target contract.

## Claims

- **C0:** Every production adapter mutation reaches the existing `IssueStorage` seam, so storage can own Blocking invariants without an adapter bypass.
- **C1:** `BlockingDependency` makes dependent and prerequisite roles explicit and rejects self-reference before storage mutation.
- **C2:** Storage adds, removes, and queries only the requested Blocking edge while preserving other edge kinds on the same pair.
- **C3:** Blocking cycle detection considers only Blocking Dependencies and rejects self, two-node, and multi-hop Blocking cycles.
- **C4:** Blocking tree traversal follows only dependent-to-prerequisite Blocking edges, preserves role identifiers, and honors depth.
- **C5:** A Blocking edge remains recorded for every prerequisite state, but a Closed prerequisite does not actively block its dependent.
- **C6:** CLI add/remove/list/tree and create-prerequisite behavior use canonical roles, deterministic JSON, correct human wording, and survive process restart.
- **C7:** MCP add/remove/list/tree use canonical roles, typed query shapes, and preserve results across context recreation.
- **C8:** Generic CLI `dep`, MCP `dep`, generic create `--deps`, and adapter calls that select `DependencyType` are absent after cutover.
- **C9:** All legacy non-blocking relationship records round-trip unchanged and may coexist with a Blocking edge on the same endpoint pair.
- **C10:** Issue creation validates all explicit prerequisites before committing and creates either the Issue with every Blocking edge or no Issue.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C0 | Every adapter mutation crosses `IssueStorage`. | Placement invariant | AST-search production adapters for Blocking mutations; any direct graph mutation falsifies C0. Control: the search must find the current CLI and MCP storage calls. | LSP references to the exported storage mutation symbol, a symbol-aware mechanism independent of AST pattern matching. | Import and call the existing `pub(super)` graph helper directly from `cli/execute.rs`; `cargo check --workspace` must turn red with privacy error E0603. | Rust module privacy plus `cargo check --workspace`; adapter integration tests use only `IssueStorage`. | <1 minute | PASS |
| C1 | Domain roles are explicit and self-reference is impossible. | Distinct pair; self pair | Construct A→B and assert accessors/serde match a literal role-named object; construct A→A and require typed self error. Swapped output or successful self construction falsifies C1. | A literal `{dependent_id:A, prerequisite_id:B}` prepared without calling production conversion. | Swap field assignments in `BlockingDependency::new`; `blocking_dependency_preserves_direction_and_rejects_self` must report reversed IDs. | Domain test `blocking_dependency_preserves_direction_and_rejects_self`. | <1 minute | PENDING — checkpointed-build, domain slice |
| C2 | Storage mutates only the selected Blocking edge. | Missing endpoints; duplicate; same-pair other kind; remove absent | Seed a legacy Related A→B, add/remove Blocking A→B, and assert typed queries plus raw saved records retain/remove exactly the intended kinds. Any collateral removal, duplicate acceptance, or wrong typed error falsifies C2. | Parse the saved JSONL line and count `(endpoint, dep_type)` tuples independently of petgraph queries. | Restore endpoint-only `find_edge` or `retain(depends_on_id != prerequisite)` in `trait_impl.rs`; `blocking_dependency_coexists_with_legacy_kind` must fail. | Storage integration test `blocking_dependency_coexists_with_legacy_kind`. | 1 minute | PENDING — checkpointed-build, storage slice |
| C3 | Only Blocking paths participate in Blocking cycle checks. | Self; two-node; multi-hop; non-blocking cross-path | Compare add results for a matrix of Blocking and legacy paths against an independent adjacency-set DFS that filters only `blocks`; any mismatch identifies C3. | Test-local DFS over literal edge tuples, not petgraph or production helpers. | Remove the edge-weight predicate in Blocking reachability; `blocking_cycles_ignore_other_relationship_kinds` must fail on the Related control path. | Storage test `blocking_cycles_ignore_other_relationship_kinds`. | 1 minute | PENDING — checkpointed-build, storage slice |
| C4 | Tree traversal follows only Blocking prerequisites and depth. | Empty; chain; branch; depth 0/1/N | Build a mixed-kind fixture and compare returned `(dependent, prerequisite, depth)` rows to a literal BFS table; inclusion of a non-blocking edge, reversed pair, or wrong depth falsifies C4. | Hand-authored expected BFS levels for the fixture. | Remove the Blocks filter or enqueue the source rather than target in `graph.rs`; `blocking_tree_preserves_direction_and_depth` must fail with the first mismatched row. | Storage test `blocking_tree_preserves_direction_and_depth`. | 1 minute | PENDING — checkpointed-build, storage slice |
| C5 | Closed prerequisites remain recorded but stop blocking. | Dependent and prerequisite in each lifecycle state | Query blockers before and after closing the prerequisite; require unchanged raw edge and blocker-set removal. Deleting the edge or retaining blockedness falsifies C5. | Direct scan of exported Issue states plus raw `blocks` records computes active blockers independently. | Treat `Closed` as active in `find_blocked_issues`; `closed_prerequisite_stays_recorded_without_blocking` must fail on blocker membership. | Storage/CLI test `closed_prerequisite_stays_recorded_without_blocking`. | 1 minute | PENDING — checkpointed-build, storage/CLI slice |
| C6 | CLI behavior is canonical and persistent. | Add/remove/list both perspectives; tree depths; create prerequisites; text/JSON; bad IDs | Drive the real binary, assert exact role words/JSON fields, restart, and compare raw JSONL direction. Generic or reversed output, invalid list shape acceptance, or restart drift falsifies C6. | Raw JSONL parsed independently plus a literal expected phrase “DEPENDENT depends on PREREQUISITE.” | Swap CLI arguments or print “PREREQUISITE depends on DEPENDENT”; `blocking_dependency_cli_direction_and_restart` must fail with both endpoint values. | CLI process test `blocking_dependency_cli_direction_and_restart`. | 2 minutes | PENDING — checkpointed-build, CLI slice |
| C7 | MCP behavior is canonical and persistent. | Default/explicit context; add/remove/list modes; tree; fresh context | Invoke real Tools and server schemas, recreate context, and compare returned role fields to raw JSONL. Missing tools, optional-state ambiguity, reversal, or lost persistence falsifies C7. | Raw JSONL endpoint tuples plus exact expected tool-name/parameter schema set. | Swap `dependent_id`/`prerequisite_id` in `tools.rs`; `blocking_dependency_mcp_direction_and_context_recreation` must fail with reversed structured fields. | MCP integration test `blocking_dependency_mcp_direction_and_context_recreation`. | 2 minutes | PENDING — checkpointed-build, MCP slice |
| C8 | Generic adapter mutation surfaces are gone. | CLI/MCP help and schema registry | Enumerate Clap subcommands and MCP tools; require canonical Blocking names and reject `dep`, `--type`, and `--deps`. Presence of any legacy mutation route falsifies C8. Positive control requires add/remove/list/tree each appear. | Literal accepted/rejected command and tool-name sets derived from the approved design. | Re-add `Commands::Dep` or server `dep`; `generic_dependency_mutation_surfaces_are_absent` must fail naming that route. | CLI/MCP registry tests `generic_dependency_mutation_surfaces_are_absent`. | <1 minute | PENDING — checkpointed-build, adapter cutover slice |
| C9 | Legacy other kinds survive and coexist. | All four kinds; parallel same-pair edges | Load one mixed JSONL fixture, perform a typed Blocking mutation, save/reload, and compare every legacy non-blocking tuple and order. Missing, merged, or reordered records falsify C9. | Original fixture tuple multiset/order compared to raw canonicalized save; it does not query the graph. | Remove all edges by endpoint in typed removal or serialize only typed queries; `legacy_relationships_survive_blocking_mutations` must fail with the missing kind. | Resilient-loader test `legacy_relationships_survive_blocking_mutations`. | 1 minute | PENDING — checkpointed-build, persistence slice |
| C10 | Create is all-or-nothing for explicit prerequisites. | Empty/single/multi/duplicate/missing prerequisites | Run create with each shape; require every valid edge or no new Issue/file change on any invalid prerequisite. Partial Issue or subset edges falsifies C10. | Before/after raw JSONL record and edge counts from a fresh Workspace. | Insert the Issue before validating the final prerequisite in `trait_impl.rs`; `create_with_prerequisites_is_atomic` must find the leaked Issue. | Storage and CLI test `create_with_prerequisites_is_atomic`. | 1 minute | PENDING — checkpointed-build, domain/storage slice |

## Non-goals and tracked work

- Parentage semantics, lifecycle invariants, and dedicated adapters are intended work tracked at verified Task `rivets-qcje`.
- Symmetric Related Associations and directed Discovery Origins are intended work tracked at verified Task `rivets-2x2i`.
- Canonical Workflow State, Ready, and removal of Parentage-derived blockedness are intended work tracked at verified Task `rivets-brai`.
- Canonical `relationships` persistence, migration warnings/Notes, and removal of the legacy `dependencies` field are intended work tracked at verified Task `rivets-vio8`.
- Workspace-wide durable mutation locking is intended work tracked at verified Task `rivets-j13o`; this slice continues to use the current save contract and does not claim cross-process serialization.
- Authorization, remote synchronization, custom relationship kinds, and distributed graph storage are permanent non-goals because Rivets remains a local Git-backed tracker with the fixed relationship vocabulary accepted by ADR-0002.

## Falsifier run log

- 2026-08-28 — C0 cheapest falsifier: AST pattern `$OBJ.add_dependency($$$ARGS)` over `crates/rivets/src;crates/rivets-mcp/src` found production calls only in CLI, MCP, and the JSONL wrapper, all through `IssueStorage`. Independent LSP references to `IssueStorage::add_dependency` found the same adapter callers and implementations. **PASS**.
- 2026-08-28 — Baseline observation for C2/C9: a real temporary CLI Workspace accepted A→B `blocks`, then rejected A→B `related` as `Dependency already exists`. This confirms the endpoint-only constraint the design removes; C2/C9 remain PENDING until implementation.

## Approval

Approved by the requester.

- Date: 2026-08-28
- Verbatim approval: “I approve this design”
- Approved risk acceptances: None
