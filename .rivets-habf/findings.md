# Probe findings

Date: 2026-08-02

## Probe

`.rivets-habf/probe.py` selected the first real workspace Issue with non-empty `design`, `acceptance_criteria`, `notes`, and `dependencies` (`rivets-fk9`). It:

1. Ran the real CLI JSON path (`cargo run -q -p rivets -- list --json -n 50 --sort oldest`).
2. Read the corresponding persisted JSONL record independently with Python.
3. Started the real `rivets-mcp` stdio server, initialized MCP 2025-06-18, and called `show` for the selected Issue.
4. Compared the MCP payload directly to the independent JSONL record and rejected non-canonical `+00:00` timestamps.

## Oracle

The independent oracle is the persisted `.rivets/issues.jsonl` record for the selected Issue. It does not call domain or MCP serialization code.

## Observed output

```text
id=rivets-fk9
keys=acceptance_criteria, assignee, closed_at, created_at, dependencies, description, design, id, issue_kind, labels, notes, priority, resources, status, title, updated_at
notes=1
dependencies=1
canonical_cli_matches_oracle=true
mcp_matches_oracle=true
```

The CLI domain serializer, MCP domain serializer, and JSONL oracle agree exactly. The direct MCP payload uses RFC-3339 `Z` timestamps for Issue and Note values.

The Slice 3 golden/parity fixture additionally confirmed that persisted dependency arrays are sorted lexicographically by dependent ID, while an in-memory mutation result retains insertion order; the parity fence therefore compares CLI output with a freshly loaded MCP `show` value for the same persisted Issue.

## What I learned

The MCP output mirrors were semantically redundant. Direct domain serialization preserves the complete Issue shape and canonical timestamp spelling; persistence reload is the seam that makes deterministic dependency ordering observable across CLI and MCP reads.

