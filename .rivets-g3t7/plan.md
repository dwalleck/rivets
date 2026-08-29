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
