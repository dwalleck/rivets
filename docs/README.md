# Rivets Documentation

## Overview

Rivets is a Rust issue tracker (edition 2024, MSRV 1.94.0) that stores Issues as Git-friendly JSONL. The Cargo workspace has three crates:

- `rivets` — CLI application and core issue-tracking domain
- `rivets-jsonl` — JSONL reading/writing library
- `rivets-mcp` — MCP server for AI assistants (25 tools)

The CLI exposes 16 top-level commands: `init`, `info`, `create`, `list`, `show`, `update`, `close`, `reopen`, `delete`, `ready`, `blocking-dependency`, `label`, `resource`, `stale`, `blocked`, and `stats`.

**Current behavior:**

- `.rivets/config.yaml` is the single configuration source; there is no config layering or environment merge.
- JSONL is the default persisted backend. PostgreSQL is a placeholder that returns "unsupported".
- JSONL loading has three stages: parse compatibility records, import Issues, then rebuild relationships.
- Issue IDs combine a prefix with an adaptive hash whose inputs include a timestamp and nonce; they are not content-addressed.
- `blocking-dependency add/remove/list/tree` uses explicit dependent and prerequisite roles. The same typed storage interface backs equivalent MCP tools; relationship changes never auto-mutate Issue status.
- Implemented surfaces include canonical Blocking Dependencies, Associated Resources, immutable Notes, mutable Issue Kind, labels, `stats`/`stale`/`info`, and MCP multi-workspace support. Most MCP issue operations accept an optional `workspace_root`; otherwise they use the context selected by `set_context`.

This index separates **current reference documentation** (describes the implemented system) from **accepted decisions** (ADRs, some of which the implementation intentionally lags) and **historical artifacts** (pre-implementation plans and research, not the work frontier).

## Current Reference Documentation

- [Architecture Overview](./architecture.md) — system architecture: CLI/App/Storage/Domain layers, storage backends, dependency handling, and readiness computation.
- [Module Structure](./module-structure.md) — workspace organization, per-crate module breakdown, public API surfaces, and testing structure.
- [Storage Architecture](./storage-architecture.md) — storage trait hierarchy, in-memory representation, JSONL persistence and error recovery, cycle detection, and readiness queries.
- [Data Flow](./data-flow.md) — end-to-end flows: command lifecycle, init, CRUD, Blocking Dependency mutation/query, JSONL load, and state transitions.
- [Terminology Reference](./terminology.md) — implementation vocabulary for storage layers, data structures, and operations.
- [CONTEXT.md](../CONTEXT.md) — canonical domain glossary (Workspace, Issue, Workflow State, Ready, Blocked, Issue Relationships, Associated Resources). Authoritative for domain meaning; ADRs record why load-bearing decisions were made.
- [AGENTS.md](../AGENTS.md) — current engineering and navigation rules for AI assistants working in the repo.
- [README.md](../README.md) — user-facing overview of the CLI.
- [CHANGELOG.md](../CHANGELOG.md) — release history.

## Accepted Decisions (ADRs)

- [ADR-0001: Notes as Chronological Log](./adr/0001-multiple-notes.md) — immutable, append-only Notes replace the legacy singular Note string.
- [ADR-0002: Separate workflow, readiness, and issue relationships](./adr/0002-issue-relationships-and-readiness.md) — Workflow State is Open, In Progress, or Closed; Blocked is derived only from explicit Blocking Dependencies; Ready means Open and unblocked. Parentage does not affect readiness; Related Association is symmetric; Discovery Origin is directed provenance.
- [ADR-0003: Model related material as Associated Resources](./adr/0003-associated-resources.md) — typed resources with explicit targets (Web URL or Workspace Path) and standard roles replace the singular untyped External Reference.
- [ADR-0004: One wire vocabulary for Issue records](./adr/0004-one-wire-vocabulary.md) — MCP tool responses and CLI `--json` serialize the domain `Issue` directly; timestamps normalize to RFC-3339 `Z` form.
- [ADR-0005: The domain owns status-transition rules](./adr/0005-domain-owned-status-transitions.md) — close/reopen rules live in the domain and fire at the single update site; "status" is the code-and-wire name for Workflow State.

**Implementation lags some accepted decisions.** Blocking Dependency mutation and queries now use canonical typed interfaces, but Parentage, Related Association, Discovery Origin, Workflow State/Ready alignment, and canonical `relationships` persistence remain in their tracked ADR-0002 slices. A legacy `blocked` status vocabulary and parent-blocking readiness behavior still exist until those slices land. Issue Kind is mutable (Bug, Feature, Task, Epic, Chore); “Issue type” is legacy vocabulary. Legacy singular Notes, External References, and generic relationship records are accepted only at compatibility seams; do not document them as canonical.

## Agent Documentation

- [Domain Docs](./agents/domain.md) — how engineering skills should consume this repo's domain documentation.
- [Issue Tracker: Rivets](./agents/issue-tracker.md) — the checked-in `.rivets/issues.jsonl` is the source of truth; use the Rivets MCP tools or the CLI, never hand-edit the store.
- [Triage Labels](./agents/triage-labels.md) — mapping of canonical triage roles to actual label strings.

## Historical Artifacts (not the work frontier)

These documents describe pre-implementation plans, proposals, and research from before the current system was built. They are kept for context only; do not treat them as descriptions of the current system or as a source of tasks.

- [Task Dependency Graph](./task-dependency-graph.md) — historical implementation roadmap for the original MVP: task ordering and planning decisions that predate the implemented system. Superseded and no longer maintained; the live tracker is authoritative.
- [plans/](./plans/2026-04-06-tethys-overview-command.md) — dated implementation plans and archives from prior work efforts (see also the plans directory).
- [design/](./design/rest-api.md) — design proposals and roadmaps (rest API, event sourcing, daemon architecture, automerge, implementation roadmap, rivets roadmap). Accepted decisions now live in the [ADR files](./adr/0001-multiple-notes.md) listed above.
- [research/](./research/automerge-research.md) — research notes (automerge, jsonl-to-sqlite).
- [rivets-jsonl-research.md](./rivets-jsonl-research.md) — early JSONL library research that predates the current `rivets-jsonl` crate.

The current work frontier is the repo's live issue tracker (`.rivets/issues.jsonl`), not these documents — see [Agent Documentation](#agent-documentation) and [AGENTS.md](../AGENTS.md).
