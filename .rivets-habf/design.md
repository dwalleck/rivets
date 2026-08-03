# Design: Delete MCP Issue output mirrors

## Purpose

Make the canonical `rivets::domain::Issue` serde implementation the sole record wire shape used by MCP and CLI JSON output. Remove adapter conversions that copy Issue fields, stringify enums, and format timestamps independently. Preserve MCP input parameter schemas and tool error/context behavior.

The probe in `findings.md` establishes the premise: CLI domain JSON and the independent JSONL record are identical for a real populated Issue; the current MCP mirror differs only by UTC timestamp spelling (`+00:00` versus `Z`).

## Input shapes

The implementation and contract tests cover these reachable shapes:

- `IssueStatus`: `Open`, `InProgress`, `Blocked`, and `Closed`.
- `IssueKind`: `Bug`, `Feature`, `Task`, `Epic`, and `Chore`.
- Optional Issue fields: `assignee`, `design`, `acceptance_criteria`, and `closed_at`, each both present and absent; every combination is valid where workflow permits it.
- `priority`: boundary values `0` and `4`, plus an interior value.
- Collections `notes`, `resources`, and `dependencies`: empty, one item, multiple distinct items, and duplicate dependency attempts rejected at the storage seam. Resource duplicate target/role combinations are rejected by the domain invariant rather than serialized.
- `ResourceTarget`: `Web` and `Path`; targets include ordinary ASCII and validated path/URL values containing spaces or Unicode where constructors permit them.
- `ResourceRole`: `Implementation`, `Documentation`, `Evidence`, `Successor`, and `Reference`, with labels both present and absent.
- `DependencyType`: `Blocks`, `Related`, `ParentChild`, and `DiscoveredFrom`, including canonical kebab-case values.
- Timestamps: UTC values with nanosecond precision and `closed_at` absent/present. Runtime `next_resource_id` values are present internally but never appear in domain JSON.
- MCP request params: canonical fields, legacy `issue_type` compatibility fields, missing optional params, and invalid values. These are read by the existing input adapters and are not transformed by this change.

## Architecture and placement

| Capability | Owner and seam | Forbidden |
|---|---|---|
| Issue-record wire shape | `rivets::domain::Issue` and nested domain serde derives; the existing `serde_json::to_*` seam | No MCP field copy, enum string table, timestamp formatter, or output DTO may define a second record shape. |
| Successful MCP record responses | `rivets-mcp::tools::Tools` returns `Issue`/`Vec<Issue>`; `RivetsMcpServer` continues to call `Content::json` | Do not convert through `McpIssue` or any companion; do not add `JsonSchema`/`Deserialize` requirements to the domain record solely for MCP transport. |
| Blocked response envelope | `rivets-mcp::models::BlockedIssueResponse` remains an envelope, but its `issue` and `blockers` fields are domain `Issue` values and it only needs serialization | Do not copy Issue fields into a second envelope record type or retain mirror derives that require domain deserialization/schema. |
| MCP inputs and non-record responses | `rivets-mcp::models` retains parameter types, `IssueKindSchema`, context responses, and statistics responses | Do not alter input compatibility fields, validation errors, or unrelated response models. |
| Contract tests | `crates/rivets-mcp/tests/integration.rs` exercises the public `Tools` seam and the actual `Content::json` serialization seam | Do not assert implementation-private conversion details; assert decoded JSON shape, ordering, omission, and timestamp values. |

The direct `Issue: Serialize` implementation already satisfies `Content::json`; MCP tool methods are internally typed and the server methods return `CallToolResult`. The parity fence uses the writer-backed `rivets::output::print_issues_to` seam that the CLI list path delegates to, so it can compare CLI JSON without starting Cargo from a test.

## Removed-invariant sweep

The deleted conversion layer silently enforced several assumptions. The design preserves each as a claim:

1. Every domain Issue field, including notes, resources, dependencies, and optional values, reached MCP output once and under its canonical key.
2. Domain enum serde names matched the MCP strings, including tagged resource targets and kebab-case dependency kinds.
3. Timestamps represented the same instant; the deliberate external form is now UTC `Z`.
4. `next_resource_id` remained absent from record JSON.
5. MCP input parsing and error behavior did not depend on output mirror types.

## Claims and falsification

Each claim has a separate observable output and names a plausible buggy implementation that would make its falsifier fail.

| # | Claim | Falsifier | Independent oracle | Cost | Status | Regression fence |
|---|---|---|---|---:|---|---|
| 1 | A domain Issue with all fields populated serializes to the exact canonical record shape, including nested Notes, Web/Path resources, dependencies, omission rules, and array order. | Build one fully-populated Issue through the real storage/tool seam; compare the normalized JSON value to a hand-written golden object. Any missing/extra key, wrong nested tag, changed order, or unexpected null falsifies the claim. Bug caught: a direct serializer accidentally exposes a private bookkeeping field or a test fixture omits a mirror-only field. | The hand-written golden JSON and the persisted-record shape from `.rivets/issues.jsonl`; neither invokes MCP conversion code. | 10m | passed | `mcp_full_issue_json_golden` |
| 2 | Every MCP operation that returns an Issue returns the domain value without adapter field conversion. | Exercise `ready`, `list`, `show`, `create`, `update`, note/resource mutations, `close`, `reopen`, and dependency operations; compare each payload's keys and nested values to direct `serde_json::to_value(&Issue)`. Bug caught: one overlooked `Ok(issue.into())` or one collection path still maps through a mirror. | The domain serializer applied to the returned Issue, the direct return signatures, and the full MCP regression suite. | 15m | passed | `cargo nextest run -p rivets-mcp` and source return-type audit |
| 3 | Resource targets, roles, dependency kinds, and enum values use the domain serde vocabulary exactly. | Include both resource target variants, all roles, all dependency kinds, and boundary workflow/kind variants in a fixture; any wrong tag, case, underscore, or hyphen falsifies the claim. Bug caught: a surviving MCP string table or a wrong `serde` tag during conversion. | Explicit expected JSON constants derived from ADR-0004 and domain enum declarations, not MCP conversion helpers. | 10m | passed | `mcp_full_issue_json_golden` |
| 4 | Every UTC timestamp emitted through MCP uses the RFC-3339 `Z` suffix while preserving its instant, including Note and optional `closed_at` timestamps. | Serialize a populated open Issue with Notes and a closed Issue; fail if any timestamp ends in `+00:00`, lacks `Z`, or changes nanosecond text. Bug caught: retaining `.to_rfc3339()` in a mirror or applying a lossy string replacement to a non-UTC value. | Parse each output timestamp with the independent `DateTime` parser and compare instants to the source Issue timestamps. | 5m | passed | `mcp_timestamps_use_z_suffix` |
| 5 | CLI `--json` list output and MCP output contain identical decoded JSON for the same Issue. | Serialize one Issue through the CLI list writer and the MCP `show` result; remove only the CLI array wrapper and compare values. Any difference in keys, enum text, nested values, order, or timestamps falsifies the claim. Bug caught: CLI or MCP retains an adapter-only field or serializes a different nested shape. | The CLI writer emits bytes into an in-memory buffer; the MCP value passes through the actual `Content::json` seam; comparison is performed on independent `serde_json::Value` values. | 15m | passed | `cli_and_mcp_issue_json_shapes_match` |
| 6 | Existing MCP input schemas, legacy `issue_type` compatibility, invalid-value errors, and workspace/storage behavior remain unchanged. | Run the existing input matrix and error cases after output return types change; any changed accepted value or error shape falsifies the claim. Bug caught: deleting models removes an input type or a refactor changes input parsing while editing shared imports. | Existing integration tests and `serde_json` input fixtures, independent of output JSON assertions. | 10m | passed | `cargo nextest run -p rivets-mcp` |
| 7 | No MCP output mirror of an Issue record remains; only input types and non-record envelopes remain in `models.rs`. | Search the final `models.rs` for the five deleted type names and conversion impls; any match falsifies the claim. Bug caught: an unused `McpResource` or `From<Issue>` survives because behavioral tests do not reach it. | Source-level architectural audit; the oracle is the repository text, not the type under test. | 2m | passed | Final source audit: no deleted mirror type or conversion implementation |

### Cheapest falsifier

Claim 1's premise survived the first falsifier before this design was written: `.rivets-habf/probe.py` compared the real CLI and MCP serializers for `rivets-fk9` against the independent JSONL record and reported `canonical_cli_matches_oracle=true; mcp_matches_oracle=true`. The Slice 3 golden, timestamp, and parity fences then passed against a fully populated Issue.

## Negative space

This design deliberately does not:

- Change the domain Issue fields, persistence `IssueRecord`, JSONL migration rules, or resource identity invariants.
- Redesign MCP input params, legacy compatibility fields, validation messages, or tool selection/filter semantics.
- Add a new MCP response envelope for ordinary Issue records or expose `next_resource_id`.
- Change CLI text output, `show` dependency-detail composition, workflow transitions, or storage concurrency.
- Normalize non-UTC timestamp offsets or introduce a general timezone policy.

## Verification contract

The implementation is accepted only when the golden, timestamp, parity, and existing input/error fences pass, the probe/oracle still agree, and the source audit finds none of the five output mirror names or conversion implementations. The manual source audit is an explicit merge-gate decision, not an inferred property of compilation.
