# Plan: rivets-u2mz

Approved design: design.md, requester "I approve", 2026-09-05; no risk acceptances. Structural route. One atomic slice: shared reporting contracts, every implementation/caller, public adapters, registry, and behavior fences must land together. Work is partitioned by file ownership for concurrent implementation, not independent incomplete deliverables.

## Integration budget

Implementation 950 + tests/fixtures 950 + docs/registry 250 + artifacts 250 = 2,400 changed lines; 25% churn margin (600) accounts for configuration/cache and trait forwarding migration. Total 3,000 <=4,000. One increment `ReportingParity`, based on discovered origin/main (refs/remotes/origin/HEAD); independently mergeable when all shared reporting behaviors and workspace tests pass. No push/PR requested. Any actual >4,000 triggers repartition, not silent scope reduction.

## Slice 1: Deliver shared reporting through both public adapters

**Claim IDs:** C1, C2, C3, C4, C5, C6.
**Expected behavior:** Exact approved Information/Statistics shapes; actual initialized configuration; no Information counts; all-Assignment Ready; direct non-Closed Blocked; all Priority buckets; separate context mechanic; no --detailed.
**Oracle:** Hand-authored full expected JSON from fixtures, direct fixture arithmetic for large data, and Rust visibility checks; never derive expected results from production Ready/Blocked/statistics APIs. Existing context test asserts both unset and selected states.
**Stress fixture:** Empty Workspace gives zero in every bucket; custom `data/issues.jsonl`, Unicode/spaced/symlinked root reports actual configuration; state/assignment/priority and direct/indirect/nonblocking relationship matrix gives hand-enumerated counts before/after prerequisite lifecycle transitions. 10,000 Issues with 50,000 Blocking edges and large payloads has analytically precomputed counts and executes aggregation <=2s.
**Regression fence:** Existing `integration::test_where_am_i`; core reporting configuration tests; storage `statistics_canonical_matrix`, `statistics_scale_budget`; MCP `reporting_parity` tests driving real CLI + registered tool dispatch. Exact test name changes that only fit repository harness conventions are recorded in gate log without changing claim/oracle. No fence gaps.
**Named mutation:** C1 force selected context_set false (existing fence, not newly required mutation); C2 replace actual database path with default; C3 exclude assigned Ready; C4 register empty Statistics; C5 recompute full blocked set per Issue; C6 privacy compile oracle: hide public report type and confirm real consumers reject inaccessible type. C6 is a deliberate compile-negative structural fence, not a behavior mutation; use adapter serialization corruption C4 as the runnable behavior mutation for shared type consumption. Restore each mutation and rerun owning fence. Keep mechanical correction notes in design if a named mutation cannot target compiled code.
**Complexity/production scale:** Statistics O(V+E), O(V) auxiliary blocked-ID set, O(1) counters, borrowed Issue iteration, no Issue payload clones. V=10k,E=50k: <=2s for isolated release aggregation, matching established graph budget. Configuration projection O(path length), uses existing loader and backend validation once; no Issue iteration. Serialization bounded by fixed reporting field count; text loops exactly 3 state/5 priority buckets, independent of Issue count.
**Wall budget/phase:** Statistics is request-time always-on work <=2s at fixture scale. Information projection one-off during Workspace initialization: N/A — one-off phase, no new wall budget beyond existing loader. Fixed-size adapter rendering not separately timed; included in real smoke timing, negligible relative to aggregation budget.
**Files:** crates/rivets/src/{reporting.rs,lib.rs,app.rs,storage/mod.rs,storage/in_memory/{trait_impl.rs,graph.rs,mod.rs},cli/{args.rs,mod.rs,execute.rs,execute/reporting.rs},output/{mod.rs,reporting.rs}}; crates/rivets/tests/{in_memory_storage.rs,cli_tests.rs}; crates/rivets-mcp/src/{context.rs,models.rs,tools.rs,server.rs}; crates/rivets-mcp/tests/reporting_parity.rs and impacted integration tests; docs/{cli-mcp-parity.json,cli-mcp-parity.md,module-structure.md}; relevant existing README/CHANGELOG reporting sections; workflow artifacts. Narrow private module extraction allowed within named ownership to avoid growing parent files.
**Estimate:** One implementation/review cycle with focused mutation experiments; no runtime correctness assumptions based on estimate.
**Diff estimate:** 2,400 lines plus 600 margin =3,000.
**PR increment:** ReportingParity.
**Commands and expected results:**
- `cargo test -p rivets-mcp --test integration test_where_am_i -- --exact` -> unset/selected context correct.
- `cargo test -p rivets reporting` and `cargo test -p rivets --test in_memory_storage statistics` -> exact configuration/error and canonical count fixtures.
- `cargo test -p rivets-mcp --test reporting_parity` -> actual CLI + registered MCP payloads equal independently expected JSON, unchanged JSONL bytes.
- `cargo test -p rivets --release statistics_scale_budget -- --ignored --nocapture` -> analytic counts and <=2s aggregation; repeated-scan mutation red, restored green.
- `cargo check --workspace` -> shared report consumers compile; deliberate privacy mutation rejected, restoration compiles.
- Per-fence named mutations -> relevant assertions fail, restoration passes.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` -> assembled repository green.
- Real CLI temporary Workspace `init`, configured info, stats text/JSON and MCP dispatch smoke -> approved shape, unconditional priorities, removed flag rejected, no production Workspace mutation.

## Ownership contract

Main integration owner, artifacts/docs/registry/final checks. Core writer owns reporting types, storage aggregation, App retention and core tests. CLI writer owns CLI/output and CLI tests. MCP writer owns Context retention, tools/server/models, MCP parity tests. Writers use dedicated worktrees; never edit parent/main or one another's files. Skip all validation during concurrent work; Main validates assembled slice. No shared-file concurrent edits. Core interfaces are fixed in assignment context before dispatch.

## Preimplementation critique and self-review

All six claims assigned exactly once; all pending falsifiers discharge in Slice 1; all thirteen fields filled. Large graph load and count oracle have different failure mechanisms. Path behavior is verified on current Linux platform; no claim of executed Windows runtime tests. Existing validation/error rules remain at configuration seam, private result construction prevents invalid public data bags. New read behavior never acquires mutation ownership or writes storage. No waived fence. Plan does not declare completion.

Permanent non-goals and verified separate work remain those in approved design; no new deferrals.

## Uncommitted Slice 1 review refinement: F1

Purpose additionally resolves verified review finding F1 under existing C2: information lookup must survive concurrent explicit-root cache eviction. The guard-paired read-fast-path/write-init implementation uses the existing Context lock and lookup helpers; no new abstraction or approved-contract change.

Oracle/stress/fence: `information_survives_concurrent_cache_eviction` in reporting_parity.rs starts 64 valid explicit-root callers, exceeding the 32-entry cache, and compares every report to its independently built fixture JSON. Original two-guard code failed with WorkspaceNotInitialized; paired-guard code passes. Command: `cargo test -p rivets-mcp --test reporting_parity information_survives_concurrent_cache_eviction -- --exact`. Mutation: restore original two-guard Tools::info; expected same missing-cache-entry failure. The observed original RED and repaired GREEN discharge this regression check.

Files: tools.rs and reporting_parity.rs within original Slice 1 ownership. No new loop in production; lookup is O(1) plus existing bounded canonicalization/initialization; cached reports retain shared-lock concurrency. Tests add 64 temporary Workspaces. Diff remains within the 600-line churn margin; same ReportingParity increment and all other mandatory Slice 1 fields remain unchanged.

Mechanical C6 mutation correction is recorded in design.md: runnable missing-ready serialization mutation replaces compile-only type visibility breakage, retaining private fields, compiler checks, independent oracle and intent.

## Slice 1 checkpoint — 2026-09-06

| Gate | State | Evidence |
|---|---|---|
| 1. Affected tests | PASS | `cargo test --workspace`: 1,227 passed, 10 ignored, 22 suites. |
| 2. Pending falsifiers | PASS | C2-C6 discharged by configuration/core/statistics/registered-MCP fixtures, compile check, and release scale run; C1 reran in full suite. |
| 3. Stress fixture | PASS | Canonical matrix and 64-root eviction tests pass; 10k Issues/50k edges gives total/open=10k, Ready=5k, Blocked=5k, each Priority=2k. |
| 4. Independent oracle | PASS | Both real adapters equal literal expected JSON, including empty/mixed states, lifecycle transitions, configured paths and cached snapshot; final CLI smoke equals the original independently expected single-assigned-Issue result. |
| 5. Production-scale budget | PASS | Final release aggregation 3.509735ms <=2s; O(V+E), one blocked-set derivation and borrowed aggregate. Configuration initialization: N/A — one-off existing loader; fixed-size serialization/text bounded by five buckets. |
| 6. Regression fences | PASS | Core reporting, canonical matrix, symlink identity, registered MCP parity/snapshot/concurrency, and explicit release scale fence green. |
| 7. Named mutations | PASS | Every mutation below failed at its intended assertion; no compile-only or unrelated-guard failures counted. |
| 8. Restored fences | PASS | All probes removed; full workspace suite plus explicit release fence and cargo check pass on assembled source. |

### Mutation evidence

- C2 hardcoded default data path: `information_uses_resolved_configuration` expected data/issues.jsonl, got .rivets/issues.jsonl (red).
- C2 ignored unsupported backend: `information_rejects_unsupported_backend` failed its typed Err assertion (red).
- C2 omitted App canonicalization: `app_canonicalizes_symlink_root` got alias storage path instead of canonical Workspace storage path (red).
- C2 reloaded config for cached information: snapshot fence got prefix newer/data/replaced.jsonl instead of rep/data/issues.jsonl (red).
- C2/F1 original two-context-guard lookup: 64-root eviction fence failed with WorkspaceNotInitialized (red); paired-guard fix green.
- C3 unassigned-only Statistics: canonical matrix Ready=3 versus expected 4 (red).
- C4 registered stats emitted Default: actual empty MCP payload versus expected total=9/Ready=6/Blocked=1 (red).
- C5 repeated blocked-set scans: exact counts still correct but 11.9026897s exceeded 2s (red); restored final 3.509735ms.
- C6 dropped ready serialization: exact zero-bucket JSON missing required ready key (red).

### Final integration commands

`cargo fmt`; `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace`; `cargo check --workspace`; `cargo test -p rivets --release --test in_memory_storage statistics_scale_budget -- --ignored --exact --nocapture`; `python scripts/render-cli-mcp-parity.py --check`: all PASS.

Real CLI smoke: configured data/issues.jsonl, one assigned Open P1 Issue; exact count-free Information and total=1/Ready=1/P1=1 Statistics; text displayed all five buckets; --detailed rejected with exit 2. Same scenario was red before implementation. Temporary Workspace removed. MCP proof drives the actual stdio server and registered tools/call paths, not a mock.

### Integration and impact record

New APIs: reporting types and IssueStorage::statistics. Migrated callers: App initialization/accessors; all three IssueStorage implementations (InMemory, JSONL wrapper, MockStorage); CLI Info/Stats dispatch and output; MCP Context, Tools, server and response models. Removed StatsArgs, StatsResponse and CLI-local StatusCounts/count_by_status; migrated CLI tests and docs. Reused RivetsConfig::load/StorageConfig::to_backend, canonicalization, graph::find_blocked_issues, existing Context locking, serde and tempfile/tokio. LSP references returned empty for new worktree symbols; grep cross-check enumerated actual callsites. LSP quick fix applied the cache conditional simplification. No new dependency.

Implementation corrections before final gate: made canonical matrix Epic explicitly P1 to match its declared Priority oracle; corrected the close-transition arithmetic (one prerequisite leaves Ready as one dependent enters, so Ready stays 4); used a real Epic for the MCP Parentage control. Removed text-wording-only tests and a redundant immutable-field snapshot test rather than re-pinning them. Core information keeps configured data-file location instead of adding an unnecessary filesystem normalization helper. No approved scope/architecture changes or waived gates.

Rust pre-commit checklist: recoverable errors propagated/typed; no new production unwrap/expect or discarded Result; enum matching exhaustive; shared path seams and private fields retained; one-snapshot linear aggregation; behavioral and edge fixtures pass; formatting, lint and workspace tests pass. CONTEXT.md/ADRs intentionally unchanged because domain semantics are unchanged. Reporting docs/registry/changelog synchronized.
