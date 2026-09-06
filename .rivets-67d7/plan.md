# Plan: rivets-67d7

Approved inputs: route.md Structural; spec.md approval "yes, I agree"; design.md approval "yes, I approve", 2026-09-06. All C1-C7 have falsifiers, independent oracles and named mutations; C1 baseline PASS, C2-C7 assigned below. Risk acceptances: None.

## Partition

One atomic public-interface cutover remains mandatory: validated IssueUpdate, dedicated storage intents and every adapter/test caller land together without a compatibility shim. Verification exposed an independently fixable guard-release defect and a reusable baseline MCP error-path fixture. Publish those prerequisites first, then the atomic cutover as a stacked increment.

Original projection was 3,100 changed lines plus 800 churn margin. Actual implementation/documentation reached 4,075, plus 319 workflow-artifact lines, so the single-increment projection failed. Repartition: `MutationPrerequisites` contains the guard fix, baseline registered-MCP error fixture and these workflow artifacts (approximately 450 lines after plan revision); `CanonicalUpdate` contains approximately 3,990 incremental lines. Re-measure actual committed diffs at each PR's base before publication. The cumulative stack still exceeds 4,000; it is not reported as a single sub-4,000 change.

Requester publication approval (verbatim): "commit, push, and open two PRs. Do not make them drafts or else the CI will not run". Both PRs are non-draft; merging is not authorized. The second PR targets the first branch until the prerequisite is merged, then must be retargeted to the discovered upstream branch. No behavioral requirement or regression fence is waived.

## Slice S0: Preserve guard lifetime and baseline protocol error behavior

**Claim IDs:** Supporting prerequisites for C3/C7; full cutover falsifiers remain assigned once to S1.
**Expected behavior:** Dropping a Workspace mutation guard releases its flock even if a forked child retains the open-file description before exec. Baseline registered MCP rejects an empty Update with invalid-params and unchanged persisted bytes; a valid title update still persists.
**Oracle:** Independent lock acquisition succeeds after owner Drop while the inherited descriptor remains alive; closing that descriptor must not release a subsequently acquired independent lock. Exact JSON-RPC code -32602 and original JSONL bytes, followed by a persisted title-change control.
**Stress fixture:** An inherited descriptor outlives its guard; registered empty request is followed by a valid request against the same real Workspace.
**Regression fence:** workspace_lock::tests::dropping_guard_releases_lock_with_inherited_descriptor and reporting_parity::registered_mcp_empty_update_is_invalid_params.
**Named mutation:** Remove explicit unlock from Drop; retained-descriptor reacquisition fails. Disable the legacy empty-Update guard; registered empty request unexpectedly succeeds. Restore each and rerun green.
**Complexity/production scale:** N/A — no new loop; one unlock operation per completed mutation transaction.
**Wall budget/phase:** N/A — one-off guard-release event and test-only protocol fixture; no new always-on phase or wall budget.
**Files:** crates/rivets/src/workspace_lock.rs; crates/rivets-mcp/tests/reporting_parity.rs; CHANGELOG.md; .rivets-67d7/{route,spec,design,plan}.md.
**Estimate:** One independently verifiable prerequisite extraction, with no public Update contract change.
**Diff estimate:** 92 implementation/test lines plus approximately 350 workflow-artifact lines, with 50 lines of documentation churn margin.
**PR increment:** MutationPrerequisites, based on discovered origin/main at 487d6b2; mergeable without S1 when its two fences and workspace checks pass.
**Commands and expected results:**
- `cargo test -p rivets --lib dropping_guard_releases_lock_with_inherited_descriptor` → guard release allows reacquisition before inherited descriptor closure, without weakening the next guard.
- `cargo test -p rivets-mcp --test reporting_parity` → existing reporting oracles and real empty-request rejection/valid-update control pass.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace` → prerequisite remains independently green.
- Each named mutation makes its owning assertion fail; restoration passes.

The controlled actual-library fork probe confirmed CLOEXEC and the pre-exec retention window. It establishes the guard-lifetime defect; unique attribution of the earlier concurrent-suite flake remains an inference. The primitive stays flock, not POSIX process-scoped record locks.

## Slice S1: Cut over Update and all dedicated mutation callers atomically

**Claim IDs:** C1, C2, C3, C4, C5, C6, C7.

**Expected behavior:** six-field validated nonempty Update with retained Priority and text semantics; no status/Note/Label/Assignment/relationship Update inputs; separate lifecycle and append_note storage intents; approved Start/ReturnToOpen/CLI Note surfaces; complete caller migration; unchanged ordered partial success, Parentage, Assignment and persistence contracts.

**Oracle:** design.md C1-C7 explicit expected records, all 64 supplied-field masks, literal grammar boundary inputs, explicit lifecycle matrix, literal Parentage child IDs, byte-for-byte before snapshots and external-write marker. Compare both adapters to independent expected records before comparing them to each other.

**Stress fixture:** six absent fields rejects, explicit empty description succeeds; forbidden status:null plus valid title rejects unchanged; valid/missing/valid batch persists first and last; duplicate Note target appends twice; Closed child of Closed Epic cannot reopen; active children block Epic closure; 10,000 direct children preserve exact active-child IDs within 100ms; 100 target mutations in 10,000-Issue Workspace preserve all unrelated records after restart. Expected results fixed by design.md.

**Regression fence:** migrated in_memory_storage Update/Parentage tests; domain/update.rs construction/error tests and compile-fail examples; crates/rivets-mcp/tests/update_contract.rs real CLI/Tools contract tests and reporting_parity.rs registered-server checks; stale_cache and Workspace locking fixtures extended to new intents. Existing broken wording/implementation-only tests are deleted, not re-pinned.

**Named mutation:** C1 lower builder priority maximum to 0; C2 treat supplied empty description as absent; C3 remove UpdateParams deny_unknown_fields; C4 omit lifecycle reason append and remove ReturnToOpen source guard; C5 remove ActiveChildren rejection; C6 return at first CLI target failure; C7 omit JsonlBackedStorage::transition prepare_mutation. Apply each independently, run owning fence red, restore, run green. Exact concrete symbols are recorded once implementation exists.

**Complexity/production scale:** new builder processing is six fields, O(total supplied text bytes), no graph/Workspace scan. Lifecycle graph checks retain indexed O(d) direct children; d=10,000, exact result IDs required, accepted maximum 100ms from existing scale fence. Batch processing retains O(b) independent calls with existing load/save costs O(b*n) for b=100,n=10,000; no added scan or cache layer. No new production input-size cap. Note append retains existing candidate copy/save behavior; no extra history traversal introduced.

**Wall budget/phase:** always-on graph check remains <=100ms at 10,000 direct children (existing accepted budget). Builder has fixed six-field dispatch; test all masks with short bounded inputs. CLI batch/stress exercise is a one-off command phase; N/A — no new wall budget for unchanged filesystem I/O. No background phase introduced.

**Files:**
- Core: crates/rivets/src/domain/{mod.rs,update.rs,lifecycle.rs,resource.rs}; crates/rivets/src/error.rs; crates/rivets/src/storage/{mod.rs,in_memory/trait_impl.rs}; crates/rivets/tests/{in_memory_storage.rs,in_memory_resilient_loading.rs}.
- CLI: crates/rivets/src/cli/{args.rs,mod.rs,execute.rs,lifecycle.rs,notes.rs,types.rs}; affected CLI tests under crates/rivets/tests (ownership excludes core tests above).
- MCP: crates/rivets-mcp/src/{models.rs,tools.rs,server.rs,error.rs}; crates/rivets-mcp/tests/{integration.rs,stale_cache.rs,workspace_lock.rs,update_contract.rs}; other compiler-identified consumers of changed interfaces are part of the same atomic slice, not optional work.
- Docs: docs/cli-mcp-parity.{json,md}; docs/adr/{0001-multiple-notes.md,0005-domain-owned-status-transitions.md}; docs/module-structure.md; AGENTS.md; README.md and other directly affected executable examples discovered by removed-surface search. No unrelated documentation rewrite.

**Estimate:** one multi-module atomic cutover with parallel core/CLI/MCP/contract-test ownership; integration and mutation verification are serial once all writers finish.

**Diff estimate:** Approximately 3,990 changed lines relative to MutationPrerequisites after extraction; exact committed delta is the publication gate. Do not count only tracked files or exclude shipped workflow artifacts.

**PR increment:** CanonicalUpdate, stacked on MutationPrerequisites. Mergeable definition: all public consumers compile, C1-C7 fences green, mutations kill fences, docs atomically match behavior, workspace checks pass. Publish non-draft under the explicit approval above; no merge authorization.

**Commands and expected results:** (cwd /home/dwalleck/repos/rivets-67d7)
- `cargo test -p rivets --test in_memory_storage` → Priority, reclassification, lifecycle and Parentage outcomes match literal fixtures.
- `cargo test -p rivets --test in_memory_storage epic_close_10k_direct_children_budget -- --ignored --exact` → exact active child IDs and <=100ms close check.
- `cargo test -p rivets-mcp --test update_contract` → C2-C6 direct expected records match real CLI/Tools/server requests; rejected mutations preserve bytes, ordered batches persist later successes.
- `cargo test -p rivets-mcp --test stale_cache --test workspace_lock` → C3/C7 empty/obsolete rejections and stale/competing writes preserve durable records.
- Run the large-Workspace ignored contract case explicitly → 100 target mutations, 9,900 unrelated records preserved after restart.
- For each named mutation run the owning command above with exact test filter → named assertion red; restore → same test green.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace` → formatting, diagnostics and all affected contracts pass. Run once after parallel edits are integrated.
- Real CLI smoke in temporary Workspace: init/create/claim/start/update/return-to-open/note append/close/reopen/show; forbidden Update flags fail; inspect persisted JSONL expected state/Notes after restart.

## Execution ownership contract

All writing agents receive isolated worktrees based on 487d6b2, never the dirty main checkout. Main integrates exact owned paths/commits into work/67d7. Core owns canonical interfaces; adapters consume the fixed contract below, and do not wait for core implementation to begin their independent edits. All agents skip formatters, linters, builds and tests; Main runs verification after integration.

Shared contract: IssueUpdate::builder() returns default-empty private-field builder; setters title/description/design/acceptance_criteria take Option<String>, priority Option<u8>, issue_kind Option<IssueKind>, return Self; build returns Result<IssueUpdate, UpdateError>. No IssueUpdate Default or public fields. LifecycleAction variants Start, ReturnToOpen, Close { reason: Option<NoteContent> }, Reopen { reason: Option<NoteContent> }; reason is existing canonically prefixed validated lifecycle NoteContent. IssueStorage methods transition(&mut self,id:&IssueId,action:LifecycleAction)->Result<Issue> and append_note(&mut self,id:&IssueId,content:NoteContent)->Result<Issue>. Error adapters preserve typed cause; Core communicates exact UpdateError variants/mapping before adapters finalize. New MCP Tools methods start(params: LifecycleParams), return_to_open(params: LifecycleParams); LifecycleParams { issue_id:String, workspace_root:Option<String> }; retained close/reopen/add_note Tools signatures unchanged. MCP UpdateParams: issue_id, title, description, priority, issue_kind:Option<IssueKind>, design, acceptance_criteria, workspace_root, deny_unknown_fields, no legacy fields.

## Self-review

All seven full-cutover design claims remain assigned exactly once to S1; S0 independently hardens pre-existing guard and protocol behavior supporting C3/C7. Required fields, named mutations, existing scale bound and partition arithmetic are explicit. No slice is declared complete by this planning artifact; checkpoint evidence belongs in its commit. Wider schema work remains rivets-mmqc and wider parity work rivets-y4vw, both verified; neither excludes acceptance work here. No other deferrals.
