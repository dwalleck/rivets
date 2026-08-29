# Route: rivets-omq0

Change: Prevent a long-lived MCP Workspace cache from overwriting out-of-band JSONL changes.
Date: 2026-08-29

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | Current repository contracts cover the required premises: `IssueStorage::reload` re-reads JSONL and discards cached state (`crates/rivets/src/storage/mod.rs`), MCP mutations already hold the per-Workspace storage write lock through mutation and save (`crates/rivets-mcp/src/tools.rs`), and the live MCP context reproduced stale state by failing to observe the restored `rivets-omq0` record after `set_context`. No external API or undocumented platform behavior is needed. | no |
| 2 | Structural boundary | The observable MCP tool schemas and persisted JSONL schema remain unchanged, but the fix governs the shared mutation/persistence seam used by every MCP mutator. This is an internal cross-operation placement decision, not a public module-boundary change. | no |
| 3 | Production-scale risk | A naive reload before every mutation adds JSONL-size-dependent I/O and parsing latency to all MCP writes; the bug also concerns concurrent writers and data-loss behavior. The route therefore requires an explicit performance and concurrency design fence. | yes |
| 4 | Explicit behavior | Given a long-lived MCP Workspace whose storage was cached before an out-of-band `issues.jsonl` change, when any MCP mutation is invoked, then the mutation operates on the latest persisted state or is rejected without writing, and the out-of-band change remains present. Given no out-of-band change, when an MCP mutation is invoked, then its existing result and persistence behavior remain unchanged. Given mutation persistence fails after in-memory state changed, when the call returns, then cached state is restored from disk as today. | yes |

Unknown tests: none

## Selected route

Structural — concurrency/data-loss correctness and JSONL-size-dependent reload cost require a falsifiable design and budgeted implementation plan.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior fully explicit (T4 yes) |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified premise (T1 no) |
| design.md | falsifiable-design | required |
| plan.md | budgeted-plan | required |

Oracle checkpoint in `checkpointed-build`: required — Structural route

## Downstream sequence

falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate.
