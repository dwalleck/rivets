# Implementation Vocabulary

[`CONTEXT.md`](../CONTEXT.md) is the canonical domain glossary. This document is limited to implementation vocabulary and must not redefine Issue, Workflow State, Ready, Issue Relationship, or other domain terms.

## Storage

**Rivets metadata directory**:
The `.rivets/` directory that marks a Workspace root and contains its configuration and persisted issue data.

**Issue store**:
The configured persistent representation of a Workspace's Issues. JSONL is the current default representation.

**In-memory storage**:
The runtime representation used for issue lookup and relationship queries. Do not call it a database.

**Storage backend**:
An adapter that loads, mutates, queries, and persists Issues without changing their domain meaning.

## Interfaces

**CLI**:
The human- and script-facing command-line interface to Rivets.

**MCP server**:
The agent-facing interface that exposes Rivets operations through the Model Context Protocol.
