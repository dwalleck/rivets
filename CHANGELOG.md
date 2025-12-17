# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
