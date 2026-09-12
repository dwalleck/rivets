# PR 102 Review Findings

> **Resolution status — verified against `main` @ `a8aae7f`.**
>
> This document is retained as a provenance record of the 2026-08-30 review of
> PR #102. Every finding below was re-verified against current `main`, and the
> original text is preserved unmodified beneath this block.
>
> - **All 15 reported findings are RESOLVED on main.** Each has a regression
>   fence; representative examples: finding 1 (a persisted `"status":"blocked"`
>   record no longer bricks a workspace) is fixed by the `PersistedIssueStatus`
>   decode migration and pinned by
>   `assignment_state_migration_is_visible_and_idempotent`; finding 6 by
>   `same_server_mutations_serialize_and_both_persist`; finding 7 by
>   `reopen_rejects_in_progress_while_return_to_open_retains_assignment`;
>   finding 8 by `relationship_mutators_refresh_stale_source_before_change`;
>   finding 9 by `claim_and_release_reject_blank_assignees_without_mutation`;
>   finding 11 by `first_mutation_upgrades_existing_workspace_ignore_idempotently`;
>   finding 13 by `mock_storage_assignment_mutations_return_typed_errors`.
> - **8 of the 9 cut findings are RESOLVED.** The exception is cut #1
>   (`.unwrap()` vs `.expect()` in test code), which is unfixed and now tracked
>   as **rivets-u940**.
> - **One further residual was found while verifying finding 15** and is tracked
>   as **rivets-cru3** (`workspace_mutation_lock::spawn_prompted` bypasses the
>   shared helper's macOS `NotFound` spawn retry).
>
> No finding in this document is an open, untracked defect.

**Review:** `/code-review xhigh PR 102` — 2026-08-30
**Process:** 10 independent finder angles → 1-vote verification → gap sweep → ranked findings, capped at 15.
**Totals:** 24 real findings (15 reported, 9 cut for the cap as lower-severity cleanup). 3 further candidates were eliminated by verification (see appendix).

---

## Reported findings (15, ranked most-severe first)

### 1. Removing `Blocked` status bricks legacy workspaces — CONFIRMED

`crates/rivets/src/storage/in_memory/issue_record.rs:282` · correctness

Removing `IssueStatus::Blocked` with no decode migration makes any pre-PR workspace containing a persisted `"status":"blocked"` record permanently read-only and silently hides that issue.

**Failure scenario:** Empirically confirmed with the PR-built binary: a workspace whose `issues.jsonl` contains `{"status":"blocked",...}` (settable on main via `rivets update X --status blocked`) loads with `LoadWarning::MalformedJson` (serde fails before the `into_domain` migration seam), the issue vanishes from `list`/`show`, and every create/update/claim/close on ANY issue fails with "Refusing to modify storage after an incomplete JSONL load" — the workspace is bricked until the user hand-edits the JSONL, while this same PR adds automatic migrations for the two adjacent legacy states (unassigned `in_progress`, assigned `closed`).

### 2. MCP `update` silently ignores old-contract `assignee` field — CONFIRMED

`crates/rivets-mcp/src/models.rs:200` · correctness

`UpdateParams` dropped the `assignee` field without `deny_unknown_fields`, and `Tools::update` has no at-least-one-field guard, so old-contract MCP update calls sending `assignee` succeed while silently changing nothing.

**Failure scenario:** Verified against vendored rmcp 1.8.0: `Parameters<T>` is plain `serde_json::from_value` with no schema validation, and the `#[serde(flatten)]` kind field absorbs unknown keys. A client on the old contract ("Use empty string for assignee to clear it") calls `update {"issue_id":"proj-1","assignee":""}`: serde discards the key, storage applies an all-`None` `IssueUpdate` (bumping only `updated_at`), and the tool returns success — the agent believes the issue was unassigned/reassigned when nothing changed, defeating the race-safety the claim/release split was built for.

### 3. CLAUDE.md workflow now always fails without a claim step

`CLAUDE.md:50` · documentation

The repo's own CLAUDE.md workflow (`rivets ready` then `rivets update <id> --status in_progress`) now fails 100% of the time: `ready` returns only unassigned issues and the new `AssigneeRequired` rule rejects `in_progress` on unassigned issues, but CLAUDE.md was not updated to mention `claim`.

**Failure scenario:** Agent follows CLAUDE.md "Working on Issues": `rivets ready` (returns only unassigned Open issues per the new `ReadyAssignmentFilter` default) then `rivets update rivets-xyz --status in_progress` → `apply_status_transition` (`crates/rivets/src/domain/mod.rs:247`) returns "Entering in_progress requires an Assignee"; every agent session following the checked-in project instructions hits a guaranteed error (AGENTS.md and docs/agents/issue-tracker.md were updated, CLAUDE.md was missed).

### 4. Workspace lock held across interactive stdin prompts

`crates/rivets/src/cli/mod.rs:230` · concurrency

Mutation commands acquire the exclusive workspace flock at `App` construction, before execute functions that block indefinitely on interactive stdin prompts, and all contenders use fail-fast `try_acquire` — so one idle prompt makes every other writer error with `WorkspaceBusy`.

**Failure scenario:** Terminal A runs `rivets create` with no `--title` (execute.rs prompts "Title: " on stdin) or `rivets close`/`reopen`/`delete` without `--yes`/`--force`: `load_app_from_cwd(true)` already holds `.rivets/workspace.lock`; while the human sits at the prompt, every CLI mutation and every MCP claim/create/update on the workspace fails instantly with "Workspace is busy ... retry the operation", and retries never succeed until the prompt is answered or killed.

### 5. Read-only loads can fail spuriously with `ExternalChange`

`crates/rivets/src/storage/mod.rs:785` · concurrency

`create_storage`'s hash-before/load/hash-after fence also runs for read-only opens that hold no lock, so a concurrent writer's atomic-rename save landing mid-load makes pure read commands fail spuriously with `ExternalChange`.

**Failure scenario:** Terminal A runs `rivets close X` (or an MCP mutation) while terminal B runs `rivets list` on a large workspace: B's `revision_before` hash, load, and `revision_after` hash straddle A's rename, the hashes differ, and B exits with "Persistent storage changed externally" even though the snapshot it parsed was internally consistent (atomic rename guarantees that); pre-PR the read succeeded. Also hits MCP queries on uncached workspaces via `storage_for_or_init`.

### 6. Concurrent same-server MCP mutations now hit `WorkspaceBusy`

`crates/rivets-mcp/src/tools.rs:165` · concurrency

`mutation_storage_for` try-acquires the per-open-file-description flock before awaiting the in-process tokio storage write lock, so two concurrent mutation tool calls in the SAME MCP server — which pre-PR serialized and both succeeded — now nondeterministically fail with `WorkspaceBusy`.

**Failure scenario:** An AI client issues `label_add` and `close` as one parallel tool batch (standard behavior): call A holds `.rivets/workspace.lock`; call B's `File::try_lock` on a second fd returns `WouldBlock` and B fails with retryable internal_error "Workspace is busy" for two non-conflicting mutations that both succeeded on main. The PR's own integration test was weakened to accept "two serialized creates or one WorkspaceBusy" (`integration.rs:1561`), pushing retry burden onto every MCP client. Taking the storage write lock first (or acquiring the flock inside the backend) removes the failure mode with no deadlock risk.

### 7. `reopen` silently moves in-progress issues back to Open

`crates/rivets/src/cli/mod.rs:125` · correctness

`reopen` (CLI and MCP) now silently moves In Progress issues back to Open — retaining the assignee and appending a "Reopened" note — while the CLI help and MCP tool description still promise it only reopens closed issues, and the pre-PR not-closed error guard is gone.

**Failure scenario:** `rivets reopen a b c` where `b` is `in_progress` (e.g. a mistyped id in a batch of closed issues): pre-PR the command errored "Issue is not closed"; now `b` is silently un-started to Open with a spurious "Reopened" note despite never having been closed (InProgress→Open allowed at `domain/mod.rs:525`, pinned by `test_reopen_in_progress_issue_returns_to_open_with_claim`), and neither the help text nor the MCP description ("Reopen a previously closed issue") warns of the new semantics.

### 8. Four association mutators skip the `prepare_mutation` reload

`crates/rivets/src/storage/mod.rs:608` · correctness

`add_related_association`, `remove_related_association`, `add_discovery_origin`, and `remove_discovery_origin` still call only `ensure_writable()` instead of `prepare_mutation()`, skipping the reload-on-external-change step every other `JsonlBackedStorage` mutator received.

**Failure scenario:** External write (git pull, non-locking legacy binary) lands after storage load but before save: `rivets label add` absorbs it via `prepare_mutation` and succeeds, while `rivets related add A B` under the identical conditions applies the association to the stale in-memory copy and then fails at save with `ExternalChange` — divergent behavior that violates the wrapper's own doc contract ("Before mutation, an externally changed source is reloaded"); the MCP path is only masked by `mutation_storage_for`'s redundant unconditional reload, so removing that reload (the obvious perf fix) turns this into a live MCP bug.

### 9. Empty-string assignee creates a phantom claim

`crates/rivets/src/storage/in_memory/trait_impl.rs:266` · correctness

`claim`/`release`/`create` accept an empty-string assignee (`validate_text_fields` only rejects control characters), creating a phantom claim by identity `""` that hides the issue from the default unassigned Ready view.

**Failure scenario:** `claim(id, "")` — e.g. an agent porting the old MCP semantics where `assignee:""` meant clear — stores `assignee=Some("")`: the issue disappears from `rivets ready` (Unassigned requires `None`) though no one owns it, a competing claim fails with the nonsensical "Issue X is already claimed by ", and only release with assignee `""` can free it.

### 10. Claim tool description overpromises idempotency

`crates/rivets-mcp/src/server.rs:163` · documentation

The claim tool description promises "Repeating the same Claim is idempotent" with no status qualifier, but storage checks NotOpen before the same-owner idempotency arm, so a re-claim by the owner of an `in_progress` issue errors.

**Failure scenario:** Agent claims proj-1 as alice, sets it `in_progress` (the documented next step), then retries `claim(proj-1, alice)` after a reconnect, relying on the advertised idempotency: `trait_impl.rs:227-246` rejects any non-Open issue before the idempotent arm, returning INVALID_PARAMS "Assignment changes require an Open Issue; proj-1 is in_progress" — an outcome the description never enumerates (the PR's own test `in_memory_storage.rs:1546` pins this error).

### 11. Upgraded workspaces commit `workspace.lock` to git

`crates/rivets/src/commands/init.rs:263` · correctness

The `workspace.lock` gitignore entry is written only by `init()`, while `try_acquire` creates the sidecar on demand in any pre-existing workspace, so upgraded workspaces commit the lock file to git with no upgrade path.

**Failure scenario:** Any workspace initialized before this PR: the first post-upgrade mutation creates `.rivets/workspace.lock` via `OpenOptions::create(true)` (`workspace_lock.rs:52-56`); the documented "issues travel with the repo / commit .rivets" flow then stages and commits the lock file into every clone. The PR hand-edited this repo's own `.rivets/.gitignore` (`+workspace.lock` in the diff) — exactly the manual fix every other workspace silently needs.

### 12. MCP mutations pay ~5 full-file reads each

`crates/rivets-mcp/src/tools.rs:171` · efficiency

`mutation_storage_for` unconditionally `reload()`s before every MCP mutation, making the revision-checked `prepare_mutation` reload dead code on the MCP path and costing ~5 full-file reads per mutation even when nothing changed.

**Failure scenario:** Traced per single MCP claim: reload's hash-before + full `load_from_jsonl` parse/graph rebuild + hash-after (`storage/mod.rs:703`/`720`), `prepare_mutation`'s re-hash (guaranteed equal, `mod.rs:522`), and save's `ensure_source_unchanged` re-hash — 5 full reads plus a full rewrite where main did 0 reads + 1 write for a cached workspace; on a multi-MB `issues.jsonl` every 2-field mutation pays linear I/O. Fix: drop this reload and rely on `prepare_mutation`'s reload-iff-changed (after first fixing the four mutators that skip `prepare_mutation`, finding 8).

### 13. `MockStorage` claim/release panic instead of returning the typed error

`crates/rivets/src/storage/mod.rs:954` · api-contract

`MockStorage::claim`/`release` use `unimplemented!()` panics instead of the `Err(StorageError::UnsupportedOperation)` contract every other `MockStorage` mutator follows and its docs promise.

**Failure scenario:** `MockStorage` is `pub` under the shipped test-util feature and its module docs instruct downstream consumers to wire it as `Box<dyn IssueStorage>`; code exercising the new `claim()` trait method then aborts the whole test process with "MockStorage::claim() is not implemented" instead of the recoverable typed error the struct's own doc ("Mutations that require state: Return a typed unsupported-operation error", `mod.rs:844`) guarantees.

### 14. Parity registry still documents the removed `blocked` status

`docs/cli-mcp-parity.json:203` · documentation

The parity registry (and its rendered `cli-mcp-parity.md`) still asserts CLI/MCP `list` accept "legacy Blocked" and that `stats` "mixes legacy Blocked status", contradicting this PR's removal of Blocked — in a file the PR itself edited for claim/release rows.

**Failure scenario:** An integrator following rows 203/218/230/984 sends MCP `list {"status":"blocked"}` or runs `rivets list --status blocked` and gets invalid_params / clap exit 2 ("open, in_progress, closed"); rows 1053/1071 describe stats output that no longer exists. The render `--check` passes because the md faithfully renders the stale json, so nothing catches the drift.

### 15. New lock test races its own readiness probe

`crates/rivets/tests/workspace_mutation_lock.rs:172` · test-coverage

The new lock test's readiness probe acquires the workspace lock every 10ms while the child CLI performs a single fail-fast acquisition — an intermittent race that hangs the test to its 5s deadline; the same file's `run()` helper also bypasses `common::run_rivets_in_dir`, dropping its documented macOS Gatekeeper NotFound retry.

**Failure scenario:** If the spawned `rivets create --yes` hits its one non-retried `try_acquire` in the microseconds the parent's probe holds the lock, the child exits WorkspaceBusy, the parent never observes Busy, and the test panics "first writer should acquire before timeout" — a CI flake that also blocks local commits since the pre-commit hook runs the full suite; separately, `run()` at line 23 spawns `CARGO_BIN_EXE_rivets` raw, reintroducing the macOS parallel-spawn NotFound flake that `common/mod.rs:17-37`'s retry exists to prevent.

---

## Cut for the 15-finding cap (9, all judged real but lower-severity)

> Note: the review transcript preserved these as one-line labels rather than full verified write-ups; the expansions below for items 2–5 and 8–9 are interpretations of those labels in codebase context, not verified failure scenarios. Item 5 (eviction leak) was explicitly verified by the review orchestrator.

1. **~150 new `.unwrap()` call sites in test code** — violates CLAUDE.md's own testing convention (`.expect("descriptive message")` for clearer failure output). Consolidated by the conventions angle into one candidate.
2. **Duplicated SHA-256 revision implementations** — the file-revision hashing logic is implemented in two places.
3. **`matches_ready_filter` / `matches_filter` duplication** — two near-identical filter matchers.
4. **`Commands::mutates_workspace` parallel table** — a hand-maintained mapping of which CLI commands mutate the workspace, parallel to the command definitions; adding a command risks silent drift.
5. **`cfg(test)` `test_workspaces` eviction leak** — verified: cache eviction removes entries from `storage_cache` and `database_paths` but never from the `test_workspaces` set, so it grows unboundedly (test-only impact).
6. **Non-ignored 10k stress tests in pre-commit** — new stress tests are not marked `#[ignore]`, and the pre-commit hook runs the full nextest suite, so every commit pays for them.
7. **Claim/release copy-paste triplets** — `execute_claim` / `execute_release` (and counterparts) are structural copy-pastes of each other.
8. **Duplicate `.with_writer(std::io::stderr)`** in `crates/rivets/src/main.rs` tracing-subscriber setup.
9. **Blocking filesystem I/O inside async MCP handlers** — hygiene nit; the verifier rated it low-severity PLAUSIBLE.

---

## Appendix: candidates eliminated by verification (not findings)

- **MCP cancellation phantom-state** — REFUTED: rmcp 1.8.0 runs each request as a detached task that always completes, so a cancelled future cannot leave partial in-memory state.
- **`stats` "Ready" narrowing** — REFUTED as a bug: the behavior change is declared in the CHANGELOG and ADR-0002.
- **`reload()` reseeding a deleted data file as an empty workspace** — judged an intentional, test-pinned semantic change (a create after external deletion persists only the new issue). Worth a deliberate look only if that semantic surprises you.
