# Route: rivets-yk25

Change: Align List and Stale query limits, ordering, filters, and Workflow State across CLI and MCP.
Date: 2026-09-05

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | Behavior is repository-owned and specified by rivets-yk25, ADR-0006, and docs/cli-mcp-parity.json List/Stale rows. No external system premise is required. Current adapter/storage paths are directly inspectable. | no |
| 2 | Structural boundary | Required CLI arguments, MCP request schemas, shared query-policy placement, and caller contracts change across rivets and rivets-mcp. | yes |
| 3 | Production-scale risk | Filtering, ordering and limiting operate over the loaded Workspace; moving policy below adapters must avoid premature truncation and unnecessary Issue clones. Explicit complexity and a large-fixture budget are required. | yes |
| 4 | Explicit behavior | Given either query, when limit is omitted or nonpositive, reject the request. Given matching List Issues, return newest-created first with deterministic Issue-ID ties, then limit. Given matching Stale Issues older than the requested age, return oldest-updated first with deterministic Issue-ID ties, then limit. Given Stale without a Workflow State filter, exclude Closed; given explicit Closed, return only matching Closed Issues. Given Workflow State filters, accept only Open, In Progress, Closed canonical wire spellings. Given shared Priority/Kind/Assignee/Label filters, enforce equivalent constraints and error meaning. Given identical fixtures and requests, CLI JSON and MCP results contain equal ordered Issues. Given passing cross-adapter behavioral fences, mark only the List/Stale registry rows conformant. | yes |

Unknown tests: none. Concrete tie-break spelling, query type placement, and age arithmetic are implementation design decisions, not unresolved product scope.

## Selected route

Structural — cross-adapter public contracts and shared query policy change, with Workspace-scale ordering risk.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior explicit in Issue acceptance criteria and ADR-0006 registry |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified external/system premise |
| design.md | falsifiable-design | required |
| plan.md | budgeted-plan | required after design approval |

Oracle checkpoint in checkpointed-build: required — Structural route.

## Downstream sequence

falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate.

## Terminal disposition — 2026-09-05

PASS. Requester approved design with \"design approved\". The single atomic slice
completed checkpointed-build's eight gates; C1-C8 mutations were observed RED
and all restored fences passed. Final `cargo test --workspace`: 1,228 passed,
9 ignored. Explicit 10,000-Issue List/Stale scale fence passed; formatting,
all-target/all-feature workspace Clippy with warnings denied, real CLI/MCP
behavior, and parity renderer verification passed. Evidence and the fixture
strengthening discovered by C7 are recorded in plan.md's Slice 1 checkpoint.
