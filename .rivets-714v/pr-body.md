## Summary

Closes **rivets-714v**. Fixes tethys's `--lsp` flag failing with `LSP error -32603: url is not a file` and producing zero Pass-3 resolutions on multi-crate Cargo workspaces on Windows.

**Bug class:** `crates/tethys/src/lsp/transport.rs::path_to_uri` calls `path.canonicalize()` before formatting the file URI. On Windows, canonicalize returns paths with `\\?\C:\...` extended-length prefixes; the existing `.replace('\\', "/")` step converts these to `//?/C:/...`, and `format!("file:///{}", ...)` produces `file://///?/C:/...` — malformed per RFC 8089. rust-analyzer correctly rejected with `-32603`. Pass 3 resolved zero references for every multi-crate workspace tethys had indexed since LSP integration landed.

**Empirical impact:** previously, Pass-3 LSP was 100% non-functional on multi-crate Windows workspaces — every cross-crate goto_definition request errored. The rivets workspace itself (4 crates, indexed via `tethys index --lsp`) was producing zero useful Pass-3 resolutions. Post-fix, the integration test confirms ≥1 cross-file ref resolves via Pass 3 on a 2-crate fixture; on real workspaces the impact will scale with the number of method-on-imported-type calls that name-matching resolvers can't handle.

## What this PR contains

The fix is one extracted function + percent-encoding (production code) plus two integration tests (regression fences):

- **Slice 1 (`df08b51`)**: split `path_to_uri` into `format_uri` (pure transformation, no I/O) + `path_to_uri` (canonicalize-then-format wrapper). `format_uri` strips the Windows `\\?\` extended-length prefix and percent-encodes RFC 3986 non-unreserved characters (preserving `/` and `:` for path-segment structure). 5 new unit tests covering input shapes: extended-length-prefix strip, regular drive-letter, Unix absolute, percent-encoded spaces, percent-encoded Unicode, non-existent path InvalidPath preservation.

- **Slice 2+3 (`6d9fdbc`)**: integration tests in `crates/tethys/tests/lsp_multi_crate.rs` that build a 2-crate Cargo workspace, run `tethys index --rebuild --lsp`, and verify (a) zero `url is not a file` errors in stderr, (b) ≥1 cross-file reference resolved in the post-index DB. Both `#[ignore]`d like existing LSP integration tests; gated `#[cfg(windows)]` since the URI bug is Windows-specific.

- **Audit trail (`a3f3705`)**: full gilfoyle workflow diagnostic dir at `.rivets-714v/` — probe.py, oracle.py, cheapest_falsifier.py, design.md, falsifiable-design.md, plan.md, related-issues.md.

- **Tracker carry-over (`336ac5c`)**: `.rivets/issues.jsonl` updates from PR #63's cleanup (rivets-6aoc/34tv closures, rivets-6jxv filing) plus this PR's rivets-276h filing.

## What this PR does NOT fix

- **UNC paths** (`\\server\share\...` and `\\?\UNC\server\share\...`): deferred to **rivets-276h** (filed during this PR's design phase). RFC 8089's mapping for UNC paths is itself ambiguous; rivets-714v is bounded to drive-letter paths.

- **Symlink resolution policy**: `canonicalize()` resolves symlinks; the fix preserves this. Whether that's the right policy (vs. preserving the user's path as written) wasn't reconsidered.

- **Other LSP transport-layer changes**: the fix touches only `path_to_uri` / `format_uri`. LSP message framing, request/response loop, error handling, and provider abstraction are untouched.

- **Replacing the existing `path_to_uri_creates_valid_file_uri` test**: it stays as a backstop. Its loose assertions (`starts_with("file://")`, `!contains('\\')`) couldn't catch this bug class (both passed for the malformed `file://///?/C:/...` form). The 5 new tests with exact-string expected outputs are the structural-correctness fence.

## Scope expansion mid-implementation

The original design had 7 claims (C1–C7). Slice 1 implementation surfaced an unanticipated issue: **`lsp_types::Uri::from_str` strictly enforces RFC 3986 at parse time, rejecting unencoded spaces locally before any LSP wire send.** The negative-space item "rust-analyzer accepts unencoded spaces in URIs" was technically true at the wire level but irrelevant — the URI parser is the gatekeeper, not rust-analyzer.

Per gilfoyle v1.0.1's STOP-and-ask rule, the test failure surfaced to user-decision. User chose "expand slice 1 to include percent-encoding now" rather than file a separate tracker for it. Added claim **C8** to the design and a corresponding `percent_encode_path` helper to `format_uri`.

This is a small scope expansion (~15 LOC of helper code, 2 new unit tests). It addresses a latent bug for users with workspaces under paths containing spaces (e.g., `C:\Program Files\...`) without introducing new design surface.

## Numbers

| Metric | Pre-fix | Post-fix |
|---|---|---|
| `tethys index --lsp` on 2-crate fixture: `url is not a file` errors | 4 | **0** |
| Cross-file refs resolved via Pass 3 on the 2-crate fixture | 0 | **≥1** |
| `format_uri` unit tests covering input shapes | 0 | **5** |
| `lsp_types::Uri::from_str` rejects URIs with unencoded spaces | yes (pre-existing) | no (now percent-encoded) |
| Tethys total tests | 605 | **612** (+5 unit + 2 #[ignore]d integration) |

## Test plan

- [x] `cargo nextest run -p tethys` — **610 pass**, 8 skipped (default run; 2 of the 8 are this PR's `#[ignore]`d integration tests)
- [x] `cargo nextest run -p tethys --run-ignored all lsp_multi_crate` — **2 pass** (C5 + C6 integration tests)
- [x] `cargo clippy -p tethys --all-targets --all-features -- -D warnings` — clean
- [x] `cargo fmt --check` — clean
- [x] Probe (`.rivets-714v/probe.py`) re-run against release build: **0 matched `url is not a file` error lines** (pre-fix was 4)
- [x] Oracle (`.rivets-714v/oracle.py`) still agrees with probe on the substantive claim (rust-analyzer accepts correctly-formed URIs)
- [x] Cheapest falsifier (`.rivets-714v/cheapest_falsifier.py`) — **all 4 synthetic cases pass** (C1 strip, C2 drive-letter, C3 Unix, C2-variant short path)

## Related issues

| Issue | Status | Relationship |
|---|---|---|
| **rivets-714v** | open → closed by this PR | The canonical "tethys --lsp emits `url is not a file` on multi-crate workspace" issue |
| **rivets-276h** | filed during this PR | UNC path handling — deferred. RFC 8089 UNC mapping is ambiguous; addressed separately |
| **rivets-nwwm** | closed (Feb) | Original LspProvider trait + RustAnalyzerProvider — infrastructure that produces this bug class |
| **rivets-h1va** | closed (Feb) | LspClient with JSON-RPC transport — where `path_to_uri` lives |
| **rivets-k3mv** | closed (Feb) | Integrate LSP into `index --lsp` — the call path that surfaces the bug |
| **rivets-9o82** | closed (Feb) | Original LSP integration tests — source of the counter-evidence (`lsp_resolves_method_on_inferred_type` passes single-crate, proving the bug is multi-crate-specific) |
| **rivets-6jxv** | filed in PR #63 | Triple-duplicated `crate_root` derivation helper — unrelated to this fix |

## Process notes — what gilfoyle caught

The v1.0.1 disciplines added during the previous PR cycle (rivets-6aoc) were exercised real-time on this fix:

1. **Step 0 (tracker prior-art check)**: ran `rivets list` for LSP/rust-analyzer keywords; surfaced 6 closed LSP infrastructure tickets as the bug's parent context. Saved by `related-issues.md`.

2. **Input-shape enumeration** (`falsifiable-design` step 2): forced explicit listing of 8 input shapes for `path_to_uri`. UNC paths surfaced as out-of-scope during this step → rivets-276h filed before any deferral language went into the design.

3. **Regression-fence column** (`falsifiable-design` step 8): every claim has a named CI test. C5 and C6 became `lsp_multi_crate_emits_no_url_errors` and `lsp_multi_crate_resolves_at_least_one_cross_file_ref`. Both would fail if the bug class re-appears.

4. **STOP-and-ask** (`checkpointed-build` step f): the `lsp_types::Uri::from_str` rejection of unencoded spaces was an unanticipated test failure during slice 1. The skill's STOP rule forced explicit user-decision rather than silent test-removal or scope creep. Outcome: C8 added with user approval; ~10 min total resolution time.

5. **Tracker discipline (interrupt-driven)**: every "deferred to / out of scope / tracked at" phrase in the design or commit messages was either backed by a verified ID or had one filed before the phrase was committed. rivets-276h is the load-bearing example.

These are the disciplines that prevented this PR from shipping with (a) silent test-removal hiding the spaces bug, (b) an untracked UNC deferral, (c) measurement-only claims with no regression fence, or (d) drift surfacing only at review time. Detailed retrospective notes in `.rivets-714v/falsifiable-design.md`.
