# Falsifiable design: canonical Issue ID parsing

## Route and inputs

- Route: **Structural**, from [`route.md`](route.md).
- Behavior source: `route.md` T4. `spec.md`: N/A — the ticket, ADR-0006, parity registry, and T4 fully specify behavior.
- Empirical inputs: N/A — no external or existing-system premise.
- Complete behavior: one domain parser accepts canonical `prefix-suffix` IDs; every listed CLI/MCP intent parses before storage; malformed IDs retain one semantic error across adapter envelopes; valid canonical persisted IDs remain readable; registry conformance follows passing cross-adapter fences.

## Input shapes

| Input | Shapes | Status |
|---|---|---|
| Issue ID string | empty; surrounding whitespace; no separator; one separator; multiple suffix separators | Covered by C1 |
| Prefix | 1 byte; 2 bytes; 20 bytes; 21 bytes; ASCII alphanumeric; punctuation; internal whitespace; Unicode | Covered by C1 |
| Suffix | empty; one ASCII-alphanumeric segment; multiple segments; leading/trailing/consecutive hyphen; punctuation/underscore/internal whitespace/control/Unicode; long segment | Covered by C1 |
| CLI single-ID operations | Show, Update, Close, Reopen, Label Add/Remove/List, Resource Add/Update/Remove/List, Note append | Covered by C2 |
| CLI ID collections/pairs | zero/single/multiple Create prerequisites; batch Show/Update/Close/Reopen/Label; both Blocking endpoints; Blocking list/tree roots | Covered by C2 |
| MCP single-ID operations | Show, Update, Close, Reopen, Label Add/Remove/List, Resource Add/Update/Remove/List, Note append | Covered by C3 |
| MCP endpoint pairs | add/remove with valid/invalid dependent and prerequisite; list perspectives; tree root | Covered by C3 |
| Persisted IDs | canonical legacy/current ID | Covered by C4 |
| Persisted noncanonical IDs | legacy hierarchical/dotted or otherwise noncanonical IDs | N/A — this ticket governs CLI/MCP input and guarantees compatibility only for persisted IDs satisfying the canonical grammar; persistence compatibility decoding remains unchanged |
| Multiple simultaneously-invalid non-ID fields | invalid ID plus invalid status/resource/note/reason | N/A — error precedence between independently invalid fields is not part of ADR-0006; each focused parity case keeps non-ID fields valid |

This change is additive with respect to constraints: it adds a shared parser at adapter seams and does not remove an existing invariant.

## Placement

### Canonical Issue ID parser

- **Owner:** `rivets::domain::IssueId`. The domain owns canonical grammar and typed failure meaning; adapters should not know its arm table.
- **New seam:** the existing `IssueId` interface gains `FromStr<Err = IssueIdError>`. No new module or trait.
- **Forbidden:** CLI and MCP must not duplicate the grammar; storage lookup/mutation must not receive unparsed adapter strings; JSONL compatibility decoding must not be silently tightened by this ticket.

### Adapter translation

- **Owner:** CLI `validate_issue_id` translates `IssueIdError` to clap's string envelope; MCP `Tools` translates through one local `parse_issue_id` helper and `Error::InvalidIssueId` to JSON-RPC `invalid_params`.
- **New seam:** none — both adapters already translate domain errors.
- **Forbidden:** malformed IDs must not become `IssueNotFound`, storage/internal errors, or operation-specific validation tables.

### Registry proof

- **Owner:** `docs/cli-mcp-parity.json`; its renderer remains the human-readable projection.
- **New seam:** an optional cross-cutting-rule `status` plus named behavioral evidence, so unfinished rules remain pending while this rule can be marked conformant.
- **Forbidden:** operation rows with unrelated remaining gaps must not be marked conformant.

## Claims

- **C1:** `IssueId::from_str` trims surrounding whitespace, validates the resulting spelling against the existing canonical CLI grammar, and returns typed `IssueIdError` variants for every rejection class.
- **C2:** Every ID-bearing CLI intent reaches C1 through its clap value parser, including every Create prerequisite and both Blocking endpoint roles.
- **C3:** Every ID-bearing MCP intent reaches C1 before storage, and malformed IDs map to JSON-RPC `invalid_params` with the same domain error meaning as CLI.
- **C4:** Canonical persisted IDs continue to deserialize unchanged, and the parity registry marks only `canonical-issue-id-input` conformant after the cross-adapter fence passes.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Domain parser implements the canonical grammar with typed errors. | Complete string/prefix/suffix matrix above. | Feed the table to `IssueId::from_str`; any accepted invalid row, rejected valid row, altered spelling, or wrong typed variant falsifies C1. Another possible cause is a wrong test row; positive valid controls and explicit constants distinguish parser failure. | The grammar table derived independently from ADR-0006, the ticket, and documented 2/20 prefix constants. | In `domain/mod.rs`, change the max-prefix comparison from `>` to `>=`; the 20-byte valid case must turn red. | Domain parameterized grammar test in `domain::tests`. | <1s targeted | PASS for cheapest pre-design compatibility check; implementation fence pending checkpointed-build Slice 1 |
| C2 | Every CLI ID input delegates to C1. | All CLI operation and collection shapes above. | Parse each real clap command with one malformed ID and one boundary-valid ID; acceptance of malformed input or rejection of valid input falsifies C2. A command-shape typo could mimic failure, so every case has a positive control using the same argv. | Clap command definitions enumerated from `Commands`/args, compared with the registry delivery-group intent list. | Remove `value_parser = validate_issue_id` from `ResourceAction::List`; its malformed case must turn red while its valid control stays green. | Parameterized CLI parser coverage test in `cli/mod.rs`. | <1s targeted | PENDING — checkpointed-build Slice 1 |
| C3 | Every MCP ID input delegates to C1 and maps malformed IDs to client-fixable errors. | All MCP operation and endpoint shapes above. | Invoke each `Tools` method against a real temporary Workspace with malformed ID and otherwise-valid args; any storage lookup, `IssueNotFound`, internal error, or mutation falsifies C3. Positive valid-shaped missing-ID controls prove storage lookup remains observable. | The same domain `IssueIdError` value expected by the CLI parser, plus JSON-RPC error-code classification independent of storage results. | In `Tools::resource_list`, replace `parse_issue_id` with `IssueId::new`; that operation's malformed case must become `IssueNotFound` and turn red. | MCP parameterized integration test plus `to_mcp_error` classification test. | <5s targeted | PENDING — checkpointed-build Slice 2 |
| C4 | Canonical persisted IDs remain readable and registry conformance is evidence-gated. | Canonical legacy ID plus registry rule status/evidence. | Deserialize/load `ab-1` and a 20-byte-prefix ID, then inspect registry rule status/evidence; changed ID text, load failure, missing conformance, or missing fence pointer falsifies C4. A parser-only test could miss persistence, so the fence uses the persisted representation/loader. | Existing serde/JSONL compatibility path and the machine-readable registry's named test evidence. | Change the canonical rule status back to `pending`; the registry contract test must turn red. | Persistence canonical-ID regression test and parity registry contract test. | <5s targeted | PENDING — checkpointed-build Slice 3 |

## Non-goals and future work

- Noncanonical persisted Issue IDs are a permanent non-goal for this change: the acceptance criterion promises legacy readability only when the identifier satisfies canonical grammar, and changing JSONL compatibility classification would be a separate migration decision.
- Exact error precedence when several independent fields are invalid is a permanent non-goal: parity cases isolate Issue ID behavior.
- Adding initial relationship inputs to MCP Create is not part of Issue ID parsing; the Create operation remains a registry gap until its canonical relationship contract is delivered.
- No intended future work is introduced by this design.

## Falsifier run log

- 2026-08-29 — `cargo test -p rivets cli::validators::tests::test_validate_issue_id_prefix_exactly_20_chars` — **PASS**. The current adapter accepts the documented 20-byte prefix boundary, falsifying an off-by-one premise before cutover.

## Approval

- Requester approval: **"claim and implement rivets-ragt"**
- Date: 2026-08-29
- Approved risk acceptances: None.
