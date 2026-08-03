# Related issues

Tracker search on 2026-08-02 for `McpIssue`, `McpNote`, `golden contract`, `wire vocabulary`, and CLI/MCP serialization found:

- `rivets-habf` — target issue.
- `rivets-bkjj` — closed prerequisite; moved the four enum vocabularies to domain `ValueEnum`/`FromStr` implementations and removed MCP enum tables.
- `rivets-wb0q` — parent record-evolution specification; the current domain `Issue`/`Note`/`AssociatedResource` model comes from this work.
- ADR-0004 (`docs/adr/0004-one-wire-vocabulary.md`) — governing decision: CLI JSON and MCP serialize the domain `Issue` directly, with RFC-3339 timestamps normalized to `Z`.

No separate open issue duplicates the target's output-mirror deletion or golden-contract work.
