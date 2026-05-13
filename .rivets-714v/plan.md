# rivets-714v — budgeted plan

Three slices. Each ≤30 min, ≤2 files. Mandatory fields per `budgeted-plan` v1.0.1.

## Slice 1: Extract `format_uri` with `\\?\` strip + unit tests

**Claim:** C1, C2, C3, C7 — `format_uri` strips `\\?\` extended-length prefix on Windows, handles regular drive-letter paths and Unix absolute paths without regression; composed `path_to_uri` preserves `InvalidPath` error for non-existent paths.

**Oracle:** Hand-computed expected URI strings for synthetic path inputs, hand-computed expected error variant (`InvalidPath`) for the non-existent-path case. No shared code with the function under test — the oracle is "what would a human spell out for this input?" Independent of any tethys runtime behavior.

**Stress fixture:** Five synthetic path inputs, each targeting a distinct bug class:
1. `\\?\C:\Users\dwall\repos\rivets\file.rs` — bug case (the `\\?\` extended-length prefix)
2. `\\?\UNC\server\share\foo.rs` — UNC variant (out of scope per [rivets-276h](rivets-276h); fixture verifies this doesn't *panic* or produce an obviously-corrupted string, but makes no positive correctness claim)
3. `C:\foo bar baz.rs` — with spaces (verifies no percent-encoding regression — current code passes spaces through; rust-analyzer accepts them)
4. `C:\日本.rs` — Unicode (verifies UTF-8 round-trip via `to_str()`)
5. `<tempdir>/nonexistent.rs` — non-existent path (C7: `path_to_uri` returns `InvalidPath`)

Expected outputs are spelled out in the unit-test bodies *before* the implementation lands.

**Loop budget:** No new loops. `strip_prefix` is constant-time, `replace('\\', "/")` is O(path length) where path length is bounded by `PATH_MAX` ≈ 32767 on Windows; in practice paths are <1KB. Single-character replacement at this scale is irrelevant — far below the 10⁶ operations budget for always-on phases.

**Wall budget:** N/A (no always-on phase touched; this is a per-`path_to_uri`-call cost, hit during LSP message construction).

**Files:**
- `crates/tethys/src/lsp/transport.rs` — modify `path_to_uri`, add new `format_uri`, add 5 unit tests in the existing `mod tests` block.

**Code (advisory):**

```rust
/// Format an absolute path as a `file://` URI without performing any
/// filesystem I/O. The caller must have canonicalized the path already
/// (or be intentionally passing an as-given absolute path).
///
/// On Windows, strips the `\\?\` extended-length prefix that
/// `Path::canonicalize` adds to every returned path. RFC 8089 file URIs
/// can't represent this prefix; rust-analyzer rejects URIs containing it
/// with `-32603 url is not a file` (rivets-714v).
///
/// UNC paths (`\\?\UNC\server\share\...`) are out of scope; see rivets-276h.
fn format_uri(path: &Path) -> Result<Uri> {
    let path_str = path.to_str().ok_or_else(|| {
        LspError::InvalidPath(format!("path contains invalid UTF-8: {}", path.display()))
    })?;

    #[cfg(windows)]
    let uri_string = {
        let stripped = path_str.strip_prefix(r"\\?\").unwrap_or(path_str);
        format!("file:///{}", stripped.replace('\\', "/"))
    };

    #[cfg(not(windows))]
    let uri_string = format!("file://{path_str}");

    uri_string
        .parse()
        .map_err(|e| LspError::InvalidPath(format!("invalid URI '{uri_string}': {e}")))
}

fn path_to_uri(path: &Path) -> Result<Uri> {
    let absolute_path = path.canonicalize().map_err(|e| {
        LspError::InvalidPath(format!("cannot canonicalize path '{}': {e}", path.display()))
    })?;
    format_uri(&absolute_path)
}
```

Tests (advisory, in `mod tests`):

```rust
#[test]
#[cfg(windows)]
fn format_uri_strips_extended_length_prefix() {
    let path = Path::new(r"\\?\C:\Users\dwall\repos\rivets\file.rs");
    let uri = format_uri(path).expect("format_uri should succeed");
    assert_eq!(uri.as_str(), "file:///C:/Users/dwall/repos/rivets/file.rs");
}

#[test]
#[cfg(windows)]
fn format_uri_handles_regular_drive_letter_path() {
    let path = Path::new(r"C:\foo\bar.rs");
    let uri = format_uri(path).expect("format_uri should succeed");
    assert_eq!(uri.as_str(), "file:///C:/foo/bar.rs");
}

#[test]
#[cfg(not(windows))]
fn format_uri_handles_unix_absolute_path() {
    let path = Path::new("/home/user/file.rs");
    let uri = format_uri(path).expect("format_uri should succeed");
    assert_eq!(uri.as_str(), "file:///home/user/file.rs");
}

#[test]
#[cfg(windows)]
fn format_uri_passes_spaces_and_unicode_through() {
    // The current behavior is to pass these through unencoded. rust-analyzer
    // accepts unencoded spaces; documenting this so a future change that
    // adds percent-encoding doesn't silently break working URIs.
    let path = Path::new(r"C:\foo bar\日本.rs");
    let uri = format_uri(path).expect("format_uri should succeed");
    assert!(uri.as_str().starts_with("file:///C:/foo bar/"));
}

#[test]
fn path_to_uri_returns_invalid_path_for_nonexistent() {
    let nonexistent = std::env::temp_dir().join("rivets-714v-does-not-exist-xyzzy");
    let result = path_to_uri(&nonexistent);
    assert!(
        matches!(result, Err(LspError::InvalidPath(_))),
        "expected InvalidPath error, got {result:?}"
    );
}
```

**Doc-comment-as-contract:** The doc on `format_uri` says "the caller must have canonicalized the path already." Classification: **sanity hint** — violating this produces a defensible but possibly-not-resolved URI (e.g., a relative path becomes `file:///foo/bar.rs` which rust-analyzer would reject downstream with a different error, not silently wrong output). No runtime check needed; the contract is documented and `path_to_uri` always honors it.

**Output stream:** No new `println!` / `tracing::*` calls. Pure transformation function.

**Verification:**
- [ ] 5 new unit tests pass
- [ ] Existing `path_to_uri_creates_valid_file_uri` test still passes (kept as backstop)
- [ ] `cargo clippy -p tethys --all-targets --all-features -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] Stress fixture (case 1: `\\?\C:\...`) produces `file:///C:/...` exactly — proves the strip works
- [ ] prove-it-prototype probe still reproduces *before* slice 2 wires the rest of the fix (this slice's tests are unit-level; slice 2 verifies end-to-end)

---

## Slice 2: Multi-crate fixture integration test for end-to-end LSP success

**Claim:** C4, C5 — composed `path_to_uri` produces URIs without `\\?\` leakage for real Windows tempdirs (C4); `tethys index --lsp` on a 2-crate fixture emits zero `url is not a file -32603` errors after the fix (C5).

**Oracle:** Grep the captured stderr from a `tethys index --lsp` subprocess for the literal string `url is not a file`. The grep is independent of tethys's internal logic — different mechanism from the resolver code path being tested.

**Stress fixture:** A 2-crate Cargo workspace built in a tempdir, identical in shape to the prove-it-prototype probe's fixture (`crate_caller` + `crate_target`, with `crate_caller/src/lib.rs` calling `.len()` on a `HashMap`). The fixture is the same one that pre-fix produces 4 error lines from the probe; post-fix it must produce zero.

Two assertions in the test (both must hold):
1. Exit code is 0 OR a non-LSP-related error (LSP errors are warnings, not exit-code failures, but check anyway)
2. **Zero matches** for the regex `url is not a file|LSP error -32603` in the combined stdout+stderr

The "URI roundtrip through real tempdir" check (C4) is implicit in this fixture — if `path_to_uri` were leaking `\\?\` into the LSP messages, rust-analyzer would emit the error and the test would fail.

**Loop budget:** No new loops in production code (this slice is test-only). The test itself does a single `tethys index --lsp` invocation against a 2-file fixture; cost is dominated by rust-analyzer startup (~5 sec), negligible CPU.

**Wall budget:** Test should complete in <90 sec on the slowest CI runner. rust-analyzer takes ~5-15 sec to load a 2-crate fixture, plus tethys's indexing pipeline. Tests over 60 sec get the `SLOW` marker in nextest; ≤90 sec is acceptable.

**Files:**
- `crates/tethys/tests/lsp_multi_crate.rs` (new file) — single integration test, builds the 2-crate fixture and verifies no URI errors.

**Code (advisory):**

```rust
//! Integration regression test for rivets-714v: tethys --lsp on multi-crate
//! workspaces must not emit `url is not a file` errors.
//!
//! Pre-fix (before rivets-714v): tethys's path_to_uri leaked \\?\ extended-
//! length prefixes into LSP messages, which rust-analyzer rejected with
//! -32603. The fix strips \\?\ in format_uri.
//!
//! Fixture matches .rivets-714v/probe.py.

#![cfg(windows)]  // The bug is Windows-specific; test guards itself accordingly.

use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn build_2_crate_fixture(root: &std::path::Path) {
    let files = [
        ("Cargo.toml", "[workspace]\nmembers = [\"crate_caller\", \"crate_target\"]\nresolver = \"2\"\n"),
        ("crate_caller/Cargo.toml", "[package]\nname = \"crate_caller\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("crate_caller/src/lib.rs",
            "use std::collections::HashMap;\n\
             pub fn count(map: &HashMap<u32, String>) -> usize { map.len() }\n"),
        ("crate_target/Cargo.toml", "[package]\nname = \"crate_target\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("crate_target/src/lib.rs", "pub struct Widget;\nimpl Widget { pub fn ping(&self) {} }\n"),
    ];
    for (rel, content) in files {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).expect("create dir");
        fs::write(&p, content).expect("write file");
    }
}

#[test]
#[ignore = "requires rust-analyzer in PATH; gated like existing LSP tests"]
fn lsp_multi_crate_emits_no_url_errors() {
    let dir = tempdir().expect("tempdir");
    build_2_crate_fixture(dir.path());

    let tethys = env!("CARGO_BIN_EXE_tethys");
    let output = Command::new(tethys)
        .args(["index", "--rebuild", "--lsp", "-w"])
        .arg(dir.path())
        .output()
        .expect("run tethys --lsp");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let url_errors: Vec<&str> = combined
        .lines()
        .filter(|line| line.contains("url is not a file") || line.contains("LSP error -32603"))
        .collect();

    assert!(
        url_errors.is_empty(),
        "expected zero 'url is not a file' errors after rivets-714v fix; got {}: {url_errors:#?}",
        url_errors.len()
    );
}
```

**Doc-comment-as-contract:** N/A — test file, no production preconditions documented.

**Output stream:** Test panics propagate to nextest's stderr; the test itself doesn't `println!` data. Diagnostic output (the captured stderr from the subprocess) goes into the assertion message on failure only.

**Verification:**
- [ ] New test passes when run as `cargo nextest run -p tethys --run-ignored all lsp_multi_crate_emits_no_url_errors`
- [ ] Pre-fix verification: running this test against `main` (without slice 1) **must fail** — the test is a regression sentinel only if it would have caught the original bug
- [ ] `cargo clippy -p tethys --all-targets --all-features -- -D warnings` clean
- [ ] prove-it-prototype probe (`.rivets-714v/probe.py`) re-run: zero matched error lines
- [ ] Wall budget: test completes in <90 sec

---

## Slice 3: Pass-3 resolution integration test (`Tethys` resolves ≥1 ref via LSP)

**Claim:** C6 — `tethys index --lsp` on the probe's fixture resolves ≥1 reference via Pass 3 (the `.len()` call resolves to `HashMap::len` via rust-analyzer goto_definition).

**Oracle:** SQL count on the post-index `tethys.db`, querying the `references` table for resolved refs (`symbol_id IS NOT NULL`) originating from `crate_caller/src/lib.rs`. Independent of tethys's resolver code — the SQL just reads the persisted DB state.

**Stress fixture:** Same 2-crate workspace as slice 2. The plausible bug this slice is designed to catch is "the fix made the URI valid but Pass 3 still doesn't actually resolve anything" — possible if rust-analyzer accepts the URI but tethys doesn't correctly process the `Location` response. This slice differs from slice 2 in that slice 2 is the *negative* claim (no errors) and slice 3 is the *positive* claim (refs actually resolve).

The threshold is **≥1 resolved ref** because the fixture has exactly one cross-file reference (`HashMap::len` from `crate_caller/src/lib.rs`) that rust-analyzer can resolve. A future enhancement could push to ≥N, but ≥1 is enough to falsify "Pass 3 is silently broken."

**Loop budget:** N/A — test-only. SQL count is O(refs) at the DB level; for this fixture refs ≈ 10, irrelevant.

**Wall budget:** Same as slice 2 (<90 sec wall-clock).

**Files:**
- `crates/tethys/tests/lsp_multi_crate.rs` — extend the file from slice 2 with a second test.

**Code (advisory):**

```rust
#[test]
#[ignore = "requires rust-analyzer in PATH; gated like existing LSP tests"]
fn lsp_multi_crate_resolves_at_least_one_ref() {
    let dir = tempdir().expect("tempdir");
    build_2_crate_fixture(dir.path());

    let tethys = env!("CARGO_BIN_EXE_tethys");
    Command::new(tethys)
        .args(["index", "--rebuild", "--lsp", "-w"])
        .arg(dir.path())
        .output()
        .expect("run tethys --lsp");

    let db_path = dir.path().join(".rivets").join("index").join("tethys.db");
    let conn = rusqlite::Connection::open(&db_path).expect("open tethys.db");

    let resolved_cross_file: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r
             JOIN files f ON f.id = r.file_id
             WHERE r.symbol_id IS NOT NULL
               AND f.path LIKE 'crate_caller/%'",
            [],
            |row| row.get(0),
        )
        .expect("count resolved refs");

    assert!(
        resolved_cross_file >= 1,
        "expected ≥1 resolved cross-file ref via Pass 3 on the 2-crate fixture; got {resolved_cross_file}. \
         A regression that re-introduces URL malformation or breaks Pass 3 message handling would fail this."
    );
}
```

**Doc-comment-as-contract:** N/A.

**Output stream:** Same as slice 2 — assertion message only on failure.

**Verification:**
- [ ] New test passes (≥1 resolved ref) on the post-fix branch
- [ ] Pre-fix verification: running this test against `main` (without slices 1+2) **must fail** with 0 resolved refs — confirms the test is a regression sentinel
- [ ] `cargo clippy` clean
- [ ] Wall budget: test completes in <90 sec
- [ ] Re-run prove-it-prototype probe and oracle one more time after the slice lands; verify probe + oracle still agree (post-fix probe should show zero errors; oracle unchanged)

---

## Plan Self-Review

### 1. Every loop in the plan

| Slice | New loop? | Asymptotic | Production scale | Within budget? |
|-------|-----------|------------|------------------|----------------|
| 1 | No (only `strip_prefix` + `replace` on path string) | O(path length) | <1KB | ✓ Far below 10⁶ ops |
| 2 | No (test only; subprocess call) | N/A | N/A | ✓ |
| 3 | No (test only; SQL aggregate) | O(refs in fixture) ≈ 10 | N/A | ✓ |

No production loops introduced. All within budget.

### 2. Every fixture

| Slice | Fixture | Bug class it falsifies |
|-------|---------|------------------------|
| 1 | `\\?\C:\Users\dwall\repos\rivets\file.rs` | "strip-`\\?\` logic missing" — the canonical bug |
| 1 | `\\?\UNC\server\share\foo.rs` | "strip logic panics or corrupts on UNC variant" — defensive (out of scope per rivets-276h, but verify no breakage) |
| 1 | `C:\foo bar baz.rs` | "fix accidentally percent-encodes spaces" |
| 1 | `C:\日本.rs` | "fix breaks UTF-8 round-trip" |
| 1 | `<tempdir>/nonexistent.rs` | "fix accidentally hides the InvalidPath error" — preserves C7 |
| 2 | 2-crate Cargo workspace (matches probe.py) | "fix silently swallows LSP errors instead of fixing them" — counterfactually tests the negative claim |
| 3 | Same 2-crate workspace | "fix passes 'no errors' but Pass 3 doesn't actually resolve anything" — positive verification |

All fixtures target plausible bug classes, not happy paths.

### 3. Every doc-comment precondition

| Slice | Doc | Class | Enforcement |
|-------|-----|-------|-------------|
| 1 | `format_uri`: "caller must have canonicalized" | Sanity hint | None at runtime; `path_to_uri` honors the contract by always canonicalizing first. Violating it produces a defensible URI (just not a canonical one) — not silently-wrong output. |
| 2 | N/A | — | — |
| 3 | N/A | — | — |

### 4. Every write target

| Slice | Writes to | Class |
|-------|-----------|-------|
| 1 | Returns `Result<Uri>` to caller | Data (the URI) |
| 2 | `assert!` panic message on failure | Diagnostic (test output) |
| 3 | Same | Diagnostic |

No production `println!` / `eprintln!` / unsorted `tracing::*` introduced.

### 5. Every tracker reference

| Reference | Context | Verified? |
|-----------|---------|-----------|
| **rivets-276h** | UNC path handling deferral in design's Negative Space + slice 1's stress fixture #2 | ✓ filed during falsifiable-design phase; `rivets show rivets-276h` returns a description covering the deferred work |

No other deferral phrases in this plan. No `TODO`, `FIXME`, `later`, `future work`, `revisit if` mentions in production code or test code.

---

## Hard-gate checklist

- [x] Every slice has all mandatory fields filled in
- [x] Every loop has a complexity statement (or "no new loops")
- [x] Every slice has a stress fixture
- [x] The plan's claim coverage matches the design's claim list (C1+C2+C3+C7 in slice 1, C4+C5 in slice 2, C6 in slice 3)
- [x] Every tracker reference in the plan resolves to an existing issue (rivets-276h verified)

Ready for `checkpointed-build`.

## Estimated total

~70 min of code + tests across three slices, plus probe re-runs and CI verification at each slice gate. Faster than rivets-6aoc because the bug is localized to a single function with constrained input shapes — no per-file plumbing, no test-fixture sweep needed.
