# Route: rivets-8rj9

Change: Make Assignment an atomic exclusive Claim/Release contract across storage, CLI, MCP, and persistence.
Date: 2026-08-30

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | No external or unverified system behavior is required. The shared mutation boundary and contention behavior are repository-owned: `crates/rivets/src/workspace_lock.rs::WorkspaceMutationLock`, `crates/rivets/src/app.rs::App::from_directory_for_mutation`, and the closed prerequisite `rivets-j13o` establish the durable Workspace lock. Current Assignment and lifecycle behavior is directly inspectable at `crates/rivets/src/storage/in_memory/trait_impl.rs::IssueStorage::{create,update}` and can be checked through existing real CLI/MCP Workspace harnesses. | no |
| 2 | Structural boundary | The change adds Claim/Release operations to the public `IssueStorage` interface, adds CLI commands and MCP tools/parameter types, adds branchable domain/storage/MCP errors, and removes Assignment from general-update adapter contracts. It also centralizes lifecycle/Assignment invariants at the shared storage/domain seam. | yes |
| 3 | Production-scale risk | Separate processes may contend for one Workspace. The correctness requirement is exactly-one durable winner without lost updates, plus retry semantics that distinguish retryable Workspace Busy from terminal Already Claimed. This is a concurrency risk and requires synchronized process regression fences. | yes |
| 4 | Explicit behavior | **G1** Given an Open, unblocked, unassigned Issue, when claimant A claims it, then only Assignee and `updated_at` change and the claim survives restart. **G2** Given A already holds the Open claim, when A retries, then the operation succeeds idempotently without mutation; when B claims, then B receives Already Claimed and the Issue is unchanged. **G3** Given A holds an Open claim, when A releases it, then it becomes unassigned; a mismatched releaser, unassigned Issue, or In Progress Issue is rejected without mutation. **G4** Given an Open Issue, when it enters In Progress, then an Assignee is required; when In Progress returns to Open, then its claim is retained. **G5** Given any non-Closed Issue, when it closes, then Assignment is cleared; when a Closed Issue reopens, then it is unassigned and Open. **G6** Given a general update request, when it attempts to set or clear Assignment, then the adapter cannot express a blind Assignment replacement; Claim and Release are the only mutation intents. **G7** Given creation with an Assignee, when initial prerequisites make the new Issue blocked, then creation is rejected; otherwise the initial claim is accepted under the same Open/unblocked readiness rule. **G8** Given two synchronized processes claiming the same unassigned Ready Issue, when both attempt mutation, then exactly one durable claimant wins; a contending attempt may first receive Workspace Busy, but retry after lock release yields idempotent success for the winner or Already Claimed for the loser. CLI and MCP expose the same error meaning and restart behavior. | yes |

Unknown tests: none

## Selected route

Structural — public storage, CLI, and MCP contracts change, while synchronized multi-process mutation carries concurrency risk; no external empirical premise is unresolved.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior is fully explicit in `rivets-8rj9`, parent specification `rivets-5mlg`, ADR-0002, and the T4 contract above |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified empirical premise (T1=no) |
| design.md | falsifiable-design | required — Structural route |
| plan.md | budgeted-plan | required — Structural route |

Oracle checkpoint in `checkpointed-build`: required — Structural route

## Downstream sequence

falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Structural — `design.md` and `plan.md` satisfy their owning stages, every independently green slice passes checkpointed-build's applicable gate, synchronized CLI/MCP process behavior is exercised through an independent persisted-state oracle, and checkpointed-build records no `FAIL`.
