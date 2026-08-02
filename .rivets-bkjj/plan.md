# Budgeted plan — rivets-bkjj: one enum vocabulary (ValueEnum + FromStr)

Design: `.rivets-bkjj/design.md` (approved 2026-08-02, 8 claims C1–C8).
Oracle: `.rivets-bkjj/baseline-cli-contract.txt` (19 `[cli]` lines, recorded
from the live Arg mirrors) and `.rivets-bkjj/cli-invalid-baseline.txt`
(5 commands, exit codes + stderr shape). Probe v2 = `probe/src/bin/after.rs`
(enumerates the **real** domain `ValueEnum` derives; must reproduce the
baseline `[cli]` lines exactly). `probe/src/bin/future.rs` was the
pre-implementation scratch falsifier (already passed).

Plan unit = slice. One commit per slice; all gates per slice.

---

## Slice 1: Domain vocabulary — ValueEnum + FromStr on the four enums

**Claim:** C1 (the `ValueEnum` derives exist and accept the 19 baseline
strings), C2 (`FromStr` roundtrips all 18 variants; typed errors for
non-canonical input).

**Oracle:** `probe/src/bin/after.rs` — enumerate real domain `ValueEnum`
names+aliases, diff vs `baseline-cli-contract.txt` `[cli]` lines (must be
identical). Independent: recorded pre-change from the Arg mirrors.

**Stress fixture:** every variant of all four enums (18) roundtrips
`parse(display) == variant`; adversarial strings rejected with the typed
error — `""`, `"OPEN"`, `"in-progress"`, `"parent_child"`,
`"discovered_from"`, `"bogus"`. Bug classes: Display/FromStr divergence
(typo), missing `in-progress` alias, case-insensitive catch-all in FromStr,
`_ =>` catch-all masking a variant.

**Loop budget:** test loops O(variants) with variants ≤ 5 per enum; no
production loops added.

**Files:**
- `crates/rivets/src/domain/mod.rs` — `pub use clap::ValueEnum;` (derive in
  scope for this module + downstream re-export for the MCP schema bridge);
  `use std::str::FromStr;`; `ValueEnum` in the derives of `IssueStatus`
  (plus `#[value(name = "in_progress", alias = "in-progress")]` on
  `InProgress`, alongside its serde rename), `IssueKind`, `DependencyType`;
  new `FromStr` impls + `IssueStatusError`/`IssueKindError`/
  `DependencyTypeError` (one `Unknown* { value }` variant each, thiserror,
  `PartialEq Eq`, doc comment + `# Errors`); roundtrip/negative/value-name
  tests in `mod.rs` tests.
- `crates/rivets/src/domain/resource.rs` — `use clap::ValueEnum;`;
  `ValueEnum` added to `ResourceRole` derive (its `FromStr` already exists);
  roundtrip + value-name tests beside the existing role tests.

**Code (advisory):**
```rust
/// A failure to parse an [`IssueStatus`] from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IssueStatusError {
    /// The string was not a canonical Issue Status name.
    #[error("Unknown issue status '{status}'")]
    UnknownStatus { status: String },
}

impl FromStr for IssueStatus {
    type Err = IssueStatusError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "in_progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "closed" => Ok(Self::Closed),
            _ => Err(IssueStatusError::UnknownStatus { status: s.to_string() }),
        }
    }
}
```
(exhaustive matches, no `_ =>` catch-alls that swallow variants; same shape
for `IssueKindError::UnknownKind`, `DependencyTypeError::UnknownDependencyType`.)

**Verification:**
- [ ] `cargo test -p rivets` — domain tests pass (roundtrip ×18, negatives, value-name==display, alias)
- [ ] `after.rs` output == `baseline-cli-contract.txt` `[cli]` lines
- [ ] clippy + fmt clean (pre-commit hook)

## Slice 2: CLI — delete Arg mirrors, consume domain enums directly

**Claim:** C1 end-to-end (CLI binary accepts the same 19 strings), C3 (CLI
invalid-value error shape unchanged), C6-rivets (mirrors + From impls gone).

**Oracle:** `after.rs` (unchanged) + `cli-invalid-baseline.txt` diff of the
5 invalid commands.

**Stress fixture:** the 5 baseline invalid commands must produce identical
exit codes + stderr; `rivets list --status in-progress` must still parse
(exit 0); `rivets create --kind task` and `rivets dep add --type blocks`
defaults still work. Bug classes: error-shape change (custom value_parser),
dropped alias, `default_value` string no longer matching a value name.

**Loop budget:** none (clap's own parsing).

**Files (4 — single mechanical wave; atomic because removing the types
breaks compilation until all call sites move):**
- `crates/rivets/src/cli/types.rs` — delete `IssueKindArg`, `IssueStatusArg`,
  `ResourceRoleArg`, `DependencyTypeArg`, their `Display` impls, the 7 `From`
  impls, and their tests. Keep `SortOrderArg`, `SortPolicyArg`,
  `BatchResult`, `BatchError`.
- `crates/rivets/src/cli/args.rs` — field types `IssueKindArg` →
  `IssueKind` etc.; imports from `crate::domain`; update tests.
- `crates/rivets/src/cli/execute.rs` — `dep_type: DependencyTypeArg` →
  `DependencyType` (import from `crate::domain`), drop `dep_type.into()`.
- `crates/rivets/src/cli/mod.rs` — re-export list drops the four Arg types;
  tests updated to domain variants.

**Verification:**
- [ ] `cargo test -p rivets` + `cargo test --test cli_tests` pass
- [ ] 5 invalid-command diffs identical to `cli-invalid-baseline.txt`
- [ ] alias + defaults smoke (commands above exit 0)
- [ ] `after.rs` still equals baseline
- [ ] new fence: `cli_tests.rs` integration test asserting exit 2 + "invalid value" for `list --status bogus` (no such test exists today)

## Slice 3: MCP — delete string tables, parse via FromStr

**Claim:** C4 (MCP invalid-string error shapes unchanged), C5 (MCP accepted
set is canonical-only; lenient spellings rejected), C6-mcp (no to-str/parse
tables or macro in rivets-mcp), C7 (wire JSON unchanged).

**Oracle:** the rewritten MCP tests (canonical accepted, lenient rejected,
same `Error::InvalidArgument` shape) + existing params/integration tests
(legacy `issue_type`, golden shape).

**Stress fixture:** MCP tool calls with canonical (`open`, `bug`, `blocks`,
`implementation`), lenient (`OPEN`, `BUG`, `in-progress`, `parent_child`),
and invalid (`bogus`) values for status/dep_type/kind/role; assert accepted
set and error variants. Bug classes: leftover lenient fallback, wrong error
variant (e.g. leaking the domain error instead of `InvalidArgument`), serde
shape drift (kind no longer lowercase), schema drift.

**Loop budget:** `JsonSchema` build O(variants ≤ 5), cold path only.

**Files:**
- `crates/rivets-mcp/src/models.rs` — delete `McpIssueKind` +
  `mcp_issue_kinds!` macro + `From<McpIssueKind>`; `IssueKindInput` fields
  become `Option<IssueKind>`; implement `JsonSchema for IssueKind` here
  (manual impl building `enum_values` from `IssueKind::value_variants()`
  names via the `rivets::domain::ValueEnum` re-export — runtime-derived, no
  second string table, no clap dep in rivets-mcp); delete `status_to_str`,
  `issue_kind_to_str`, `dep_type_to_str`, `parse_status`, `parse_dep_type`;
  `McpIssue::from` / `McpDependency::from` use `Display` (`.to_string()`);
  rewrite tests: drop `mcp_kind_input_remains_case_insensitive` (replaced
  by canonical-accepted + "BUG" rejected), drop the `test_parse_status` /
  `test_parse_dep_type` rstest tables, keep legacy `issue_type` tests; add
  schema fence test (schema enum values == domain Display names).
- `crates/rivets-mcp/src/tools.rs` — `validate_status`/`validate_dep_type`
  use `status.parse()` / `dep_type.parse()` mapped to the existing
  `Error::InvalidArgument` shapes (same as `validate_resource_role` already
  does); drop the three helper imports; `dep_type_to_str` usage → Display;
  test helper `kind_input` → `IssueKind`.

**Code (advisory):**
```rust
fn validate_status(status: &str) -> Result<IssueStatus> {
    status.parse().map_err(|_| Error::InvalidArgument {
        field: "status",
        value: status.to_string(),
        valid_values: "open, in_progress, blocked, closed",
    })
}
```

**Verification:**
- [ ] `cargo test -p rivets-mcp` (unit + integration) passes
- [ ] canonical accepted / lenient rejected per the rewritten tests
- [ ] schema fence test passes (enum values == Display)
- [ ] `cargo tree -p rivets-mcp` shows no clap (C8 partial)

## Slice 4: Final integration — all oracles, fences, full gates

**Claim:** C8 (no new dependency edges) + every fence from the design table.

**Verification:**
- [ ] `after.rs` == `baseline-cli-contract.txt` (C1 oracle)
- [ ] 5 CLI invalid-command diffs identical (C3)
- [ ] full suite: `cargo nextest run` (all 1049+ tests), clippy
  `--all-targets --all-features -- -D warnings`, `cargo fmt --check`,
  doctests
- [ ] `cargo tree -p rivets -p rivets-mcp` diff: no new edges (clap already
  under rivets; absent from rivets-mcp) (C8)
- [ ] grep sweep: deleted names (`IssueKindArg`, `IssueStatusArg`,
  `ResourceRoleArg`, `DependencyTypeArg`, `McpIssueKind`, `parse_status`,
  `parse_dep_type`, `status_to_str`, `issue_kind_to_str`,
  `dep_type_to_str`) and `"in_progress"`/`"parent-child"` literals absent
  from `crates/rivets/src/cli/` + `crates/rivets-mcp/src/` except the two
  `valid_values` error-message literals in `tools.rs` (C6)
- [ ] probe crate itself still builds (after.rs + future.rs compile)

## Plan Self-Review

1. **Loops:** Slice 1 test loops O(≤5); Slice 3 schema build O(≤5) cold
   path; no always-on loops introduced anywhere. Budgets stated per slice.
2. **Fixtures:** every logic slice names a bug class its fixture fails
   under (listed per slice above).
3. **Doc-comment preconditions:** new `FromStr` impls carry `# Errors`
   sections; error enums documented; `JsonSchema` impl documented. No
   load-bearing preconditions needing runtime checks.
4. **Write targets:** no new stdout/stderr writers (tests only).
5. **Tracker references:** no deferrals; rivets-habf (verified open, blocked
   by this issue) owns `McpIssue` deletion; ADR-0004 governs. Nothing new
   to file.
