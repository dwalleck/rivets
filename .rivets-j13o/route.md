# Route: rivets-j13o

Change: Serialize every existing-Workspace CLI and MCP mutation under one durable Workspace-scoped lock from load through atomic save.
Date: 2026-08-29

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | The design depends on cross-process filesystem-lock behavior not covered by current repository evidence: a nonblocking exclusive lock must contend across independent processes on Linux/macOS/Windows, release on process exit and handle drop, classify contention separately from other I/O, and remain scoped by canonical Workspace identity. No locking implementation or dependency exists in current source/manifests. Official Rust 1.94 APIs and a local process probe must establish these premises. | yes |
| 2 | Structural boundary | The change introduces a Workspace mutation guard spanning CLI application construction and MCP cached-storage reload/mutate/save, adds a typed retryable Workspace Busy error across core/CLI/MCP seams, and changes ownership of the load-to-save transaction. | yes |
| 3 | Production-scale risk | Lock contention, cross-process concurrency, crash release, and per-Workspace independence are production concurrency risks. Acquisition is always-on for every mutation and needs bounded latency plus synchronized multi-process stress fixtures. | yes |
| 4 | Explicit behavior | Given an existing Workspace with no active writer, when any CLI or MCP mutation starts, then it acquires that Workspace's durable lock before loading/reloading mutable state, holds it through validation, mutation, and atomic save, and releases afterward. Given one writer holds the lock, when another mutation targets the same Workspace, then the second returns a typed retryable Workspace Busy error without loading stale mutable state or changing persisted bytes. Given simultaneous mutations target different Workspaces, both may acquire independently and complete. Given the holder exits or crashes, the lock becomes acquirable without manual stale-lock cleanup. Given ordinary single-process CLI/MCP mutation and restart flows, their existing observable behavior remains unchanged. Workspace initialization is N/A because it creates, rather than mutates, an existing Workspace; read-only CLI/MCP operations and MCP context selection do not acquire the write lock. Network-distributed locking is not introduced. | yes |

Unknown tests: none

## Selected route

Empirical — correctness depends on unverified cross-process and cross-platform filesystem-lock behavior; precedence selects Empirical before the structural boundary and concurrency-risk tests.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior fully explicit (T4 yes) |
| evidence.md, probe.* | prove-it-prototype | required — Empirical route (T1 yes) |
| design.md | falsifiable-design | required |
| plan.md | budgeted-plan | required |

Oracle checkpoint in `checkpointed-build`: required — Empirical route

## Downstream sequence

prove-it-prototype → falsifiable-design → budgeted-plan → checkpointed-build

## Terminal criterion

Empirical — prove-it-prototype records PASS for every filesystem-lock premise, every later artifact satisfies its owning stage's completion criterion, and checkpointed-build records no FAIL.
