# Route: rivets-gf4j

Change: Express Blocking Dependencies end to end with explicit dependent-to-prerequisite direction.
Date: 2026-08-28

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | Current repository code exposes generic `DependencyType`, `Dependency`, `IssueStorage::{add_dependency,remove_dependency,get_dependencies,get_dependents}`, CLI `dep`, and matching MCP tools. ADR-0002 and the parent specification fully define the replacement semantics; no external or unverified system behavior is required. | no |
| 2 | Structural boundary | The change replaces public domain/storage APIs, CLI command vocabulary and JSON output, MCP tools/models/router entries, and relationship persistence semantics across crate boundaries. | yes |
| 3 | Production-scale risk | Blocking Dependency operations retain the existing bounded in-memory graph and JSONL persistence model. The request introduces no new latency, throughput, memory, concurrency, or data-volume premise. | no |
| 4 | Explicit behavior | Given a dependent and prerequisite, when a Blocking Dependency is added, then every API, persisted edge, JSON response, and human phrase identifies that exact direction. Given the relationship, when it is listed, traversed, removed, reloaded, or queried from either endpoint, then identifiers and direction agree across CLI, MCP, and storage. Given a self-edge or cycle, when added, then it is rejected without mutation. Given multiple prerequisites/dependents or another relationship kind on the same pair, when added, then each valid relationship coexists independently. Given a prerequisite closes, when blockedness is recomputed, then the relationship remains recorded but stops blocking the dependent. | yes |

Unknown tests: none

## Selected route

Structural — the behavior is explicit, but it changes public APIs, adapter schemas, persistence semantics, and cross-crate boundaries.

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
