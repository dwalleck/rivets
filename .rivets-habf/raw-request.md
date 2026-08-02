# Raw request

Source: tracker issue `rivets-habf`, requested by the `/ship rivets-habf` command.

> Delete McpIssue: MCP serializes the domain Issue, pinned by a golden contract test
>
> ## What to build
>
> McpIssue and its companions (McpNote, McpResource, McpResourceTarget, McpDependency) mirror the domain types field-for-field through five hand-written From impls totaling about 37 field assignments. Their only unique output behavior is RFC-3339 timestamp strings and lowercase enum strings — both already produced by the domain types' own serde derives: next_resource_id is serde-skipped, and ResourceTarget already carries the tagged snake_case representation the MCP mirror copies. Delete the mirrors and serialize the domain types directly in tool responses. The one byte-level wire change is timestamp offsets: +00:00 becomes Z (same RFC-3339 instant), accepted in ADR-0004.
>
> Add a golden contract test in rivets-mcp that serializes a fully-populated Issue (every optional field set, at least one note, web resource, path resource, and dependency) through the MCP path and asserts the exact JSON. Add a companion assertion that CLI --json and MCP output produce the identical shape for the same Issue — the ADR-0004 invariant.
>
> ## Placement / boundaries
>
> - Owner: rivets::domain serde derives own the wire shape (ADR-0004); rivets-mcp owns only the golden test and its input params types.
> - Seam: Content::json(issue) at the tool layer; no output DTOs.
> - Must not: rivets-mcp must not reintroduce output mirror types for domain records; a domain serde attribute change must fail the golden test rather than silently altering the MCP contract; MCP input params structs are out of scope and stay as they are.
>
> ## Acceptance criteria
>
> - [ ] McpIssue, McpNote, McpResource, McpResourceTarget, McpDependency and their From impls are deleted.
> - [ ] A golden contract test pins the full-field Issue JSON via the MCP path and fails on any shape change.
> - [ ] A test asserts CLI --json and MCP tool output produce the identical shape for the same Issue.
> - [ ] MCP integration tests pass with timestamps normalized to the Z suffix.
