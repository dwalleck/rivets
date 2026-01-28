# Cross-File Reference Resolution — Design Document

**Status:** Approved
**Date:** 2026-01-28
**Author:** Claude + Human collaboration

## Problem Statement

Currently, Tethys only stores references when the target symbol is in the *same file*. When `lib.rs` calls `Index::open()` (defined in `db.rs`), the reference is extracted but not linked to the symbol. This breaks:

- `tethys callers "Index::open"` — returns 0 callers
- `tethys impact --symbol "Index::open"` — shows no dependents

## Goals

1. **Cross-file reference resolution via tree-sitter** — Resolve references to symbols in other files by matching import paths to the symbol database. Target: ~85% of cross-file references resolved without LSP.

2. **LSP refinement for ambiguous cases** — For cases tree-sitter can't resolve (trait methods, method calls on unknown types), delegate to rust-analyzer/csharp-ls. Target: ~98% accuracy when LSP enabled.

3. **Backward compatible** — Non-LSP mode remains fast and works offline. LSP is opt-in via `--lsp` flag.

4. **Language-agnostic design** — Same resolution infrastructure for Rust and C#, with language-specific import semantics handled by `LanguageSupport` trait.

## Success Criteria

- `tethys callers "Index::open"` finds callers in `lib.rs`
- `tethys impact --symbol "extract_symbols"` shows transitive dependents
- `tethys index --lsp` completes successfully with rust-analyzer
- C# cross-file resolution works for namespace imports

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Resolution approach | Two-pass indexing | Simple, uses existing infrastructure, handles circular imports |
| Language scope | Both Rust and C# | Infrastructure is language-agnostic, minimal extra work |
| LSP servers | rust-analyzer first, csharp-ls fast follow | Validate architecture before multiple LSPs |
| LSP lifecycle | User-controlled via `--lsp` flag | User decides when to pay startup cost |
| LSP missing behavior | Error and halt | Explicit request should mean explicit failure |

---

## Phase 1: Tree-Sitter Cross-File Resolution

### Two-Pass Indexing

The current single-pass indexing extracts symbols and references but can't resolve cross-file references because target symbols haven't been indexed yet. We add a second pass:

```
Pass 1 (existing):
  For each file:
    → Extract symbols → store in DB
    → Extract references → store with symbol_id = NULL for unresolved
    → Extract imports → store in imports table

Pass 2 (new):
  For each unresolved reference:
    → Look up import context (what did this file import?)
    → Match reference name against imported symbols in DB
    → Update reference with resolved symbol_id
```

### Schema Changes

Add `imports` table to track what each file imports:

```sql
CREATE TABLE imports (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    symbol_name TEXT NOT NULL,      -- "Index" or "*" for globs
    source_module TEXT NOT NULL,    -- "crate::db" or "MyApp.Services"
    alias TEXT,                      -- for "use foo as bar"
    PRIMARY KEY (file_id, symbol_name, source_module)
);

CREATE INDEX idx_imports_file ON imports(file_id);
CREATE INDEX idx_imports_symbol ON imports(symbol_name);
```

### Resolution Algorithm

When resolving a reference, try these strategies in order:

1. **Same-file lookup** (existing) — Is the symbol defined in this file?

2. **Explicit import lookup** — Does this file have `use crate::db::Index`? Query imports table, then look up symbol in that module.

3. **Qualified path resolution** — Is the reference `db::Index::open()`? Parse the path, resolve module to file, look up symbol.

4. **Glob import search** — Does this file have `use crate::db::*`? Search all symbols from that module.

5. **Unresolved** — Mark as LSP candidate for Phase 2.

### Handling Ambiguity

- Exactly one match → resolve
- Multiple matches → leave unresolved (LSP candidate)
- Zero matches → likely external crate, leave unresolved

### Performance Considerations

- Batch lookups by source module (one query per import, not per reference)
- Index the imports table on `(file_id, symbol_name)`
- Benchmark after implementation to validate approach

---

## Phase 2: LSP Integration

### When LSP Is Needed

After tree-sitter resolution, these cases remain unresolved:

| Case | Example | Why Tree-Sitter Can't Resolve |
|------|---------|------------------------------|
| Method on trait object | `lang_support.tree_sitter_language()` | Don't know concrete type |
| Method on inferred type | `let x = foo(); x.bar()` | Type inference needed |
| Trait method dispatch | `item.into()` | Multiple `Into` impls possible |
| Macro-generated code | `println!`, `#[derive]` | Macro expansion needed |

### LSP Client Design

Use `lsp-types` crate for all protocol types. Implement thin JSON-RPC transport (~100 lines):

```rust
use lsp_types::{
    InitializeParams, InitializeResult,
    GotoDefinitionParams, GotoDefinitionResponse, Location,
    ReferenceParams,
    request::{GotoDefinition, References, Initialize},
};

pub struct LspClient {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: i64,
}

pub trait LspProvider: Send + Sync {
    fn command(&self) -> &str;              // "rust-analyzer" or "csharp-ls"
    fn initialize_options(&self) -> Value;  // language-specific init
}

impl LspClient {
    pub fn start(provider: &dyn LspProvider, workspace: &Path) -> Result<Self>;

    /// Send any LSP request using lsp-types traits
    fn send_request<R: lsp_types::request::Request>(
        &mut self,
        params: R::Params
    ) -> Result<R::Result>;

    pub fn goto_definition(&mut self, file: &Path, line: u32, col: u32) -> Result<Option<Location>>;
    pub fn find_references(&mut self, file: &Path, line: u32, col: u32) -> Result<Vec<Location>>;
    pub fn shutdown(&mut self) -> Result<()>;
}
```

### Language Providers

```rust
pub struct RustAnalyzerProvider;
impl LspProvider for RustAnalyzerProvider {
    fn command(&self) -> &str { "rust-analyzer" }
}

pub struct CSharpLsProvider;
impl LspProvider for CSharpLsProvider {
    fn command(&self) -> &str { "csharp-ls" }
}
```

### CLI Integration

The `--lsp` flag opts into LSP refinement:

```bash
tethys index --lsp        # Tree-sitter + LSP pass for unresolved refs
tethys callers "foo" --lsp  # Stored refs + live LSP query
tethys impact --symbol "bar" --lsp
```

| Scenario | Behavior |
|----------|----------|
| `--lsp` and LSP available | Use LSP |
| `--lsp` and LSP missing | **Error and halt** |
| No flag | Tree-sitter only |

Error message when LSP missing:
```
error: rust-analyzer not found

LSP refinement was requested but the language server is not available.
Install rust-analyzer: https://rust-analyzer.github.io/manual.html#installation

To index without LSP refinement, omit the --lsp flag.
```

### LSP Lifecycle

- Spawned lazily on first LSP query
- Kept alive until `Tethys` is dropped
- Avoids startup cost if no LSP queries are made

---

## Testing Strategy

### Tree-Sitter Resolution Tests

```rust
#[test]
fn cross_file_callers_via_explicit_import() {
    // file1.rs: pub fn target() {}
    // file2.rs: use crate::file1::target; fn caller() { target(); }
    // → callers("target") should find caller in file2.rs
}

#[test]
fn cross_file_callers_via_qualified_path() {
    // file1.rs: pub fn target() {}
    // file2.rs: fn caller() { crate::file1::target(); }
    // → callers("target") should find caller in file2.rs
}

#[test]
fn cross_file_callers_via_glob_import() {
    // file1.rs: pub fn target() {}
    // file2.rs: use crate::file1::*; fn caller() { target(); }
    // → callers("target") should find caller in file2.rs
}

#[test]
fn csharp_cross_file_via_namespace() {
    // Services/UserService.cs: namespace App.Services; class UserService {}
    // Controllers/Home.cs: using App.Services; var svc = new UserService();
    // → callers("UserService") should find caller in Home.cs
}
```

### LSP Integration Tests

Gated tests requiring rust-analyzer:

```rust
#[test]
#[ignore] // Run with: cargo test --ignored
fn lsp_resolves_trait_method_call() {
    // Tests that lang_support.tree_sitter_language() resolves via LSP
}
```

### Manual Test Checklist

Before release, verify on rivets codebase:
- [ ] `tethys callers "Index::open"` finds calls in lib.rs
- [ ] `tethys impact --symbol "extract_symbols"` shows dependents
- [ ] `tethys index --lsp` completes with rust-analyzer

---

## Implementation Plan

### Phase 1: Tree-Sitter Cross-File Resolution

| Task | Description |
|------|-------------|
| 1.1 | Add `imports` table to schema, update `db.rs` |
| 1.2 | Store imports during indexing (Rust + C#) |
| 1.3 | Store unresolved references with `symbol_id = NULL` |
| 1.4 | Implement Pass 2: resolve references against symbol DB |
| 1.5 | Update `get_callers` / `get_symbol_impact` to use resolved refs |
| 1.6 | Add integration tests for cross-file resolution |
| 1.7 | Performance benchmarking on rivets codebase |

### Phase 2: LSP Integration

| Task | Description |
|------|-------------|
| 2.1 | Add `lsp-types` dependency |
| 2.2 | Implement `LspClient` with JSON-RPC transport |
| 2.3 | Implement `LspProvider` trait + `RustAnalyzerProvider` |
| 2.4 | Add `--lsp` flag to CLI commands |
| 2.5 | Integrate LSP into `index --lsp` (resolve remaining refs) |
| 2.6 | Integrate LSP into `callers --lsp` (live queries) |
| 2.7 | Add `CSharpLsProvider` (fast follow) |
| 2.8 | Add gated integration tests for LSP |

### Suggested Order

1. Tasks 1.1–1.6 (tree-sitter resolution) — delivers immediate value
2. Task 1.7 (benchmarking) — validate performance
3. Tasks 2.1–2.6 (rust-analyzer) — full LSP support
4. Task 2.7–2.8 (csharp-ls) — language parity

---

## Dependencies

```toml
# Existing
tree-sitter = "0.24"
rusqlite = { version = "0.32", features = ["bundled"] }

# New for Phase 2
lsp-types = "0.97"
```

## References

- [Tethys Design Doc](../design/tethys-code-intelligence.md) — Original architecture
- [LSP Specification](https://microsoft.github.io/language-server-protocol/)
- [lsp-types crate](https://docs.rs/lsp-types/)
- [rust-analyzer](https://rust-analyzer.github.io/)
- [csharp-ls](https://github.com/razzmatazz/csharp-language-server)
