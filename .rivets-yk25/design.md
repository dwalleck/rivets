# Design: List and Stale query parity

## Route and inputs

Structural route: `route.md`. Complete behavior source: its T4 given/when/then set, rivets-yk25 Acceptance Criteria, ADR-0006, and `docs/cli-mcp-parity.json` rows `list_issues` and `stale_issues`.

Spec: N/A — explicit behavior. Empirical evidence/probe: N/A — repository-owned behavior, no external premise. No parent specification is attached to this Task; ADR-0006 and the registry are its governing specification.

Existing evidence: generic `IssueStorage::list` includes all Workflow States, filters, sorts creation descending, then limits. CLI List re-sorts by Priority by default; MCP defaults to 100 versus CLI 50. CLI Stale excludes Closed and sorts update ascending; MCP Stale includes Closed and preserves creation order. Generic list also feeds information, statistics and label enumeration, so its callers must not inherit new public-query restrictions.

## Input shapes

| Shape | Coverage |
|---|---|
| Limit omitted, zero, negative, fractional/string JSON, one, less/equal/greater than match count, maximum representable usize, overflow | C2/C7; maximum uses no limit-sized preallocation |
| Status absent, each of open/in_progress/closed, blocked, in-progress alias, empty/unknown/case variants | C1/C4/C5/C7 |
| List Priority absent, 0..4, negative, 5, 255, overflow/noninteger; Kind absent and all five variants, invalid/empty, legacy issue_type field | C4/C7 |
| Assignee absent, empty, ASCII, Unicode, embedded spaces; Label absent, valid canonical, empty, uppercase, malformed separators, Unicode and length boundaries | C4/C7; exact Assignee matching, existing Label grammar |
| All 32 presence combinations of List status/priority/kind/assignee/label; explicit required limit independent of filter combination | C4/C7; table-driven expected subsets, not a new default policy |
| Stale days absent (30), zero, positive, u32 maximum, negative/overflow/noninteger, representable and unrepresentable cutoff | C5/C7; checked subtraction rejects impossible cutoff |
| Stale days/status presence matrix (four cells), each status branch with explicit limit | C5/C7 |
| Empty Workspace, singleton, multiple distinct Issues, duplicate timestamps, creation/update ordering disagreement, matching/nonmatching filters, limit cutting a tied group | C3/C5/C7 |
| Duplicate Issue IDs in canonical loaded storage | N/A — storage identity invariant; no JSONL compatibility change |
| Timestamps strictly before, exactly at, and strictly after cutoff; future update dates | C5; strict updated_at < cutoff |
| Relative/absolute Workspace paths, spaces/Unicode, explicit/implicit MCP context | C7; preserve existing Workspace resolution rather than introduce a new path policy |
| 10,000 loaded Issues with large nonmatching subsets and tied timestamps | C8 |

### Removed invariants

This is a contract replacement: remove implicit limits, CLI alternative List sorts, and adapter-local ordering/default-state policy. No mutation lock or lifecycle guard is removed. A caller can no longer rely on an omitted limit or --sort; documentation/examples and affected callers migrate atomically. Generic all-Issue enumeration remains unchanged (C6). Workspace read operations remain read-only; rejected requests do not persist changes (C7).

## Placement

### Validated query values

Owner: `rivets::domain`, new `domain/query.rs` exported by `domain/mod.rs`. Public opaque `ListQuery` and `StaleQuery` types with private fields own required positive `NonZeroUsize` limits, validated Priority 0..4, canonical Workflow State parsing, existing IssueKind/Label values, and exact optional Assignee matching. `StaleQuery` accepts a single supplied current instant plus u32 days (default 30 at both adapter seams), uses checked cutoff arithmetic, and stores the resulting cutoff. Dedicated typed query errors preserve causes until CLI/MCP rendering.

New seam alternatives: (A) extend public `IssueFilter` with policy flags and ordering; fewer signatures but permits contradictory optional defaults and silently affects statistics/Ready consumers. (B) dedicated intent query values and two methods on existing `IssueStorage`; slightly larger trait but isolates the contract, makes missing/zero limits unrepresentable, and leaves internal enumeration stable. Select B.

Forbidden: public raw query structs with detached validation; adapters reimplementing range checks after canonical construction; broad changes to IssueStatus aliases used by unrelated intents. CLI List/Stale use canonical FromStr parsing rather than the shared ValueEnum alias. List MCP drops the hidden issue_type alias without affecting other intents.

### Selection and ordering

Owner: existing `rivets::storage` seam, adding `list_issues(&ListQuery)` and `stale_issues(&StaleQuery)` to IssueStorage; `storage/in_memory/query.rs` owns selection/order/limit behind private backend helpers. JsonlBackedStorage delegates; existing MockStorage receives real query behavior consistent with its data or migrates the affected tests to in-memory storage, never new unimplemented stubs.

List default state selection remains all Workflow States. Stale defaults to non-Closed, explicit status means only that state. List order is `(created_at descending, Issue ID ascending bytewise)`; Stale order is `(updated_at ascending, Issue ID ascending bytewise)`. Apply all filters and stale cutoff before ordering and truncation. Collect borrowed candidates, sort references, and clone only selected results; allocation scales with matches, not requested limit. No new trait, cache, pagination or persistence representation.

Forbidden: call generic list and then re-sort cloned Issues in either adapter; truncate before canonical ordering; alter generic list to require limits or exclude Closed. Mechanical placement fence: core storage tests exercise the same typed query methods without CLI/MCP dependencies; private query fields reject bypass construction at compile time.

### Adapters and verification

Owner: CLI args/execute for parsing and presentation; MCP models/tools/server for wire/schema/protocol translation. Both construct query values and delegate once. Remove CLI List --sort; require --limit; MCP list/stale require limit and publish integer minimum 1. Publish canonical state enums and Priority bounds; retain existing IssueKind and Label constraints. Existing MCP error mapping renders typed query rejection as invalid_params; CLI uses its normal invalid-input path. Identical English envelopes are not required; rejected fields and causes are.

Owner: `crates/rivets-mcp/tests/query_parity.rs`, a focused integration test file reusing the real CLI launcher pattern in `tests/workspace_lock.rs::run_cli`: Cargo run with an explicit repository manifest derived from CARGO_MANIFEST_DIR, and the existing direct Tools fixture pattern. MCP package tests cannot use CARGO_BIN_EXE_rivets. Server schema tests inspect actual published router schemas. Compare whole ordered serialized Issue arrays, not just IDs/counts. Core tests use fixed instants for exact cutoff; cross-process fixtures place dates safely away from the cutoff so independent clocks cannot introduce flakes. Build the CLI before the focused integration run to avoid repeated compilation.

Forbidden: testing only a duplicate selector implementation; declaring registry conformity on inventory equality alone; adding a general parity framework for unrelated intents.

## Claims

C1. The existing real CLI continues to accept all three canonical Workflow States and reject Blocked as a filter.
C2. Both public query intents require a positive explicit limit and the canonical query values cannot represent an absent or zero limit.
C3. List filters before selecting newest-created Issues with ascending Issue-ID ties.
C4. List/Stale shared filters accept the same canonical values and reject the same invalid causes across adapters.
C5. Stale applies a checked strict-age cutoff, excludes Closed unless explicitly requested, and selects oldest-updated Issues with ascending Issue-ID ties.
C6. Query policy is usable through the core storage interface while unrelated generic enumeration keeps all Workflow States.
C7. Real CLI JSON and MCP semantic results agree item-for-item, invalid requests remain read-only, and published schemas describe the accepted contracts.
C8. Loaded-Workspace query selection is O(N + M log M) time and O(M + K) auxiliary space and completes each 10,000-Issue query within 100 ms on the reference workstation.

## Falsification

All PENDING rows are discharged by checkpointed-build at the implementing slice checkpoint; budgeted-plan assigns slices after approval.

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Canonical states accepted, Blocked rejected by real CLI | Three states and blocked | Initialize valid empty Workspace; each canonical state must return []; blocked must fail specifically at state parsing. A bad Workspace could otherwise explain failure; successful canonical calls control for it. | Literal state truth table from CONTEXT | In ListArgs canonical status parser, map blocked to Open; blocked request must incorrectly succeed and turn fence red | query_parity::canonical_workflow_filters | Four real process calls | PASS — pre-design CLI run below; permanent fence migrated during build |
| C2 | Explicit positive limits | Full numeric limit shapes | Omitted/zero/negative limits fail with limit cause while limit=1 returns the literal first matching Issue. Valid filter/Workspace controls isolate limit validation. | Literal requests and first expected ID; schema required/minimum assertions complement actual calls | In models.rs make ListParams.limit optional with fallback 100; missing-limit MCP request must incorrectly succeed | query_parity::explicit_limits | Focused integration fixture | PENDING — checkpointed-build |
| C3 | Deterministic List selection | Tied creation times, differing Priority/update dates, filter removes newest, cutoff through tie | Request limit 2 against reverse-insertion fixture; exact expected ordered IDs must match; differing clocks/Priority deliberately make accidental comparators disagree. | Hand-authored timestamp/ID ordering, independent from production sorter | In in_memory/query.rs reverse the List ID tie comparator; tied output must reverse and fail | storage query tests::list_selection_contract | Focused core fixture | PENDING — checkpointed-build |
| C4 | Equivalent filter causes and exact subsets | Status/Kind/Priority/Assignee/Label presence matrices and invalid inputs | Compare exact subsets for valid filters and field-specific rejected causes for one invalid field at a time. Positive matching Issues prevent empty-fixture false passes. | Literal filter membership tables and canonical grammar | Remove Priority >4 rejection in domain/query.rs; priority 5 must stop producing typed invalid Priority | query_parity::shared_filter_contract and domain query tests::priority_range | Core and adapter fixtures | PENDING — checkpointed-build |
| C5 | Canonical Stale semantics | All states, exact cutoff boundaries, opposing creation/update order, days and tie shapes | Fixed-time core fixture includes before/equal/after cutoff and Closed/Open/InProgress; exact oldest subset and explicit Closed subset must match; future timestamp and valid small age control overflow rejection. | Literal fixed UTC timeline and selected IDs | In in_memory/query.rs remove omitted-status Closed exclusion; oldest Closed sentinel must leak into output and fail | storage query tests::stale_selection_contract; query_parity::stale_contract | Core and adapter fixtures | PENDING — checkpointed-build |
| C6 | Core policy, generic behavior retained | Public bounded queries versus generic unbounded all-state enumeration | Owning-crate tests invoke typed storage query methods directly and separately enumerate an Open and Closed Issue with default IssueFilter; both remain in generic output. Interface imports compile only when core owns policy. | Literal all-state set plus C3/C5 expected sequences | In generic list implementation add unconditional Closed exclusion; generic fixture loses Closed and fails | storage query tests::generic_enumeration_is_unchanged; core query contract tests | Focused core fixture | PENDING — checkpointed-build |
| C7 | Cross-adapter results/schema/read-only behavior | Whole Issue records, limit/filter/status/day inputs, context variants | Compare complete ordered Issue arrays to literal fixture and each other; check persisted bytes unchanged; actual router schemas require limit/canonical enum/ranges. Both adapters agreeing incorrectly is excluded by literal oracle. | Raw persisted fixture fields and manually selected ordered Issue arrays | In Tools::stale return list_issues ordering instead of stale_issues ordering for the same eligible fixture; exact MCP/CLI sequence differs | query_parity::list_and_stale_semantics; server schema tests for query contracts | Real CLI + Tools integration | PENDING — checkpointed-build |
| C8 | Bounded query scale | 10,000 Issues, tied timestamp blocks, sparse matches, limits 1 and 100 | Time only loaded-storage query calls (not fixture creation or JSONL); compare exact expected IDs and assert each <100 ms. Correct results isolate latency from skipped work. | Fixed generated ID/timestamp groups with closed-form expected sequence, not production sorting | In in_memory/query.rs repeat the full candidate selection once per stored Issue before producing result; 10k fixture exceeds bound | storage query tests::query_scale_budget (explicit ignored scale fence) | Explicit 10k fixture | PENDING — checkpointed-build |

## Non-goals and future work

Permanent non-goals: configurable List sort order (conflicts with this fixed shared intent); changing generic storage enumeration; adding pagination, caching, new storage adapters, changing Workspace resolution, changing unrelated lifecycle/Ready contracts, or rewriting JSONL compatibility. No risk waiver requested.

Intended future work: `rivets-2va7` owns Ready/Blocked parity; `rivets-mmqc` owns broader MCP schemas/defaults; `rivets-y4vw` owns the generalized cross-intent parity suite; `rivets-vio8` owns canonical legacy persistence migration. These IDs exist in the current tracker and cover those scopes. This change still owns complete List/Stale schema and behavior proof; none of its acceptance criteria is deferred.

## Falsifier run log

2026-09-05: `cargo build -p rivets --bin rivets` PASS. Python subprocess smoke created an isolated Workspace using `target/debug/rivets init --prefix probe`, then invoked `list --limit 1 --status STATE --json` for open/in_progress/closed/blocked. Canonical values each exited 0 with []; blocked exited 2 and named invalid --status with possible values open/in_progress/closed. C1 PASS. Temporary Workspace removed. This preserves a currently working property; it does not claim the remaining parity changes already work.

Self-review: every input shape has a claim or explicit N/A; all eight rows identify falsifier, independent oracle, named mutation, fence, cost, and status; no FAIL or waived fence; structural assertions are anchored in owning-crate methods/private types; C1 cheapest falsifier passed; all intended future work is verified and no acceptance criterion deferred.

## Approval

Requester approval: "design approved" — 2026-09-05.

Approved risk acceptances: None.
