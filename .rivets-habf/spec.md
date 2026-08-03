# Feature: Serialize domain Issues directly from MCP

## What this is

The `rivets-mcp` adapter will serialize the canonical `rivets::domain::Issue` value in tool responses instead of converting it through output mirror structs. The domain serde representation becomes the only MCP/CLI Issue-record shape; RFC-3339 UTC timestamps use the `Z` suffix.

## Users

- **MCP integration consumer**: Reads Issue records from tool responses and needs the documented domain JSON shape without adapter-only field or enum transformations.
- **CLI JSON consumer**: Reads `--json` Issue records and needs the same decoded record shape as the MCP response for the same Issue.
- **Rivets maintainer**: Changes domain serde attributes and needs a deterministic golden test to expose external wire-shape changes.

## Behavior

### Direct MCP Issue serialization
- **Given**: An Issue with every optional field populated, at least one Note, one Web resource, one Workspace Path resource, and one dependency.
- **When**: An MCP tool returns that Issue through its JSON content seam.
- **Then**: The response payload is the domain Issue serde representation; the exact expected JSON key set, nested tags, enum strings, array order, and omission rules are pinned by a golden contract test.

### Empty optional fields
- **Given**: An Issue with optional fields absent and empty collections where the domain permits them.
- **When**: An MCP tool serializes the Issue.
- **Then**: The domain serde rules determine omission and empty-array behavior; no MCP output mirror inserts, removes, or renames fields. Existing integration fixtures remain valid.

### Timestamp normalization
- **Given**: An Issue containing UTC timestamps that would otherwise serialize with a `+00:00` offset.
- **When**: MCP serializes the Issue.
- **Then**: Each RFC-3339 UTC timestamp uses the `Z` suffix and denotes the same instant; non-UTC offsets remain semantically unchanged unless the domain serializer already normalizes them.

### Persistence-only field omission
- **Given**: An Issue whose runtime state contains `next_resource_id`.
- **When**: MCP serializes the Issue.
- **Then**: `next_resource_id` is absent from the JSON payload because the domain serde contract skips it.

### CLI/MCP shape parity
- **Given**: The same fully-populated Issue supplied to the CLI JSON output path and the MCP JSON output path.
- **When**: Both paths serialize the Issue and their JSON payloads are decoded.
- **Then**: The decoded objects are identical, including field names, nested structures, enum vocabulary, omission rules, array order, and normalized timestamp strings.

### Output mirror removal
- **Given**: The `rivets-mcp` crate is compiled and its integration tests are run.
- **When**: The source and test suite are inspected after the change.
- **Then**: `McpIssue`, `McpNote`, `McpResource`, `McpResourceTarget`, `McpDependency`, and their output conversion implementations no longer exist; MCP input parameter structs remain available.

## Success criteria

- **Mirror removal**: 5 named output mirror structs and their 5 conversion implementations are absent from `crates/rivets-mcp/src/models.rs`, measured by source search and successful `cargo test -p rivets-mcp`.
- **Golden contract**: 1 fully-populated Issue produces an exact JSON value matching the checked-in golden assertion, measured by a deterministic `rivets-mcp` test.
- **Shape parity**: 1 identical Issue yields equal decoded JSON values from CLI `--json` and MCP, measured by a deterministic test using `serde_json::Value` equality.
- **Timestamp contract**: 100% of UTC timestamps in the golden fixture use the `Z` suffix, measured by exact JSON assertions; the MCP integration test suite passes with no timestamp-offset failures.
- **Regression coverage**: 100% of existing `rivets-mcp` unit and integration tests pass, measured by `cargo test -p rivets-mcp`.

## Edge cases and decisions

| Edge | Decision | Rationale |
|---|---|---|
| Fully populated Issue | Include every reachable optional value in the golden fixture. | Exercises the complete domain serde shape and every nested output type. |
| Optional field absent | Preserve the domain serializer's existing omission behavior. | The adapter must not create a second null/omission policy. |
| Empty collections | Preserve the domain serializer's existing array behavior. | Collection semantics belong to the canonical domain representation. |
| UTC timestamp offset | Emit `Z` for the same instant. | ADR-0004 accepts this deliberate byte-level normalization. |
| Non-UTC timestamp offset | Do not add MCP-specific conversion. | Direct domain serialization must not alter unrelated timestamp semantics. |
| `next_resource_id` present at runtime | Omit it from MCP JSON. | It is persistence/runtime bookkeeping, not the external Issue record. |
| Web and Workspace Path resources | Include both in the golden fixture with their typed target tags. | Confirms the domain `ResourceTarget` representation is used unchanged. |
| Dependency present | Include at least one dependency in the golden fixture. | Confirms relationship serialization is not lost with mirror removal. |
| Array ordering | Preserve domain vector order exactly. | Ordering is observable and must not be reconstructed by the adapter. |
| Invalid MCP input parameters | Keep existing input parameter types and error behavior unchanged. | Input adapters are explicitly outside this change. |
| Concurrent serialization | Treat serialization as a read-only operation; no new synchronization is introduced. | The change has no mutable shared output state. |
| Partial tool failure | Preserve the existing MCP error path; no output is emitted for a failed tool operation. | This change only replaces successful Issue serialization. |
| Retried tool request | Serialization is deterministic and side-effect free. | Repeating a successful read must produce the same JSON for the same Issue. |
| Soft-deleted or closed Issue | Serialize according to the Issue's current domain state. | Workflow filtering is owned by storage/tool selection, not output conversion. |
| Workspace boundaries and permissions | Preserve existing storage/context authorization behavior. | No workspace lookup or permission seam changes. |
| Time zones and DST | Compare instants through the existing timestamp type; only UTC's suffix is pinned. | DST rules do not apply to UTC output and no new timezone conversion is introduced. |

## Out of scope

This change does NOT include:

- MCP input parameter redesign or new validation rules.
- Changes to Issue domain fields, storage records, or persistence migration behavior.
- Changes to CLI command selection, filtering, or non-JSON presentation.
- New MCP tools, resource roles, dependency semantics, or workflow transitions.
- General timestamp policy changes beyond the ADR-0004 UTC `Z` representation.

## Constraints

| Dimension | Limit | How measured |
|---|---|---|
| Output schema | Exactly the canonical domain serde shape | Golden JSON test |
| Adapter mirrors | 0 output mirror structs or conversions for Issue records | Source search plus compilation |
| CLI/MCP parity | 1.0 equality for decoded JSON values | Deterministic parity test |
| Timestamp format | 100% of UTC fixture timestamps end in `Z` | Golden JSON assertions |
| Existing behavior | 0 failures in `rivets-mcp` tests | `cargo test -p rivets-mcp` |

## Decisions log

| # | Question | Decision | Why |
|---|---|---|---|
| 1 | Which type owns the external Issue JSON shape? | The domain `Issue` and its serde derives. | ADR-0004 makes domain serialization the single wire vocabulary. |
| 2 | Should MCP retain output DTO mirrors? | No. Delete the five output mirror structs and their conversions. | They duplicate domain fields and enum transformations without unique behavior. |
| 3 | How should UTC timestamp bytes change? | `+00:00` becomes `Z` while preserving the instant. | ADR-0004 explicitly accepts this one wire change. |
| 4 | What test protects the complete shape? | One fully-populated golden contract test plus CLI/MCP decoded-value parity. | A domain serde change must fail deterministically at the adapter contract. |
| 5 | What remains adapter-owned? | MCP input parameter structs and existing tool error/context behavior. | The ticket changes successful Issue output only. |

## Sign-off

Agent's summary of the decisions:

> `rivets-mcp` will return canonical domain `Issue` JSON directly, with no `McpIssue`-family output mirrors. A fully-populated golden fixture will pin every field, nested resource/dependency shape, omission rule, array order, enum spelling, and UTC `Z` timestamps; a companion test will require decoded CLI `--json` and MCP payloads to be equal. MCP input params, storage/persistence, tool selection, and unrelated timestamp behavior stay unchanged.

The requester agreed: “Agree.”

Date: 2026-08-02
