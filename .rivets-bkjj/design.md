# Falsifiable design — rivets-bkjj: one enum vocabulary (ValueEnum + FromStr)

Implements the grilled-session decision recorded in the issue and ADR-0004:
each of the four domain enums (`IssueKind`, `IssueStatus`, `ResourceRole`,
`DependencyType`) carries serde attributes, `Display`, `FromStr`, and clap
`ValueEnum` value names in its declaration. The CLI Arg mirrors and the MCP
hand-written tables are deleted.

## Input shapes

For each of the four enums (IssueStatus 4, IssueKind 5, ResourceRole 5,
DependencyType 4 variants — all production-reachable; `Blocked` is a legacy
persisted status):

- **CLI value input**: canonical name → variant; `in-progress` alias →
  `InProgress`; any other string → clap error (exit 2). 19 accepted strings
  total (probe-verified baseline).
- **FromStr input**: canonical `Display` string → variant; empty, uppercase,
  `in-progress`, `parent_child`, `discovered_from` → typed error.
- **MCP input** (after change): canonical string → variant via `FromStr`
  (status/dep_type/role) or serde derive (kind); everything else →
  `Error::InvalidArgument` (status/dep_type/role) / serde unknown-variant
  (kind).
- **Persisted/wire input**: serde rename attributes unchanged, so JSONL
  records and JSON output shapes are untouched.

Out of scope (one-sentence justification each): `SortOrderArg`/`SortPolicyArg`
mirrors (no serde-wired domain twins for them; the issue scopes to four
enums); `McpIssue`/`McpDependency` output mirror structs (owned by
rivets-habf, which this issue unblocks); `IssueStatus::Blocked` variant
(legacy persistence, not a vocabulary question).

## Subtractive sweep

Core move: removes the MCP leniency (case-folded + alias spellings) and the
Arg mirror types.

- Constraint removed: "MCP enum parsing accepts any case and three extra
  aliases (`in-progress`, `parent_child`, `discovered_from`)". Chain:
  MCP tool inputs pass through `validate_status`/`validate_dep_type`/kind
  deserialization → lenient spellings were accepted → after the change they
  error. No internal state or ordering depended on the leniency; the only
  consumer is external MCP callers, and the issue explicitly decides
  canonical-only. Covered by claim C5.
- Constraint removed: the Arg mirror types exist as a translation layer.
  All consumers are CLI-internal (`args.rs`, `execute.rs`, `cli/mod.rs`
  tests); removal is compile-enforced (claim C6).
- Nothing else: no lock, ordering, or uniqueness property is relaxed.

## Placement

- **Owner**: `rivets::domain` — the four enum declarations in
  `domain/mod.rs` + `domain/resource.rs` gain `ValueEnum` derive (and
  `FromStr` + a small typed error enum for the three that lack it;
  `ResourceRole` already has `FromStr` via `ResourceError`).
- **Seam**: no new seam. CLI args consume the domain enums directly
  (existing `#[arg(value_enum)]` on the args fields, types swapped);
  MCP parses via `FromStr` in the existing `validate_*` helpers
  (same `Error::InvalidArgument` shape as `validate_resource_role` already
  uses today).
- **Forbidden**: no Arg mirror may reappear in `cli/types.rs`; no
  to-str/parse helper may reappear in `rivets-mcp`; no new dependency edge
  (clap is already a lib dep of `rivets`; `rivets-mcp` must not gain clap —
  it calls `FromStr`/`Display` only).

## Falsification

Cheapest falsifier ran **2026-08-02**: `probe/src/bin/future.rs` — copies of
the four enums with the exact proposed attribute shapes, enumerated through
clap's real derive machinery, compared to `baseline-cli-contract.txt`
(recorded from the live Arg mirrors). **Passed**: all 19 CLI lines identical,
including the `in-progress` alias and kebab-case names.

| # | Claim | Falsifier | Oracle | Cost | Status | Regression fence |
|---|-------|-----------|--------|------|--------|------------------|
| 1 | CLI accepted set is unchanged: domain `ValueEnum` derives accept exactly the 19 baseline strings mapping to the same variants. | Probe v2 enumerates domain `ValueEnum` names+aliases; diff vs `baseline-cli-contract.txt` `[cli]` lines. | Baseline file (recorded pre-change from live Arg mirrors). | 5m (scratch already passed) | passed (scratch) | domain test: per-variant `to_possible_value().get_name() == Display`, `InProgress` alias `in-progress`; existing `test_parse_list_status_in_progress_alias`, `test_cli_list_status_filter_parsing` |
| 2 | `FromStr` roundtrips: `parse(display(variant)) == variant` for all 18 variants; empty/uppercase/lenient spellings → typed error. | Unit tests iterating every variant + negative cases. | Display impl (independent hand-written path). | 5m | pending | the roundtrip + negative tests |
| 3 | CLI invalid-value error shape unchanged: exit 2 + clap `invalid value ... [possible values: ...]` for status/kind/dep_type/role. | Run the 5 baseline commands after the change; diff stderr + exit codes vs `cli-invalid-baseline.txt`. | `cli-invalid-baseline.txt` (recorded 2026-08-02 from current binary). | 10m | pending | integration test asserting exit 2 + "invalid value" for `list --status bogus` (add if absent) |
| 4 | MCP invalid-string error shapes unchanged: status/dep_type/role → `Error::InvalidArgument` with same field/value/valid_values; kind → serde unknown-variant error. | MCP unit tests calling `validate_status`/`validate_dep_type`/kind deserialization with `bogus`; assert variant + fields. | Current behavior (valid_values literals pinned in source). | 10m | pending | the new unit tests |
| 5 | MCP accepted set narrows to canonical-only: uppercase, `in-progress`, `parent_child`, `discovered_from` rejected (deliberate change). | Rewritten tests: `mcp_kind_input_remains_case_insensitive` becomes rejection test; `test_parse_status`/`test_parse_dep_type` replaced by canonical-only + rejection cases. | Design decision (probe: 39 accepted today; 19 canonical after). | 5m | pending | the rewritten tests |
| 6 | All duplicate tables deleted: 4 Arg mirrors + 7 `From` impls from `cli/types.rs`; `McpIssueKind` + `mcp_issue_kinds!` macro + `From<McpIssueKind>` + `status_to_str`/`issue_kind_to_str`/`dep_type_to_str`/`parse_status`/`parse_dep_type` from `models.rs`; no hand-written enum-string match remains outside domain declarations (error-message `valid_values` literals in `tools.rs` stay per AC 4). | `grep` for deleted names and `"in_progress"`/`"parent-child"` literals in `cli/` + `rivets-mcp/` (excluding tests and the two `valid_values` literals) → zero hits; workspace compiles. | grep (independent of implementation). | 5m | pending | compile + pre-PR review grep (mechanical) |
| 7 | Wire and persisted shapes unchanged: `IssueKindInput` switches to domain `IssueKind` with identical lowercase JSON; serde attrs untouched; golden/params/integration tests pass. | Existing MCP params tests + full test suite. | Existing tests. | 15m | pending | existing tests (`ready_params_read_legacy_issue_type`, `create_params_read_legacy_issue_type`, `conflicting_mcp_kind_fields_use_canonical_kind`, MCP integration) |
| 8 | No new dependency edges: `rivets` gains no new dep (clap already lib-level); `rivets-mcp` gains no clap. | `cargo tree -p rivets-mcp` and `-p rivets` diffed before/after; `grep clap crates/rivets-mcp/Cargo.toml`. | Cargo manifests. | 5m | pending | cargo tree check at checkpointed-build (mechanical) |

## Negative space

1. **No MCP leniency preservation** — `BUG`, `in-progress`, `parent_child`,
   `discovered_from` stop parsing in MCP. Deliberate; the issue decides
   canonical-only (`FromStr` accepts Display strings).
2. **No new seam or trait** — CLI uses domain enums directly; no
   `From<XArg>` translation layer survives.
3. **`SortPolicyArg`/`SortOrderArg` stay** — they mirror non-serde domain
   types; out of the issue's four-enum scope.
4. **`McpIssue`/`McpDependency` output structs stay** — rivets-habf (open,
   blocked by this issue) deletes them; this issue only swaps their string
   conversions onto `Display`.
5. **`valid_values` literals stay** — error-message text, not parse tables;
   AC 4 pins the error shape. No golden contract test here (habf's scope).
   Implementation note (post-approval): `McpIssueKind` is replaced by a
   local **schema-only** mirror `McpIssueKindSchema` (derive-only, no
   strings/parsing) because schemars cannot impl `JsonSchema` for the
   foreign domain type (orphan rule) and adding schemars to rivets would be
   a new dependency edge; a fence test pins its rendered values to the
   domain Display strings.
6. **No `IssueStatus::Blocked` removal** — legacy persisted variant;
   vocabulary change only.

## Tracker references (all verified)

- ADR-0004 — governing decision (read; `docs/adr/0004-one-wire-vocabulary.md`).
- rivets-habf — downstream deletion of `McpIssue`; open, blocked by this
  issue (verified via `rivets show rivets-habf`).
- No other deferrals; no "follow-up" language used.
