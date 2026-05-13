# Review decisions — round 2 (PR #64)

Date: 2026-05-13
Reviewer: `claude[bot]` (auto-triggered by round-1 push). No new Gemini activity.

Per `assessing-review-feedback` v1.0.1.

---

## Finding A: Use existing `percent-encoding` crate

**Source:** Claude review.

**Claim:** `crates/tethys/Cargo.toml` already lists `percent-encoding = "2.3"`. The hand-rolled `percent_encode_path` reimplements a subset of what the crate provides.

**Verification:** Confirmed via `grep -n "percent-encoding" crates/tethys/Cargo.toml` → line 40. The crate was already in scope when I wrote the hand-rolled version during slice 1's scope expansion. The v1.0.1 helper-search discipline failed: I grepped for existing `percent_encode_*` *functions* in source code, didn't check `Cargo.toml` for already-imported *crates*. Same lesson as rivets-6aoc's discovery of `cargo::get_crate_for_file` — but the search target was different (deps, not symbols), and I missed it.

**Decision: ACCEPT.** Replace the hand-rolled function with `percent_encoding::utf8_percent_encode` + a custom `AsciiSet` listing characters that need encoding (RFC 3986 non-unreserved minus `/` and `:`).

Verification strategy: existing exact-string tests are the regression sentinel. If the crate produces identical output to the hand-rolled version, all tests pass. If it differs (e.g., lowercase hex), the tests fail and surface the difference before merge.

---

## Finding B: `Connection::open` creates an empty DB on wrong path

**Source:** Claude review.
**Location:** `crates/tethys/tests/lsp_multi_crate.rs::lsp_multi_crate_resolves_at_least_one_cross_file_ref`.

**Claim:** `rusqlite::Connection::open` creates the database file if it doesn't exist. If `db_path` is ever wrong (e.g., tethys changes its DB location), the test opens an empty DB, the COUNT query returns 0, and the assertion fires with a misleading "expected ≥1 resolved cross-file reference" message that hides the root cause (wrong path).

**Verification:** True per rusqlite docs — `Connection::open` is equivalent to `Connection::open_with_flags(path, SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE)`. The default flags include `SQLITE_OPEN_CREATE`.

This is the same silent-failure-hunter discipline that caught finding 4 in round 1 (the SQL LIKE filter). Same shape of bug class: "test silently fails for the wrong reason and the failure message points at the wrong cause."

**Decision: ACCEPT.** Use `Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)`. If the DB doesn't exist, the open fails immediately with a clear error rather than producing a silent zero count.

---

## Finding C: `with_capacity(s.len())` under-allocates

**Source:** Claude review.

**Claim:** Each percent-encoded byte expands from 1 to 3 chars (`%XX`). For paths with even one space, the allocator will reallocate at least once.

**Verification:** True at the math level. Negligible impact in practice (file paths are <1KB, allocator amortization is microseconds).

**Decision: MOOT** — after Finding A's fix, the `String::with_capacity` line goes away (replaced by `utf8_percent_encode(s, PATH_CHARS).to_string()` which the crate manages internally). The crate's implementation likely allocates appropriately.

---

## Finding D: Windows unit tests don't run on Linux/macOS CI

**Source:** Claude review.

**Claim:** `format_uri_strips_extended_length_prefix`, `format_uri_handles_regular_drive_letter_path`, `format_uri_percent_encodes_spaces`, `format_uri_percent_encodes_unicode_as_utf8_bytes` are all `#[cfg(windows)]`. These directly verify the bug fix but never run in Linux CI runners.

**Verification:** True. The bug is Windows-specific (canonicalize's `\\?\` prefix is Windows-only behavior), so the regression-sentinel tests are correctly Windows-gated. Linux CI will not exercise them — the Windows CI runner does.

**Decision: NO ACTION.** Informational; the Windows-specific nature is inherent to the bug. The PR's test plan already lists Windows CI as the test runner for these.

Could note in CI docs that "Windows-specific regression tests require Windows CI runner coverage" — but that's a project-wide CI docs concern, not specific to this PR. Out of scope.

---

## Summary

| # | Finding | Decision |
|---|---|---|
| A | Use existing `percent-encoding` crate | **Accept** — refactor `percent_encode_path` to use `utf8_percent_encode` |
| B | `Connection::open` silent-failure risk | **Accept** — switch to `open_with_flags(_, SQLITE_OPEN_READ_ONLY)` |
| C | `with_capacity` under-allocates | **Moot** (subsumed by A) |
| D | Windows tests on Linux CI | **No action** (informational) |

**Files touched:**
- `crates/tethys/src/lsp/transport.rs` — replace `percent_encode_path` with crate-backed version; remove HEX lookup table
- `crates/tethys/tests/lsp_multi_crate.rs` — use `open_with_flags`

No tracker filings needed — all inline fixes.

## Lesson for future runs

The helper-search step should explicitly include checking `Cargo.toml` for already-imported crates, not just grep for in-source helper functions. Filing a memory update separately.
