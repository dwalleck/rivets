# Falsifiable design: canonical Label grammar

## Route and inputs

- Route: **Structural**, from [`route.md`](route.md).
- Behavior source: requester-approved [`spec.md`](spec.md), including strict persisted-data rejection and explicit repository cleanup.
- Empirical inputs: N/A — current repository evidence is direct and complete; the pre-design audit found 25 invalid Label occurrences on 15 Issues and verified all 11 distinct reviewed replacements are canonical and collision-free per Issue.
- Complete behavior: parse the canonical grammar once in the domain; carry typed Labels through Issue, mutation, filtering, storage, JSON, CLI, and MCP; reject invalid external and persisted input visibly; preserve add/remove/list behavior; normalize only the explicitly reviewed repository records; evidence-gate parity conformance.

## Input shapes

| Input | Shapes | Status |
|---|---|---|
| Label spelling | exact empty; 1 byte; 50 bytes; 51 bytes; lowercase letters; digits; internal `-`; internal `_`; both separators nonadjacent | Covered by C1 |
| Invalid spelling | uppercase; leading/trailing separator; adjacent `--`, `__`, `-_`, `_-`; spaces including surrounding whitespace; tab/newline/control; punctuation/dot/slash; Unicode | Covered by C1/C3/C4 |
| Label collections | empty; one; several distinct; repeated equal values; mixed valid/invalid | Covered by C2/C3/C4/C5 |
| Optional filter | absent; canonical present; invalid present | Covered by C2/C3 |
| Mutation state | Label absent/present; duplicate add; absent remove; Open/Closed Issue | Covered by C5 |
| Adapter operation | CLI and MCP Create, temporary compatibility Update, List, Ready, Add, Remove; JSONL load/save; direct storage trait | Covered by C2/C3/C4 |
| Persistence | canonical record; one invalid Label among valid Labels; 25 known invalid occurrences on 15 repository Issues | Covered by C4 |
| Resource Label | arbitrary human-readable metadata | N/A — permanent non-goal: `ResourceLabel` is a distinct domain concept and interface |
| Permissions/auth/tenant/time/replica/cache | no reachable variants | N/A — local Workspace path has none of these behaviors |

This change is additive with respect to constraints: it strengthens previously raw strings into a canonical value without removing ordering, idempotency, or persistence safety constraints.

## Placement

### Canonical Label value

- **Owner:** new `rivets::domain::label` module, re-exporting `Label`, `LabelError`, and `MAX_LABEL_LENGTH`. The domain wins because CLI, MCP, JSONL, storage, filtering, and output all consume the invariant.
- **Competing seam A:** keep CLI `validate_label` and call it from other crates. Rejected: domain/storage would depend on a human adapter and MCP would inherit string errors.
- **Competing seam B:** validate separately in each storage method. Rejected: Create, Update, filters, output, JSONL, and adapters could still bypass it.
- **Chosen interface:** private `Label(String)` with borrowed `FromStr`, owned `TryFrom<String>`, fallible `new`, `as_str`, `into_string`, `Display`, validated serde string representation, equality/order/hash, and typed `LabelError`. Owned conversion retains the input buffer; all constructors share private grammar validation.
- **Forbidden:** no public unchecked constructor, no trimming/case-folding, no grammar duplication outside the domain, and no conflation with `ResourceLabel`.

### Domain and storage propagation

- **Owner:** existing `Issue`, `NewIssue`, `IssueUpdate`, `IssueFilter`, `ReadyFilter`, and `IssueStorage` interfaces carry `Label`; in-memory storage compares typed values and preserves insertion/idempotency semantics.
- **New seam:** none — this deepens the existing domain/storage interface instead of layering another validator.
- **Forbidden:** storage methods may not accept `&str`; no adapter may hand raw Issue-Label strings to storage; general Update compatibility remains temporary under `rivets-67d7`.

### Adapter translation

- **Owner:** clap parses `Label` directly; MCP `Tools` uses the domain's borrowed or owned parser as appropriate and `Error::InvalidLabel`, with JSON-RPC `invalid_params`. MCP schema-only string constraints mirror the domain grammar and are fenced against drift.
- **New seam:** no production seam; the MCP schema stand-in is documentation-only because `rivets` must not depend on schemars.
- **Forbidden:** adapter-local arm tables, resource-label validation reuse, internal-error mapping, or mutation/query before parsing.

### Persistence and repository cleanup

- **Owner:** `IssueRecord` remains a raw-string compatibility DTO and converts each Label through the domain parser so failures retain Issue ID context; `CanonicalIssueRecord` emits typed Labels as canonical strings. The tracker JSONL cleanup explicitly changes only the 25 reviewed occurrences.
- **New seam:** none — existing compatibility conversion owns persisted validation.
- **Forbidden:** automatic arbitrary normalization, silent dropping, generic malformed-JSON classification, changes to unrelated Label associations, or bypassing the partial-load write guard.

### Parity registry

- **Owner:** `docs/cli-mcp-parity.json` and its existing renderer.
- **New seam:** none — existing optional target-rule status/evidence fields are reused.
- **Forbidden:** Create/List/Update/Ready rows with unrelated remaining gaps may not be marked conformant. Add/Remove and the cross-cutting Label rule may; stale Issue-label List classification from the completed Issue ID work is corrected using its existing evidence.

## Claims

- **C1:** `Label` accepts exactly `[a-z0-9]+(?:[-_][a-z0-9]+)*` at 1-50 ASCII bytes and returns typed errors for every rejection class.
- **C2:** Domain and storage interfaces carry only `Label` values for Issue labels, creation, compatibility updates, filters, add, and remove.
- **C3:** Every CLI and MCP Label input rejects the same invalid spellings before query or mutation, and MCP reports `invalid_params` with the domain error meaning.
- **C4:** JSONL loads canonical Labels byte-identically, reports noncanonical Labels as Issue-specific invalid data under the partial-load guard, and the explicit repository cleanup leaves zero noncanonical records without collisions.
- **C5:** Typed Label propagation preserves Issue insertion order, duplicate-add/absent-remove timestamp idempotency, filter equality, and sorted/deduplicated list-all output.
- **C6:** The registry marks `canonical-label-input`, Add Label, and Remove Label conformant only with named passing fences, while unrelated gaps remain open.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Domain parser enforces the exact grammar with typed errors. | Complete valid/invalid spelling matrix. | Feed every matrix row through `Label::from_str`; any wrong acceptance, rejection, spelling, byte boundary, or error variant falsifies C1. Positive 1/50-byte controls isolate parser behavior from test-data errors. | Independent canonical regular expression plus explicit length/error table from `CONTEXT.md` and approved spec. | In `domain/label.rs`, change adjacent-separator detection to reject only equal separators; the `a-_b` and `a_-b` cases must turn red. | Parameterized domain parser/serde tests in `domain/label.rs`. | <1s targeted | PENDING — checkpointed-build Slice 1 |
| C2 | Domain/storage interfaces propagate typed Labels. | Collections, optional filter, all storage paths. | Compile and exercise create/update/filter/add/remove with `Label`; any raw-string storage seam, bypass, or mismatched filter falsifies C2. Runtime filter controls prevent a compile-only false positive. | Compiler type checking plus an independent expected Issue-state table. | In `matches_filter`, make a present Label filter return true unconditionally; the nonmatching-filter control must turn red. | Storage create/update/filter tests plus all-target compilation. | <5s targeted | PENDING — checkpointed-build Slice 1 |
| C3 | CLI/MCP reject identically before behavior. | Every adapter input plus invalid classes. | Drive real clap parsing and MCP Tools/server requests with otherwise-valid inputs; acceptance, storage lookup/mutation, different domain message, or non-`invalid_params` MCP code falsifies C3. Valid controls for each operation prove the request reaches the intended path. | Same domain `LabelError` expected independently at both adapter envelopes; persisted bytes confirm no mutation. | Remove `Error::InvalidLabel` from `to_mcp_error`; the server wire-code fence must change from -32602 and turn red. | CLI process/parser matrix, MCP operation matrix, and server error-code test. | <10s targeted | PENDING — checkpointed-build Slice 2 |
| C4 | Persistence is strict and cleanup is explicit/lossless for unrelated fields. | Canonical/invalid JSONL plus 25 known occurrences. | Load canonical and invalid fixtures, attempt mutation after partial load, save/reload canonical data, and audit repository Labels; wrong warning kind/Issue ID, allowed write, spelling drift, non-Label field drift, remaining invalid Label, or collision falsifies C4. A valid neighboring record is the positive load control. | Independent Python/JSON regex audit and byte comparison, separate from Rust conversion. | In `IssueRecord::into_domain`, use `filter_map(Result::ok)` for Labels; the invalid-label warning/guard fence must turn red. | Resilient-loader strict/round-trip tests plus repository JSONL audit. | <10s targeted | PASS for cleanup inventory/oracle; implementation fences pending checkpointed-build Slice 3 |
| C5 | Idempotency, ordering, filtering, and list-all semantics remain unchanged. | absent/present/duplicate Labels; Open/Closed; multiple Issues. | Compare timestamps and ordered outputs before/after duplicate add, absent remove, create/update/filter, and list-all; any unintended timestamp/order/duplicate change falsifies C5. Distinct-label controls prove mutations still work. | Pre-cutover behavioral tables and direct state timestamps, independent of parser logic. | In storage `add_label`, always push and update the timestamp; duplicate-add fence must turn red. | Storage and CLI/MCP behavioral integration tests. | <10s targeted | PENDING — checkpointed-build Slice 2 |
| C6 | Registry conformance is evidence-gated without closing unrelated gaps. | target rule and operation rows. | Parse registry and render Markdown; missing status/evidence, conformant unrelated rows, or generated drift falsifies C6. | Registry contract test plus deterministic renderer check. | Change `canonical-label-input` status to `pending`; the registry contract must turn red. | Registry contract assertion and renderer `--check`. | <5s targeted | PENDING — checkpointed-build Slice 3 |

## Non-goals and future work

- Resource Labels remain a separate, human-readable domain value; this is a permanent non-goal because their grammar intentionally permits spaces and case.
- Arbitrary automatic legacy-label normalization is a permanent non-goal: the requester chose strict rejection to avoid lossy guesses and collisions.
- Removing Labels from general Update is intended work tracked by verified `rivets-67d7`; this change validates that compatibility input until the clean cutover lands.
- Query/default/workflow parity remains owned by its existing delivery groups; this design does not mark those operation rows conformant.

## Falsifier run log

- 2026-08-29 — Python stdlib JSON/regex audit through `functions.eval` — **PASS** for C4's cleanup inventory: 25 invalid occurrences on 15 Issues; 11 distinct lowercase/dot-to-hyphen mappings all satisfy the canonical regex and produce no per-Issue collisions.

## Approval

Requester approval (verbatim): "Approve design"
Date: 2026-08-29
Approved risk acceptances: None.
