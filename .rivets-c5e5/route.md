# Route: rivets-c5e5

Change: Prove and complete atomic canonicalization of a mixed legacy Issue Workspace after one mutation.
Date: 2026-08-28

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | Current `issue_record.rs` owns legacy-to-domain conversion, `jsonl.rs` owns atomic canonical writes, and existing CLI, MCP, and resilient-loader integration suites already exercise each seam. The change requires composing those established behaviors, not assuming unverified external behavior. | no |
| 2 | Structural boundary | The accepted canonical Issue schema and CLI/MCP contracts already exist. This change extends behavioral integration proof through existing loader, persistence, CLI, and MCP seams; it does not alter a public API, schema, module boundary, or placement. | no |
| 3 | Production-scale risk | The requested fixture is a small mixed temporary Workspace. It introduces no latency, throughput, memory, concurrency, or data-volume behavior. | no |
| 4 | Explicit behavior | Given one Workspace containing missing, null, legacy, canonical, multiline, long, URL, opaque, and all legacy Kind shapes, when CLI and MCP load it and one successful mutation occurs, then every supported Issue remains observable and every persisted record uses only canonical Kind, Notes, and Resources. Given conflicting canonical and legacy values, when the Workspace loads, then a visible warning identifies the conflict rather than silently choosing. Given the first canonical rewrite, when the canonical Workspace is reloaded and saved again, then the bytes and semantic ordering remain unchanged. Given CLI process restart, MCP context recreation, and direct loader reload, when the same Workspace is inspected, then converted values persist rather than existing only in memory. | yes |

Unknown tests: none

## Selected route

Local — behavior and seams are explicit, current repository evidence covers all premises, and no contract or boundary changes are requested.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior fully explicit (T4 verdict) |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified premise (T1 verdict) |
| design.md | falsifiable-design | N/A — Local route: no design gate |
| plan.md | budgeted-plan | N/A — Local route: no plan gate |

Oracle checkpoint in `checkpointed-build`: N/A — Local route: checkpointed-build does not run

## Downstream sequence

none — implement with normal repository fix/TDD

## Terminal criterion

Local — focused behavioral verification records PASS for the mixed legacy Workspace tests at the resilient loader, CLI process, and MCP context-recreation seams, including canonical second-save byte stability.

Result: 2026-08-28 | `cargo test -p rivets --test in_memory_resilient_loading mixed_legacy_fixture_round_trips_to_stable_canonical_jsonl` | PASS
Result: 2026-08-28 | `cargo test -p rivets --test cli_tests mixed_legacy_fixture_loads_migrates_and_persists_via_cli` | PASS
Result: 2026-08-28 | `cargo test -p rivets-mcp --test integration mixed_legacy_fixture_migrates_through_mcp_and_context_recreation` | PASS
