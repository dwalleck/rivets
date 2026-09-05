# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Canonicalized Issue ID parsing across CLI and MCP, including Assignment and every relationship endpoint; malformed IDs are rejected before Workspace lookup while canonical persisted IDs remain readable.
- Added canonical typed Labels across CLI, MCP, filters, storage, and JSONL; invalid persisted Labels retain the partial-load write guard, and owned spellings are validated without an extra copy.
- Added atomic Assignment Claim/Release across domain storage, CLI, and MCP; claims serialize under the durable Workspace lock, lifecycle transitions cannot strand owners, and only Workspace Busy failures are retryable.
- Canonicalized Workflow State to Open/In Progress/Closed and made Ready an Open, direct-unblocked, Assignment-aware query that defaults to unassigned Issues across CLI and MCP.
- Replaced the generic `dep` CLI/MCP/storage mutation contract with role-safe Blocking Dependency add, remove, list, and tree interfaces; creation now accepts explicit prerequisites.
- Added single-Epic Parentage with role-safe CLI and MCP set, clear, move, and show operations; Parentage enforces ownership and lifecycle invariants without affecting Blocked or Ready. Valid opposing Parentage and Blocking edges survive restart, and atomic moves return prior ownership without adapter pre-checks.
- Replaced Issue Type with mutable Issue Kind across domain, CLI, MCP, output, and canonical JSONL contracts while retaining legacy `issue_type` loading.
- Renamed `MigrationField` persistence accessors to reflect emitted and migration-only JSONL field names.
- Upgraded `rmcp` to 1.8.0 to clear RUSTSEC-2026-0189 while preserving stdio transport.
- Upgraded `anyhow` to 1.0.103 to clear RUSTSEC-2026-0190 surfaced by Cargo Deny CI.

## [0.1.0] - 2025-12-17

### Added

#### rivets-jsonl
- Initial release of the JSONL library
- High-performance streaming JSONL parser
- Async read/write support with tokio
- Type-safe serialization via serde

#### rivets
- Initial release of the rivets issue tracking system
- CLI for managing issues, dependencies, and workflows
- JSONL-based storage backend
- Dependency graph with cycle detection
- Support for epics, tasks, bugs, features, and chores
- Priority and status management
- Label support

#### rivets-mcp
- Initial release of the MCP server
- Model Context Protocol integration for AI assistants
- Full issue CRUD operations via MCP tools
- Dependency management tools
- Statistics and reporting tools

[Unreleased]: https://github.com/dwalleck/rivets/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/dwalleck/rivets/releases/tag/v0.1.0
