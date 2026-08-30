# Route: rivets-brai

Change: Separate persisted Workflow State from derived Blocked and assignee-aware Ready semantics across storage, CLI, MCP, and output.
Date: 2026-08-29

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | The behavior is defined by `CONTEXT.md`, ADR-0002, ADR-0004, ADR-0005, ADR-0006, and the parent specification `rivets-5mlg`. Current repository code exposes the implementation gap directly: `IssueStatus` still contains `Blocked`; `find_blocked_issues` propagates Parentage; and `ready_to_work` accepts every non-Closed, unblocked Issue. No external-system behavior or stale empirical measurement determines the design. | no |
| 2 | Structural boundary | The change removes a public domain enum variant, changes accepted CLI/MCP state inputs and their generated schemas, changes the `IssueStorage::ready_to_work` query contract, and updates persisted/domain serialization plus CLI/MCP text and JSON behavior. Placement crosses domain, storage, CLI, MCP, output, loader compatibility, and integration-test seams. | yes |
| 3 | Production-scale risk | The existing Ready/Blocked implementation already traverses the in-memory Issue and relationship graph in O(n + e). The requested predicate and assignee filtering do not add a new latency, throughput, memory, concurrency, or data-volume dimension. | no |
| 4 | Explicit behavior | **G1** Given any canonical CLI, MCP, domain, or persisted state input, when it is parsed or serialized, then only `open`, `in_progress`, and `closed` are canonical Workflow States and `blocked` is never accepted or emitted as one. **G2** Given a non-Closed dependent with zero or more Blocking Dependencies, when blockedness is queried, then it is Blocked iff at least one recorded prerequisite is non-Closed; closing the final prerequisite clears Blocked immediately without deleting the relationship. **G3** Given Parentage, Related Associations, or Discovery Origins, when Blocked or Ready is queried, then those relationships have no effect. **G4** Given Issues in all lifecycle, blocker, and assignment combinations, when Ready is queried without assignment visibility options, then only unassigned Open, unblocked Issues are returned; an assignee filter returns only that assignee's Open, unblocked Issues; explicit administrative all-assignee visibility returns both assigned and unassigned Open, unblocked Issues; Closed and In Progress are always excluded. **G5** Given the same persisted Workspace, when queried through CLI text, CLI JSON, MCP, and again after process/server restart, then state, Blocked, Ready membership, assignment visibility, identifiers, and relationships agree. | yes |

Unknown tests: none

## Selected route

Structural — the behavior is explicit and internally specified, but it changes public domain, storage, CLI/MCP schema, serialization, and cross-module adapter boundaries.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior is fully explicit in T4 and the parent specification |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified empirical premise (T1 verdict) |
| design.md | falsifiable-design | required — Structural route |
| plan.md | budgeted-plan | required — Structural route |

Oracle checkpoint in `checkpointed-build`: required — Structural route

## Downstream sequence

falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate.
