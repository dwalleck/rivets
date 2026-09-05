# Route: rivets-g3t7

Change: Enforce one canonical Label grammar across CLI, MCP, storage, and JSONL
Date: 2026-08-29

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | Current repository evidence covers the material data premise: `.rivets/issues.jsonl` contains 15 Issues with noncanonical persisted Labels (`DRY`, `M-DOCUMENTED-MAGIC`, `M-LOG-STRUCTURED`, and `*.rs` spellings). The canonical grammar and present adapter/storage behavior are directly documented in `CONTEXT.md`, `docs/cli-mcp-parity.json`, CLI validators, domain fields, JSONL records, and storage methods. No external premise remains unverified. | no |
| 2 | Structural boundary | The change introduces a public private-field `Label` domain value, changes public `Issue`, `NewIssue`, `IssueUpdate`, `IssueFilter`, and `IssueStorage` interfaces, and converts CLI, MCP, JSONL, output, and storage callers. | yes |
| 3 | Production-scale risk | Label parsing is linear in at most 50 ASCII bytes and label collections are already bounded by Issue size. No material latency, throughput, memory, concurrency, or data-volume risk. | no |
| 4 | Explicit behavior | Resolved contract: Given a 1-50 byte lowercase ASCII-alphanumeric Label with optional single internal `-`/`_` separators, parse and preserve its spelling; given empty, uppercase, whitespace/control/Unicode, invalid endpoint, overlength, or adjacent same/mixed separators, reject before query/mutation; create/update/filter/add/remove/JSONL/storage must use the same value; add/remove remain idempotent; list-all remains sorted/deduplicated; CLI and MCP errors share domain meaning; canonical persisted Labels round-trip. Unresolved decision: 15 existing repository Issues carry noncanonical Labels, but the ticket does not define whether JSONL loading rejects their records, migrates their spellings, or preserves them through a compatibility representation. | no |

Unknown tests: none

## Selected route

Structural — public domain/storage interfaces and persisted conversion change, and the persisted noncanonical-Label policy requires requester interrogation before design.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | required — persisted noncanonical-Label behavior is unresolved |
| evidence.md, probe.* | prove-it-prototype | N/A — repository evidence covers all premises |
| design.md | falsifiable-design | required after spec approval |
| plan.md | budgeted-plan | required after design approval |

Oracle checkpoint in `checkpointed-build`: required — Structural route

## Downstream sequence

interrogated-spec → falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate.

2026-09-05 integration result: **PASS**. The approved grammar and strict-loading policy remain unchanged; current Ready-filter propagation, owned conversion, and F2/F3 corrections are recorded in `plan.md`'s integration checkpoint.
