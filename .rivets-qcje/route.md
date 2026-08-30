# Route: rivets-qcje

Change: Enforce single-Epic Parentage without blocking descendants across domain, storage, CLI, MCP, and persistence seams.
Date: 2026-08-30

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | All premises are repository-owned and specified by `rivets-qcje`, parent specification `rivets-5mlg`, `CONTEXT.md`, and ADR-0002. Current code provides directly inspectable legacy `DependencyType::ParentChild`, graph, JSONL, CLI, MCP, and integration-test behavior; no external-system behavior is needed. | no |
| 2 | Structural boundary | The change replaces legacy ParentChild behavior with canonical Parentage across public storage interfaces, CLI commands and structured output, MCP tool/model contracts, and adapter error mappings. Placement spans `crates/rivets/src/domain`, `storage`, `cli`, and `crates/rivets-mcp`; cross-module placement decisions are required. Persistence reuses the existing dependency-record compatibility seam because canonical relationship migration belongs to `rivets-vio8`. | yes |
| 3 | Production-scale risk | Parentage mutations and invariant checks operate on the existing in-memory Workspace graph and are serialized by the durable Workspace mutation lock. No new concurrency primitive, unbounded external I/O, or production-scale latency/throughput/memory premise is introduced. Cycle and direct-child checks remain bounded by the loaded Workspace graph. | no |
| 4 | Explicit behavior | Given any child and candidate parent, when Parentage is set or moved, then the parent must be an Epic, the child has at most one parent, self-parentage and Parentage cycles are rejected independently of Blocking Dependency cycles, and the old parent remains unchanged if validation fails. Given an existing Parentage, when it is cleared or shown, then CLI and MCP expose equivalent clear/show contracts and structured results. Given a blocked Epic with descendants, when Ready/Blocked is computed, then descendants remain unaffected unless each has an explicit Blocking Dependency. Given an Epic with non-Closed direct children, when closure is attempted, then closure fails, reports those children, and performs no cascade. Given a Closed Epic, when a non-Closed child is attached or an existing child is reopened beneath it, then the operation is rejected. Given successful Parentage mutations and failures, when the Workspace or MCP context restarts, then relationships and structured errors preserve the same behavior. | yes |

Unknown tests: none

## Selected route

Structural — the request is behaviorally explicit but changes public CLI/MCP and storage interfaces across domain and adapter modules.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior fully explicit (T4 verdict) |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified premise (T1 verdict) |
| design.md | falsifiable-design | required |
| plan.md | budgeted-plan | required |

Oracle checkpoint in `checkpointed-build`: required — Structural route

## Downstream sequence

falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate.

## Terminal disposition

Complete. Slice gates for C1-C11 recorded no FAIL:

- `29039e1` — typed Parentage, atomic storage operations, persistence, and C1-C5/C9 falsifiers.
- `f85ddb3` — Epic lifecycle invariants, Parentage-independent readiness, and C6-C8 falsifiers.
- `2f7b5d6` — role-safe CLI commands, real-process restart contract, and C10 falsifier.
- `7cbaef3` — guarded MCP tools, router/error/lock/cache contracts, and C11 falsifier.

Final proof: `cargo fmt --all -- --check`, Clippy with all targets/features and
warnings denied, and `cargo test -p rivets -p rivets-mcp` passed with 861 tests
and 5 ignored production-scale fences. Both Parentage production-scale fences
were run explicitly and passed their 100 ms assertions.
