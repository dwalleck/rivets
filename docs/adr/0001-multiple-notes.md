# Notes as Chronological Log

Issues track work history via an append-only log of timestamped notes, replacing the previous single `notes: Option<String>` field. Each note is immutable after creation, carrying only `content` and `created_at`.

## Why

The original `notes` field was a single optional string. Users could only replace the entire field, losing history. The new model preserves a complete chronological record of work done on an issue — "what happened during the work" — without requiring users to manually maintain history in a text blob.

Key decisions:
- **Append-only**: Notes cannot be edited or deleted through the CLI. The JSONL file is the audit trail; `git` provides recovery if needed.
- **Minimal metadata**: No `author` field. Git already tracks who changed what. Duplicating author would drift from reality.
- **Migration**: Existing single notes become the first entry in the new `Vec<Note>`, timestamped at the issue's `updated_at`.
- **CLI semantics**: `rivets update <id> --notes "text"` appends rather than replaces.
- **MCP tooling**: Dedicated `add_note` tool for AI assistants, separate from `update_issue`.

## Rejected Alternatives

- **Mutable notes** — Would require note IDs, edit commands, conflict resolution. Adds complexity without clear benefit.
- **Author metadata** — Redundant with git history. Can drift out of sync.
- **Limits on count/length** — Creates edge cases (what happens at limit?) without meaningful benefit for typical usage.
