# Review decisions — round 1 (PR #64)

Date: 2026-05-13
Reviewers: `gemini-code-assist[bot]` (positive summary, no findings) + `claude[bot]` (4 substantive findings + 2 nits).

Per `assessing-review-feedback` v1.0.1: each finding verified as a hypothesis before acceptance.

---

## Finding 1: `use std::fmt::Write` inside inner loop (style)

**Source:** Claude review.
**Location:** `crates/tethys/src/lsp/transport.rs::percent_encode_path`.

**Claim:** Re-importing the `fmt::Write` trait inside the inner loop body is unconventional. Move to the top of the function OR eliminate the trait usage entirely.

**Verification:** Confirmed. The current code has `use std::fmt::Write;` inside the `for b in ...` loop, which is valid Rust but wasteful (the import is re-resolved each iteration in terms of reader cognition, not runtime).

**Decision: ACCEPT.** Eliminate the trait usage entirely — `push_str(&format!("%{b:02X}"))` does the same thing without the trait import OR the discarded `Result`. Subsumes finding 2.

---

## Finding 2: `let _ = write!(out, ...)` on an infallible writer (style)

**Source:** Claude review.

**Claim:** `String`'s `fmt::Write` impl is infallible; `let _ =` silences the must-use lint but doesn't document the invariant. Future readers may wonder why the result is discarded.

**Verification:** Confirmed. The `Result` from `write!` to a `String` is always `Ok(())`.

**Decision: ACCEPT** (folded into finding 1's fix). Using `push_str(&format!(...))` sidesteps the discarded `Result` entirely.

---

## Finding 3: No Unix percent-encoding test (coverage gap)

**Source:** Claude review.
**Location:** `crates/tethys/src/lsp/transport.rs` tests module.

**Claim:** All percent-encoding tests are `#[cfg(windows)]`, but `percent_encode_path` runs on Unix too (the non-Windows branch of `format_uri`). A Unix runner would never verify the encoding works on its platform.

**Verification:** Confirmed. The percent_encodes_spaces test and percent_encodes_unicode_as_utf8_bytes test are both Windows-gated. The Unix-only test `format_uri_handles_unix_absolute_path` uses a path with no chars needing encoding (`/home/user/file.rs`), so it doesn't exercise the encoding path.

**Decision: ACCEPT.** Add `#[cfg(not(windows))]` counterpart: `format_uri_percent_encodes_spaces_unix` with a path like `/home/user/my project/file.rs`.

---

## Finding 4: SQL path filter assumes workspace-relative paths (silent-failure risk)

**Source:** Claude review.
**Location:** `crates/tethys/tests/lsp_multi_crate.rs::lsp_multi_crate_resolves_at_least_one_cross_file_ref`.

**Claim:** The SQL filter `AND f_caller.path LIKE 'crate_caller/%'` assumes tethys stores workspace-relative paths in `files.path`. If tethys ever switches to absolute paths (or already stores them when indexing temp directories), the LIKE never matches, the COUNT is always 0, the assertion fires every run regardless of whether the URI fix works.

**Verification:** Two-step:

1. **Does the LIKE work today?** The test PASSED on the post-fix branch with `>= 1`. If the LIKE didn't match anything, the count would be 0 and the assertion would fail. So paths ARE stored workspace-relative in the current implementation.

2. **Could it silently break later?** Yes. The test would still pass-or-fail based on the storage form rather than the URI-fix state. That's the silent-failure mode the reviewer flagged.

The reviewer's proposed fix — drop the LIKE entirely, just assert any resolved cross-file ref — is strictly stronger. Doesn't depend on the path storage form.

**Decision: ACCEPT.** Refactor the query to:

```sql
SELECT COUNT(*) FROM refs r
JOIN symbols s ON s.id = r.symbol_id
WHERE r.symbol_id IS NOT NULL
  AND r.file_id != s.file_id
```

This still asserts the C6 claim (cross-file resolved ref exists) without the path-schema assumption. The 2-crate fixture is small enough that any cross-file ref is meaningful evidence.

---

## Nit 1: Existing backstop test rationale

**Source:** Claude review.

**Claim:** `path_to_uri_creates_valid_file_uri` has weaker assertions than the new exact-string tests; a brief inline comment would explain why it's kept.

**Decision: ACCEPT.** One-line comment.

---

## Nit 2: Cargo.toml edition update

**Source:** Claude review.

**Claim:** Inline Cargo.toml strings in the test fixture will need updating when the minimum edition requirement changes.

**Decision: REJECT.** Speculative; would create noise. The edition string is appropriately scoped to the current Cargo edition.

---

## Summary

| # | Finding | Decision |
|---|---|---|
| 1 | `Write` import in loop | Accept — refactor to `push_str(&format!(...))` |
| 2 | `let _ = write!()` on infallible writer | Accept — folded into #1 |
| 3 | No Unix percent-encoding test | Accept — add `#[cfg(not(windows))]` test |
| 4 | SQL LIKE filter — silent-failure risk | Accept — drop LIKE, assert any cross-file ref |
| Nit 1 | Backstop test rationale | Accept — inline comment |
| Nit 2 | Cargo.toml edition speculation | Reject |

**Files touched:**
- `crates/tethys/src/lsp/transport.rs` — refactor `percent_encode_path`; add Unix test; add backstop comment
- `crates/tethys/tests/lsp_multi_crate.rs` — drop LIKE from SQL query

No tracker filings needed — all decisions are inline fixes, no deferrals.
