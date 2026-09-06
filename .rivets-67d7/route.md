# Route: rivets-67d7

Change: Restrict general Update to canonical mutable fields across adapters.
Date: 2026-09-06

## Route tests

| # | Test | Evidence | Verdict |
|---|---|---|---|
| 1 | Empirical premise | No proposed behavior depends on an external system premise. Existing Rust domain/storage, CLI and MCP implementations and ADR-0005/0006 define the seams; persistence continues using existing Workspace locking and JSONL saves. | no |
| 2 | Structural boundary | UpdateArgs, MCP UpdateParams/Tools update, and shared IssueUpdate/storage must lose intent-owned fields. Dedicated lifecycle and CLI Note entry points require interface decisions. | yes |
| 3 | Production-scale risk | Existing Workspace lock and whole-Workspace persistence must remain intact; CLI batch processing must preserve ordered per-target partial success. Cross-target transaction and concurrency behavior are load-bearing. | yes |
| 4 | Explicit behavior | Issue acceptance criteria require descriptive fields and Kind only, empty rejection without timestamp/byte changes, dedicated Assignment/Label/Note/lifecycle/relationship ownership, equal single-target validation/state, and loop-equivalent batches. Neither the issue nor docs/cli-mcp-parity.json resolves whether Priority counts among allowed Update fields; current Update accepts Priority in both adapters. The registry currently classifies CLI update --notes as intentional Append Note syntax, conflicting with strict field removal. Lifecycle replacement surface must preserve entering In Progress and returning to Open. | no |

Unknown tests: none; the identified behavior choices require requester resolution.

## Selected route

Structural — public interfaces and mutation ownership change, with unresolved scope decisions but no unverified external premise.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | required — T4 unresolved allowed-field and dedicated-intent choices |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified premise |
| design.md | falsifiable-design | required — Structural route |
| plan.md | budgeted-plan | required — Structural route |

Oracle checkpoint in checkpointed-build: required — Structural route.

## Downstream sequence

interrogated-spec → falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Every downstream artifact satisfies its owning stage's completion criterion, ending with no FAIL in checkpointed-build's recorded gate. Not yet satisfied: specification decisions and requester design approval precede implementation.
