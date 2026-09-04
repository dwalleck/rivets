# Plan: durable Workspace mutation lock

## Partition and review budget

- Slice 1 estimate: 600 changed lines.
- Slice 2 estimate: 1,500 changed lines.
- Sum: 2,100 changed lines.
- Churn margin: 35% (735 lines), because exhaustive CLI/MCP caller migration, process synchronization fixtures, and typed error propagation commonly expand after compile-time impact analysis.
- Projected total: 2,835 changed lines.
- Review-size gate: PASS — 2,835 is below 4,000.
- PR increments: one, **durable Workspace mutation lock**.
  - Slices: 1–2.
  - Mergeable definition: core lock/error/sidecar lifecycle, all CLI and MCP mutation wiring, direct process/oracle fences, synchronized docs, and full quality gates land together.
  - Independent verification: Slice 1 proves the core primitive without adapters; Slice 2 drives real CLI/MCP interfaces and direct JSONL oracles. No later increment is required.

## Slice 1: Add the canonical Workspace lock module, typed errors, and sidecar lifecycle

**Claim IDs:** C0, C1, C5, C8

**Expected behavior:** One canonical Workspace root maps to one persistent `.rivets/workspace.lock`; acquisition is exclusive and nonblocking, same-Workspace contention is typed Busy, other I/O remains causal, different Workspaces acquire independently, and drop/kill releases without removing the sidecar.

**Oracle:** The approved empirical Rust/Python comparison, direct canonical path/sidecar checks, direct SHA-256 source snapshots, and child-process liveness/exit status.

**Stress fixture:** Relative and symlink aliases to a Unicode/space-containing Workspace; missing and existing empty sidecars; 32 distinct Workspace roots; same-process separate descriptors; a child holder killed without unlock; an unreadable/missing-parent error path. Expected: aliases contend, all distinct roots acquire, killed holder releases, sidecar persists, Busy and I/O variants remain distinct.

**Named mutation:** C0 map `TryLockError::WouldBlock` as generic I/O; C1 replace the Workspace-relative sidecar with one global temp path; C5 leak a cloned locked descriptor from a faulty guard `Drop`; C8 replace `try_lock` with blocking `lock`. Each mutation must turn its named fence red, then restore green.


**Complexity/production scale:** No collection loop. Acquisition is $O(P)$ for canonical path length $P$, one read/write/create open, and one nonblocking OS syscall; memory is $O(P)$. Production maximum: 4 KiB path representation and 32 simultaneous distinct Workspaces. Accepted structural bound: one canonicalization, one open, one `try_lock`, no retry loop and no sleep.

**Wall budget/phase:** Always-on once wired to each mutation. On a warmed local filesystem, free and contended acquisitions must each complete within 50 ms; permanent nonblocking fence fails at 1 second to catch accidental blocking. Rationale: Busy must be immediately actionable and far below interactive latency.

**Files:** `crates/rivets/src/workspace_lock.rs` (new), `crates/rivets/src/lib.rs`, `crates/rivets/src/error.rs`, `crates/rivets/src/commands/init.rs`, `crates/rivets/tests/workspace_lock.rs` (new), `.rivets/.gitignore`, applicable module/sidecar documentation.

**Estimate:** 3–5 hours.

**Diff estimate:** 600 changed lines.

**PR increment:** durable Workspace mutation lock.

**Commands and expected results:**
- `rustc --edition 2024 .rivets-j13o/probe.rs -o /tmp/rivets-j13o-probe && /tmp/rivets-j13o-probe` and `python3 .rivets-j13o/oracle.py` → item-for-item `BUSY`, `ACQUIRED`, `ACQUIRED`, `BUSY` agreement remains.
- `cargo test -p rivets workspace_lock` → alias, independence, Busy/I/O, nonblocking, normal drop, killed-holder release, persistent sidecar, init creation, and ignore behavior match the fixture.
- Apply C0/C1/C5/C8 named mutations separately, run their focused fences, restore, rerun → each is red for its claimed mismatch/timeout and green after restoration.
- One-shot warmed acquisition measurement over free and held sidecars → each attempt ≤50 ms; 32 distinct Workspaces all acquire.

## Slice 2: Wire one durable transaction lifetime through every CLI and MCP mutator

**Claim IDs:** C2, C3, C4, C6, C7, C9

**Expected behavior:** Exhaustive CLI classification uses locked App construction for existing-Workspace mutators and unlocked construction for init/reads. MCP current/explicit, cache hit/miss/duplicate instances, and all twelve mutators use one durable transaction resolver. Busy outranks config/load/reload errors, maps retryably through CLI/MCP, leaves bytes exact, and retry after release preserves both changes. Different Workspaces proceed independently.

**Oracle:** Real process exit/stderr, typed Rust error matching, direct JSON-RPC data inspection, direct pre/post SHA-256 bytes, and independent canonical JSONL parse/set comparison.

**Stress fixture:** Every CLI command class; all twelve MCP mutators alternating current and explicit roots; cache hit/miss plus two Contexts for one Workspace; malformed config and malformed JSONL under held lock; empty/single/multi-target CLI batches; interactive create/delete; two synchronized same-Workspace writers with one retry; simultaneous 10,000-record Workspace mutation and an independent Workspace mutation. Expected: exact Busy/success classifications, no write on Busy, both changes after retry, B unaffected by A, and 10,000 records preserved.

**Regression fence:** CLI command-classification/App precedence unit tests; new real-process `crates/rivets/tests/workspace_mutation_lock.rs`; MCP Tools/server error tests; new `crates/rivets-mcp/tests/workspace_lock.rs` all-mutator/cache/concurrency tests; existing lifecycle/restart and stale-source fences.

**Named mutation:** C2 classify Create unlocked; C3 route MCP `label_add` through query-only storage; C4 move lock acquisition after CLI storage load; C6 use one global sidecar path; C7 map MCP Busy through generic storage/internal data without retry metadata; C9 drop the App/MCP guard before save. Each mutation must turn the exact public fence red, then restore green.

**Complexity/production scale:** CLI adds Slice 1's $O(P)$ acquisition before its existing $O(B)$ JSONL load. MCP mutation adds one guarded canonical `reload`, $O(B)$ time and the loader's existing $O(B)$ memory, before mutation/save for JSONL size $B$; no retry loop. Production fixture: 10,000 Issues / up to 20 MiB JSONL, 12 tool intents, 32 cached Workspaces. Accepted maximum: one OS acquisition and one reload per MCP mutation; no global lock; independent Workspace B completes while A is held.

**Wall budget/phase:** Always-on mutation phase. Warming excluded, a free-lock 10,000-record CLI or MCP mutation must complete within 2 seconds; a contended attempt must return within 50 ms. Rationale: prior 10,000-record load/mutation evidence is ~62 ms on this workstation, while 2 seconds leaves broad CI/filesystem margin without allowing human-visible indefinite waits.

**Files:** `crates/rivets/src/app.rs`, `crates/rivets/src/cli/mod.rs`, `crates/rivets/tests/workspace_mutation_lock.rs` (new), `crates/rivets-mcp/src/context.rs`, `crates/rivets-mcp/src/tools.rs`, `crates/rivets-mcp/src/error.rs`, `crates/rivets-mcp/src/server.rs`, `crates/rivets-mcp/tests/workspace_lock.rs` (new), affected MCP unit fixtures, `docs/architecture.md`, `docs/data-flow.md`, `docs/module-structure.md`, `docs/storage-architecture.md`, and applicable crate READMEs.

**Estimate:** 6–10 hours.

**Diff estimate:** 1,500 changed lines.

**PR increment:** durable Workspace mutation lock.

**Commands and expected results:**
- `cargo test -p rivets workspace_mutation_lock` → real CLI mutators return retryable Busy with byte-identical source under a held lock; read succeeds; malformed config loses to Busy; distinct Workspace succeeds; synchronized retry leaves exactly both changes.
- `cargo test -p rivets-mcp workspace_lock` → all twelve mutators return typed Busy under a held production lock for current/explicit/cache hit/miss shapes; query positive controls succeed; dropping the guard permits mutation; JSON-RPC data is `retryable: true` with canonical root.
- `cargo test -p rivets-mcp test_concurrent_workspace_root_initialization` → updated expected contention contract is one durable winner plus retry, not two uncoordinated first-attempt successes.
- Apply C2/C3/C4/C6/C7/C9 named mutations separately, run their public fences, restore, rerun → each is red for the named command/tool/error/record mismatch and green after restoration.
- `cargo test -p rivets stale_source && cargo test -p rivets-mcp --test stale_cache` → partial-load/source-revision and all-mutator external-edit protection remain unchanged.
- One-shot warmed 10,000-record CLI/MCP measurement → free mutation ≤2 s, held-lock Busy ≤50 ms, direct JSONL oracle retains 10,000 records.

## Branch coverage checklist

- `WorkspaceMutationLock::try_acquire`: canonicalization success/error; open success/error; `try_lock` success/Busy/other error; missing/existing sidecar; drop and killed process.
- `Commands::mutates_workspace`: absent command; Init; seven read-only families; eight mutating families, with Blocking Dependency/Label/Resource sub-actions still classified as mutating at the command family.
- `App`: unlocked constructor; locked constructor free/Busy; config/storage failure after acquisition; guard retained through success and early handler error.
- MCP resolution: current/explicit; no context; cache hit/miss; test-injected ephemeral; Context initialization error; storage lock/reload error; Busy before each production load/reload.
- MCP protocol: Busy versus lock I/O versus existing invalid-params errors; retryable data present only for Busy.
- Persistence: save success; save conflict; save failure followed by reload while guard lives; retry after Busy.

## Tracker taxonomy

- Permanent non-goals: Workspace initialization locking, read locking, non-Rivets writers, network-distributed filesystems/machines, automatic retries/fairness/leases, and sidecar deletion; rationales remain in approved `design.md`.
- Intended future work: atomic Assignment claim/release under verified `rivets-8rj9`.

## Self-review

- [x] C0–C9 are assigned exactly once; every PENDING falsifier has one owning slice.
- [x] Both slices contain all thirteen mandatory fields with explicit N/A only where no new loop exists.
- [x] Every claim's fence and named mutation land with its implementation; there are no approved fence gaps.
- [x] Path, acquisition, reload complexity and always-on wall budgets are explicit at production scale.
- [x] Projected total with churn is 2,835 lines, below the 4,000-line partition threshold; both slices name the single increment.
- [x] Tracker deferrals are classified and the only intended future work cites verified `rivets-8rj9`.
- [x] No slice is declared complete; checkpointed-build owns completion.
