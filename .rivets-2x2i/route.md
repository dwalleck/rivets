# Route: rivets-2x2i

Change: Make Related Associations symmetric and preserve directed Discovery Origins across storage, CLI, MCP, and persistence.
Date: 2026-08-29

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | `CONTEXT.md`, ADR-0002, parent specification `rivets-5mlg`, and Task `rivets-2x2i` define both relationship contracts completely. Current repository evidence includes the explicit `BlockingDependency` domain/storage seam plus dedicated CLI and MCP adapter patterns; no behavior depends on an external system or an unverified production premise. | no |
| 2 | Structural boundary | The change adds public domain/storage relationship APIs, dedicated CLI command vocabulary and structured output, dedicated MCP tools/models/router entries, and canonical relationship persistence semantics across crate boundaries. | yes |
| 3 | Production-scale risk | Related and Discovery operations remain bounded graph mutations over the existing in-memory Workspace and JSONL persistence model. No new latency, throughput, memory, concurrency, or data-volume premise is introduced. | no |
| 4 | Explicit behavior | Given two distinct Issues, when Related is added in either endpoint order, then exactly one canonically ordered Association is visible from both endpoints; retrying either order is idempotent, removing from either endpoint removes it, and no cycle check or readiness effect applies. Given a discovered Issue and a distinct source Issue, when Discovery Origin is added, then the directed discovered-to-source provenance is recorded; multiple sources are allowed, but self-reference and provenance cycles are rejected without mutation. Given either relationship, when other relationship kinds exist on the same pair, then all meanings coexist independently. Given CLI or MCP add/remove/list operations followed by restart, then both interfaces return agreeing structured relationships with deterministic persistence, while Blocked and Ready remain unchanged. | yes |

Unknown tests: none

## Selected route

Structural — behavior is explicit and repository-defined, but the change crosses public domain, storage, persistence, CLI, and MCP boundaries.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior fully explicit (T4 verdict) |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified premise (T1 verdict) |
| design.md | falsifiable-design | required — Structural route |
| plan.md | budgeted-plan | required — Structural route |

Oracle checkpoint in `checkpointed-build`: required — Structural route

## Downstream sequence

falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate.

## Result

Structural route completed with no failed gate.

- Core increment: `638b32c`, `5ac72b6`, `2536e28`; draft PR #99.
- Adapter increment: `040b188`, `e0360f8`; stacked branch
  `work/2x2i-adapters`.
- Focused domain/storage/loader/CLI/MCP oracles passed, including every named
  mutation's red-to-green check.
- Final seam check found graph mutations only under `storage::in_memory`.
- Final Rust gate: `cargo fmt --check`, workspace clippy with warnings denied,
  and 1,106 tests passed with one ignored.
- Real CLI smoke: Related and Discovery add/list/remove passed across separate
  processes in a temporary Workspace.
- Approved-design base assumptions about a delivered Workspace lock and parity
  registry were corrected explicitly in `design.md`; verified Task
  `rivets-j13o` owns the lock, and no `rivets-2x2i` acceptance criterion was
  reduced.
