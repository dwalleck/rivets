# Review decisions — round 1 (post-PR two-axis review)

Two-axis review (`/code-review xhigh PR 92`) after the PR was opened. Standards
axis: 7 findings (2 hard, 5 judgement). Spec axis: no missing requirements; 2
design-sanctioned scope notes and 1 coverage nuance. Decisions below.

## Standards findings

1. **Bare `.unwrap()` in new test code** (CLAUDE.md Test Design Patterns) —
   **Accepted, fixed.** The four new `WorkspacePath::new(...).unwrap()` sites in
   `integration.rs` now use `.expect(...)`, as do the touched carried-forward
   sites (`closed_at.unwrap()` in integration.rs; the reworked call sites in the
   `tools.rs` test module). Untouched pre-existing `.unwrap()` elsewhere is out
   of scope for this PR.
2. **Stale module doc in `models.rs`** ("wrap or transform rivets domain
   types") — **Accepted, fixed.** Reworded to say inputs plus the few response
   envelopes; domain records serialize via their own serde derives (ADR-0004).
3. **Duplicated timestamp-key list** in `normalize_wire_timestamps` /
   `assert_and_count_utc_timestamps` — **Accepted, fixed.** Extracted a shared
   `is_timestamp_key` predicate. The recursive walks stay separate: one mutates,
   one counts; unifying them would cost more than the duplication.
4. **Name hides the counting contract** (`assert_utc_timestamp_strings`
   returning `usize`) — **Accepted, fixed.** Renamed to
   `assert_and_count_utc_timestamps`.
5. **Weak `is_err()` assertion for duplicate dependency** — **Accepted,
   fixed.** Now matches
   `Err(Error::Storage(RivetsError::Storage(StorageError::DuplicateDependency { .. })))`.
6. **`print_issues_to` untested in its own crate** (rust-best-practices
   Rule 39) — **Accepted, fixed.** Added two unit tests in
   `crates/rivets/src/output/mod.rs`: JSON output parses to exactly
   `serde_json::to_value(issues)`, and Text output includes the id and title.
7. **Unnecessary `.as_str()` in `IssueNotFound` match guard** — **Accepted,
   fixed.** `String == &str` compares directly.

## Spec findings

1. **`print_issues_to` extends rivets API surface beyond the issue's stated
   placement** — **Accepted as-is, no change.** design.md names the seam
   explicitly; it exists so the CLI/MCP parity fence does not spawn Cargo.
   Standards finding 6 adds the in-crate coverage that made it thin.
2. **Duplicate-dependency assertion not in acceptance criteria** — **Accepted
   as-is, kept.** design.md lists duplicate rejection as an input shape;
   Standards finding 5 upgraded it to a typed match.
3. **Golden test alone cannot catch a `Z` → `+00:00` regression** (timestamps
   normalized to `"<timestamp>"` before comparison) — **No change needed.**
   The regression is fenced by `timestamp_as_utc` /
   `mcp_timestamps_use_z_suffix`; both reviews agree the combined fences pin
   the contract.
