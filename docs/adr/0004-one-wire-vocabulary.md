# One wire vocabulary for Issue records

MCP tool responses and CLI `--json` output serialize the domain `Issue` directly; the domain types' serde derives are the single source of the external JSON shape, and each domain enum's declaration carries its complete string vocabulary (serde attributes, `Display`, `FromStr`, CLI value names) in one place. We chose this over per-adapter DTO mirrors (`McpIssue` and companions) because the mirrors duplicated every field and enum string table across three crates while producing an almost-identical shape, and the one real divergence — timestamp offsets, `+00:00` versus `Z` — was accidental, not designed. Timestamps normalize to the RFC-3339 `Z`-suffix form.

## Consequences

A golden contract test in `rivets-mcp` pins the serialized shape of a fully-populated Issue; changing a domain serde attribute fails that test rather than silently altering the external contract, and a companion assertion holds CLI `--json` and MCP output to the identical shape. Adapters may define wire-only input types (params structs) but not output mirrors of domain records. `next_resource_id` stays serde-skipped: the persistence record (`IssueRecord`) owns its serialized form and remains the only sanctioned parallel representation of an Issue, as the legacy-migration adapter established by ADR-0003.
