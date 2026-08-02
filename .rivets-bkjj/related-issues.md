# Related issues and prior art — rivets-bkjj

Probe date: 2026-08-02. Tracker surveyed via `rivets list` (full dump).

## Prior art

- **ADR-0004** (`docs/adr/0004-one-wire-vocabulary.md`) — governing decision:
  each domain enum's declaration carries its complete string vocabulary
  (serde attributes, `Display`, `FromStr`, CLI value names) in one place;
  adapters must not define parallel output mirrors. This issue implements
  that decision for `IssueKind`, `IssueStatus`, `ResourceRole`,
  `DependencyType`. Created 2026-08-02 from the grilled design session.
- **rivets-habf** (open; **blocked by this issue**) — "Delete McpIssue: MCP serializes the
domain Issue, pinned by a golden contract test". Its blocker note: "FromStr and
Display must exist before the models.rs string tables can be deleted." So the
`McpIssue`/`McpDependency` mirror structs and their `From` impls survive this
issue (habf deletes them later); this issue deletes only the four-enum
string tables (`status_to_str`, `issue_kind_to_str`, `dep_type_to_str`,
`parse_status`, `parse_dep_type`) and the `mcp_issue_kinds!` macro, and
swaps `McpIssue::from`/`McpDependency::from` onto `Display`.
- **rivets-yx1h** (closed, merged PR #86) — "Add and list typed Web Associated
Resources"; delivered typed resources, not the MCP mirror deletion.
- No open or closed issue describes the Arg-mirror duplication in
  `cli/types.rs` beyond this issue.

## Deliberate behavior changes surfaced by probing

- MCP currently accepts **case-insensitive** enum strings and lenient
  aliases (`in-progress`, `parent_child`, `discovered_from`), pinned by
  `mcp_kind_input_remains_case_insensitive` and the `test_parse_status` /
  `test_parse_dep_type` rstest cases in `rivets-mcp/src/models.rs`.
  Post-change, MCP parses via domain `FromStr` and accepts only canonical
  strings. This is the issue's decided scope ("MCP parses via FromStr";
  "rivets-mcp must not keep parallel to-str or parse functions"); the
  CLI's accepted set (including the `in-progress` alias) is unchanged.
