# Plan: Delete MCP Issue output mirrors

The approved design is in `.rivets-habf/design.md`; the probe/oracle is `.rivets-habf/probe.py` and `.rivets-habf/findings.md`. The repository's all-target quality gate requires integration callers to compile with the production return-type cutover, so Slice 1 and the originally planned Slice 2 migration landed atomically in commit `f13e105`; Slice 2's verification gates are recorded as satisfied below.

## Slice 1: Return canonical domain Issues from MCP tools

**Claim:** Every successful Issue-returning `Tools` method exposes `rivets::domain::Issue` values; MCP `Content::json` can serialize them without output mirrors, and existing input models remain intact.

**Oracle:** `.rivets-habf/probe.py` starts the built MCP stdio binary, calls `show`, parses its payload, and compares it to the independent `.rivets/issues.jsonl` record after the known pre-change UTC normalization. After this slice, the same payload must compare without normalization; the JSONL record remains independent of MCP code.

**Stress fixture:** The real workspace Issue `rivets-fk9` has populated design, acceptance criteria, one Note, and one dependency. Run the probe against it and ensure the MCP payload uses `Z` timestamps, retains all record keys, and omits `next_resource_id`. The unit stress case also calls `create`, `show`, `update`, `close`, and `reopen` with enum assertions so a missed return path fails.

**Loop budget:** No new production loop. Existing collection traversals change from five adapter `map(Into::into)` passes to one serde traversal per response: `O(1 + notes + resources + dependencies)` per Issue. At current workspace scale (`252` Issues; observed fixture maxima below `10` nested records per Issue), this is below `10^3` element visits per response and no new syscalls.

**Wall budget:** None; MCP request serialization remains request-scoped.

**Files:**
- `crates/rivets-mcp/src/models.rs`
- `crates/rivets-mcp/src/tools.rs`
- `crates/rivets-mcp/tests/integration.rs`

**Change:**
- Delete `McpIssue`, `McpNote`, `McpResource`, `McpResourceTarget`, `McpDependency`, and all five output conversions.
- Change `BlockedIssueResponse` to serialize domain `Issue` fields in its envelope without `Deserialize`/`JsonSchema` requirements that the domain Issue does not satisfy.
- Change every `Tools` return type and collection path from mirror values to `Issue`/`Vec<Issue>`; remove `Into` conversions.
- Migrate every integration caller and assertion from mirror fields to domain enums, IDs, accessors, and typed resource targets so the all-target quality gate remains green.
- Preserve all MCP input model imports, compatibility fields, validation functions, storage/context behavior, and output stream classification (`Content::json` data remains stdout; tracing remains stderr).

**Impact analysis:** Existing callers are `RivetsMcpServer` methods in `crates/rivets-mcp/src/server.rs`, the `tools.rs` unit tests, and `crates/rivets-mcp/tests/integration.rs`; all callers are migrated in this atomic cutover.

**Verification:**
- [x] `cargo nextest run -p rivets-mcp --lib`
- [x] `cargo nextest run -p rivets-mcp` (all 189 unit and integration tests)
- [x] `.rivets-habf/probe.py` produces direct MCP/JSONL agreement and `Z` timestamps
- [x] Unit and integration stress fixtures exercise create/show/update/close/reopen, notes, resources, persistence, and invalid input behavior
- [x] `cargo fmt -- --check` for touched Rust files
- [x] `cargo clippy -p rivets-mcp --all-targets --all-features -- -D warnings`

## Slice 2: Integration-domain verification (completed with Slice 1)

**Claim:** Existing MCP integration behavior and assertions remain valid when test callers consume domain `Issue`, `Note`, `AssociatedResource`, and `ResourceTarget` values directly.

**Oracle:** The pre-existing integration expectations plus direct `serde_json::to_value` of returned domain values. The oracle does not call any deleted mirror conversion.

**Stress fixture:** The resource lifecycle test with Web and Unicode/space-containing Workspace Path targets, the Note lifecycle test with four ordered Notes, and the persistence reload tests all passed in the atomic Slice 1 commit.

**Loop budget:** No production loops. Test-only assertions traverse `O(notes + resources)` values; fixture scale is at most four Notes and five resources, under `10^2` visits.

**Wall budget:** None for production; test-only storage reload remains bounded by the existing fixture size.

**Files:**
- `crates/rivets-mcp/tests/integration.rs`

**Change:**
- Replace mirror imports and return annotations with domain `Issue`/`ResourceTarget` values.
- Update helper return types and assertions from mirror strings/fields to domain enums, `IssueId`, Note accessors, AssociatedResource accessors, and typed target matching.
- Preserve every existing lifecycle, error, persistence, workspace, filter, relationship, and resource expectation; this slice changes only how tests observe the same public behavior.

**Impact analysis:** All helper call sites and assertions in `crates/rivets-mcp/tests/integration.rs` were migrated in `f13e105`; no production symbol changes occur.

**Verification:**
- [x] `cargo nextest run -p rivets-mcp` — 189 tests passed
- [x] Resource target stress fixture passed for Web, Unicode Path, and normalized Path values
- [x] Note ordering and timestamp assertions passed after accessor migration
- [x] `.rivets-habf/probe.py` still agrees with the binary
- [x] `cargo fmt -- --check`
- [x] No new loop or wall-budget violation

## Slice 3: Add permanent golden and CLI/MCP parity fences

**Claim:** A fully populated Issue pins the exact external JSON shape and the CLI list representation equals the MCP representation for the same Issue.

**Oracle:** A hand-written `serde_json::json!` golden object after replacing only dynamic timestamp strings with a fixed marker, plus an independently parsed CLI `list --json` value and an MCP `Content::json` payload from `show`. The expected object does not use any MCP mirror type or conversion.

**Stress fixture:** In a temporary real workspace, create an Issue with every optional field, four ordered Notes, both Web and Path resources, all five resource roles where target uniqueness permits, all four dependency kinds, labels, a closed timestamp, and a nonzero internal `next_resource_id`. Assert exact normalized JSON keys/tags/values, `next_resource_id` omission, array order, and `Z` suffixes. Compare the same persisted Issue through the CLI list and freshly loaded MCP show paths after removing the CLI array wrapper.

**Loop budget:** Test-only recursive timestamp normalization is `O(serialized JSON nodes)`; the fixture has fewer than `10^2` nodes. No production loop or syscall is introduced.

**Wall budget:** None.

**Files:**
- `crates/rivets-mcp/tests/integration.rs`

**Change:**
- Add `mcp_full_issue_json_golden` with deterministic timestamp normalization and explicit expected JSON.
- Add `mcp_timestamps_use_z_suffix` that checks Issue, Note, and `closed_at` timestamp strings and parses them back to equal instants.
- Add `cli_and_mcp_issue_json_shapes_match` using the CLI list JSON representation and MCP `show` payload.
- Keep the fixture setup through existing Tools/storage seams; do not add output DTOs or source-text tests.

**Impact analysis:** New tests have no existing callers; they consume the public `Tools` methods and existing integration helpers.

**Verification:**
- [x] `cargo nextest run -p rivets-mcp --test integration mcp_full_issue_json_golden cli_and_mcp_issue_json_shapes_match mcp_timestamps_use_z_suffix` — 3 passed
- [x] Golden stress fixture matches exact normalized JSON
- [x] `.rivets-habf/probe.py` agrees with the assembled binary without timestamp normalization
- [x] All design falsifiers and regression fences pass
- [x] Loop budget holds at fixture scale
- [x] `cargo fmt -- --check` for the test file
- [x] `cargo clippy -p rivets-mcp --all-targets --all-features -- -D warnings`
- [x] `cargo nextest run -p rivets-mcp` — 192 tests passed

## Plan self-review

- **Loops:** Slices 1 and 2 add no production loops; their test-only traversals are bounded in terms of Notes/resources, and Slice 3 bounds JSON traversal by node count.
- **Fixtures:** Every logic slice has an adversarial fixture: populated real Issue, typed Unicode/space path plus ordered Notes, and a fully populated all-variant golden Issue.
- **Doc-comment preconditions:** No new doc-comment preconditions are introduced; existing domain constructors enforce value invariants at their seams.
- **Writes:** MCP Issue payloads and CLI JSON are data on stdout; tracing and process diagnostics remain stderr. Tests write only temporary workspace fixtures.
- **Tracker references:** No deferral or anonymous future-work reference appears in this plan.
- **Gate correction:** The all-target quality hook made the production cutover and integration accessor migration one atomic commit; both slices' gates passed before Slice 3.
