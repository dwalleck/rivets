# Rivets Terminology Reference

Use these terms consistently across all tasks and documentation:

## Storage Layers

- **Phase 1 (MVP)**: In-memory storage with JSONL persistence
  - "in-memory storage" (not "database" or "SQLite")
  - "JSONL files" for persistence
  - "petgraph" for graph operations

- **Phase 2**: Configuration and backend selection
  - "storage backend configuration"
  - "backend factory pattern"

- **Phase 3 (Future)**: PostgreSQL for multi-user
  - "PostgreSQL backend" (production)
  - "recursive CTEs" for complex queries
  - "connection pooling"

## Data Structures

- **Issue**: Core entity representing a task, bug, feature, epic, or chore
- **Dependency**: Directed edge between issues (blocks, related, parent-child, discovered-from)
- **IssueId**: Hash-based identifier (format: {prefix}-{hash})
- **IssueFilter**: Query parameters for listing issues

## Operations

- **CRUD**: Create, Read, Update, Delete (basic operations)
- **Ready work**: Issues with no blocking dependencies
- **Blocked issues**: Issues with unresolved blocking dependencies
- **Cycle detection**: Preventing circular dependencies

## File Formats

- **JSONL**: JSON Lines (newline-delimited JSON)
- **config.yaml**: YAML configuration file

## Dependency Types

- **blocks**: Hard blocker that prevents work on dependent issue
- **related**: Soft link for informational purposes
- **parent-child**: Hierarchical relationship (epics to tasks)
- **discovered-from**: Dependencies found during implementation

## Development Phases

- **Phase 1 (MVP)**: In-memory + JSONL + petgraph
- **Phase 2**: Configuration system and backend selection
- **Phase 3**: PostgreSQL backend for production multi-user scenarios
