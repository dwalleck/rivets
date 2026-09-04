# Design: durable Workspace mutation lock

## Route and inputs

- Route: **Empirical**, from `route.md`.
- Behavior source: `route.md` T4; `spec.md` is N/A because the Issue acceptance criteria fully specify observable behavior.
- Required behavior:
  - Existing-Workspace CLI and MCP mutations acquire one Workspace-scoped durable lock before authoritative load/reload, hold it through validation, mutation, atomic save, and save-failure reload, then release it.
  - Same-Workspace contention returns typed retryable Workspace Busy without changing persisted bytes.
  - Different Workspaces remain independently writable.
  - Process exit/crash releases the lock without stale cleanup.
  - Existing single-process lifecycle and restart behavior remains unchanged.
  - Init, reads, and MCP context selection do not take the mutation lock; network-distributed locking is not introduced.
- Empirical source: `evidence.md`, `probe.rs`, and `oracle.py`.
- Validated premises: Rust 1.94 `File::try_lock` exposes typed nonblocking contention, distinct lock files are independent, killed holders release on handle close, separate same-process descriptors contend, and supported Unix/Windows implementations share the documented nonblocking exclusive contract.
- Empirical comparison: Rust probe and direct Python `fcntl.flock` oracle both returned `BUSY`, `ACQUIRED`, `ACQUIRED`, `BUSY` for same Workspace, different Workspace, killed holder, and second same-process descriptor respectively.

## Input shapes

| Shape | Status |
|---|---|
| Canonical Workspace root; relative, symlinked, Unicode, and space-containing aliases | Covered by C1 |
| Missing persistent sidecar versus existing empty sidecar | Covered by C1 |
| Lock free, lock busy, and non-contention lock I/O failure | Covered by C1, C7, and C8 |
| Holder returns normally, drops guard on error, or is killed | Covered by C5 |
| Same Workspace versus two distinct Workspaces | Covered by C5 and C6 |
| CLI command absent, init, read-only commands, and every existing-Workspace mutator family | Covered by C2 |
| CLI empty/single/multi-target batches and interactive confirmation/prompt paths | Covered by C2 and C9 |
| MCP current context versus explicit `workspace_root` | Covered by C3 |
| MCP cache hit, cache miss, eviction/duplicate cache instance, and injected in-memory test storage | Covered by C3 and C4 |
| All twelve MCP mutators; read-only tools and context selection | Covered by C3 |
| Complete, partially loaded, externally revised, missing, and malformed JSONL | Covered by C4 and C9 |
| Contention metadata consumed through core, CLI, Rust MCP Tools, and JSON-RPC MCP surfaces | Covered by C7 |
| Network filesystems or multiple machines | N/A — permanent non-goal: the accepted Issue explicitly excludes network-distributed locking |
| Non-cooperating editors that ignore the advisory sidecar | N/A — permanent non-goal: `rivets-omq0` protects completed external revisions; this lock coordinates Rivets writers only |
| Automatic retry, backoff, fairness, leases, or stale-file deletion | N/A — permanent non-goal: the contract requires immediate retryable Busy and handle-lifetime release, not waiting or lease ownership |

## Subtractive-change sweep

Purely additive serialization guard: no validation, ordering, uniqueness, partial-load protection, source-revision check, or atomic-save constraint is removed. The design strengthens the existing process-local serialization with a cross-process invariant. Existing same-process concurrent MCP mutations may now observe immediate Workspace Busy instead of waiting behind the cached storage lock; this is the required contention contract, not a removed safety guarantee.

## Placement

### Capability: Workspace mutation ownership

- **Owner:** new `rivets::workspace_lock` module. Workspace identity and the load-to-save transaction belong to the core `rivets` crate; `rivets-jsonl` remains a generic format library and neither adapter reimplements OS locking.
- **New seam, chosen shape:** `WorkspaceMutationLock::try_acquire(workspace_root: &Path) -> Result<WorkspaceMutationLock>`. The private file handle is the guard; its lifetime is the transaction lifetime. It canonicalizes the existing Workspace root, opens `.rivets/workspace.lock` read/write/create without truncation, exhaustively maps `TryLockError::WouldBlock` to Workspace Busy and preserves other I/O causes, and never removes the sidecar.
- **Alternative A — lock `issues.jsonl`:** rejected because atomic rename changes the path's inode and Windows replacement/region-lock interaction is platform-sensitive.
- **Alternative B — lock inside `JsonlBackedStorage::prepare_mutation`:** rejected because `IssueStorage` separates mutation from `save`, so it cannot own one guard from pre-load through save without hidden transaction state and omitted-save hazards.
- **Alternative C — process-local Tokio lock:** rejected because separate CLI/MCP processes do not share it and cache eviction can leave duplicate live storage instances.
- **Forbidden:** no blocking `File::lock`, no sidecar unlink-on-drop, no per-command lock-file naming, no string matching on lock errors, and no locking implementation in CLI, MCP, or `rivets-jsonl`.

### Capability: CLI transaction lifetime

- **Owner:** `App` construction plus `Commands` mutation classification in `rivets::cli`.
- **New seam:** `App::from_directory_for_mutation` acquires the guard after Workspace discovery but before config/storage load and stores it privately until `App` drops. `load_app_from_cwd(command)` chooses locked or unlocked construction from one exhaustive `Commands::mutates_workspace` match. Init stays outside because it creates the Workspace; query commands use unlocked `App`.
- **Forbidden:** handlers may not acquire locks independently or load a second mutable App. Batch commands hold one guard for the whole command. Interactive mutation prompts may hold the guard; contenders fail immediately rather than wait.

### Capability: MCP transaction lifetime

- **Owner:** `Tools` mutation-storage resolver, using canonical Workspace identity supplied by `Context`.
- **New seam:** private `mutation_storage_for` acquires the core guard before cache initialization or cached reload, obtains the storage write guard, reloads under the durable lock, and returns one private aggregate whose fields keep storage and OS guards alive through `save_or_reload`. Every mutator uses it; queries retain `storage_for`.
- **Test-only shape:** `Context::set_test_workspace` explicitly marks injected ephemeral storage as no-sidecar under `cfg(test)`; production cache entries are always lock-bearing. Production JSONL behavior cannot silently choose the test path.
- **Forbidden:** no Context lock is held while attempting the OS lock; no durable lock for queries or `set_context`; no mutation may call the query-only storage resolver.

### Capability: retryable error semantics

- **Owner:** core `Error` owns typed Workspace Busy and causal lock I/O; MCP adapter owns JSON-RPC mapping.
- **New seam:** CLI displays a retry instruction through the typed core error. MCP Tools preserve a first-class Busy variant; `to_mcp_error` returns server error data containing `retryable: true` and canonical `workspace_root`. Other lock failures retain path plus `io::Error` source and are not mislabeled retryable.
- **Forbidden:** Busy is not invalid user input, ordinary lock I/O is not Busy, and no adapter branches on display text.

### Sidecar lifecycle

- Stable path: `<canonical-workspace>/.rivets/workspace.lock`.
- Open mode: read + write + create + no truncate, satisfying Windows and preserving stable file identity.
- The empty sidecar persists permanently; dropping all handles releases only the OS lock.
- New Workspace `.rivets/.gitignore` includes `workspace.lock`; this repository's existing `.rivets/.gitignore` is synchronized in the same change.

## Claims

- **C0:** Rust 1.94's standard file lock satisfies the validated nonblocking cross-process primitive contract.
- **C1:** One canonical Workspace maps to one persistent sidecar and typed acquisition result, while distinct Workspaces remain distinct.
- **C2:** CLI classification locks every existing-Workspace mutator before App storage load and leaves init/read-only commands unlocked.
- **C3:** Every MCP mutator, for current or explicit Workspace and cache hit or miss, holds the same durable lock through reload, mutation, save, and recovery.
- **C4:** A held lock outranks malformed config, stale cache, and malformed JSONL, proving authoritative mutable state is not loaded before ownership.
- **C5:** Same-Workspace contention and holder crash never change Issue bytes or leave stale ownership.
- **C6:** Different Workspaces can mutate concurrently without a global serialization point.
- **C7:** Workspace Busy remains typed and retryable through core, CLI, MCP Tools, and JSON-RPC, while other lock I/O remains causal and non-retryable.
- **C8:** Lock acquisition is nonblocking and bounded; ordinary contention never waits for the holder.
- **C9:** After Busy, retrying against freshly loaded state preserves both successful mutations with no lost update or partial JSONL.

## Falsification

| # | Claim | Input shape | Falsifier | Oracle | Named mutation | Regression fence | Cost | Status |
|---|---|---|---|---|---|---|---|---|
| C0 | Standard lock primitive matches the empirical contract. | Same/different files, killed holder, same-process second descriptor | Run Rust probe and independent Python oracle; falsified by any item mismatch. Source/API failure could mimic compile failure, so output comparison starts only after successful compile. | Direct Python `fcntl.flock` plus current Rust std source audit | In production `WorkspaceMutationLock::try_acquire`, map `TryLockError::WouldBlock` through the generic I/O variant; the core typed-Busy contention fence turns red. | Core typed contention, independence, drop-release, and nonblocking tests | <1 second | PASS |
| C1 | Canonical Workspace identity owns one persistent typed lock. | Relative/symlink/Unicode/spaces; missing/existing sidecar; Busy/I/O | Acquire through two aliases and a second Workspace; falsified if aliases both acquire, other Workspace is Busy, sidecar is removed, or I/O becomes Busy. A global lock path could also block the second Workspace, so all cases run together. | Canonical paths from `std::fs::canonicalize`, distinct path identity, and direct sidecar existence checks | Replace the Workspace-relative sidecar with one global temp-directory lock path; the distinct-Workspace acquisition returns Busy and its fence turns red. | Core `workspace_lock` alias/independence/error tests | 1–2 seconds | PENDING — checkpointed-build, core lock slice |
| C2 | CLI locks exactly existing-Workspace mutators before load. | Init/read commands and all mutation families; malformed config | Hold the sidecar and invoke real CLI mutation plus read command; mutation must return Busy with byte-identical source, read must succeed. With malformed config and held lock, Busy must win. Falsified by mutation success, config error precedence, or locked read. | Direct process exit/stderr plus pre/post SHA-256 bytes and independently parsed command classification table | Classify `Commands::Create` as read-only; held-lock real `create` succeeds or reaches storage instead of returning Busy. | CLI command-classification unit test and real-process lock-precedence test | 2–4 seconds | PENDING — checkpointed-build, CLI slice |
| C3 | Every MCP mutator uses the durable transaction resolver. | Twelve mutators; current/explicit roots; hit/miss/duplicate cache; injected tests | Hold the sidecar and invoke each real Tools mutator; every production JSONL row must return typed Busy without byte change, while queries succeed. Falsified by any successful mutation or wrong error. Positive control drops the guard and repeats one mutation successfully. | Direct JSONL byte snapshots and exhaustive expected mutator inventory independent of helper implementation | Route `label_add` through query-only `storage_for`; its held-lock row succeeds and turns red. | MCP all-mutator busy table plus cache-hit/miss tests | 3–6 seconds | PENDING — checkpointed-build, MCP slice |
| C4 | Ownership precedes authoritative load/reload. | Malformed config; malformed/stale JSONL; cache miss/hit | Hold the lock over a malformed Workspace and call CLI locked constructor and MCP explicit cache miss; Busy must outrank parse/load errors. For a cache hit, Busy must outrank reload. Falsified by config, JSON, or partial-load error. | Error-variant precedence with unchanged direct bytes | Move lock acquisition after `create_storage` in `App::from_directory_for_mutation`; malformed config wins and the CLI precedence fence turns red. | Core App precedence test and MCP cache miss/hit precedence test | 2–4 seconds | PENDING — checkpointed-build, CLI/MCP slices |
| C5 | Contention/crash preserves bytes and releases ownership. | Holder live, normal drop, killed holder | While holder lives, contender returns Busy and source hash remains exact; kill holder, then acquisition succeeds without deleting sidecar. Falsified by write, stale Busy, or missing sidecar. | Direct SHA-256 bytes plus child-process liveness/exit status | Leak a cloned locked file descriptor from a faulty guard `Drop`; normal-drop reacquisition remains Busy and the release fence turns red. | Real-process holder/kill/reacquire integration test | 2–4 seconds | PENDING — checkpointed-build, core lock slice |
| C6 | Different Workspaces do not block each other. | Two canonical Workspace roots with simultaneous mutations | Hold/mutate A while mutating B; falsified if B returns Busy or waits beyond the bounded channel timeout. Another cause is invalid B fixture, so B first succeeds as a positive control. | Separate direct JSONL record sets and process completion channels | Replace sidecar path with one global temp path; B returns Busy and its fence turns red. | Parallel two-Workspace CLI/MCP integration test | 2–4 seconds | PENDING — checkpointed-build, integration slice |
| C7 | Busy is retryable and other lock I/O remains causal. | Core/CLI/Tools/JSON-RPC; Busy versus permission/open error | Cause contention and an open failure; falsified if variants collapse, source path/cause is lost, CLI lacks retry wording, or MCP data lacks `retryable: true` and canonical root. | Typed enum matching, `Error::source`, and direct JSON-RPC data inspection | Map MCP Busy through the generic storage arm; retryable metadata assertion turns red. | Core error tests, CLI stderr process test, MCP conversion/router test | 1–3 seconds | PENDING — checkpointed-build, adapter slices |
| C8 | Contention is nonblocking. | Live holder under same Workspace | Time a contender through a channel with a 1-second upper fence; falsified if it does not return typed Busy before timeout. Fixture proves holder remains live throughout. | Child liveness plus monotonic timeout independent of lock implementation | Replace `try_lock` with blocking `lock`; contender misses timeout and turns red. | Core nonblocking contention timeout test | 1–2 seconds | PENDING — checkpointed-build, core lock slice |
| C9 | Retry after Busy preserves both mutations. | Two synchronized same-Workspace writers, one retry | Start two real mutations; require one success and one Busy, retry loser after release, then directly parse exactly both changes and valid JSONL. Falsified by lost record, partial file, two first-attempt successes, or failed retry. | Direct canonical JSONL parse/set comparison, not CLI/MCP responses | Release the guard before `save`; both first attempts can enter save and final set/first-attempt outcome fence turns red. | Synchronized real-process writer/retry integration test | 3–6 seconds | PENDING — checkpointed-build, integration slice |

## Non-goals and future work

- Permanent non-goal: lock Workspace initialization; no existing Workspace or mutable snapshot exists yet, and ADR-0006 intentionally keeps init CLI-only.
- Permanent non-goal: lock reads or MCP context selection; they do not overwrite state and the accepted contract excludes them.
- Permanent non-goal: coordinate non-Rivets writers or network-distributed filesystems/machines; advisory local-host coordination is the accepted scope, while completed direct edits remain protected by `rivets-omq0` source revisions.
- Permanent non-goal: automatic retries, fairness, leases, stale-owner metadata, or deleting the sidecar; immediate typed Busy and OS handle lifecycle are the selected contract.
- New Workspace initialization pre-creates the empty sidecar and ignores it; older Workspaces create it on first mutation.
- Intended future work: atomic Assignment claim/release consumes this lock under verified `rivets-8rj9`.

## Falsifier run log

- 2026-08-29 — `rustc --edition 2024 .rivets-j13o/probe.rs -o /tmp/rivets-j13o-probe && /tmp/rivets-j13o-probe` and `python3 .rivets-j13o/oracle.py` — **PASS**: both produced `BUSY`, `ACQUIRED`, `ACQUIRED`, `BUSY` item-for-item for C0/P1–P4; current Rust 1.94 source confirmed C0/P5.

## Approval

- Requester words: “Approve durable lock design.”
- Date: 2026-08-29
- Approved risk acceptances: None.
- Post-approval mechanical correction: C1 and C5 named mutations were sharpened after applicability checks; scope, placement, behavior, and risk acceptance are unchanged.
