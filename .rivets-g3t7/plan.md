# Plan: canonical Label grammar

## Integration budget

- Slice estimate: **2,030 changed lines**.
- Churn margin: **30% / 609 lines**, covering compiler-driven test literal migration, MCP schema snapshots, and generated parity Markdown.
- Projected total: **2,639 changed lines**.
- Review-size result: one PR increment; 2,639 ≤ 4,000.

### PR increment: canonical-label-input

One atomic slice. The public Label/storage cutover, MCP error/schema changes, repository JSONL cleanup, and evidence-gated registry test share caller and file boundaries—especially `server.rs`—and cannot be independently green without staging half a contract test. The increment is mergeable when all claims and fences pass.

## Slice 1: Cut over canonical Labels and publish parity proof

**Claim IDs:** C1, C2, C3, C4, C5, C6
**Expected behavior:** One typed domain Label implements the approved grammar; every core, CLI, MCP, JSONL, output, and storage path carries or parses it; invalid adapter/persisted input fails before behavior; the 15-Issue tracker cleanup leaves only canonical Labels; mutation/filter/list semantics remain unchanged; parity status is conformant only with named evidence while unrelated gaps remain open.
**Oracle:** Approved regex/error table; compiler type checking; independent Python JSON audit and byte comparison; pre-cutover timestamp/order/idempotency behavior tables; machine registry plus deterministic renderer.
**Stress fixture:** Grammar matrix includes empty, 1/50/51 bytes, uppercase, spaces/surrounding whitespace, control, Unicode, bad endpoints, `--`, `__`, `-_`, `_-`; collection fixtures include empty, one, 1,000 valid Labels, duplicates, and one invalid among valid; JSONL fixture pairs one valid and one invalid record; tracker audit covers all 25 invalid occurrences on 15 Issues and asserts unrelated fields unchanged; registry fixture asserts both conformant and deliberately nonconformant Label-aware rows.
**Regression fence:** Domain parser/serde tests; storage create/update/filter/idempotency tests; resilient-loader strict/round-trip/guard tests; CLI parser/process tests; MCP Tools/server/schema/integration tests; repository JSONL audit; parity registry contract and renderer check.
**Named mutation:** (C1) reject only equal adjacent separators; mixed-adjacent cases red. (C2) ignore present Label in `matches_filter`; nonmatch control red. (C3) remove `InvalidLabel` from `to_mcp_error`; JSON-RPC code fence red. (C4) silently drop failed persisted Label parses with `filter_map(Result::ok)`; warning/guard fence red. (C5) always push/update in `add_label`; duplicate timestamp/order fence red. (C6) set `canonical-label-input.status` to `pending`; registry fence red.
**Complexity/production scale:** Parser O(n) for n≤50 with one success allocation. Collection conversion O(k×50) for k Labels and one allocation per Label; 1,000 Labels means ≤50,000 inspected bytes. Explicit maximum accepted cost: one byte inspection per input byte plus one allocation per accepted Label; the 1,000-Label fixture must complete without extra normalization passes or intermediate collections.
**Wall budget/phase:** N/A — parsing deepens the existing CLI/MCP/JSONL input phase rather than adding a distinct runtime phase; route T3 records no production-scale risk, and the deterministic operation bound above is the applicable budget.
**Files:** `crates/rivets/src/domain/label.rs` (new), `crates/rivets/src/domain/mod.rs`, CLI args/execute/parser tests, storage trait/in-memory/JSONL conversion and tests, output formatting/tests, `crates/rivets-mcp` models/errors/server/tools/integration tests, `.rivets/issues.jsonl`, `docs/architecture.md`, `docs/module-structure.md`, `docs/cli-mcp-parity.json`, `docs/cli-mcp-parity.md`, and `.rivets-g3t7/` owned artifacts.
**Estimate:** 3.5 hours.
**Diff estimate:** 2,030 lines.
**PR increment:** canonical-label-input.
**Commands and expected results:**
- `cargo test -p rivets label` → grammar, typed propagation, strict JSONL, filtering, ordering, idempotency, and write guard agree with approved tables.
- `cargo test -p rivets-mcp label` → every MCP Label path rejects invalid inputs with typed errors; schema, valid operations, and sorted/deduplicated list-all pass.
- Python JSON/regex audit → zero noncanonical repository Labels; exactly 25 reviewed replacements on 15 Issues; every unrelated field byte-semantically equal.
- `cargo test -p rivets-mcp parity_registry_classifies_every_cli_leaf_and_mcp_tool` and renderer `--check` → intended Label rows conformant with evidence; negative rows remain gaps; generated Markdown matches JSON.
- Each named mutation → owning fence red for the named reason; restore → green.
- Final gates: `cargo fmt --check`; affected-crate clippy with `-D warnings`; full `rivets` and `rivets-mcp` tests; actual CLI invalid/valid Label smoke.

## Self-review

- [x] Every design claim is assigned exactly once; all caller migration and the shared registry/server mutation boundary are atomic.
- [x] The slice includes all thirteen mandatory fields.
- [x] Every fence lands with its claim and names a red mutation.
- [x] Runtime complexity and the route-backed wall-budget N/A are explicit.
- [x] The review-size partition arithmetic is recorded.
- [x] Intended Update cleanup cites verified `rivets-67d7`; permanent non-goals carry rationale.
- [x] The slice is not declared complete here; checkpointed-build owns completion.

## Slice 2: Retain owned Label strings and document their owner — F2, F3

**Claim IDs:** C1, C2, C3, C4.
**Expected behavior:** Owned String inputs transfer into validated Labels without another spelling allocation; borrowed, owned, and JSON inputs retain identical grammar and errors. Module navigation names the Label owner.
**Oracle:** The existing independent grammar/error table plus a throwaway buffer-identity check for 1,000 owned canonical spellings.
**Stress fixture:** Existing valid/invalid grammar classes through borrowed, owned, and serde entrypoints; 1,000 independently constructed owned strings.
**Regression fence:** Extend `parses_canonical_label` and `rejects_noncanonical_label` to the owned constructor and serde rejection path.
**Named mutation:** Bypass validation in the owned conversion; invalid grammar cases must become accepted and turn the fence red.
**Complexity/production scale:** One O(n) validation, n <= 50, with no additional spelling allocation for owned input; 1,000 Labels inspect at most 50,000 input bytes. Borrowed input still allocates only after validation.
**Wall budget/phase:** N/A — no new runtime phase; only avoidable copying is removed.
**Files:** `domain/label.rs`, `storage/in_memory/issue_record.rs`, MCP `tools.rs`, `docs/module-structure.md`, and these workflow artifacts.
**Estimate:** One focused ownership correction.
**Diff estimate:** 160 lines including evidence; with the original 30% margin, the Label increment remains below 4,000 lines.
**PR increment:** canonical-label-input.
**Commands and expected results:**
- `cargo test -p rivets label` — owned and borrowed grammar agree, and strict persistence stays intact.
- `cargo test -p rivets-mcp label` — adapter validation and Label behavior remain unchanged.
- Throwaway Rust smoke — all 1,000 owned inputs retain their original buffer after conversion.
- Final workspace tests, Clippy with warnings denied, formatting, and parity renderer remain green.

### Integration and Slice 2 checkpoint — 2026-09-05

Requester approved the rebase/review plan with **"Execute the plan you suggested"**. The original Label grammar and strict-loading policy are unchanged.

| Gate | Result |
|---|---|
| Affected tests | PASS — final workspace suite: 1,213 passed, 8 ignored; focused Label suites: 45 core and 23 MCP tests passed. |
| Assigned falsifier | PASS — borrowed, owned, and serde inputs agree with the existing grammar/error table. |
| Stress fixture | PASS — throwaway Rust exercise converted 1,000 owned spellings while retaining every original buffer and exact text. |
| Independent oracle | PASS — buffer-identity smoke, fixed grammar cases, and independent Python tracker regex/field comparisons agree. |
| Production-scale budget | PASS — owned conversion reuses the String allocation; one bounded validation scan remains. No additional runtime phase or wall-clock budget. |
| Regression fence | PASS — existing grammar cases now also defend owned conversion and serde rejection; loader/adapter/ordering/idempotency checks pass. |
| Named mutation | PASS — removing owned-conversion validation made all 16 invalid grammar cases fail because noncanonical Labels were accepted. |
| Restored fence | PASS — validation restored; final full suite green. |

Actual CLI smoke passed canonical/invalid Label input, insertion order, duplicate-add/absent-remove timestamp idempotency, filtering, Parentage/Assignment integration, and refusal to write a partially loaded tracker. All 262 upstream Issues remain: 245 unrelated rows are byte-identical; 15 rows change only their explicitly approved Labels; two rows retain main's fields and append the intended closure history with no assignee. The existing `rivets-omq0` record is untouched.

Clippy with warnings denied, formatting, and parity renderer checks passed. The tautological collection-length budget test was removed in favor of actual throwaway ownership proof; the standalone wording-pinned serde assertion was replaced by typed data-error checks in the existing invalid grammar cases. Temporary smoke sources, binaries, and workspaces were removed.

### Review decisions

| finding-id | finding | reviewer | evidence-state | evidence | decision | fix | note |
|---|---|---|---|---|---|---|---|
| F2 | Owned Label strings were borrowed and copied; introduce owned conversion. | ParityStandards | Verified | `parse_labels` and JSONL DTO conversion borrowed owned Strings into allocating `FromStr`; corrected ownership smoke retained all 1,000 original buffers. | Accept | Add validated `TryFrom<String>` and migrate owned MCP/JSONL/serde inputs; Slice 2 checkpoint passed. | Borrowed parsing still allocates only after successful validation; reviewer recheck confirmed resolution. |
| F3 | Module navigation omitted the new Label owner; add the graph node and edge. | ParityStandards | Verified | The prior domain graph contained only Relationship and Resource children; updated tree and graph now name `label.rs`. | Accept | Synchronize `docs/module-structure.md` with the Label owner. | Documentation-only correction; reviewer recheck confirmed resolution. |

Independent `CanonicalLabelSpec` review reported no actionable Label acceptance defects.
