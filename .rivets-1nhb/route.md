# Route: rivets-1nhb

Change: Record the current Closed period through existing lifecycle transitions.
Date: 2026-09-26

## Route tests

| # | Test | Evidence | Verdict |
|---|------|----------|---------|
| 1 | Empirical premise | No external premise. Current-source CLI smoke reproduced reopen retaining closed_at. Existing storage transition captures one instant and atomically validates/commits candidate state and optional Note. | no |
| 2 | Structural module shape | Issue::apply_status_transition remains the domain owner; InMemoryStorage::transition remains the atomic mutation owner, JsonlStorage the persistence owner, and CLI/MCP adapters unchanged. Existing fields, wire schema, public interfaces, dependencies and module ownership remain unchanged. A private explicit-instant helper, if needed for the approved exact timeline test, remains within the existing storage responsibility. | no |
| 3 | Production-scale risk | Constant-size timestamp assignments in existing locked mutations; no additional allocation, traversal, I/O, concurrency mechanism or scale dimension. | no |
| 4 | Explicit behavior | Approved rivets-1nhb, parent rivets-ffqs and resolution Notes rivets-c6ql/rivets-ilnj supply the complete contract below. | yes |

Unknown tests: none.

### Given / when / then contract

- Given a new Issue, when created, then Created equals Updated and Closed is absent.
- Given an eligible Open/In Progress Issue, when closed at Monday's event instant, then state becomes Closed, Assignment clears, Closed and Updated equal that instant, Created is preserved, and an optional reason Note receives the same instant atomically.
- Given a Closed Issue, when reopened Tuesday, then state becomes unassigned Open, Closed clears, Updated and optional reason Note share Tuesday's event instant, and Created is unchanged.
- Given that reopened Issue, when closed Thursday, then Closed records Thursday rather than Monday, and Updated and the optional reason Note share Thursday's instant.
- Given legacy non-Closed closure data, when read, then bytes and history remain unchanged; subsequent actual lifecycle transitions establish current-period semantics.
- Given a Closed Issue, when its ordinary content actually changes, then Updated advances but Created and Closed do not change. General same-value content/resource suppression remains rivets-g1rd, not this task.
- Given existing identity-preserving loading/imports, when read or reloaded, then Created and recorded closure remain unchanged; missing closure stays unknown, including Closed records. No backfill or chronology repair.
- Given a rejected transition (including domain state/Assignment and Parentage restrictions), when requested with or without a reason, then state, Assignment, timestamps, history and persisted bytes remain unchanged.
- Given invalid missing/null/malformed required Created/Updated data, when loaded or mutated, then existing warnings, record exclusion and incomplete-load write protections remain intact.
- Given supported CLI and registered MCP lifecycle operations, when exercised over isolated persisted JSONL fixtures and reloaded, then existing payloads and adapter mechanics expose the same lifecycle semantics. Existing claim/Assignment rules remain unchanged; no public clock override, DTO mirrors or unrelated parity repair.
- Given this slice is delivered, then help, MCP descriptions, user-facing and architecture/data-flow guidance, authoritative/generated Interface Parity references describe only this implemented slice; parent Feature and later tasks remain unchanged.

## Selected route

Local — explicit correction within existing domain and storage lifecycle responsibilities, no public API or schema changes.

## Required artifacts

| Artifact | Owner | Status |
|---|---|---|
| route.md | change-workflow | this file |
| spec.md | interrogated-spec | N/A — behavior fully explicit in approved task/specification |
| evidence.md, probe.* | prove-it-prototype | N/A — no unverified premise |
| design.md | falsifiable-design | N/A — Local route: no design gate |
| plan.md | budgeted-plan | N/A — Local route: no plan gate |

Oracle checkpoint in checkpointed-build: N/A — Local route: checkpointed-build does not run.

## Downstream sequence

None — implement with normal repository fix/TDD. Confirmed seams are the parent specification's real CLI, registered MCP stdio harness, persistence/reload, and highest controllable mutation seam for literal event times.

## Terminal criterion

Focused behavioral verification must PASS: literal Monday/Tuesday/Thursday storage timeline with one event timestamp; CLI and registered MCP lifecycle/rejection/Closed-edit scenarios; legacy loading and required-timestamp protections; relevant core/MCP tests, parity regeneration check and formatting/lint checks. Run a separate actual CLI/MCP smoke after implementation.

Baseline: current-source cargo build succeeds with empty CARGO_TARGET_DIR unset. Real CLI init/create/close/reopen/show reproduces stale closure on reopen: expected null, observed 2026-09-27T03:50:45.208188539Z. Existing user changes in AGENTS.md, CONTEXT.md and untracked .agents/ are preserved.

### Verification receipt

Result: 2026-09-26 | PASS.

Checked state: base `3072f2f` plus this uncommitted implementation. Delivery
file-set SHA-256: `a396c155cfc76ed70fa7d1a01798c7c6042712e29254e7c9da5c9633fab33aba`.
Digest input is each sorted repository-relative path, NUL, complete file bytes,
NUL, for these 16 files: `README.md`; `crates/rivets/src/{domain/mod.rs,storage/in_memory/trait_impl.rs,cli/args.rs,cli/mod.rs}`;
`crates/rivets/tests/{cli_tests.rs,in_memory_resilient_loading.rs}`;
`crates/rivets-mcp/src/{models.rs,server.rs}`; `crates/rivets-mcp/tests/reporting_parity.rs`;
`docs/{architecture.md,data-flow.md,storage-architecture.md,cli-mcp-parity.json,cli-mcp-parity.md}`;
`docs/adr/0005-domain-owned-status-transitions.md`.

Environment: Linux x64; Rust/Cargo 1.94.0 toolchain; commands run from this
checkout with the ambient empty `CARGO_TARGET_DIR` unset. Evidence: session
`artifact://274` for final tests/lint/format/parity; `artifact://260` for the
executable build and prior gate.

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --workspace`: PASS, 1,239 tests across 26 suites; 11 ignored.
  All new lifecycle/legacy/registered-adapter tests executed, not ignored.
  Unrelated ignored tests were not claimed as exercised.
- `cargo fmt --all --check`: PASS.
- `python scripts/render-cli-mcp-parity.py --check`: PASS after regeneration.
- `cargo build -p rivets -p rivets-mcp`: PASS.
- Literal mutation timeline: Monday `2026-09-21T09:00:00Z`, Tuesday
  `2026-09-22T10:00:00Z`, Thursday `2026-09-24T11:00:00Z`; closure Monday,
  absent Tuesday, closure Thursday, with Updated/reason Note sharing each event.
- Actual CLI smoke reran the originally failing Workspace: close/reopen/reclose,
  reason timestamps, preserved Created, Closed content edit, rejected close
  leaving bytes unchanged, and updated `close --help`: PASS. Observed closure
  `2026-09-27T04:13:20.559367139Z` → null → `2026-09-27T04:13:20.573562068Z`.
- Actual `rivets-mcp` stdio smoke used initialize, initialized, set_context,
  tools/list and registered tools/call: create, close, rejected close
  (`-32602` and unchanged bytes), Closed edit, reopen, reclose, then CLI reload:
  PASS. Observed closure `2026-09-27T04:13:23.704044177Z` → null →
  `2026-09-27T04:13:23.709470158Z`. Registered lifecycle descriptions observed.
- Parent Feature `rivets-ffqs`: byte-identical to its pre-implementation record.

Integration corrections: retained lock-before-clock ordering by making the
private explicit-instant helper synchronous over the held storage guard;
removed the superseded CLI test that assumed a reopened Issue retained its
closure, preserving its meaningful Note assertions in the fuller lifecycle
test; separated registered lifecycle coverage from Update coverage to satisfy
the existing function-size lint. These change no approved behavior or interface.

Independent review: production semantics PASS. Two acceptance-fence findings
were accepted and repaired: assert strictly advancing Updated on Closed edits;
cover reason-bearing active-child Epic Close and closed-parent child Reopen
rejections through both CLI and registered MCP, comparing complete Issues,
original bytes and a fresh MCP reload. Bounded independent re-review: PASS,
no residual findings. `artifact://274` reverified all affected tests and gates.
The earlier live CLI/MCP smoke and executable-build evidence is retained:
these final repairs changed tests only, not production paths or descriptions.

### Publication authorization

Requester: "commit, push the changes, and open a PR".
Scope: the verified `rivets-1nhb` implementation and its already-Closed tracker
record, on a dedicated branch targeting the discovered default branch `main`.
The branch retains prerequisite tracker commit `3072f2f`, which publishes the
approved date-filter specification and executable tasks. No other tracker
record is changed by the implementation commit. Pre-existing edits to
`AGENTS.md`, `CONTEXT.md`, and unrelated untracked configuration/skills are
excluded. Publication rechecked the 16-file digest against the verification
receipt; tests, review, and smoke evidence remain applicable without source drift.
