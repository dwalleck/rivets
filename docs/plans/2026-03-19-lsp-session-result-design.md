# Design: `LspSessionResult` Structured Return Type

**Date**: 2026-03-19
**Status**: Approved
**Scope**: `crates/tethys` — LSP resolution pipeline and `IndexStats`

## Problem

LSP session diagnostics are scattered across five channels:
- Return value `(usize, Vec<String>)` — resolved count + startup errors
- Local variables logged at `debug` — total error count, unresolved attempted
- `warn!`/`trace!` logs — pre-open failures, did-open failures, solution load timeout
- `Option<i32>` from `shutdown()` — server exit code (currently discarded by caller)
- Background thread `debug!` — stderr output

Callers cannot distinguish "server didn't start" from "server started but queries
failed" from "server crashed mid-session" without parsing error strings. The
`(usize, Vec<String>)` return type forces the orchestrator to flatten per-language
results into two unrelated fields on `IndexStats`.

## Design

### Types

```rust
/// Result of a single LSP resolution session for one language.
#[derive(Debug, Clone)]
pub struct LspSessionResult {
    pub language: Language,
    pub outcome: LspOutcome,
}

/// What happened during an LSP resolution session.
#[derive(Debug, Clone)]
pub enum LspOutcome {
    /// No unresolved references existed for this language.
    NothingToResolve,

    /// The LSP server failed to start.
    ServerUnavailable {
        reason: String,
        install_hint: String,
    },

    /// The session ran to completion (possibly with errors).
    Completed(LspCompletedSession),
}

/// Statistics from a completed LSP session.
#[derive(Debug, Clone)]
pub struct LspCompletedSession {
    /// References successfully resolved via goto_definition.
    pub resolved_count: usize,
    /// Total unresolved references that were attempted.
    pub unresolved_attempted: usize,
    /// Number of goto_definition calls that failed.
    pub error_count: usize,
    /// First N error messages (capped at `MAX_ERROR_MESSAGES`).
    pub errors: Vec<String>,
    /// Server process exit code. `None` if we couldn't wait on the process.
    pub server_exit_code: Option<i32>,
}

impl LspCompletedSession {
    /// Maximum number of error messages retained per session.
    pub const MAX_ERROR_MESSAGES: usize = 5;

    /// Whether the server exited cleanly (exit code 0 or unknown).
    pub fn server_exited_cleanly(&self) -> bool {
        self.server_exit_code.map_or(true, |c| c == 0)
    }
}
```

### Convenience methods

```rust
impl LspSessionResult {
    pub fn has_resolutions(&self) -> bool;
    pub fn has_errors(&self) -> bool;
}

impl IndexStats {
    /// Total references resolved across all LSP sessions.
    pub fn total_lsp_resolved(&self) -> usize;
    /// Whether any LSP session encountered errors.
    pub fn has_lsp_errors(&self) -> bool;
}
```

### `IndexStats` field change

Replace:
```rust
pub lsp_resolved_count: usize,
pub lsp_errors: Vec<String>,
```

With:
```rust
pub lsp_sessions: Vec<LspSessionResult>,
```

### `resolve_via_lsp` return type change

From: `Result<(usize, Vec<String>)>`
To: `Result<LspSessionResult>`

The method constructs the appropriate `LspOutcome` variant at each exit point:
- No unresolved refs → `NothingToResolve`
- Server fails to start → `ServerUnavailable { reason, install_hint }`
- Session runs → `Completed(LspCompletedSession { ... })`

### CLI output (`cli/index.rs`)

Iterate `stats.lsp_sessions` and match on outcome:
- `NothingToResolve` — skip silently
- `ServerUnavailable` — print reason + install hint
- `Completed` — print resolved count; if errors, show capped messages

### Out of scope

- `get_callers_with_lsp` — different semantics (single symbol query with
  DB fallback), not batch resolution. Stays as-is.
- Stderr capture — stays in the background thread logging at `debug`. Not
  aggregated into the result struct (too noisy, unbounded size).
- Per-reference trace logs — stay in tracing. These are operator diagnostics,
  not API-level information.

## Integration

| Component | Change |
|-----------|--------|
| `types.rs` | Add `LspSessionResult`, `LspOutcome`, `LspCompletedSession` |
| `types.rs` | Replace `lsp_resolved_count` + `lsp_errors` with `lsp_sessions` on `IndexStats` |
| `types.rs` | Add convenience methods on `IndexStats` |
| `resolve.rs` | `resolve_via_lsp` returns `Result<LspSessionResult>` |
| `indexing.rs` | Push per-language `LspSessionResult` into `lsp_sessions` vec |
| `cli/index.rs` | Match on `LspOutcome` variants for display |
| `cli/stats.rs` | Use `total_lsp_resolved()` convenience method |
| Tests | Existing integration tests (LSP tests are `#[ignored]`) — update assertions |

## Error cap constant

`LspCompletedSession::MAX_ERROR_MESSAGES = 5` — defined on the struct so the
cap is documented next to the field it governs. The resolution loop uses this
constant instead of a magic `5`.
