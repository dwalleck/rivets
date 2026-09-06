# Route: rivets-u2mz

Change: Expose shared Workspace Information and Statistics contracts.
Date: 2026-09-05

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| T1 | Empirical premise | Configuration loading is repository-owned (`App::from_root`, `RivetsConfig`); canonical readiness is implemented by storage and governed by ADR-0002. No external behavior or experimental technology is required. | no |
| T2 | Structural boundary | Shared reporting result types, CLI info/stats JSON contracts, MCP tool inventory, and shared configuration projection change. ADR-0006 and docs/cli-mcp-parity.json explicitly require this cutover. | yes |
| T3 | Production-scale risk | Statistics traverse Workspace Issues and Blocking Dependencies; avoid issue cloning and repeated complete graph scans. Use a bounded large fixture and independent count oracle. | yes |
| T4 | Explicit behavior | Full given/when/then contract below comes from rivets-u2mz, CONTEXT.md, ADR-0002, and ADR-0006 registry. Field names and placement are design decisions for approval. | yes |

### T4 behavior contract

1. Given an initialized Workspace, when CLI info or MCP Workspace Information executes, then both return equivalent Workspace root, actual backend and storage location, Issue ID prefix, and configuration identity, with no Issue counts.
2. Given an MCP session with or without selected context, when where_am_i executes, then context inspection remains an intentional MCP-only mechanic separate from Workspace Information.
3. Given the same Workspace snapshot, when CLI stats or MCP Statistics executes, then total, Workflow State counts, derived Ready/Blocked counts, and Priority counts agree exactly.
4. Given empty or populated Issues, when Statistics executes, then Workflow State counts contain only Open, In Progress, and Closed, including zeros.
5. Given direct unresolved Blocking Dependencies, when Statistics executes, then shared canonical predicates determine Blocked and Ready; Parentage, Related Association, and Discovery Origin do not block. Closed Issues are not Blocked; only Open unblocked Issues are Ready. Workspace-wide Ready includes all Assignments explicitly, not only unassigned work.
6. Given any Priority distribution, when Statistics executes, then P0 through P4 counts are always present and deterministic; each counts all Issues at that Priority regardless of Workflow State.
7. Given default or custom supported storage configuration, when Information executes, then fields describe the actual resolved Workspace configuration, not assumed default storage paths.
8. Given cross-adapter fixtures, when both real adapter paths execute, then tests compare complete semantic payloads against independently specified expectations, not just each other.

Unknown tests: none.

## Selected route

Structural — public reporting contracts and cross-adapter ownership change; no unverified empirical premise.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior explicit in task and governing decisions |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified empirical premise |
| design.md | falsifiable-design | required |
| plan.md | budgeted-plan | required after design approval |

Oracle checkpoint in checkpointed-build: required — Structural route.

## Downstream sequence

falsifiable-design → requester approval → budgeted-plan → checkpointed-build

## Terminal criterion

PASS — 2026-09-06. Every downstream artifact satisfies its owning stage's completion criterion; design approved verbatim, plan implemented as one atomic slice, all eight checkpoint gates PASS. Final integration: 1,227 workspace tests passed; fmt, Clippy -D warnings, cargo check and parity renderer passed; explicit scale fence and real CLI/MCP oracles passed. Evidence and mutations are recorded in plan.md; the atomic slice commit includes these artifacts.
