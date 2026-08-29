# Plan: prevent stale JSONL replacement

## Partition and review budget

- Slice diff estimates: 360 + 150 = 510 changed lines.
- Churn margin: 40% (204 lines), because typed-error exhaustiveness, twelve async wrapper callsites, and stress-fixture setup may expand while preserving the approved interface.
- Projected total: 714 changed lines.
- Review-size gate: PASS — 714 is below 4,000.
- PR increments: one, **stale-source guard**.
  - Slices: 1–2.
  - Mergeable definition: typed JSONL source-revision behavior, every wrapper caller migrated atomically, core persistence fences, MCP same-instance regression fence, synchronized documentation, and focused verification all land together.
  - Independent verification: core adapter tests prove revision/conflict semantics; MCP integration drives the public tool seam. No later increment is required.

## Slice 1: Deepen the JSONL Adapter with source revisions and typed conflict behavior

**Claim IDs:** C0, C1, C2, C3, C5, C6, C7, C8

**Expected behavior:** Every JSONL-backed mutation refreshes a completed external source revision before changing memory; save rejects a newer source revision with typed `StorageError::ExternalChange` before opening output; successful save/reload advances the captured revision; partial loads remain non-writable.

**Oracle:** Direct source-byte snapshots and independent `serde_json::Value` line parsing, plus concrete `StorageError` enum matching. These do not depend on the resilient loader's in-memory result.

**Stress fixture:** Empty/missing source transitions; Unicode and multiline records; a malformed record; two sequential own saves; an external edit between mutation and save; and 10,000 canonical records. Expected: exact external bytes on conflict, exact record count/content after safe refresh, no false conflict after own save, and no partial-load write.

**Regression fence:** Core tests in `crates/rivets/src/storage/mod.rs`: stale pre-mutation merge, post-mutation typed conflict/byte preservation, partial stale load, sequential revision advancement, missing-file transition, and 10,000-record guarded mutation.

**Named mutation:** C0 remove `self.inner` replacement in reload; C1 skip reload on revision mismatch; C2 remove pre-save revision comparison; C3 run the partial-load guard before reload but not after; C5 omit revision update after save and delete a source created by that save; C7 stop revision scanning after the first 64 KiB buffer; C8 return typed `InvalidFormat` instead of `ExternalChange`. C6: N/A — approved risk: no fence to mutate.

**Complexity/production scale:** Revision scan is $O(B)$ time and $O(1)$ additional memory with a fixed 64 KiB buffer for source size $B$. Ordinary mutation performs at most three scans plus the existing $O(B)$ atomic rewrite; changed source performs one canonical $O(B)$ reload before mutation. Production fixture: 10,000 Issues / up to 20 MiB JSONL. Maximum accepted structural cost: three scans, one reload only when changed, one fixed buffer; rationale is no worse asymptotic behavior or file-sized duplicate allocation beyond the existing full-file persistence path.

**Wall budget/phase:** Always-on revision scans on each JSONL mutation/save. One-shot checkpoint budget: a warmed 10,000-record guarded mutation must complete within 2 seconds on this workstation; rationale is keeping an interactive MCP mutation below a human-visible multi-second stall. C6's approved risk means no permanent timing assertion remains.

**Files:** `crates/rivets/src/error.rs`, `crates/rivets/src/storage/mod.rs`, `crates/rivets/src/storage/in_memory/mod.rs`, `crates/rivets/src/storage/in_memory/jsonl.rs`, `docs/architecture.md`, `docs/module-structure.md`, `docs/storage-architecture.md`.

**Estimate:** 2–4 hours.

**Diff estimate:** 360 changed lines including tests and documentation.

**PR increment:** stale-source guard.

**Commands and expected results:**
- `cargo test -p rivets storage::tests::test_jsonl_reload_restores_disk_state` → C0 remains green.
- `cargo test -p rivets stale_source` → direct parsed records retain external additions/updates/deletions; interleaved save returns typed `ExternalChange`; source bytes stay exact; malformed stale input refuses mutation; sequential saves do not false-conflict; missing/present transitions work.
- Temporarily apply each named mutation, run its named fence, restore, and rerun → fence is red under the mutation and green after restoration, with the claimed field/byte/error mismatch identifying the claim.
- `cargo test -p rivets stale_source_10k -- --ignored --nocapture` after warming the test binary → 10,000 records remain, selected Unicode/multiline values match the direct oracle, scanner uses a fixed buffer, and elapsed mutation is at most 2 seconds; record the one-shot result, then retain no timing assertion.

## Slice 2: Fence the same-instance MCP mutation seam

**Claim IDs:** C4

**Expected behavior:** One cached `Tools` instance, through current context and explicit `workspace_root`, preserves a valid out-of-band JSONL sentinel when each MCP mutator family executes; the requested mutation either applies atop refreshed state or returns typed conflict without a write.

**Oracle:** Parse `issues.jsonl` directly as `serde_json::Value` records before and after each call and compare the independently written sentinel plus the operation-specific expected field/relationship.

**Stress fixture:** A table-driven same-instance lifecycle exercises create, update, add Note, Resource add/update/remove, close/reopen, Blocking Dependency add/remove, and Label add/remove, alternating current and explicit Workspace selection. Before each family, write a distinct valid sentinel externally. Expected: every sentinel survives and each successful operation's distinct observable survives.

**Regression fence:** `crates/rivets-mcp/tests/integration.rs` same-instance stale-cache mutation test covering all mutator families and both Workspace selectors.

**Named mutation:** In one `JsonlBackedStorage` wrapper mutator, bypass `prepare_mutation`; the corresponding table row loses its sentinel and turns the fence red.

**Complexity/production scale:** No new loop beyond Slice 1's revision scanner. The table-driven test is test-only $O(MB)$ over mutators $M=12$ and fixture bytes $B$; production calls remain one mutator at a time.

**Wall budget/phase:** N/A — no new runtime phase beyond Slice 1; this slice adds only an integration fence.

**Files:** `crates/rivets-mcp/tests/stale_cache.rs`.

**Estimate:** 1–2 hours.

**Diff estimate:** 150 changed lines.

**PR increment:** stale-source guard.

**Commands and expected results:**
- `cargo test -p rivets-mcp stale_cache` → every current/explicit Workspace row retains its direct-write sentinel and requested operation result.
- Bypass the shared guard in a representative wrapper mutator, run the focused test, restore, rerun → that mutation's sentinel assertion is red under mutation and green after restoration.

## Tracker taxonomy

- Read-only freshness is a permanent non-goal because it cannot trigger destructive persistence.
- Cross-process serialization for cooperating CLI/MCP writers is intended future work verified under `rivets-j13o`.
- Arbitrary non-cooperating writes during the final compare-to-rename window are a permanent non-goal because portable atomic rename is not compare-and-swap.
- Non-JSONL adapter freshness is a permanent non-goal because those adapters do not persist this source.

## Self-review

- [x] Every design claim C0–C8 is assigned exactly once; each PENDING falsifier has an owning slice.
- [x] Both slices contain all thirteen mandatory fields with explicit N/A rationales where applicable.
- [x] Each claim's permanent fence and named mutation land in its implementing slice; C6 copies the approved no-fence risk.
- [x] New loops state asymptotic cost, production size, structural maximum, and the always-on phase has a wall budget.
- [x] Partition arithmetic includes a documented churn margin and one independently verifiable increment.
- [x] Deferrals are classified; intended future work cites verified `rivets-j13o`.
- [x] No slice is declared complete; checkpointed-build owns completion.
