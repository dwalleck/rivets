# Route: rivets-ragt

Change: Parse canonical Issue IDs at every CLI and MCP boundary
Date: 2026-08-29

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | No external or production-only premise. The grammar is fully present in `crates/rivets/src/cli/validators.rs::validate_issue_id`, its prefix bounds in `commands/init.rs`, and ADR-0006's `canonical-issue-id-input` rule. | no |
| 2 | Structural boundary | Adds a fallible public interface to the domain `IssueId` type and moves validation ownership from the CLI adapter into the domain for use by both the `rivets` and `rivets-mcp` crates. | yes |
| 3 | Production-scale risk | Parsing is linear in one short Issue ID. No data-volume, latency, memory, concurrency, or throughput risk beyond the existing CLI validator. | no |
| 4 | Explicit behavior | Given a canonical ID with a 2-20 ASCII-alphanumeric prefix and one or more ASCII-alphanumeric suffix segments separated by single hyphens, when CLI or MCP receives it, then parsing succeeds before lookup/mutation. Given empty input, a missing separator, a prefix outside 2-20 bytes, non-ASCII/non-alphanumeric prefix input, an empty suffix, invalid suffix characters, or leading/trailing/consecutive suffix hyphens, when any shared ID-bearing CLI/MCP intent receives it, then that adapter rejects it before storage with the same domain error meaning. Given CLI Create prerequisites, one or many IDs are parsed through the same seam. Given a valid canonical ID loaded from a legacy Workspace, persistence remains readable. Given all cross-adapter fences pass, the registry marks `canonical-issue-id-input` conformant. | yes |

Unknown tests: none

## Selected route

Structural — the change introduces a public domain parsing interface consumed across crate and adapter seams.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior is fully explicit in the ticket, ADR-0006, registry rule, and T4 contract above |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified empirical premise |
| design.md | falsifiable-design | required |
| plan.md | budgeted-plan | required |

Oracle checkpoint in `checkpointed-build`: required — Structural route

## Downstream sequence

falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate.

2026-09-05 integration result: **PASS**. The approved contracts remain unchanged; current adapter coverage and the F1 review correction are recorded in `plan.md`'s integration checkpoint.
