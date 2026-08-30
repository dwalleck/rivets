# Design: symmetric Related Associations and directed Discovery Origins

## Route and inputs

- Route: **Structural**, from `.rivets-2x2i/route.md`.
- Behavior source: `.rivets-2x2i/route.md` T4; `spec.md` is `N/A — behavior was fully explicit`.
- Governing domain sources: `CONTEXT.md`, ADR-0002, parent Feature `rivets-5mlg`, and Task `rivets-2x2i`.
- Empirical premises: `N/A — Structural route; current repository code and accepted domain decisions cover every premise`.
- Behavior set:
  1. Given two distinct Issues, adding Related in either endpoint order records one symmetric Association with canonical endpoint order and makes it visible from both Issues.
  2. Given an existing Related Association, either endpoint order can re-add it idempotently or remove it; Related self-association is rejected and Related paths never participate in cycle checks.
  3. Given a discovered Issue and a distinct source Issue, adding Discovery Origin records directed discovered-to-source provenance, permits several sources, and rejects self-reference or a Discovery-only provenance cycle.
  4. Given Related or Discovery records, adding another relationship kind to the same Issue pair preserves every independent meaning.
  5. Given any Related or Discovery mutation, Blocked and Ready results remain determined only by Workflow State, Assignment visibility, and unresolved Blocking Dependencies.
  6. Given dedicated CLI or MCP add, remove, and list operations, both adapters expose the same endpoint roles and structured relationship values.
  7. Given a successful mutation followed by a process or context restart, the relationship remains observable and deterministic in structured output and JSONL persistence.

## Input shapes

| Shape | Status |
|---|---|
| Related endpoints are distinct existing Issues, supplied in canonical and reverse order | Covered by C1, C2 |
| Related first endpoint missing, second missing, or both missing | Covered by C2 |
| Related endpoints are the same Issue | Covered by C1 |
| No Related Associations, one Association, or several Associations touching one Issue | Covered by C2 |
| Repeated Related add in the same or reverse order | Covered by C2 |
| Remove an existing Related Association in either order or remove an absent Association | Covered by C2, C7, C8 |
| Related chain, triangle, and larger cyclic graph | Covered by C4 |
| Discovery endpoints are distinct existing Issues with discovered/source roles preserved | Covered by C1, C3 |
| Discovery discovered endpoint missing, source missing, or both missing | Covered by C3 |
| Discovery discovered endpoint equals source endpoint | Covered by C1 |
| No Discovery Origins, one origin, or several distinct sources for one discovered Issue | Covered by C3 |
| One source with zero, one, or several discovered Issues | Covered by C3 |
| Repeated identical Discovery Origin | Covered by C3 — rejected as the existing directed-edge duplicate contract, unlike Related's explicit idempotent contract |
| Remove an existing, absent, or role-reversed Discovery Origin | Covered by C3, C7, C8 |
| Two-node and multi-hop Discovery-only provenance cycles | Covered by C3, C4 |
| Blocking, Parentage, or Related paths between Discovery endpoints | Covered by C4, C5 |
| Same endpoint pair carrying Related, Discovery, Blocking, and legacy Parentage records | Covered by C5, C6 |
| Endpoint Issues in Open, In Progress, or Closed Workflow State | Covered by C5 |
| Ready and Blocked queries before and after non-blocking mutations | Covered by C5 |
| Legacy one-way Related and directed Discovery records | Covered by C2, C6 |
| Persisted self-referential Related record | Covered by C6 — warn and skip before graph insertion |
| Legacy reciprocal Related records | Covered by C2 for symmetric query/removal; canonical migration collapse is assigned to `rivets-vio8` |
| Deterministic save after add order A/B versus B/A and after reload | Covered by C6, C7, C8 |
| CLI text and JSON modes | Covered by C7 |
| CLI Related list by Issue and Discovery list by discovered Issue | Covered by C7 |
| MCP default context and explicit `workspace_root`; fresh context after mutation | Covered by C8 |
| Syntactically invalid, empty, spaced, Unicode, and absolute-path-like Issue ID inputs | Covered by C7 and C8: CLI rejects invalid syntax at its parser seam; MCP/storage return typed not-found outcomes without creating relationships |
| Empty relationship collections serialized in structured output | Covered by C7, C8 |

## Removed-invariant sweep

This change is subtractive in two places.

1. **Directed ownership is removed from Related.** A legacy Related record silently made the record-owning Issue the source and made reverse add/remove/list behavior different. Once Related is symmetric, canonical endpoint construction, duplicate detection, queries, removal, and serialization must treat A/B and B/A as one value. C1, C2, and C6 guard every link.
2. **The one global cycle graph is removed.** It previously made all relationship kinds participate in one reachability rule. Related must permit cycles, Discovery must reject only Discovery-only cycles, and Blocking behavior must remain Blocking-only. C3, C4, and C5 guard those properties.

Still safe: Issue existence checks, Workspace mutation locking, stale-source revision checks, and atomic JSONL save remain owned by the existing storage seam and adapters. C2, C3, C6, C7, and C8 drive those existing paths.

## Placement

### Domain relationship values

- **Owner:** `rivets::domain::relationship`; it already owns `BlockingDependency`, role vocabulary, private endpoints, self-reference rejection, and invariant-preserving deserialization.
- **New seam:** `RelatedAssociation::new(issue_id, related_issue_id)` sorts its private endpoints into canonical `left_issue_id < right_issue_id`, rejects equality, and serializes `{left_issue_id, right_issue_id}`. `DiscoveryOrigin::new(discovered_issue_id, source_issue_id)` preserves its private semantic roles, rejects equality, and serializes those role names.
- **Competing shape A:** pass raw `IssueId` pairs to every storage and adapter call. It avoids constructing values but repeats ordering and role knowledge at each caller, so swapped Discovery roles and duplicate Related representations remain possible.
- **Competing shape B — chosen:** carry typed values across domain, storage, CLI output, and MCP output. One construction centralizes canonical order or directed roles and gives every adapter the same structured representation.
- **Forbidden:** public struct fields, generic `from`/`to`, adapter-owned self checks, or a Related representation whose serialized bytes depend on caller order.

### Storage and graph behavior

- **Owner:** the existing `IssueStorage` seam; the in-memory adapter owns graph implementation and the JSONL-backed adapter owns mutation preparation and persistence forwarding.
- **New seam:** dedicated `add_related_association`, `remove_related_association`, `related_associations`, `add_discovery_origin`, `remove_discovery_origin`, and `discovery_origins` methods. Related add is idempotent; Related queries and removal inspect both graph directions and deduplicate canonical values. Discovery add rejects duplicates and Discovery-only cycles while allowing several sources.
- **Competing shape A:** expose generic `(from, to, DependencyType)` calls. Rejected because it makes symmetry, direction, and cycle scope runtime choices in every adapter.
- **Competing shape B — chosen:** expose intent-named typed methods while retaining `DependencyType` only inside the compatibility representation. This deepens the current storage seam and keeps adapters from learning petgraph or legacy DTO details.
- **Forbidden:** CLI/MCP imports from `storage::in_memory`, endpoint-only edge lookup/removal, Related cycle detection, or cycle traversal that follows a different relationship kind.

### Persistence compatibility

- **Owner:** `storage::in_memory` and its JSONL/Issue-record adapters.
- **New seam:** none. Until `rivets-vio8`, typed values map to the existing compatibility `dependencies` field. A newly added Related record is owned by canonical left endpoint and points to the right endpoint; Discovery remains discovered-to-source. Existing issue and dependency sorting makes saves deterministic. Loader cycle checks become per-kind so a persisted Related cycle and mixed-kind same-pair records survive restart.
- **Forbidden:** introducing the final `relationships` schema early, dropping an unrelated relationship kind, rewriting all legacy reciprocal Related records as migration, or letting add order choose the persisted owner.

### CLI adapter and output

- **Owner:** `rivets::cli` for parsing/orchestration and `rivets::output` or typed domain serialization for presentation, following the existing Blocking Dependency family.
- **New seam:** `related add/remove/list` with `--issue` and `--related` on mutations and `--issue` on list; `discovery add/remove` with `--discovered` and `--source`, plus `discovery list --discovered`. Mutation success JSON names `relationship`, `action`, canonical/role-named endpoints, and `status`; list JSON is a sorted array of typed relationship values. Human text says “A is related to B” and “DISCOVERED was discovered from SOURCE.”
- **Forbidden:** `--type`, generic `dep`, ambiguous arrows, direct compatibility-record mutation, or graph validation in CLI execution.

### MCP adapter

- **Owner:** `rivets-mcp` models/tools/server, delegating through `IssueStorage` and the existing Workspace mutation-lock path.
- **New seam:** `related_add`, `related_remove`, `related_list`, `discovery_add`, `discovery_remove`, and `discovery_list`. Related pair params carry `issue_id`/`related_issue_id`; Discovery pair params carry `discovered_issue_id`/`source_issue_id`; Discovery list carries `discovered_issue_id` and returns its source relationships.
- **Forbidden:** generic relationship-kind strings, optional endpoint-role combinations, direct graph access, skipped mutation locking, or MCP-only relationship semantics.

### Parity documentation

- **Owner:** `docs/cli-mcp-parity.json` as the parity inventory and its generated Markdown renderer output; canonical user documentation records the delivered dedicated commands/tools.
- **New seam:** none. Existing inventory rows move from empty future surfaces to exact CLI/MCP names and argument schemas.
- **Forbidden:** hand-editing generated parity Markdown without its JSON source or documenting the future canonical persistence schema as already shipped.

## Claims

- **C0:** Every production Related or Discovery mutation crosses `IssueStorage`, so storage owns symmetry, direction, cycle, persistence, locking, and stale-source invariants without an adapter bypass.
- **C1:** `RelatedAssociation` canonicalizes unordered endpoints and `DiscoveryOrigin` preserves directed roles, and both reject self-reference during construction and deserialization.
- **C2:** Related add is symmetric and idempotent, list is visible from either endpoint, and remove from either order removes only that Association.
- **C3:** Discovery Origin preserves discovered-to-source direction, permits multiple sources, rejects duplicates, and rejects self-reference plus Discovery-only cycles without partial mutation.
- **C4:** Cycle evaluation is per relationship kind: Related cycles are allowed, Discovery follows only Discovery edges, and Blocking behavior remains isolated from non-blocking paths.
- **C5:** Related and Discovery never alter Blocked or Ready and coexist independently with every other relationship kind on the same pair.
- **C6:** Typed Related writes use canonical endpoint ownership, Discovery writes preserve semantic direction, and both survive deterministic JSONL save/reload without losing unrelated legacy records.
- **C7:** Real CLI add/remove/list operations expose canonical endpoint roles in text and JSON, classify only mutations as Workspace writes, and preserve results across process restart.
- **C8:** MCP exposes matching typed add/remove/list tools, preserves directed Discovery roles, uses the existing mutation lock, and preserves results across context recreation.
- **C9:** The CLI/MCP parity inventory, generated documentation, command help, and MCP registry enumerate exactly the six dedicated Related/Discovery operations and their structured arguments.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C0 | Every production mutation crosses `IssueStorage`. | Placement invariant | AST-search all production `add_edge`/`remove_edge` calls; any call outside `storage::in_memory` falsifies C0. Positive control requires current graph mutations inside that module. | LSP references to `InMemoryStorageInner::graph`, a symbol-aware mechanism independent of AST matching, must identify the same owning module and no adapter callsite. | Without changing storage visibility, attempt to import `storage::in_memory::inner::InMemoryStorageInner` and mutate its graph from `cli/execute.rs`; `cargo check -p rivets` must fail with E0603 before the bypass can compile. The AST seam check must separately name any direct adapter call if visibility is ever widened. | Private `storage::in_memory::inner` module and `graph` field, the `cargo check -p rivets` compile-fail mutation, and the independent AST/LSP seam check. | <1 minute | PASS |
| C1 | Typed values encode canonical symmetry or directed roles and reject self-reference. | Related A/B and B/A; Discovery A→B; both self pairs; deserialization | Construct both Related orders and require equal values/literal `{left_issue_id:A,right_issue_id:B}`; construct Discovery and require literal role fields; deserialize self pairs and require typed errors. Unequal reverse Related values, swapped Discovery roles, or accepted self input falsifies C1. | Hand-authored literal JSON values and direct `IssueId` ordering, independent of production serde conversion. | Remove endpoint sorting in `RelatedAssociation::new`; `relationship_values_preserve_semantics_and_reject_self` must fail equality and literal JSON assertions. | Domain test `relationship_values_preserve_semantics_and_reject_self`. | <1 minute | PENDING — checkpointed-build, domain slice |
| C2 | Related is symmetric, idempotent, visible from both endpoints, and removable either way. | Missing endpoints; repeated both orders; list empty/single/multi; remove both orders/absent | Drive `IssueStorage` through both endpoint orders, compare both lists, then remove using reverse order; any duplicate, asymmetric list, collateral edge removal, or unexpected success for missing endpoints falsifies C2. | Test-local `BTreeSet<(min_id,max_id)>` built from requested pairs, not graph queries; raw exported compatibility records confirm exactly one selected pair. | Change related lookup to outgoing edges only; `related_association_is_symmetric_idempotent_and_removable_from_either_side` must fail the endpoint-B list assertion. | Storage integration test `related_association_is_symmetric_idempotent_and_removable_from_either_side`. | 1 minute | PENDING — checkpointed-build, storage slice |
| C3 | Discovery is directed, multi-source, duplicate-rejecting, and acyclic within its own kind. | Missing/self/duplicate; multiple sources; one/multi-hop cycle; role-reversed remove | Add two sources to one discovered Issue and a chain, list the discovered Issue's origins, then attempt self, duplicate, reverse removal, and cycle mutations; any swapped role, lost source, accepted invalid edge, or partial mutation falsifies C3. | Test-local adjacency-set DFS over literal `(discovered,source)` tuples, independent of petgraph and production cycle helpers. | Remove the `DiscoveredFrom` edge-weight filter from reachability; `discovery_origin_is_directed_multi_source_and_acyclic` must reject its non-Discovery control path or accept the literal cycle. | Storage integration test `discovery_origin_is_directed_multi_source_and_acyclic`. | 1 minute | PENDING — checkpointed-build, storage slice |
| C4 | Cycle rules are isolated by kind. | Related triangle; Discovery cycle; Blocking/Parentage/Related cross-paths | Add a matrix of mixed-kind paths and compare acceptance to a literal per-kind reachability table; rejecting a Related triangle, rejecting a Discovery edge only because another kind closes a path, or accepting a Discovery-only cycle falsifies C4. | Hand-authored expected acceptance matrix computed separately for each enum kind. | Restore `has_cycle_impl` over every graph edge in the JSONL loader or Discovery add; `relationship_cycles_are_scoped_by_kind` must fail on the mixed-kind positive control. | Storage/loader test `relationship_cycles_are_scoped_by_kind`. | 1 minute | PENDING — checkpointed-build, storage slice |
| C5 | Related and Discovery do not affect readiness and coexist with all kinds. | All Workflow States; before/after Ready/Blocked; same-pair four-kind records | Record Ready/Blocked IDs, add Related and Discovery on pairs already carrying Blocking and legacy Parentage, and require unchanged eligibility plus all typed/raw tuples. Changed readiness or a missing kind falsifies C5. | Literal expected Ready/Blocked ID sets from Issue states and unresolved Blocking tuples, computed without production ready helpers. | Broaden `find_blocked_issues` from `Blocks` to every edge or use endpoint-only duplicate detection; `nonblocking_relationships_do_not_change_ready_or_blocked_and_coexist` must fail eligibility or tuple count. | Storage integration test `nonblocking_relationships_do_not_change_ready_or_blocked_and_coexist`. | 1 minute | PENDING — checkpointed-build, storage slice |
| C6 | Persistence is canonical for new typed writes, rejects invalid compatibility records, and preserves unrelated compatibility records. | Related add order; Discovery direction; restart; mixed kinds; legacy one-way/reciprocal Related; persisted Related self-reference | Perform typed mutations in opposite caller orders in equivalent Workspaces, save/reload twice, and compare raw relationship tuples and bytes; load a literal self-referential Related record and require one warning plus no inserted Association. Differing Related owner/order, reversed Discovery, accepted self-reference, dropped other kinds, or restart drift falsifies C6. Reciprocal legacy input is queried symmetrically but remains a migration input until `rivets-vio8`. | Independent raw JSONL parser normalizes literal `(record_owner,target,kind)` tuples, includes a self-reference control, and compares files without querying petgraph. | Store Related under the caller's first endpoint rather than canonical left endpoint, or remove the loader's Related self-reference guard; `related_and_discovery_persist_deterministically_across_restart` or `related_self_reference_warns_and_is_skipped_on_load` must fail on raw ownership or the warning/empty-query oracle. | Resilient-loader tests `related_and_discovery_persist_deterministically_across_restart` and `related_self_reference_warns_and_is_skipped_on_load`. | 2 minutes | PENDING — checkpointed-build, persistence/review-fix slices |
| C7 | CLI behavior is canonical, structured, correctly classified, and persistent. | Related add/remove/list perspectives; Discovery add/remove/list by discovered Issue; text/JSON; bad IDs; restart | Drive the real binary in text and JSON, list Related from both endpoints and Discovery sources from the discovered Issue, restart, remove, and inspect raw JSONL. Missing commands, wrong role fields/phrases, read commands taking a mutation path, or persistence drift falsifies C7. | Raw JSONL tuples plus literal expected JSON objects and phrases “A is related to B” / “DISCOVERED was discovered from SOURCE.” | Swap Discovery CLI arguments or omit `RelatedAction::Add` from `mutates_workspace`; `related_and_discovery_cli_are_structured_symmetric_and_persistent` or mutation-classification test must fail naming the wrong endpoint/class. | CLI process test `related_and_discovery_cli_are_structured_symmetric_and_persistent` plus exhaustive mutation-classification test. | 2 minutes | PENDING — checkpointed-build, CLI slice |
| C8 | MCP behavior matches typed storage and survives context recreation under the Workspace lock. | Default/explicit root; pair/list parameters; self/cycle errors; fresh context; lock contention | Invoke all six real tools, compare structured values to raw JSONL, recreate context, and hold the Workspace lock during add/remove; missing lock rejection, role reversal, wrong error category, or lost persistence falsifies C8. | Raw JSONL tuples and exact literal tool/parameter schema set, independent of returned storage objects. | Use `storage_for` instead of `mutation_storage_for` in `related_add`; `related_and_discovery_mcp_match_cli_and_persist` or Workspace-lock test must succeed unexpectedly under contention and turn red. | MCP integration test `related_and_discovery_mcp_match_cli_and_persist`, schema test, stale-cache test, and Workspace-lock tests. | 2 minutes | PENDING — checkpointed-build, MCP slice |
| C9 | Exactly six dedicated adapter operations and their schemas are documented and registered. | CLI help; MCP registry; parity source/rendered output | Enumerate Clap commands/actions, MCP tools, and parity inventory rows; require related/discovery add/remove/list with exact argument roles and no generic relationship-kind selector. Missing, extra, or mismatched names falsify C9. Positive control requires all existing Blocking operations too. | Literal accepted operation/argument set derived from this approved design, compared independently to CLI help, server registry, and parity JSON. | Remove `discovery_list` from the MCP router or leave its parity surface empty; registry/parity checks must fail naming that operation. | CLI help test, MCP schema registry test, and `python scripts/render-cli-mcp-parity.py --check`. | <1 minute | PENDING — checkpointed-build, adapter/documentation slice |

## Non-goals and tracked work

- The final canonical `relationships` field, reciprocal-legacy Related collapse, migration Notes/warnings, and removal of the compatibility `dependencies` field are intended work tracked by verified Task `rivets-vio8`.
- Single-Epic Parentage semantics and dedicated Parent interfaces are intended work tracked by verified Task `rivets-qcje`; this design supplies kind-scoped graph machinery that Parentage can reuse but does not implement Parentage rules.
- Claim/release semantics are intended work tracked by verified Task `rivets-8rj9`; relationship mutations continue through the already delivered Workspace lock.
- Generic Issue show enrichment is not part of this accepted add/remove/list interface; dedicated list operations are the authoritative relationship query surface for this change, with no separate future-work commitment.
- Authorization, remote synchronization, custom relationship kinds, distributed graph storage, and graphical relationship editing are permanent non-goals because Rivets is a local Git-backed tracker with the fixed relationship vocabulary accepted by ADR-0002.

## Falsifier run log

- 2026-08-29 — C0 cheapest falsifier: AST pattern `$GRAPH.add_edge($$$ARGS)` and `$GRAPH.remove_edge($EDGE)` over `crates/rivets/src;crates/rivets-mcp/src` found production mutations only under `crates/rivets/src/storage/in_memory` (with test positive controls there). Independent LSP references to `InMemoryStorageInner::graph` found references only in `storage::in_memory::{trait_impl,jsonl,inner}`. No CLI or MCP adapter touches graph implementation. **PASS**.
- 2026-08-30 — F1 C0 privacy falsifier: temporarily importing `crate::storage::in_memory::inner::InMemoryStorageInner` from `cli/execute.rs` without changing visibility made `cargo check -p rivets` fail with E0603 because `inner` is private. Removing the deliberate bypass restored the ordinary build path. **PASS**.

## Approval

Approved by the requester.

- Date: 2026-08-29
- Verbatim approval: “I approve this design”
- Approved risk acceptances: None.
