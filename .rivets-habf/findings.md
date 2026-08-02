# Probe findings

Date: 2026-08-02

## Probe

`.rivets-habf/probe.py` selected the first real workspace Issue with non-empty `design`, `acceptance_criteria`, `notes`, and `dependencies` (`rivets-fk9`). It:

1. Ran the real CLI JSON path (`cargo run -q -p rivets -- list --json -n 50 --sort oldest`).
2. Read the corresponding persisted JSONL record independently with Python.
3. Started the real `rivets-mcp` stdio server, initialized MCP 2025-06-18, and called `show` for the selected Issue.
4. Compared the MCP payload to the independent JSONL record after normalizing only `+00:00` to `Z`.

## Oracle

The independent oracle is the persisted `.rivets/issues.jsonl` record for the selected Issue. It does not call domain or MCP serialization code.

## Observed output

```text
id=rivets-fk9
keys=acceptance_criteria, assignee, closed_at, created_at, dependencies, description, design, id, issue_kind, labels, notes, priority, resources, status, title, updated_at
notes=1
dependencies=1
canonical_cli_matches_oracle=true
mcp_matches_after_utc_normalization=true
```

The CLI domain serializer and JSONL oracle agree exactly. The existing MCP mirror emits the same decoded object except UTC timestamps use `+00:00` instead of `Z` (observed on Issue timestamps and Note timestamps). No other field difference was observed.

## What I learned

The domain/CLI serializer already produces the complete Issue shape expected by the independent JSONL oracle; the MCP mirror is not adding field semantics, and its only observed divergence is the known UTC timestamp suffix.
