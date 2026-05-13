# rivets-714v — falsifiable design

## Purpose

Fix `tethys index --lsp` failing with `LSP error -32603: url is not a file` on multi-crate Cargo workspaces by correcting `path_to_uri`'s URI construction. Pre-fix, Pass-3 LSP resolution is 100% non-functional on multi-crate Windows workspaces.

## Empirical premise (from prove-it-prototype)

`.rivets-714v/design.md` recorded the probe + oracle agreement: tethys's `path_to_uri` calls `path.canonicalize()` at `transport.rs:688`, which on Windows returns `\\?\C:\...` extended-length prefixes; the existing `.replace('\\', "/")` on line 704 doesn't strip them; `format!("file:///{}", ...)` then produces `file://///?/C:/...` — a malformed URI that rust-analyzer rejects per RFC 8089.

The independent oracle (direct rust-analyzer stdio client with a different URI construction) proved rust-analyzer accepts the corrected form `file:///C:/...` and that the bug is wholly in tethys's URI construction (not in rust-analyzer, the fixture, or workspace shape).

## Architecture

### Fix shape

Split `path_to_uri` into two functions:

- **`format_uri(path: &Path) -> Result<Uri>`** — pure transformation; no filesystem access. Strips `\\?\` prefix on Windows, replaces backslashes with forward slashes, formats with `file:///` (Windows) or `file://` (Unix). Testable with synthetic inputs covering every input shape.
- **`path_to_uri(path: &Path) -> Result<Uri>`** — wraps `format_uri` with `path.canonicalize()`. The composed function preserves existing `InvalidPath` error behavior for non-existent paths.

This split lets unit tests cover the URI-formatting logic with hand-constructed paths (no filesystem dependence), while a smaller set of integration tests covers the canonicalize-and-format composition.

### Why split, not just patch in-place?

The existing `path_to_uri_creates_valid_file_uri` test is a false negative — its assertions (`starts_with("file://")` and `!contains('\\')`) both pass for the malformed `file://///?/C:/...` form. Splitting forces every Windows path shape to be exercised against an exact-string expected output, which the existing test pattern can't structurally enforce.

## Input shapes (step 2)

| # | Shape | In scope? | Notes |
|---|---|---|---|
| S1 | Windows path with `\\?\C:\...` extended prefix | **Yes — claim C1** | The reported bug case |
| S2 | Windows path with `C:\...` drive-letter, no `\\?\` | **Yes — claim C2** | The "working but not via canonicalize" shape; regression check |
| S3 | Unix absolute `/home/user/file.rs` | **Yes — claim C3** | Unix baseline; regression check |
| S4 | UNC `\\server\share\file.rs` or `\\?\UNC\server\share\...` | **No — deferred to [rivets-276h](rivets-276h)** | Rare in production; RFC 8089 form is ambiguous for UNC |
| S5 | Path containing spaces | Implicit — covered by S1/S2 | No percent-encoding in current code; rust-analyzer accepts raw spaces |
| S6 | Path with Unicode characters | Implicit — covered by S1/S2 | UTF-8 round-trip; existing `to_str()` check handles invalid UTF-8 |
| S7 | Non-existent path | **Yes — claim C7** | Preserve `InvalidPath` error |
| S8 | Relative path | Out of scope | Callers always pass absolute; canonicalize would absolutize against CWD anyway |

## Claims

- **C1.** `format_uri` strips a leading `\\?\` extended-length prefix from Windows paths before URI formatting.
- **C2.** `format_uri` handles regular Windows drive-letter paths (`C:\foo\bar.rs` → `file:///C:/foo/bar.rs`) — no regression on the working shape.
- **C3.** `format_uri` handles Unix absolute paths (`/home/user/file.rs` → `file:///home/user/file.rs`) — no regression on Unix.
- **C4.** Composed `path_to_uri` produces URIs with no `\\?\` leakage for real-existing paths on Windows (integration test against a tempdir).
- **C5.** End-to-end: `tethys index --lsp` on the prove-it-prototype probe's 2-crate fixture emits zero `url is not a file -32603` errors after the fix.
- **C6.** End-to-end: `tethys index --lsp` on the probe's fixture resolves ≥1 reference via Pass 3 (Pass 3 produces measurable resolution after the fix, not just "no error").
- **C7.** The fix preserves the `InvalidPath` error path for non-existent paths.

## Falsification

| # | Claim | Falsifier | Oracle | Cost | Status | Regression fence |
|---|---|---|---|---|---|---|
| C1 | strip `\\?\` prefix | Synthetic input `\\?\C:\Users\foo\bar.rs` → `format_uri` → expect `file:///C:/Users/foo/bar.rs`. Wrong output (contains `?`, extra slashes) falsifies. | Hand-computed expected string. | 5m | **passed** (`.rivets-714v/cheapest_falsifier.py`) | unit test `format_uri::strips_extended_length_prefix` |
| C2 | drive-letter path | Synthetic `C:\foo.rs` → expect `file:///C:/foo.rs`. | Hand-computed. | 5m | **passed** (same script) | unit test `format_uri::handles_drive_letter_path` |
| C3 | Unix path | Synthetic `/home/u/f.rs` (on Unix) → expect `file:///home/u/f.rs`. | Hand-computed. | 5m | **passed** (same script, cross-platform logic verified) | unit test `format_uri::handles_unix_absolute_path` (Unix-gated) |
| C4 | composed `path_to_uri` no `\\?\` leakage | Real Windows tempdir → call `path_to_uri` → assert output starts with `file:///` and contains no `?`. | Filesystem inspection (`Path::exists()` on the file the URI claims to point at). | 10m | pending | unit test `path_to_uri::roundtrip_through_real_tempdir` |
| C5 | probe shows zero errors | Re-run `.rivets-714v/probe.py` after fix. Assert zero matched error lines. | grep for `url is not a file` (independent of tethys's internal logic). | 5m + build | pending | integration test `lsp_multi_crate_emits_no_url_errors` indexing a 2-crate fixture |
| C6 | Pass-3 resolves ≥1 ref | Extend probe to query `references` table; pre-fix `WHERE symbol_id IS NOT NULL AND <crate_caller's len() call>` is 0; post-fix ≥1. | SQL count on tethys.db `references` table — independent of resolver code. | 10m + build | pending | integration test `lsp_multi_crate_resolves_at_least_one_ref` — same 2-crate fixture; asserts SQL count ≥1 |
| C7 | `InvalidPath` preserved | Synthetic non-existent path → `path_to_uri` → assert `Err(InvalidPath)`. | Hand-computed expected error variant. | 5m | pending | unit test `path_to_uri::returns_invalid_path_for_nonexistent` |

All regression fences are deterministic CI tests with fixtures that embed the bug class — pre-fix code fails them, post-fix code passes.

## Negative space — what the fix does NOT do

1. **UNC path handling.** `\\server\share\...` and `\\?\UNC\server\share\...` are deferred to **[rivets-276h](rivets-276h)** (filed during this design phase). The fix may incidentally break or pass UNC inputs; we make no claim about UNC behavior.
2. **Percent-encoding of URI-reserved characters.** Paths containing `?`, `#`, `%`, etc. are not percent-encoded by current or proposed code. rust-analyzer's behavior on such paths is untested. If a user reports a failure on a path with `#` (e.g., `C:\C#-project\...`), a separate fix is needed.
3. **Symlink resolution semantics.** Canonicalize() resolves symlinks; the fix preserves this. Whether that's the right policy (vs. preserving the user's path as written) is not addressed here.
4. **Other LSP transport-layer changes.** The fix touches only `path_to_uri`. `LspClient`'s message framing, request/response loop, error-handling, and provider abstraction are untouched.
5. **The existing `path_to_uri_creates_valid_file_uri` test.** The test will be left in place but will be supplemented by the new fence tests (C1-C3, C7). The existing test's loose assertions remain valuable for backstop coverage of obvious mis-formatting; the new tests are the structural-correctness fence.

## Hard gate (v1.0.1 checklist)

- [x] Every production-reachable input shape (S1, S2, S3, S5, S6, S7) is covered by at least one claim — or explicitly noted as out-of-scope (S4 → rivets-276h, S8 → "callers always pass absolute")
- [x] Every claim has a falsifier in the table
- [x] Every falsifier names an independent oracle
- [x] Every claim has a distinct verifiable output
- [x] Every claim has a `Regression fence` entry pointing at a named CI test (all are deterministic tests, not measurement-only)
- [x] Every deferral reference cites a verified tracker ID (`rivets-276h` filed during this design phase, before being cited)
- [x] The cheapest falsifier (C1, C2, C3) has been run and passed
- [x] Negative space has ≥3 entries (5 listed)

Ready for budgeted-plan.

## Code surface (confirmed from prove-it-prototype)

- **`crates/tethys/src/lsp/transport.rs:686-712`** — current `path_to_uri`. Will be split into `format_uri` (pure) + `path_to_uri` (wrapper with canonicalize).
- **`crates/tethys/src/lsp/transport.rs:815-829`** — existing false-negative test. Stays in place; new tests added below it.
- New test surface (constrained):
  - `format_uri` unit tests (4 covering shapes S1-S3, S7)
  - `path_to_uri` integration test (S2 via real tempdir for C4)
  - `tests/lsp_resolution.rs` or new test file: multi-crate fixture integration tests (C5, C6)

## Plan-phase preview

The `budgeted-plan` skill will slice this into checkpointed-build slices. Likely shape:

1. **Slice 1: Extract `format_uri` from `path_to_uri`** + unit tests for C1, C2, C3, C7. ~30 min.
2. **Slice 2: Wire `format_uri` into the composed `path_to_uri`** with canonicalize-then-format. Run the prove-it-prototype probe. Verify zero `url is not a file` errors (C4, C5). ~20 min.
3. **Slice 3: Integration test with SQL count assertion** for Pass-3 resolves ≥1 ref (C6). ~30 min.

Each slice's stress fixture, complexity budget, and regression fence will be filled in by budgeted-plan.
