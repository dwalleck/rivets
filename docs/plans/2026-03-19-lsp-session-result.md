# `LspSessionResult` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the scattered `(usize, Vec<String>)` LSP return type with a structured `LspSessionResult` that aggregates all session diagnostics.

**Architecture:** Three new types (`LspSessionResult`, `LspOutcome`, `LspCompletedSession`) in `types.rs`. `resolve_via_lsp` returns `LspSessionResult`. `IndexStats` replaces `lsp_resolved_count` + `lsp_errors` with `lsp_sessions: Vec<LspSessionResult>`. Convenience methods bridge the gap for simple consumers.

**Tech Stack:** Rust, thiserror (for existing error types), tethys crate internals.

**Design doc:** `docs/plans/2026-03-19-lsp-session-result-design.md`

---

### Task 1: Add the three new types to `types.rs`

**Files:**
- Modify: `crates/tethys/src/types.rs` (add types after `IndexStats` definition, ~line 865)

**Step 1: Write unit tests for the new types**

Add to the existing `#[cfg(test)] mod tests` block in `types.rs`:

```rust
// ========================================================================
// LspSessionResult tests
// ========================================================================

#[test]
fn lsp_completed_session_exited_cleanly_with_zero() {
    let session = LspCompletedSession {
        resolved_count: 5,
        unresolved_attempted: 10,
        error_count: 0,
        errors: vec![],
        server_exit_code: Some(0),
    };
    assert!(session.server_exited_cleanly());
}

#[test]
fn lsp_completed_session_exited_cleanly_with_none() {
    let session = LspCompletedSession {
        resolved_count: 0,
        unresolved_attempted: 0,
        error_count: 0,
        errors: vec![],
        server_exit_code: None,
    };
    assert!(session.server_exited_cleanly());
}

#[test]
fn lsp_completed_session_not_clean_with_nonzero() {
    let session = LspCompletedSession {
        resolved_count: 3,
        unresolved_attempted: 10,
        error_count: 2,
        errors: vec!["timeout".to_string()],
        server_exit_code: Some(1),
    };
    assert!(!session.server_exited_cleanly());
}

#[test]
fn lsp_session_result_has_resolutions_completed() {
    let result = LspSessionResult {
        language: Language::Rust,
        outcome: LspOutcome::Completed(LspCompletedSession {
            resolved_count: 5,
            unresolved_attempted: 10,
            error_count: 0,
            errors: vec![],
            server_exit_code: Some(0),
        }),
    };
    assert!(result.has_resolutions());
    assert!(!result.has_errors());
}

#[test]
fn lsp_session_result_has_errors_server_unavailable() {
    let result = LspSessionResult {
        language: Language::Rust,
        outcome: LspOutcome::ServerUnavailable {
            reason: "not found".to_string(),
            install_hint: "install rust-analyzer".to_string(),
        },
    };
    assert!(!result.has_resolutions());
    assert!(result.has_errors());
}

#[test]
fn lsp_session_result_nothing_to_resolve() {
    let result = LspSessionResult {
        language: Language::CSharp,
        outcome: LspOutcome::NothingToResolve,
    };
    assert!(!result.has_resolutions());
    assert!(!result.has_errors());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p tethys -- lsp_session_result`
Expected: compilation errors (types don't exist yet)

**Step 3: Add the type definitions**

Add after the `IndexStats` struct (after line 865):

```rust
/// Maximum number of error messages retained per LSP session.
/// Defined here so the cap is documented next to the types it governs.
const LSP_MAX_ERROR_MESSAGES: usize = 5;

/// Result of a single LSP resolution session for one language.
#[derive(Debug, Clone)]
pub struct LspSessionResult {
    /// The language this session resolved references for.
    pub language: Language,
    /// What happened during the session.
    pub outcome: LspOutcome,
}

/// What happened during an LSP resolution session.
#[derive(Debug, Clone)]
pub enum LspOutcome {
    /// No unresolved references existed for this language.
    NothingToResolve,

    /// The LSP server failed to start.
    ServerUnavailable {
        /// Why the server failed to start.
        reason: String,
        /// How to install the missing server.
        install_hint: String,
    },

    /// The session ran to completion (possibly with errors).
    Completed(LspCompletedSession),
}

/// Statistics from a completed LSP session.
///
/// Created when the LSP server starts successfully and processes references,
/// regardless of how many succeed or fail.
#[derive(Debug, Clone)]
pub struct LspCompletedSession {
    /// References successfully resolved via `goto_definition`.
    pub resolved_count: usize,
    /// Total unresolved references that were attempted.
    pub unresolved_attempted: usize,
    /// Number of `goto_definition` calls that failed.
    pub error_count: usize,
    /// First few error messages (capped at [`Self::MAX_ERROR_MESSAGES`]).
    pub errors: Vec<String>,
    /// Server process exit code. `None` if we couldn't wait on the process.
    pub server_exit_code: Option<i32>,
}

impl LspCompletedSession {
    /// Maximum number of error messages retained per session.
    pub const MAX_ERROR_MESSAGES: usize = LSP_MAX_ERROR_MESSAGES;

    /// Whether the server exited cleanly (exit code 0 or unknown).
    #[must_use]
    pub fn server_exited_cleanly(&self) -> bool {
        self.server_exit_code.map_or(true, |c| c == 0)
    }
}

impl LspSessionResult {
    /// Whether this session resolved any references.
    #[must_use]
    pub fn has_resolutions(&self) -> bool {
        matches!(&self.outcome, LspOutcome::Completed(s) if s.resolved_count > 0)
    }

    /// Whether this session encountered any errors (including server unavailable).
    #[must_use]
    pub fn has_errors(&self) -> bool {
        match &self.outcome {
            LspOutcome::NothingToResolve => false,
            LspOutcome::ServerUnavailable { .. } => true,
            LspOutcome::Completed(s) => s.error_count > 0,
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p tethys -- lsp_session_result`
Expected: all 6 tests PASS

**Step 5: Commit**

```
feat(tethys): add LspSessionResult, LspOutcome, LspCompletedSession types
```

---

### Task 2: Add convenience methods to `IndexStats` and update its fields

**Files:**
- Modify: `crates/tethys/src/types.rs:837-865` (replace `lsp_resolved_count` + `lsp_errors`)

**Step 1: Write unit tests for the convenience methods**

Add to the test block in `types.rs`:

```rust
#[test]
fn index_stats_total_lsp_resolved_sums_sessions() {
    let stats = IndexStats {
        files_indexed: 10,
        symbols_found: 20,
        references_found: 30,
        duration: Duration::from_secs(1),
        files_skipped: 0,
        directories_skipped: vec![],
        errors: vec![],
        unresolved_dependencies: vec![],
        lsp_sessions: vec![
            LspSessionResult {
                language: Language::Rust,
                outcome: LspOutcome::Completed(LspCompletedSession {
                    resolved_count: 5,
                    unresolved_attempted: 10,
                    error_count: 0,
                    errors: vec![],
                    server_exit_code: Some(0),
                }),
            },
            LspSessionResult {
                language: Language::CSharp,
                outcome: LspOutcome::Completed(LspCompletedSession {
                    resolved_count: 3,
                    unresolved_attempted: 8,
                    error_count: 1,
                    errors: vec!["timeout".to_string()],
                    server_exit_code: Some(0),
                }),
            },
        ],
    };
    assert_eq!(stats.total_lsp_resolved(), 8);
    assert!(stats.has_lsp_errors());
}

#[test]
fn index_stats_no_lsp_sessions() {
    let stats = IndexStats {
        files_indexed: 10,
        symbols_found: 20,
        references_found: 30,
        duration: Duration::from_secs(1),
        files_skipped: 0,
        directories_skipped: vec![],
        errors: vec![],
        unresolved_dependencies: vec![],
        lsp_sessions: vec![],
    };
    assert_eq!(stats.total_lsp_resolved(), 0);
    assert!(!stats.has_lsp_errors());
}
```

**Step 2: Run tests to verify compilation fails**

Run: `cargo nextest run -p tethys -- index_stats_total`
Expected: compilation errors (field doesn't exist yet)

**Step 3: Replace fields and add convenience methods**

In `IndexStats`, replace:
```rust
    pub lsp_resolved_count: usize,
    // ... doc comments ...
    pub lsp_errors: Vec<String>,
```

With:
```rust
    /// Results from LSP resolution sessions (one per language attempted).
    ///
    /// Empty when `IndexOptions::use_lsp` was not set.
    pub lsp_sessions: Vec<LspSessionResult>,
```

Add an `impl IndexStats` block:
```rust
impl IndexStats {
    /// Total references resolved across all LSP sessions.
    #[must_use]
    pub fn total_lsp_resolved(&self) -> usize {
        self.lsp_sessions.iter().map(|s| match &s.outcome {
            LspOutcome::Completed(c) => c.resolved_count,
            _ => 0,
        }).sum()
    }

    /// Whether any LSP session encountered errors.
    #[must_use]
    pub fn has_lsp_errors(&self) -> bool {
        self.lsp_sessions.iter().any(LspSessionResult::has_errors)
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p tethys -- index_stats_total`
Expected: 2 tests PASS (but other code won't compile yet — that's expected)

**Step 5: Commit**

```
refactor(tethys): replace lsp_resolved_count + lsp_errors with lsp_sessions on IndexStats
```

---

### Task 3: Update `resolve_via_lsp` to return `LspSessionResult`

**Files:**
- Modify: `crates/tethys/src/resolve.rs:382-565` (change return type and exit points)

**Step 1: Change the function signature**

Change:
```rust
pub(crate) fn resolve_via_lsp(
    &self,
    provider: &dyn lsp::LspProvider,
    language: Language,
    lsp_timeout_secs: u64,
) -> Result<(usize, Vec<String>)> {
```

To:
```rust
pub(crate) fn resolve_via_lsp(
    &self,
    provider: &dyn lsp::LspProvider,
    language: Language,
    lsp_timeout_secs: u64,
) -> Result<LspSessionResult> {
```

Add the import at the top of `resolve.rs`:
```rust
use crate::types::{LspSessionResult, LspOutcome, LspCompletedSession};
```

**Step 2: Update the three exit points**

Exit 1 — no unresolved refs (line ~405):
```rust
return Ok(LspSessionResult {
    language,
    outcome: LspOutcome::NothingToResolve,
});
```

Exit 2 — server failed to start (line ~431):
```rust
return Ok(LspSessionResult {
    language,
    outcome: LspOutcome::ServerUnavailable {
        reason: format!("LSP server for {language:?} failed to start: {e}"),
        install_hint: provider.install_hint().to_string(),
    },
});
```

Exit 3 — session completed (line ~565):
```rust
let exit_code = match client.shutdown() {
    Ok(code) => code,
    Err(e) => {
        warn!(error = %e, "LSP shutdown failed");
        None
    }
};

Ok(LspSessionResult {
    language,
    outcome: LspOutcome::Completed(LspCompletedSession {
        resolved_count,
        unresolved_attempted: unresolved.len(),
        error_count: lsp_error_count,
        errors: lsp_error_messages,
        server_exit_code: exit_code,
    }),
})
```

**Step 3: Update the error cap to use the constant**

In `resolve_single_ref_via_lsp`, change:
```rust
if *lsp_error_count <= 5 {
```
To:
```rust
if *lsp_error_count <= LspCompletedSession::MAX_ERROR_MESSAGES {
```

**Step 4: Verify resolve.rs compiles (other files will still fail)**

Run: `cargo check -p tethys 2>&1 | head -20`
Expected: errors in `indexing.rs` and `cli/index.rs` (not yet updated)

**Step 5: Commit**

```
refactor(tethys): return LspSessionResult from resolve_via_lsp
```

---

### Task 4: Update `index_with_options` orchestrator

**Files:**
- Modify: `crates/tethys/src/indexing.rs:386-458` (the LSP resolution block)

**Step 1: Replace the orchestration logic**

Change the LSP block (lines ~386-458) from:
```rust
let mut lsp_errors_collected: Vec<String> = vec![];
let lsp_resolved_count = if options.use_lsp() {
    let mut total_resolved = 0;
    // ... per-language calls ...
    total_resolved
} else {
    0
};
```

To:
```rust
let mut lsp_sessions: Vec<LspSessionResult> = Vec::new();
if options.use_lsp() {
    let rust_provider = lsp::AnyProvider::for_language(Language::Rust);
    let rust_result =
        self.resolve_via_lsp(&rust_provider, Language::Rust, options.lsp_timeout_secs())?;
    if rust_result.has_resolutions() {
        tracing::info!(
            language = "rust",
            resolved_count = rust_result.total_resolved(),
            "Resolved references via LSP"
        );
    }
    lsp_sessions.push(rust_result);

    let csharp_provider = lsp::AnyProvider::for_language(Language::CSharp);
    let csharp_result = self.resolve_via_lsp(
        &csharp_provider,
        Language::CSharp,
        options.lsp_timeout_secs(),
    )?;
    if csharp_result.has_resolutions() {
        tracing::info!(
            language = "csharp",
            resolved_count = csharp_result.total_resolved(),
            "Resolved references via LSP"
        );
    }
    lsp_sessions.push(csharp_result);
}
```

Note: `total_resolved()` doesn't exist yet on `LspSessionResult`. We need a small helper — or just inline the match. Check if it's cleaner to add a method or inline.
Decision: add a small `resolved_count()` method to `LspSessionResult`:

```rust
impl LspSessionResult {
    /// Number of references resolved in this session (0 if not completed).
    #[must_use]
    pub fn resolved_count(&self) -> usize {
        match &self.outcome {
            LspOutcome::Completed(s) => s.resolved_count,
            _ => 0,
        }
    }
}
```

**Step 2: Update the `IndexStats` construction**

Change:
```rust
lsp_resolved_count,
lsp_errors: lsp_errors_collected,
```
To:
```rust
lsp_sessions,
```

**Step 3: Update the doc example**

In the `index_with_options` doc comment (~line 96), change:
```rust
/// println!("Resolved {} references via LSP", stats.lsp_resolved_count);
```
To:
```rust
/// println!("Resolved {} references via LSP", stats.total_lsp_resolved());
```

**Step 4: Verify indexing.rs compiles**

Run: `cargo check -p tethys 2>&1 | head -20`
Expected: errors in `cli/index.rs` and test files (not yet updated)

**Step 5: Commit**

```
refactor(tethys): use LspSessionResult in index_with_options orchestrator
```

---

### Task 5: Update CLI output (`cli/index.rs`)

**Files:**
- Modify: `crates/tethys/src/cli/index.rs:89-102`

**Step 1: Replace the LSP output block**

Change:
```rust
if stats.lsp_resolved_count > 0 {
    println!(
        "{}: {} references via LSP",
        "LSP resolved".cyan(),
        stats.lsp_resolved_count
    );
}

if !stats.lsp_errors.is_empty() {
    println!();
    for err in &stats.lsp_errors {
        println!("{}: {err}", "LSP error".red());
    }
}
```

To:
```rust
for session in &stats.lsp_sessions {
    match &session.outcome {
        tethys::LspOutcome::NothingToResolve => {}
        tethys::LspOutcome::ServerUnavailable {
            reason,
            install_hint,
        } => {
            println!();
            println!(
                "{} ({:?}): {reason}",
                "LSP unavailable".red().bold(),
                session.language,
            );
            println!("  {install_hint}");
        }
        tethys::LspOutcome::Completed(s) => {
            if s.resolved_count > 0 {
                println!(
                    "{} ({:?}): {} of {} references",
                    "LSP resolved".cyan(),
                    session.language,
                    s.resolved_count,
                    s.unresolved_attempted,
                );
            }
            if !s.errors.is_empty() {
                for err in &s.errors {
                    println!("{} ({:?}): {err}", "LSP error".red(), session.language);
                }
                if s.error_count > s.errors.len() {
                    println!(
                        "  ... and {} more errors",
                        s.error_count - s.errors.len()
                    );
                }
            }
            if !s.server_exited_cleanly() {
                println!(
                    "{} ({:?}): server exited with code {:?}",
                    "LSP warning".yellow(),
                    session.language,
                    s.server_exit_code,
                );
            }
        }
    }
}
```

Add import at top of `cli/index.rs`:
```rust
use tethys::{IndexOptions, LspOutcome, Tethys};
```

**Step 2: Verify compilation**

Run: `cargo check -p tethys 2>&1 | head -20`
Expected: errors only in test files (not yet updated)

**Step 3: Commit**

```
refactor(tethys): update CLI to display LspSessionResult per-language outcomes
```

---

### Task 6: Update test files

**Files:**
- Modify: `crates/tethys/src/lib.rs:1003-1006` (non-LSP assertion)
- Modify: `crates/tethys/tests/lsp_resolution.rs:102,192,254,269,333` (LSP test assertions)

**Step 1: Update lib.rs unit test**

Change:
```rust
assert_eq!(
    stats.lsp_resolved_count, 0,
    "LSP resolved count should be 0 when use_lsp is false"
);
```
To:
```rust
assert!(
    stats.lsp_sessions.is_empty(),
    "LSP sessions should be empty when use_lsp is false"
);
```

**Step 2: Update lsp_resolution.rs integration tests**

Each reference to `stats.lsp_resolved_count` becomes `stats.total_lsp_resolved()`.

For example, change:
```rust
stats.lsp_resolved_count
```
To:
```rust
stats.total_lsp_resolved()
```

At line 254:
```rust
assert_eq!(
    stats_no_lsp.total_lsp_resolved(), 0,
    "non-LSP index should report 0 LSP resolutions"
);
```

**Step 3: Run full test suite**

Run: `cargo nextest run -p tethys`
Expected: all tests PASS (LSP tests are `#[ignore]` so they won't run)

Run: `cargo clippy --workspace`
Expected: zero warnings

Run: `cargo fmt --check`
Expected: clean

**Step 4: Commit**

```
test(tethys): update test assertions for LspSessionResult migration
```

---

### Task 7: Final verification

**Step 1: Run full workspace build + clippy**

Run: `cargo clippy --workspace`
Expected: zero warnings

**Step 2: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all 1357+ tests PASS

**Step 3: Run doc tests**

Run: `cargo test --doc --workspace`
Expected: all doc tests PASS

**Step 4: Verify formatting**

Run: `cargo fmt --check`
Expected: clean

**Step 5: Commit (if any fixups needed)**

```
fix(tethys): address clippy/fmt findings from LspSessionResult migration
```
