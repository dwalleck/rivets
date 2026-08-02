# Budgeted plan — rivets-p1g4: Curate Associated Resources and link Workspace Paths

Plan for the approved design (`.rivets-p1g4/design.md`, 12 claims, cheapest
falsifier passed 432/432). One slice per claim group; every slice re-runs the
prove-it-prototype oracle (`.rivets-p1g4/probe.py` against `target/debug/rivets`)
and the full gate.

Claim → slice map (design table): C1–C4 → S1; C5 → S2; C6 → S3; C5/C6
storage-level → S4; C7 → S5; C8/C9 → S6; C10/C11 → S7; C12 → S8.

---

## Slice 1: WorkspacePath newtype + ResourceTarget::Path variant

**Claim:** C1–C4 (normalization rule, rejections, normalized-equality duplicate
detection).

**Oracle:** `realpath -m` (probe Section A + falsify-normalization.py corpus).
Normalization and policy are separate claims; policy rejections (control
chars, whitespace-only) have no realpath counterpart and are asserted by the
case table only.

**Stress fixture:** deterministic corpus generated with the same seed as
`falsify-normalization.py` (seed 20260802, 432 cases: 1–5 random components
from `src|lib.rs|docs|a b|é|文件.md|.hidden|x.y.z|a-b_c|deep|..|.|""|dir:|C:|with space`,
plus 100 `/`-prefixed and 100 `../`-prefixed variants) embedded as a
`#[test]` in `resource.rs`. Plausible bug killed: normalization that handles
the 20 hand-picked probe cases but drops/reorders components or misses an
escape variant in the broader space; also the unicode/spaces/dots cases.

**Loop budget:** `WorkspacePath::new` is O(len) — one pass over components
(split + stack, each component touched once). Production scale: paths < 200
chars, resources per issue < 100. No always-on phase.

**Files:**
- `crates/rivets/src/domain/resource.rs` (modify: `WorkspacePath` newtype,
  `ResourceTarget::Path` variant, new `ResourceError` variants `EmptyPath`,
  `PathControlCharacter{position}`, `AbsoluteWorkspacePath{path}`,
  `WorkspacePathEscape{path}`, `EmptyNormalizedWorkspacePath{path}`; Display
  arms; tests incl. generated corpus)

**Code (advisory):**
```rust
pub struct WorkspacePath(String);   // normalized, workspace-relative
impl WorkspacePath {
    pub fn new(raw: impl Into<String>) -> Result<Self, ResourceError> {
        let raw = raw.into();
        if raw.trim().is_empty() { return Err(ResourceError::EmptyPath); }
        if let Some(pos) = find_control_char(&raw) {
            return Err(ResourceError::PathControlCharacter { position: pos });
        }
        if raw.starts_with('/') { return Err(ResourceError::AbsoluteWorkspacePath { path: raw }); }
        let mut stack: Vec<&str> = Vec::new();
        for comp in raw.split('/') {
            match comp {
                "" | "." => {}
                ".." => { if stack.pop().is_none() {
                    return Err(ResourceError::WorkspacePathEscape { path: raw }); } }
                _ => stack.push(comp),
            }
        }
        let normalized = stack.join("/");
        if normalized.is_empty() { return Err(ResourceError::EmptyNormalizedWorkspacePath { path: raw }); }
        Ok(Self(normalized))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
// ResourceTarget::Path { path: WorkspacePath } + ResourceTarget::path() ctor + Display arm.
```

**Verification:**
- [ ] `cargo test -p rivets --lib domain` — new tests incl. 432-case corpus pass
- [ ] Stress fixture: corpus test passes (embedded above)
- [ ] probe.py Sections B/C still pass against rebuilt binary (S1 doesn't touch CLI; must hold)
- [ ] Loop budget: O(len), held trivially
- [ ] Regression fences: `workspace_path_*` unit tests (C1–C3), `duplicate_detection_normalizes_paths` (C4)

---

## Slice 2: Issue::update_resource + ResourceUpdate

**Claim:** C5 (update by stable id; position/id unchanged; label set/clear;
empty update rejected; dup check on post-update state; unknown id typed).

**Oracle:** hand-computed expected resource state per case (probe C method:
state after mutation = edited-record expectation, cross-checked against raw
JSONL on the S6 fence).

**Stress fixture:** 3-resource issue; update first (target web→path), middle
(role only), last (label clear); update-only-resource; empty update → error;
unknown rid (`r99`) → `ResourceNotFound`; target update to a
normalized-equivalent duplicate (`docs/../x` vs `x`, same role) →
`DuplicateTargetRole`; update to existing target with distinct role → OK.
Plausible bug killed: positional/index-based updates, silent id regeneration,
dup check against pre-update state, missing clear semantics.

**Loop budget:** O(n) scan for the target resource + O(n) dup check
(n = resources on the issue, < 100; not always-on).

**Files:**
- `crates/rivets/src/domain/resource.rs` (modify: `ResourceUpdate` struct with
  `Option<ResourceTarget>` target, `Option<ResourceRole>` role,
  `Option<Option<ResourceLabel>>` label (double-Option per `IssueUpdate.assignee`
  precedent); `ResourceError::ResourceNotFound{id}` and `ResourceError::EmptyUpdate`)
- `crates/rivets/src/domain/mod.rs` (modify: `Issue::update_resource` + tests)

**Verification:**
- [ ] Unit tests pass
- [ ] Stress fixture passes (above matrix)
- [ ] probe.py B/C still agree with binary
- [ ] Budget: O(n), held
- [ ] Fence: `update_resource_*` domain tests

---

## Slice 3: Issue::remove_resource

**Claim:** C6 (remove by stable id; remaining ids/positions unchanged;
`next_resource_id` untouched — ids never reused).

**Oracle:** hand-count from JSONL (probe C2: after removing r2 of r1..r3, next
add gets r4).

**Stress fixture:** remove first / middle / last / only-remaining; then add →
id continues from pre-removal sequence (r4, not r2); remove unknown rid →
`ResourceNotFound`; remove from empty issue → `ResourceNotFound`.
Plausible bug killed: id/sequence reset on removal, positional removal,
partial mutation on error.

**Loop budget:** O(n) find + O(n) Vec::remove shift.

**Files:**
- `crates/rivets/src/domain/mod.rs` (modify: `Issue::remove_resource` + tests)

**Verification:**
- [ ] Unit tests pass
- [ ] Stress fixture passes
- [ ] probe.py B/C still agree
- [ ] Budget: O(n), held
- [ ] Fence: `remove_resource_*` domain tests

---

## Slice 4: storage trait update/remove + in-memory impl

**Claim:** C5/C6 at the storage seam (same semantics through
`IssueStorage`, `updated_at` bumped, issue returned, mutations persist in
in-memory store).

**Oracle:** raw JSONL after `app.save()` on the S6 fence; for this slice:
in-memory state via `Issue::resources()`.

**Stress fixture:** JSONL-backed storage: add 3, update middle (role+label),
remove first; assert in-memory order/ids, `updated_at` strictly newer than
pre-mutation, save → reload → same state. Plausible bug killed: storage
layer bypassing domain invariants, missing `updated_at` bump, save of stale
state.

**Loop budget:** O(n) per op; storage ops are per-call, not always-on.

**Files:**
- `crates/rivets/src/storage/mod.rs` (modify: trait `update_resource`,
  `remove_resource` + doc; `MockStorage` stubs; wiring in delegating impl)
- `crates/rivets/src/storage/in_memory/trait_impl.rs` (modify: impls mirroring
  `add_resource`: find issue → domain method → `updated_at = Utc::now()`)

**Verification:**
- [ ] Unit/integration tests pass
- [ ] Stress fixture passes
- [ ] probe.py B/C still agree
- [ ] Budget: O(n), held
- [ ] Fence: `in_memory_storage` round-trip test (S4 adds it)

---

## Slice 5: JSONL Path record + rehydrate validation

**Claim:** C7 (`{"type":"path","path":"..."}` round-trips; invalid persisted
path → typed `InvalidResourceData` load warning, issue skipped).

**Oracle:** raw JSONL parse (stdlib) of the persisted record; warning type
from `PartialLoadError` causes.

**Stress fixture:** hand-write a record with an escaping path
(`{"type":"path","path":"../etc/passwd"}`) → load produces
`SkippedIssueRecordCause::InvalidResourceData` with `ResourceError::WorkspacePathEscape`
source; hand-write a valid normalized-un-normalized form
(`"docs/../src"` — note: record stores the *normalized* form; a raw
un-normalized persisted value is accepted and re-normalized by
`WorkspacePath::new`; assert stored form is normalized after next save);
round-trip via CLI add (web) + JSONL-edit to path + reload.
Plausible bug killed: record layer skipping domain validation, wrong
serialized shape, rehydrate accepting escapes.

**Loop budget:** O(n) per record validation at load; load is per-process
startup, n = resources per issue < 100.

**Files:**
- `crates/rivets/src/storage/in_memory/issue_record.rs` (modify:
  `ResourceTargetRecord::Path { path: String }`, `into_domain` arm,
  `to_record` arm, migration/legacy paths untouched)
- `crates/rivets/tests/in_memory_resilient_loading.rs` (modify: invalid-path
  fixture test)

**Verification:**
- [ ] Unit/integration tests pass
- [ ] Stress fixture passes
- [ ] probe.py B/C still agree
- [ ] Budget: O(n) load-time, held
- [ ] Fence: `in_memory_resilient_loading` path-record test

---

## Slice 6: CLI add --path, update, remove

**Claim:** C8, C9 (CLI surfaces; path normalized workspace-root-relative —
semantics enforced by escape rejection since normalization is root-free; typed
errors; text/JSON post-state).

**Oracle:** raw JSONL of the scratch workspace (stdlib parse) — CLI output
must equal file state; cross-check normalization with `realpath -m`.

**Stress fixture:** run CLI from a **subdirectory** of the scratch workspace:
`--path src/lib.rs` must store `src/lib.rs` (workspace-root-relative, not
cwd-relative); `--path ../escape.md` → typed escape error; unicode path
`é/文件.md`; add `docs/../src` then add `src` same role → duplicate error;
update middle resource role/label via `--resource r2`; clear label with
`--no-label`; remove r2 → list shows r1,r3; remove unknown rid → typed error;
`--url` and `--path` together → clap conflict error; neither → error.
Plausible bug killed: cwd-relative interpretation, un-typed errors, update
mutating the wrong resource, silent label-clear failure.

**Loop budget:** O(n) per op (domain-side, per-call).

**Files:**
- `crates/rivets/src/cli/args.rs` (modify: `ResourceAction::Add` gains
  `--path` (conflicts with `--url`); new `Update` and `Remove` variants with
  `--resource <rid>`, `--url`/`--path`/`--role`/`--label`/`--no-label`)
- `crates/rivets/src/cli/execute.rs` (modify: `execute_resource` — add/update/
  remove handlers, JSON/text output; `ResourceUpdate` assembly with
  `ResourceId::new` at the seam)
- `crates/rivets/tests/cli_tests.rs` (modify: integration tests)

**Verification:**
- [ ] Unit tests + cli_tests pass
- [ ] Stress fixture passes (subdir + unicode + dup + clear + remove matrix)
- [ ] probe.py B/C still agree
- [ ] Budget: O(n), held
- [ ] Fence: cli_tests resource update/remove/path tests (C8, C9)

---

## Slice 7: MCP tools resource_update/resource_remove, add path support

**Claim:** C10, C11 (MCP `resource_add` path xor url; `resource_update`,
`resource_remove` by resource id; `McpResourceTarget::Path` distinct from Web;
same domain paths as CLI).

**Oracle:** cross-surface: same workspace mutated via CLI vs MCP must produce
identical JSONL (raw parse); normalization equality with CLI results.

**Stress fixture:** MCP: add web + path (unicode) to one issue; update path
resource role + label; update target web→path; clear label; remove middle;
assert `McpResource` output has `target.type == "path"` and `path` field,
ids/order stable; restart context (new `Tools`) → same state; duplicate
target-role via MCP → `Error::InvalidResource`; unknown rid →
`Error::InvalidArgument`-mapped typed error.
Plausible bug killed: MCP/CLI divergence (parallel-path symmetry audit:
same error variants, same validation), missing Path arm in
`McpResourceTarget` conversion (compile-forced), workspace_root vs context
divergence.

**Loop budget:** O(n) per op, per-call.

**Files:**
- `crates/rivets-mcp/src/models.rs` (modify: `McpResourceTarget::Path{path}` +
  From arm; `ResourceAddParams` url → Option + `path` field, exactly-one check
  helper; new `ResourceUpdateParams`, `ResourceRemoveParams`)
- `crates/rivets-mcp/src/tools.rs` (modify: `resource_add` path handling;
  new `resource_update`, `resource_remove`)
- `crates/rivets-mcp/src/server.rs` (modify: register two tools)
- `crates/rivets-mcp/tests/integration.rs` (modify: tests)

**Verification:**
- [ ] Unit + integration tests pass
- [ ] Stress fixture passes (incl. context restart)
- [ ] probe.py B/C still agree
- [ ] Budget: O(n), held
- [ ] Fence: rivets-mcp integration resource tests (C10, C11)

---

## Slice 8: restart-persistence fences (claim 12)

**Claim:** C12 (updates, removals, path resources, ordering, identifiers
persist across process and MCP context restart).

**Oracle:** probe B/C method — raw JSONL parse in a fresh process, ids/order
hand-count.

**Stress fixture:** full lifecycle across two process generations: CLI session
1 (add web r1, add path r2, update r1 role, remove nothing) → CLI session 2
(assert exact state) → CLI session 3 (remove r1, add r3 → r3) → assert;
MCP equivalent with a fresh `Tools` (context restart) after each mutation.
Plausible bug killed: in-memory-only mutations, next_resource_id resets,
order reshuffle on save (the jsonl `export_all` ordering contract).

**Loop budget:** O(n) per op; none always-on.

**Files:**
- `crates/rivets/tests/cli_tests.rs` (modify: restart test extends
  `resource_add_list_show_and_validation_survive_process_restart` pattern)
- `crates/rivets-mcp/tests/integration.rs` (modify: context-restart test)

**Verification:**
- [ ] Integration tests pass
- [ ] Stress fixture passes (3-generation lifecycle)
- [ ] probe.py B/C still agree
- [ ] Budget: O(n), held
- [ ] Fence: the two restart tests ARE the fence

---

## Plan Self-Review

1. **Loops:** `WorkspacePath::new` O(len); add/update/remove dup checks O(n);
   rehydrate O(n²) is pre-existing (not introduced here). All stated, all
   within budget at production scale (< 100 resources/issue, < 200-char paths).
2. **Fixtures:** S1 corpus (432 seeded cases incl. unicode/escapes/spaces);
   S2 update matrix (every field alone + clear + dup + unknown + empty);
   S3 remove matrix (first/middle/last/only + sequence continuation);
   S4 storage round-trip + updated_at; S5 hand-written invalid/unnormalized
   records; S6 subdir-cwd trap + unicode + dup + label-clear; S7 cross-surface
   CLI-vs-MCP equivalence + context restart; S8 three-generation lifecycle.
   No happy-path-only fixtures.
3. **Doc-comment preconditions:** no new preconditions asserted without
   enforcement; all validation is constructor-enforced (`WorkspacePath::new`,
   `ResourceId::new` at seams) — runtime checks, not debug_asserts. Trait
   docs carry `# Errors` sections per existing convention.
4. **Write targets:** CLI data (resource JSON/records) → stdout via existing
   `output::print_json`; errors → stderr via existing error plumbing.
   No new fd writes introduced.
5. **Tracker references:** no deferrals in the plan. Scope boundaries cite
   the design's negative space (settled rationale). Tracker IDs used:
   rivets-p1g4 (this task, verified open/in_progress), rivets-wb0q
   (epic, verified open), rivets-yx1h (closed, verified).
