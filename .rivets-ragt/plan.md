# Plan: canonical Issue ID parsing

## Integration budget

- Slice estimates: 220 + 260 + 180 = **660 changed lines**.
- Churn margin: **35% / 231 lines**, covering exhaustive adapter-case tables and renderer output drift.
- Projected total: **891 changed lines**.
- Review-size result: one PR increment; 891 ≤ 4,000.

### PR increment: canonical-issue-id-input

Slices 1-3. Mergeable when the domain parser, every caller, cross-adapter fences, compatibility fence, and registry projection are green. No later increment is required to make the repository valid.

## Slice 1: Add the domain parser and migrate the CLI boundary

**Claim IDs:** C1, C2
**Expected behavior:** The domain returns typed results for the complete grammar matrix; real clap command shapes accept canonical/boundary IDs and reject malformed IDs through the same parser.
**Oracle:** The explicit grammar matrix in `design.md`, plus the registry's complete CLI intent inventory.
**Stress fixture:** A table containing empty, trimmed, missing-separator, 1/2/20/21-byte prefix, invalid prefix, empty suffix, one/multiple suffix segments, edge/consecutive hyphens, invalid ASCII, Unicode, control, and long suffix cases; expected canonical spelling or typed error is fixed before implementation.
**Regression fence:** Domain parameterized parser tests and CLI parameterized parser coverage in `crates/rivets/src/domain/mod.rs` and `crates/rivets/src/cli/mod.rs`.
**Named mutation:** Change the domain max-prefix comparison from `>` to `>=`, and separately remove the Resource List value parser; their owning cases must fail.
**Complexity/production scale:** One O(n) scan over an Issue ID plus one final allocation; production IDs are normally <64 bytes. Explicit maximum accepted cost: one pass plus one allocation for any accepted input; rationale: no regex, collection, or repeated lowercase/copy work is needed.
**Wall budget/phase:** Always-on CLI parsing; <1 ms per ID on the 10,000-byte stress suffix, a conservative interactive-command budget.
**Files:** `crates/rivets/src/domain/mod.rs`, `crates/rivets/src/cli/validators.rs`, `crates/rivets/src/cli/mod.rs`, `CONTEXT.md`.
**Estimate:** 45 minutes.
**Diff estimate:** 220 lines.
**PR increment:** canonical-issue-id-input.
**Commands and expected results:**
- `cargo test -p rivets domain::tests::issue_id` → every grammar/error case agrees with the table.
- `cargo test -p rivets cli::tests::all_issue_id_inputs_use_domain_parser` → every CLI argv has malformed-red and canonical-green controls.
- Named mutations above → owning fence red; restore → green.

## Slice 2: Migrate every MCP Issue ID boundary

**Claim IDs:** C3
**Expected behavior:** Every ID-bearing `Tools` method parses before storage; malformed IDs produce `Error::InvalidIssueId` and JSON-RPC `invalid_params`, never `IssueNotFound` or internal/storage errors.
**Oracle:** Domain `IssueIdError` returned for the same string through the CLI parser, compared with the MCP error source and wire classification.
**Stress fixture:** One otherwise-valid request per single-ID operation and Blocking endpoint role, all using malformed `invalid`; a valid-shaped missing ID positive control must still reach `IssueNotFound`.
**Regression fence:** MCP parameterized integration test plus server error-classification test.
**Named mutation:** Replace `parse_issue_id` with `IssueId::new` in `Tools::resource_list`; that operation's malformed case must fail with `IssueNotFound` and turn the fence red.
**Complexity/production scale:** No new loop beyond existing operation enumeration in tests; runtime parser is the Slice 1 O(n) seam. Maximum accepted cost unchanged from Slice 1.
**Wall budget/phase:** Always-on MCP parsing; <1 ms per ID on the 10,000-byte stress suffix, matching Slice 1.
**Files:** `crates/rivets-mcp/src/error.rs`, `crates/rivets-mcp/src/server.rs`, `crates/rivets-mcp/src/tools.rs`, `crates/rivets-mcp/tests/integration.rs`.
**Estimate:** 60 minutes.
**Diff estimate:** 260 lines.
**PR increment:** canonical-issue-id-input.
**Commands and expected results:**
- `cargo test -p rivets-mcp invalid_issue_id` → every Tools operation rejects malformed IDs with the same typed source; valid-shaped missing control reaches storage.
- `cargo test -p rivets-mcp test_to_mcp_error_classifies_invalid_issue_id_as_invalid_params` → JSON-RPC code is -32602 and message is the domain error.
- Named mutation above → resource-list case red; restore → green.

## Slice 3: Fence persistence compatibility and publish conformance

**Claim IDs:** C4
**Expected behavior:** Canonical legacy/current IDs deserialize/load byte-identically; the machine registry and rendered reference show `canonical-issue-id-input` conformant with named behavioral evidence, while unrelated operation gaps remain unchanged.
**Oracle:** Existing serde/JSONL representation plus the registry source read independently by its contract test and renderer check.
**Stress fixture:** Canonical IDs at both prefix boundaries and a multi-segment suffix loaded through persisted representation; expected spelling is identical.
**Regression fence:** Canonical persisted-ID test and parity registry contract assertion naming the cross-adapter test evidence.
**Named mutation:** Change the canonical target-rule status from `conformant` to `pending`; the registry contract test must turn red.
**Complexity/production scale:** N/A — no new production loop; renderer iterates the existing bounded rule list only during documentation generation.
**Wall budget/phase:** N/A — documentation rendering and contract validation are one-off developer operations.
**Files:** `crates/rivets/src/domain/mod.rs` or existing JSONL compatibility test seam, `crates/rivets-mcp/src/server.rs`, `docs/cli-mcp-parity.json`, `scripts/render-cli-mcp-parity.py`, `docs/cli-mcp-parity.md`.
**Estimate:** 35 minutes.
**Diff estimate:** 180 lines.
**PR increment:** canonical-issue-id-input.
**Commands and expected results:**
- `cargo test -p rivets canonical_persisted_issue_ids_remain_readable` → both boundary IDs retain exact spelling.
- `cargo test -p rivets-mcp parity_registry` → rule is conformant and points to behavioral evidence; unrelated gaps remain valid.
- `python scripts/render-cli-mcp-parity.py --check` → generated Markdown equals the registry projection.
- Named mutation above → registry fence red; restore → green.

## Self-review

- [x] Every design claim is assigned exactly once; pending falsifiers stay with their implementing slice.
- [x] Every slice includes all thirteen mandatory fields.
- [x] Every fence lands with its claim and names a red mutation.
- [x] Runtime complexity and always-on budgets are explicit.
- [x] The review-size partition arithmetic is recorded.
- [x] No deferred or intended-future work is introduced.
- [x] No slice is declared complete here; checkpointed-build owns completion.
