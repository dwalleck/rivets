# Plan: rivets-yk25

## Approved inputs and partition

Design approval: "design approved" — 2026-09-05; `.rivets-yk25/design.md` C1-C8, no risk waivers. Route Structural. Spec/probe N/A as recorded in route. Default branch discovered via `git symbolic-ref refs/remotes/origin/HEAD`: origin/main at 20378ab.

One atomic slice covers C1-C8: bounded storage query contracts plus both public adapters, caller migration, and their shared proof must land together. Internal implementation work can proceed concurrently across disjoint ownership; no intermediate partially migrated contract is a deliverable.

Estimated diff: core 600 + CLI/callers 400 + MCP/callers 450 + cross-adapter/core fixtures 650 + docs/artifacts 450 = 2,550 lines. Churn margin 30% = 765; total 3,315 < 4,000. One PR increment Q contains slice 1 and is independently mergeable when all eight criteria, mutations, scale and workspace gates pass. No push or PR/merge is requested.

## Slice 1: Deliver canonical bounded List and Stale end-to-end

**Claim IDs:** C1, C2, C3, C4, C5, C6, C7, C8.

**Expected behavior:** Both queries require positive explicit limits; List selects newest-created then ascending ID; Stale selects oldest-updated then ascending ID with strict checked cutoff and default Closed exclusion. Filters and error causes agree; schema/default alias cutovers and documentation/callers are atomic. Generic storage enumeration remains unchanged and read-only requests never mutate persisted records.

**Oracle:** Literal fixed UTC records and manually selected ordered Issue arrays, independent from production comparator/predicate; explicit canonical input truth tables and raw persisted bytes. Core fixed-clock comparisons distinguish before/equal/after cutoff; subprocess fixtures avoid clock boundaries.

**Stress fixture:** Reverse insertion order; tied created/update timestamps; differing priorities; newest fresh record plus oldest stale record with limit=1; Open/InProgress/Closed; all five optional List filters and their 32 presence combinations; valid/invalid boundaries including usize maximum without size-based preallocation; 10,000 loaded Issues with sparse and tied matches. Expected selection is newest-created for List and oldest-updated non-Closed for default Stale, ID ascending for ties; explicit Closed selects only Closed; equal cutoff excluded.

**Regression fence:** Core query contract tests in `crates/rivets/tests/query_contract.rs` (or owning `storage/in_memory/query.rs` tests where private fixed-time fixtures are needed): list_selection_contract, stale_selection_contract, generic_enumeration_is_unchanged, query_scale_budget. Domain priority_range. Adapter file `crates/rivets-mcp/tests/query_parity.rs`: canonical_workflow_filters, explicit_limits, shared_filter_contract, list_and_stale_semantics, stale_contract. Server published query schema coverage. Existing behavioral tests migrate only where the intentional contract breaks them; delete wording/default-pinning tests instead of repinning incidental implementation.

**Named mutation:** C1 CLI List canonical status parser maps blocked to Open. C2 MCP List missing limit defaults to 100. C3 List ID tie comparator reversed. C4 domain query Priority >4 rejection removed. C5 default Stale Closed exclusion removed. C6 generic list unconditionally excludes Closed. C7 MCP Stale yields newest-created order instead of canonical stale ordering. C8 repeat full candidate selection once per stored Issue. For each, owning fence must turn red for its exact claim, then pass after restoration. Concrete locations from design are resolved against implemented symbols before edits; no weakening of claimed behavior.

**Complexity/production scale:** Selection scans N borrowed Issues, filters M candidates, sorts O(M log M), truncates to K before Issue clones: O(N + M log M) time and O(M + K) auxiliary space. No requested-limit-sized allocations. Query computation only, excluding existing JSONL load, fixture creation, Cargo process startup and presentation: each 10,000-Issue query <=100 ms on reference workstation. Budget is loose relative to a single borrowed sort but detects repeated full scans/quadratic work. Existing loaded-Workspace memory scaling is not redesigned.

**Wall budget/phase:** Always-on loaded storage query computation <=100 ms for 10,000 Issues. CLI/MCP invocation and schema translation are one-off constant-shape parsing, no separate wall budget.

**Files:** `crates/rivets/src/domain/query.rs` (new), `domain/mod.rs`, `error.rs`, `storage/mod.rs`, `storage/in_memory/mod.rs`, `storage/in_memory/query.rs` (new), `storage/in_memory/trait_impl.rs`; `crates/rivets/tests/query_contract.rs` (new if needed); `crates/rivets/src/cli/args.rs`, `cli/execute.rs`, `cli/mod.rs`, `cli/types.rs` if obsolete SortOrderArg has no remaining callers; affected `crates/rivets/tests/*.rs`; `crates/rivets-mcp/src/models.rs`, `tools.rs`, `server.rs`, `error.rs`; affected `crates/rivets-mcp/tests/*.rs`, new `query_parity.rs`; README.md, CHANGELOG.md, docs/README.md, docs/agents/issue-tracker.md, docs/module-structure.md, docs/data-flow.md, docs/cli-mcp-parity.json and generated docs/cli-mcp-parity.md; other existing tracked command examples identified by scoped caller search migrate when they invoke List/Stale without required limit. No unrelated untracked user documents touched.

**Estimate:** One cross-adapter implementation increment; ~2 engineering days as planning signal, not gate.

**Diff estimate:** 2,550 plus 765 churn margin = 3,315.

**PR increment:** Q — complete bounded query parity, no later increment required.

**Commands and expected results:**
- `cargo test -p rivets --test query_contract` and domain query tests — exact ordered IDs and fixed-cutoff truth table, generic enumeration retains Closed; C3/C4/C5/C6 mutations fail only corresponding behavior then return green.
- `cargo test -p rivets-mcp --test query_parity` — real CLI versus Tools exact ordered full records, field-specific input rejection, omitted/zero limits, aliases, bytes unchanged; C1/C2/C7 mutations red then restoration green.
- `cargo test -p rivets-mcp query_schema` — real published schemas require positive limit and canonical state/priority values.
- `cargo test -p rivets --test query_contract query_scale_budget -- --ignored --exact --nocapture` (or exact owning-crate path if placed there) — closed-form expected sequence and each measured query <=100 ms; C8 named quadratic mutation fails bound then restoration passes.
- Real CLI/MCP smoke against an isolated fixed-timestamp Workspace — manually expected newest List and oldest non-Closed Stale, positive/invalid limits and explicit Closed observed; no mutation.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`, and parity renderer `--check` — all applicable gates green, no stale caller/default expectations.

## Concurrent ownership and interface contract

Main integrates, owns plan/checkpoints, mutations, final formatting/lint/tests, docs/registry, and query_parity.rs. Workers skip all build/lint/test/format commands while concurrent. Every writing worker uses a dedicated git worktree; no parent writes.

Core owns domain/query.rs and storage paths plus query_contract.rs. Public `ListQuery: TryFrom<IssueFilter, Error=QueryError>` consumes the legacy adapter filter once, requires Some positive limit and valid Priority, and stores canonical private fields including NonZeroUsize (not a bypassable IssueFilter data bag). Public `StaleQuery::new(limit: NonZeroUsize, status: Option<IssueStatus>, days: u32, now: DateTime<Utc>) -> Result<Self, QueryError>` validates cutoff arithmetic. New `IssueStorage::list_issues(&ListQuery)` and `stale_issues(&StaleQuery)` return existing Result<Vec<Issue>>. Reexport types from domain; `rivets::Error::Query(#[from] QueryError)` maps domain query error causally.

CLI owns cli args/execute/mod/types and existing rivets CLI integration tests; uses ListQuery::try_from with explicit limit and StaleQuery::new, canonical FromStr only for List/Stale status, no List --sort. CLI Args limits are NonZeroUsize. MCP owns models/tools/server/error and existing MCP tests (not query_parity.rs); required limits NonZeroUsize, ListParams direct optional canonical IssueKind (no legacy alias), canonical status parser, shared QueryError mapped to invalid_params, schema describes runtime range. Both workers consume core interface exactly above; Main owns conflict integration.

## Tracker taxonomy and self-review

Permanent non-goals match design: no alternative List sorts, generic enumeration change, pagination/cache, persistence rewrite or unrelated state transitions. Verified future scopes remain rivets-2va7, rivets-mmqc, rivets-y4vw, rivets-vio8. No slice acceptance is deferred.

All C1-C8 assigned exactly once, all thirteen fields present, each fence and named mutation in same atomic slice, independent literal oracles, query complexity/scale bound explicit, partition arithmetic documented, no approved risk omissions. No slice is declared complete here; checkpointed-build owns completion.

## Slice 1 checkpoint — 2026-09-05

| Gate | State | Evidence |
|---|---|---|
| 1. Affected tests | PASS | `cargo test --workspace`: 1,228 passed across 21 suites, 9 ignored; all migrated callers compile and pass. |
| 2. Pending falsifiers | PASS | C2-C8 discharged by core query_contract, real CLI/MCP query_parity, published router schema tests, and explicit scale run. C1 remains PASS and its permanent fence also passed. |
| 3. Stress fixture | PASS | All 32 List-filter presence combinations, complete ordered Issue arrays, creation/update ties, checked age boundaries, explicit Closed, invalid inputs, maximum limit, context selection and read-only persisted-byte checks. |
| 4. Independent oracle | PASS | Fixed records compared to handwritten ordered IDs/whole records; cross-adapter agreement is also checked against that independent expected result, not merely against each other. |
| 5. Production-scale budget | PASS | 10,000 loaded Issues: List limit1 248.667 us, limit100 220.323 us; Stale limit1 1.037437 ms, limit100 1.010556 ms. Each <100 ms. JSONL load and process startup excluded as planned. One-off adapter parsing: N/A — no separate loop/always-on budget. |
| 6. Regression fences | PASS | All core, CLI/MCP, schema, and scale fences green after restoration. |
| 7. Named mutations | PASS | C1-C8 all compiled and failed their named behavior fence; details below. |
| 8. Restored fences | PASS | Final full workspace run and explicit scale run after all mutants were removed; formatting, all-target/all-feature Clippy with warnings denied, and parity renderer check also passed. |

### Mutation evidence

| Claim | Applied mutation | Observed RED |
|---|---|---|
| C1 | CLI canonical parser accepted blocked as Open | canonical_workflow_filters: `list accepted state \"blocked\"` |
| C2 | ListParams serde supplied implicit positive default 100 | explicit_limits: `MCP list accepted {}` |
| C3 | Reversed List Issue-ID tie comparator | list_selection_contract selected the wrong member at the tied limit boundary |
| C4 | Disabled shared query Priority upper-bound rejection | priority_range no longer returned InvalidPriority(5) |
| C5 | Omitted-status Stale included Closed | stale_selection_contract leaked the Closed sentinel |
| C6 | Generic list excluded Closed | generic_enumeration_is_unchanged lost a required record |
| C7 | Tools::stale re-sorted canonical results newest-created | list_and_stale_semantics: MCP Stale limit3 disagreed with literal oldest-updated order |
| C8 | Repeated full candidate selection once per stored Issue | query_scale_budget: List took 162.243790 ms, exceeding 100 ms |

C2's mechanical mutation preserves the current NonZeroUsize field and uses serde's default hook to reproduce the approved missing-limit fallback; it does not require an uncompilable Option field change. C7 reorders the already-selected records to reproduce the approved adapter ordering bug without changing the eligible set. Both retain the original claim, oracle and behavioral fence.

C7 initially survived because the fixture's creation/update sequences accidentally agreed. The fixture was strengthened by making test-b's creation tie with test-c while its update precedes test-a; assertions were not weakened. The corrected fixture killed C7 and passed after restoration. New List tie boundary uses limit4. The core literal List oracle was corrected before verification to include its deliberately newest test-e.

### Impact, reuse, and symmetry audit

- Caller analysis: core IssueStorage references and fallback scoped searches identified JsonlBackedStorage, InMemoryStorage, MockStorage, CLI execute_list/execute_stale, MCP Tools list/stale and all request/test constructors. New list_issues/stale_issues had no preexisting callers. CLI SortOrderArg references were empty in LSP; scoped search confirmed no remaining source consumers after removal. CLI global-JSON and Label parity parser fixtures also migrated to explicit limits.
- Reuse: existing matches_common_filter supplies Priority/Kind/Label predicate logic; query module adds only intent-specific status/Assignee selection and ordering. Existing canonical domain parsers, chrono checked arithmetic, NonZeroUsize, serde/schemars and the workspace_lock real CLI launcher are reused. No new dependency.
- Core query errors preserve typed causes; MCP maps them to invalid_params with field-specific cause. CLI retains its invalid-input rendering. Read-only storage paths use the existing guarded in-memory access; no persistent mutation or fallback introduced.
- Generic enumeration and zero-data MockStorage keep their existing documented behavior. Empty bounded query results in that mock are consistent with its empty enumeration, not new unimplemented methods. General mock retirement remains rivets-5of9.
- Independent review found no durable actionable defect. Two candidate findings were explicitly withdrawn after evidence checks: ReopenParams never had Default on the baseline, and the existing MockStorage empty-data contract was unchanged. Temporary mutation changes were excluded from review conclusions.
- Stale-reference sweep: current tracked command examples, AGENTS/CLAUDE guidance, module/data-flow docs, MCP request docs and generated parity reference updated. Historical design snapshots and preexisting untracked user documents left unchanged.
- Real CLI smoke independently observed newest List `[test-new,test-closed]`, default Stale `[test-old,test-new]`, explicit Closed Stale `[test-closed]`, and unchanged source JSONL.
- Three task-created isolated worktrees removed after integration. Temporary smoke Workspaces cleaned structurally by TempDir/TemporaryDirectory.

Final integration state: PASS. Slice commit records this checkpoint; no push or merge is part of this task.
