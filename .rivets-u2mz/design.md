# Design: rivets-u2mz

## Route and inputs

Structural; route.md T4 contains the complete eight-part given/when/then behavior contract. Sources: rivets-u2mz, CONTEXT.md, ADR-0002, ADR-0006, docs/cli-mcp-parity.json operations workspace_information/workspace_statistics/inspect_workspace_context. No Parent specification is linked by the task; the parity registry and ADR-0006 govern it. spec.md: N/A — behavior supplied by these sources, with concrete interface choices below submitted for approval. evidence.md/probe: N/A — no unverified empirical premise.

Baseline real CLI: `target/debug/rivets info --json` returns hardcoded database_path, prefix and Issue counts; `stats --json` returns no Priority counts. Source confirms default-unassigned Ready and repeated cloned queries. No code implementation has begun.

### Proposed observable contract choices

- MCP names `info` and `stats`, each with the existing optional workspace_root convention; CLI names remain info/stats.
- Information JSON: `{workspace_root, database_path, storage_backend, issue_prefix, config_path}`. Root and config_path are canonical absolute identities; database_path is the absolute configured storage location resolved using existing StorageConfig rules. Backend is `jsonl`, the only supported persisted Workspace backend. No statistics in this result.
- Configuration identity means the loaded configuration file's path, not a content hash or a new version scheme. Report the configuration snapshot used to initialize the storage, never combine cached storage with a newly reloaded prefix.
- Statistics JSON: `{total, by_status: {open, in_progress, closed}, ready, blocked_by_dependencies, by_priority: {p0_critical, p1_high, p2_medium, p3_low, p4_backlog}}`. Retain existing CLI key vocabulary, verified at execute.rs:1598-1602; all keys are always present, including zeros.
- Ready is Workspace-wide: explicitly all Assignments, Open and direct-unblocked. It is not the count of the default unassigned Ready query. Blocked counts non-Closed dependents with any unresolved direct Blocking Dependency, once per dependent.
- Priority counts cover all Workflow States, not just Ready work. Remove redundant CLI --detailed because Priority counts become unconditional in both text and JSON; update every caller/documented invocation.
- where_am_i retains its separate context-inspection behavior, including unset context; no CLI counterpart or repurposing.

## Input shapes

| Shape | Coverage |
|---|---|
| Context unset / set / explicit workspace_root override; missing Workspace | C1, C2, C4 |
| Default config / custom relative data file; empty or absent configured data file at initialization | C2; preserve existing loader semantics |
| Root supplied absolute / relative / symlink; Unicode and spaces in valid paths | C2 |
| Missing/malformed YAML, invalid prefix, empty or unknown backend, configured PostgreSQL, absolute/traversing/empty invalid storage paths | C2; existing typed errors, no fallback |
| StorageBackend Jsonl / InMemory / PostgreSQL | C2; JSONL reachable from Workspace config; InMemory exercised by C3; PostgreSQL remains unsupported, no new backend |
| Empty / singleton / mixed Issue collections, identical field values on distinct IDs | C3, C4 |
| Every Workflow State crossed with unassigned/assigned where legal | C3, C4; unassigned In Progress and assigned Closed are invalid and rejected/migrated by existing domain seams |
| Priorities 0 through 4, zero-count buckets, distinct/matching priorities | C3, C4; negatives and >4 are rejected at existing input seams |
| No relationships / direct unresolved / resolved / several prerequisites on one dependent / chained Blocks | C3, C4 |
| Parentage / Related / Discovery; duplicate insertion of the same relationship | C3, C4; existing relationship semantics own insertion and duplicate handling |
| Close prerequisite / reopen prerequisite / Closed dependent retaining relationships | C3, C4 |
| 10,000 Issues and 50,000 edges, including large descriptions | C5 |

Subtractive-invariant sweep: no locking, validation, uniqueness, or ordering constraint is removed. This is a reporting projection cutover. Removing --detailed and info counts removes obsolete presentation surfaces, not a safety invariant. MCP storage/config snapshot correspondence must remain intact (C2); aggregation must observe one locked snapshot (C3).

## Placement

### Shared information

Owner: core `commands/init.rs` owns RivetsConfig loading/backend resolution; a small `reporting` module owns the serialized WorkspaceInformation value and shared configuration projection. App and MCP Context retain the projection created from the same loaded config and resolved backend used to construct storage. Canonicalize root at their existing initialization seams.

New seam alternatives: (A) App-only reporting, with MCP constructing a second App per request, reuses code but bypasses MCP caching and duplicates storage loads; (B) shared core projection used by both existing initialization paths preserves each adapter's lifecycle. Choose B. Do not introduce another configuration loader or use the unused src/config.rs placeholder.

Forbidden: CLI/MCP reconstruct default paths or assemble competing payloads; reporting depends on MCP; per-info Issue scans. Private report fields and core-owned construction plus owning-crate tests mechanically fence placement (C2/C6).

### Statistics

Owner: existing IssueStorage interface gains statistics returning shared WorkspaceStatistics; InMemoryStorage aggregates under one guard; JsonlBackedStorage delegates. Shared serialized result types live in core reporting. MockStorage must implement an honest empty result consistent with its empty read model, not another panic stub.

New seam: capability slots behind existing IssueStorage. Alternatives: compose public list/ready/blocked queries (simple but clones/sorts and allows inconsistent snapshots) versus borrowed aggregation under one guard. Choose aggregation. Reuse graph::find_blocked_issues once and factor the Open/unblocked eligibility check used by ready_to_work and statistics into one private predicate. Assignment visibility remains a query selector; Statistics uses all Assignments.

Forbidden: new aggregation logic in CLI or MCP; sorting/cloning Issues to count; changing Ready/Blocked query ordering or filtering. Report fields private with read access; builder/aggregation mutation internal to core. C3/C5/C6 mechanically protect ownership, correctness and bounded execution.

### Adapters and tests

Owner: CLI reporting orchestration belongs in `cli/execute` reporting command-family module, extracted from existing execute.rs as needed; text rendering belongs in output. MCP methods/registration follow current tools.rs/server.rs patterns, with substantive aggregation absent from those large modules. Delete unused MCP StatsResponse and CLI-local StatusCounts/count_by_status if no remaining consumer. Registry and generated reference change atomically with tool inventory and --detailed removal.

New seam: none beyond shared report results. Extend the existing cross-adapter testing pattern from query_parity.rs for reporting; exercise real CLI processes and registered MCP tool dispatch, not just direct inner calls. Owning-crate tests and exact public payload tests mechanically fence placement; no new reporting trait layer.

## Claims

- C1: MCP context inspection remains available and distinguishes unset from selected context independently of shared reporting.
- C2: Both Information adapters report the same actual initialized Workspace configuration without Issue counts or default-path assumptions.
- C3: Core Statistics returns canonical state, all-Assignment Ready, direct Blocked and complete Priority counts from one snapshot.
- C4: Real CLI and registered MCP reporting return the complete independently expected semantic payload, without mutating persisted Issues.
- C5: Statistics stays bounded on the established 10k-Issue/50k-edge workload without cloning Issue payloads or sorting Issues.
- C6: Shared reporting semantics are owned by core, with adapters consuming the same immutable report types.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Context inspection remains distinct | unset/set context | Existing test_where_am_i must observe false/no root then true/root/database. Positive selected-context control rules out always-unset implementation. | Explicit two-state expected responses independent of reporting | tools.rs where_am_i force context_set=false in selected-context branch; existing test becomes red | rivets-mcp integration::test_where_am_i | Existing focused test | PASS |
| C2 | Information reflects actual configuration | Config/path matrix and concurrent cache eviction | Compare exact custom-path, snapshot, symlink, Unicode/spaced-root reports and typed unsupported-backend rejection; 64 explicit-root calls retain valid reports | Independently authored fixture paths/configuration and literal JSON | Default database path; unsupported-backend fallback; omit App root canonicalization; reload cached report from changed config; original two-guard info lookup | reporting configuration tests, app_canonicalizes_symlink_root, reporting_parity snapshot/concurrency tests | Focused temporary Workspaces | PASS — all fences restored green; mutations red |
| C3 | Statistics obeys canonical predicates and bucket meanings | State/Assignment/Priority/relationship matrix | Exact tuples before/after prerequisite close/reopen, Closed dependent control, multiple prerequisites and nonblocking relations | Hand-enumerated fixture counts, not production query output | Aggregation excludes assigned Open Issues; Ready must incorrectly fall from 4 to 3 | statistics_canonical_matrix | In-memory tests | PASS — mutation red, restored green |
| C4 | Public adapters expose semantic parity | Empty/mixed fixtures, explicit roots and context | Real CLI and registered stdio MCP tools/call match complete literal JSON and preserve source bytes | Independently authored full payloads | MCP stats handler discards computed result and serializes an empty report | reporting_matches_literal_oracle_through_cli_and_registered_mcp; empty_reporting_contains_every_zero_bucket | Real processes | PASS — mutation returned zeros instead of 9-Issue report; restored green |
| C5 | Aggregation is scale-bounded | 10k Issues, 50k edges, large descriptions | Exact analytic counts and isolated aggregation <=2s, with borrowed iteration/no sorting | Fixture index arithmetic independent of graph traversal | Repeat full blocked-set derivation per Issue | statistics_scale_budget | Isolated release run | PASS — restored 3.509735ms; mutation failed at 11.9026897s |
| C6 | Core owns shared reporting | Shared private-field types and both adapters | Cross-crate compile plus core/public exact serialization fences | Compiler visibility rules and literal JSON | #[serde(skip)] on ready removes a required serialized key | cargo check --workspace, statistics_serialization_includes_zero_buckets and C4 | Compile and serialization tests | PASS — missing-key mutation red, restored green |

### Mechanical gate refinements

- C6's original make-the-type-private mutation only fails compilation. Per checkpointed-build applicability rules it is replaced by the runnable missing-ready-key serializer mutation above, preserving the claim, independent payload oracle, and intent. Private fields and cross-crate compilation still enforce placement.
- C2/C4 public fence's implemented name is `reporting_matches_literal_oracle_through_cli_and_registered_mcp`; `empty_reporting_contains_every_zero_bucket` covers empty explicit-root/no-context behavior. `cached_mcp_report_keeps_initialized_configuration_snapshot` covers configuration changes while cached.
- F1 strengthens existing C2 snapshot coverage with `information_survives_concurrent_cache_eviction`: 64 explicit roots exceed the 32-entry cache; all must return their independently authored report. Its mutation is the original two-guard Tools::info sequence (initialize storage, release Context, then read report), reproduced red before the guard-paired fix.
- Information database_path preserves the resolved backend location; it does not invent a new filesystem-normalization policy for data files. Canonical root/config identity remains unchanged.

## Non-goals and future work

Permanent non-goals: new backend support, configuration hashes/versioning, resource/relationship lifecycle changes, MCP context redesign, compatibility aliases for removed reporting shapes, persistent precomputed statistics caches. None is required for reporting parity; preserving old mixed info/counts or optional Priority shapes would violate the requested cutover.

Verified intended work outside this change: rivets-2va7 owns Ready/Blocked query parity, limits and ordering; rivets-y4vw owns the broader all-intent semantic parity suite. Both verified in tracker output; this task still supplies its complete reporting cross-adapter fences, not a deferral to y4vw.

## Falsifier run log

2026-09-05, worktree /home/dwalleck/repos/rivets-u2mz:
`cargo test -p rivets-mcp --test integration test_where_am_i -- --exact`
PASS: 1 passed, 0 failed, 120 filtered out. Build completed in 13.89s; test in 0.00s. This proves C1's existing preservation baseline only, not unimplemented reporting behavior.

## Approval

Requester approval: "I approve" — 2026-09-05.
Approved risk acceptances: None; every claim has a regression fence.
