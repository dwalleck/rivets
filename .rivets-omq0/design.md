# Design: prevent stale JSONL replacement

## Route and inputs

- Route: **Structural**, from `route.md`.
- Behavior source: `route.md` T4; `spec.md` is N/A because behavior is explicit.
- Required behavior:
  - Given a long-lived MCP Workspace cached before a completed out-of-band `issues.jsonl` change, an MCP mutation must use the latest persisted state or fail without writing; the external change remains.
  - With no out-of-band change, existing mutation results and persistence remain unchanged.
  - A failed save restores cached state from disk as today.
- Empirical inputs: N/A — the route records no unverified premise, so `evidence.md` and probes do not apply.
- Existing premise check: `IssueStorage::reload` replaces cached JSONL state from disk; the cheapest falsifier below passed.

## Input shapes

| Shape | Status |
|---|---|
| Present, unchanged JSONL source | Covered by C5 and C6 |
| Completed external addition, update, or deletion before mutation | Covered by C1 and C4 |
| Source transition from present to missing or missing to present | Covered by C1 and C5 |
| External malformed or schema-incompatible record | Covered by C3 |
| External change after in-memory mutation but before save | Covered by C2 |
| Current-context and explicit `workspace_root` MCP resolution | Covered by C4 |
| All twelve MCP mutators: create, update, add Note, add/update/remove Resource, close, add/remove Blocking Dependency, reopen, add/remove Label | Covered by C4 through the shared JSONL adapter guard |
| Empty, single-record, multi-record, duplicate-ID, ASCII, Unicode, and multiline JSONL content | Covered by C1, C3, and C7; canonical loading remains the parser |
| Concurrent MCP mutations sharing one cached storage instance | Covered by C5 |
| Read-only MCP calls after an external edit | N/A — permanent non-goal: stale reads cannot overwrite data, and the requested fence is explicitly before mutation or save |
| In-memory and unimplemented PostgreSQL adapters | N/A — permanent non-goal: neither adapter writes `issues.jsonl` |
| Cooperating CLI/MCP writers changing the file during one mutation transaction | N/A — intended future work: durable cross-process serialization is `rivets-j13o`, verified open with matching acceptance criteria |
| An arbitrary editor that ignores Rivets locking and writes between the final revision check and atomic rename | N/A — permanent non-goal: portable rename provides no compare-and-swap against a process that does not participate in a lock protocol |

## Subtractive-change sweep

Purely additive guard: no serialization point, validation, ordering guarantee, uniqueness rule, or precondition is removed. Existing atomic temp-file replacement, partial-load write refusal, storage lock ordering, save-failure reload, and canonical ordering remain in force.

## Placement

### Capability: persisted-source revision tracking and conflict prevention

- **Owner:** `JsonlBackedStorage` in `crates/rivets/src/storage/mod.rs`. It is the JSONL persistence Adapter and already owns the `ensure_writable`, mutation delegation, save, and reload ordering. Putting the invariant here gives CLI and MCP callers leverage without duplicating persistence knowledge.
- **New seam:** no external seam. Add a private `SourceRevision` implementation and deepen the existing `IssueStorage` interface: callers continue to call the same mutation, `save`, and `reload` methods.
- **Forbidden:** MCP tools, CLI commands, and the domain model may not hash files, compare revisions, parse JSONL for freshness, or special-case this conflict. Direct `in_memory::save_to_jsonl` remains a lower-level unguarded primitive and must not become the application persistence seam.

### Capability: stale-source behavior

- **Owner:** `JsonlBackedStorage` refreshes before mutation and rejects a post-mutation source change before save.
- **New seam:** one new typed `StorageError::ExternalChange { path }` variant; it crosses the existing storage error seam and preserves its source classification through MCP.
- **Forbidden:** no string matching, silent overwrite, silent discard of the requested mutation, or catch-all conversion that erases the typed storage cause.

### Revision algorithm and ordering

1. Represent the source as `Missing` or `Present(SHA-256)`; hash incrementally with a fixed-size buffer.
2. On JSONL storage creation, capture the revision corresponding to the loaded source.
3. Before every JSONL-backed mutation, compare disk to the captured revision while the caller holds its storage write lock. If changed, reload canonical JSONL, update the revision, then re-run the existing partial-load write guard before mutating.
4. Before `save`, compare disk again. A mismatch returns `StorageError::ExternalChange` before the atomic writer opens its temporary output.
5. After successful atomic rename, capture the new revision. After `reload`, capture the revision corresponding to the reloaded state.
6. `save_or_reload` keeps its existing behavior: a post-mutation conflict triggers reload and returns the original typed error.

## Claims

- **C0:** `IssueStorage::reload` can replace cached JSONL state with persisted state.
- **C1:** A completed external source change before mutation is loaded before the requested mutation, so both changes survive the later save.
- **C2:** A source change after mutation but before save returns a typed external-change conflict and performs no source write.
- **C3:** Reloading externally malformed or schema-incompatible JSONL cannot launder a partial load into a save.
- **C4:** A same-instance MCP mutation through either Workspace selector preserves an out-of-band JSONL sentinel because the JSONL Adapter owns the guard for every mutator.
- **C5:** Successful saves advance the captured revision, and concurrent MCP mutations remain serialized by the existing per-Workspace write lock without false conflicts.
- **C6:** An unchanged-source mutation adds at most three sequential fixed-buffer revision scans and no file-sized allocation; an externally changed source adds at most one canonical reload before mutation.
- **C7:** The guard preserves empty, Unicode, multiline, duplicate-ID-warning, and 10,000-record source behavior because revision bytes are opaque and canonical loading remains unchanged.
- **C8:** Revision tracking and conflict classification remain inside the JSONL Adapter and typed storage error seam.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C0 | Reload replaces cached JSONL state. | Existing disk state plus divergent cache | Run the existing reload test; falsified if the post-reload title is not the direct disk title. A no-op mutation could otherwise mimic success, so the test first proves divergent in-memory state. | Direct assertion against the title originally persisted before the unsaved cache mutation | In `JsonlBackedStorage::reload`, remove assignment of `self.inner`; `test_jsonl_reload_restores_disk_state` turns red. | `storage::tests::test_jsonl_reload_restores_disk_state` | <1 second | PASS |
| C1 | Completed external changes are merged before mutation. | Addition/update/deletion; present/missing transitions | Cache storage, directly replace JSONL with an independently serialized sentinel revision, mutate through storage, save, and directly parse bytes; falsified if either sentinel or requested mutation is absent. Positive control directly parses the sentinel before mutation. | Direct `serde_json::Value` line parsing of source bytes, independent of the resilient production loader and in-memory export | In `prepare_mutation`, skip `reload` when revisions differ; the sentinel disappears or the mutation targets stale data. | New core JSONL stale-source merge test | 1–2 seconds | PENDING — checkpointed-build, persistence slice |
| C2 | Interleaved external change rejects save without writing. | Change after mutation/before save | Mutate cached storage, directly replace source with sentinel bytes, call save; falsified if error is not typed `ExternalChange` or source bytes differ from the sentinel. A successful control save first proves the writer can replace the file. | Byte-for-byte snapshot of the direct external write plus typed enum matching | In `JsonlBackedStorage::save`, remove the pre-save revision comparison; save succeeds and sentinel bytes change. | New core JSONL post-mutation conflict test | 1–2 seconds | PENDING — checkpointed-build, persistence slice |
| C3 | Partial external loads remain non-writable. | Malformed/schema-incompatible and duplicate-ID warning shapes | Externally inject one malformed record, invoke a mutation, and compare source bytes; falsified if mutation succeeds or bytes change. Duplicate IDs remain loadable with their existing warning behavior. | Existing `UnsafePartialLoad` typed cause plus byte snapshot | In `prepare_mutation`, mutate before running `ensure_writable`; the existing partial-load test or new stale partial-load case observes changed memory. | Extend `partial_jsonl_load_rejects_mutation_before_changing_memory` with stale-cache setup | 1–2 seconds | PENDING — checkpointed-build, persistence slice |
| C4 | Same-instance MCP mutation preserves external state for both Workspace selectors. | Current context, explicit root, twelve mutators | Cache one `Tools`, perform a direct out-of-band JSONL edit, invoke table-driven representative mutation families through current and explicit Workspace selection, then parse source; falsified if the sentinel disappears. The direct pre-call parse is the positive control. | Direct JSONL parsing and expected domain fields constructed independently of MCP responses | In any wrapper mutation, bypass `prepare_mutation`; its table row loses the sentinel. | New MCP integration stale-cache mutation test covering every mutator family | 3–5 seconds | PENDING — checkpointed-build, MCP slice |
| C5 | Own saves advance revision and MCP writers stay serialized. | Two sequential and two concurrent mutations | Perform sequential then synchronized concurrent mutations on one cached Workspace; falsified by `ExternalChange`, lost issue, or missing mutation. Another cause is ID collision, so assertions use distinct deterministic targets rather than generated-ID equality. | Final canonical record set parsed directly from disk | Omit the revision update after save; a missing-to-present own save followed by external deletion is missed, and `stale_source_own_save_revision_detects_later_deletion` turns red. | Core sequential-save and later-deletion revision tests plus existing MCP concurrent test extended to persisted record assertions | 2–4 seconds | PENDING — checkpointed-build, persistence/MCP slices |
| C6 | Revision overhead stays bounded without file-sized allocation. | Unchanged 10,000-record JSONL | Inspect one instrumented stress run and record scan/reload counts plus peak buffer; falsified by more than three scans, any unchanged-source reload, or a buffer growing with file size. Timing alone is not decisive because CI load can mimic regression. | Test-only counters at the private revision scanner and reload branch, compared with independently counted operation boundaries | N/A — approved risk: no permanent production-performance fence; retaining test-only counters or timing thresholds would add global state/flakiness solely for implementation accounting. | N/A — approved risk: one-shot checkpoint stress measurement only | 5–15 seconds | PENDING — checkpointed-build, persistence slice |
| C7 | Opaque revision tracking preserves canonical content behavior at scale. | Empty, Unicode, multiline, duplicate warning, 10,000 records | Run canonical loader/save assertions before and after guarded mutation; falsified by changed record count/content or warning class. A direct record-count/content oracle distinguishes loader loss from hash mismatch. | Direct line count and selected `serde_json::Value` fields | Stop `SourceRevision::read` after its first 64 KiB buffer; `stale_source_revision_scans_every_buffer` turns red, and the 10,000-record last-record external edit is no longer protected. | `stale_source_revision_scans_every_buffer`, the 10,000-record guarded-mutation stress fixture, and existing mixed-record fixture tests | 5–15 seconds | PENDING — checkpointed-build, persistence slice |
| C8 | Persistence knowledge stays in the JSONL Adapter and typed error seam. | Placement/invariant | Compile MCP and core tests while matching the concrete error variant; falsified by MCP/CLI hashing code, string matching, or an untyped storage failure. | Rust exhaustive/type checking plus owning-crate tests | Return `StorageError::InvalidFormat` from `ensure_source_unchanged` instead of `StorageError::ExternalChange`; the typed post-mutation conflict fence turns red. | Storage error display/conversion test, typed post-mutation conflict test, and MCP error conversion test | <1 second | PENDING — checkpointed-build, persistence slice |

## Non-goals and future work

- Permanent non-goal: refresh read-only MCP queries. They cannot cause the destructive save in this Bug; mutation/save is the guarded interface.
- Intended future work: serialize cooperating CLI and MCP processes across the full load/mutate/save transaction under `rivets-j13o`.
- Permanent non-goal: guarantee against an arbitrary process that writes during the final comparison-to-rename window without participating in Rivets locking; portable atomic rename is not a filesystem compare-and-swap.
- Permanent non-goal: add freshness behavior to in-memory or PostgreSQL adapters; they do not write this JSONL source.

## Falsifier run log

- 2026-08-29 — `cargo test -p rivets test_jsonl_reload_restores_disk_state` — **PASS**: 1 passed, 626 filtered out.

## Approval

- Requester words: “Approve design and C6 risk.”
- Date: 2026-08-29
- Approved risk acceptances: C6 lacks a permanent performance-count/timing fence; the checkpoint records a one-shot 10,000-record measurement instead.
