# Tethys Quality Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Bring the tethys crate to the same zero-tolerance discipline as the rivets crate: zero `#[allow(...)]` directives, zero production `.expect()`/`.unwrap()`, edition 2024, and properly sized modules.

**Architecture:** Incremental cleanup in 6 phases — config, dead code, structural refactors, minor fixes, TODOs, file splitting. Each phase is independently committable. No behavioral changes to any public API.

**Tech Stack:** Rust 1.94.0, edition 2024, clippy pedantic, thiserror

---

### Task 1: Create rust-toolchain.toml

**Files:**
- Create: `rust-toolchain.toml`

**Step 1: Create the file**

```toml
[toolchain]
channel = "1.94.0"
profile = "minimal"
components = ["rustfmt", "clippy"]
```

**Step 2: Verify toolchain installs**

Run: `rustup show`
Expected: Shows 1.94.0 as the active toolchain for this directory.

**Step 3: Commit**

```bash
git add rust-toolchain.toml
git commit -m "build: pin Rust toolchain to 1.94.0"
```

---

### Task 2: Create .rustfmt.toml

**Files:**
- Create: `.rustfmt.toml`

**Step 1: Create the file**

```toml
edition = "2024"
```

**Step 2: Run formatter to check for drift**

Run: `cargo fmt --all -- --check`
Expected: May show formatting changes. If so, run `cargo fmt --all` and include in commit.

**Step 3: Commit**

```bash
git add .rustfmt.toml
git commit -m "build: add .rustfmt.toml with edition 2024"
```

---

### Task 3: Bump workspace edition and add build profiles

**Files:**
- Modify: `Cargo.toml` (root)

**Step 1: Update workspace edition and add rust-version**

In `[workspace.package]`:
- Change `edition = "2021"` to `edition = "2024"`
- Add `rust-version = "1.94.0"` after the edition line

**Step 2: Add build profiles at end of file**

```toml
[profile.test]
opt-level = 1

[profile.release]
lto = "fat"
codegen-units = 1
strip = "symbols"

[profile.release-with-debug]
inherits = "release"
debug = 2
strip = "none"
```

**Step 3: Build to check for edition 2024 errors**

Run: `cargo build --workspace 2>&1 | head -50`
Expected: May surface new warnings/errors from edition 2024 changes (lifetime capture rules, `gen` keyword reservation). Fix any that appear.

**Step 4: Run full test suite**

Run: `cargo nextest run --all-features --workspace`
Expected: All tests pass.

**Step 5: Run clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: Passes (existing allows still in place).

**Step 6: Commit**

```bash
git add Cargo.toml
git commit -m "build: bump workspace to edition 2024, add build profiles, set MSRV 1.94.0"
```

---

### Task 4: Remove dead_code allows — resolver.rs

**Files:**
- Modify: `crates/tethys/src/resolver.rs:15,22`

The `SymbolOriginMap` struct and impl are annotated `#[allow(dead_code)] // Phase 3+: advanced symbol resolution`. These are preparatory code for future features. Since they're unused, **delete them entirely**. They can be recreated from git history when needed (YAGNI).

**Step 1: Remove the `SymbolOriginMap` struct and its impl block**

Delete from line 10 (the doc comment above `#[allow(dead_code)]`) through the end of the impl block. Keep the module's use statements that are still needed.

**Step 2: Run clippy to verify**

Run: `cargo clippy -p tethys --all-features -- -D warnings`
Expected: No new errors related to resolver.rs.

**Step 3: Run tests**

Run: `cargo nextest run -p tethys`

**Step 4: Commit**

```bash
git commit -am "refactor(tethys): remove unused SymbolOriginMap (YAGNI)"
```

---

### Task 5: Remove dead_code allows — languages/common.rs

**Files:**
- Modify: `crates/tethys/src/languages/common.rs:17,76`

**Line 17:** `ExtractedSymbol.signature_details` field has `#[allow(dead_code)] // Populated for future use by callers`. It's populated by the Rust extractor but never read outside tests. **Remove the allow and add an underscore prefix `_signature_details`** OR remove the field entirely if no tests use it. Check tests first.

**Line 76:** `ImportContext` struct has `#[allow(dead_code)] // Fields accessed by LanguageSupport implementations`. Check if `ImportContext` is actually used by any `LanguageSupport` impl. If not, delete it.

**Step 1: Search for usages**

Run: `grep -r "signature_details" crates/tethys/src/ --include="*.rs"` to see if the field is read anywhere in production code.
Run: `grep -r "ImportContext" crates/tethys/src/ --include="*.rs"` to check usage.

**Step 2: Based on findings, either delete unused code or remove the allow**

**Step 3: Run clippy + tests**

Run: `cargo clippy -p tethys --all-features -- -D warnings && cargo nextest run -p tethys`

**Step 4: Commit**

```bash
git commit -am "refactor(tethys): remove dead_code allows in languages/common.rs"
```

---

### Task 6: Remove dead_code allows — languages/mod.rs and languages/rust.rs

**Files:**
- Modify: `crates/tethys/src/languages/mod.rs:43`
- Modify: `crates/tethys/src/languages/rust.rs:141`

**mod.rs:43:** `LanguageSupport` trait has `#[allow(dead_code)] // Some trait methods not yet used`. Trait methods that are part of the public interface shouldn't be marked dead_code — they're API surface. **Remove the allow.** If clippy complains about specific unused methods, consider if they belong in the trait yet.

**rust.rs:141:** `UseStatement` struct member or method with `#[allow(dead_code)]`. The `to_import_statement()` method (line 142). Check if this is called. If not, remove it.

**Step 1: Remove allows, check compilation**

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): remove dead_code allows in language support"
```

---

### Task 7: Remove dead_code allows — db/call_edges.rs, db/references.rs, db/symbols.rs

**Files:**
- Modify: `crates/tethys/src/db/call_edges.rs:18,40,65,90,141`
- Modify: `crates/tethys/src/db/references.rs:53,116`
- Modify: `crates/tethys/src/db/symbols.rs:13,163`

All these are `#[allow(dead_code)] // Public API, not yet used internally`. These are `pub` methods on `Index` — they're public API surface. Clippy's dead_code lint shouldn't fire on `pub` items in a library crate. **Remove all the `#[allow(dead_code)]` annotations.** If clippy complains, the crate may need `lib` target type confirmed in Cargo.toml.

**Step 1: Remove all `#[allow(dead_code)]` lines from these three files**

**Step 2: Run clippy**

Run: `cargo clippy -p tethys --all-features -- -D warnings`

If clippy still warns about dead code on pub items, it means tethys is being compiled as a binary only. Check that `crates/tethys/Cargo.toml` has a `[lib]` section or that `src/lib.rs` exists.

**Step 3: Run tests**

Run: `cargo nextest run -p tethys`

**Step 4: Commit**

```bash
git commit -am "refactor(tethys): remove dead_code allows from public db API"
```

---

### Task 8: Remove dead_code allow — lib.rs:604

**Files:**
- Modify: `crates/tethys/src/lib.rs:604`

The `index_file` method has `#[allow(dead_code)]` with comment: "preserved for reference but is no longer used." **Delete the entire method.** Git history preserves it.

**Step 1: Delete the `index_file` method** (from the `#[allow(dead_code)]` line through the closing brace of the method)

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): remove unused index_file method (preserved in git history)"
```

---

### Task 9: Remove dead_code allow — lsp/transport.rs:663

**Files:**
- Modify: `crates/tethys/src/lsp/transport.rs:663`

This is inside `#[cfg(test)]` — it's a test helper. `#[allow(dead_code)]` on test helpers is acceptable if the helper is used by some but not all test functions. **Verify:** if the helper `mock_error_response` is used by any test. If yes, remove the allow (it won't warn in test cfg). If no, delete the helper.

**Step 1: Check usage**

Run: `grep -n "mock_error_response" crates/tethys/src/lsp/transport.rs`

**Step 2: Remove allow or delete helper based on findings**

**Step 3: Run tests**

**Step 4: Commit**

```bash
git commit -am "refactor(tethys): clean up dead_code in lsp test helpers"
```

---

### Task 10: Fix too_many_arguments — db/symbols.rs insert_symbol

**Files:**
- Modify: `crates/tethys/src/db/symbols.rs:14-53`
- Modify: all callers of `insert_symbol`

**Step 1: Create `InsertSymbolParams` struct**

Add above the `impl Index` block:

```rust
/// Parameters for inserting a symbol into the index.
pub struct InsertSymbolParams<'a> {
    pub file_id: FileId,
    pub name: &'a str,
    pub module_path: &'a str,
    pub qualified_name: &'a str,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub span: Option<Span>,
    pub signature: Option<&'a str>,
    pub visibility: Visibility,
    pub parent_symbol_id: Option<SymbolId>,
    pub is_test: bool,
}
```

**Step 2: Update `insert_symbol` signature**

```rust
pub fn insert_symbol(&self, params: &InsertSymbolParams<'_>) -> Result<SymbolId> {
```

Remove both `#[allow(dead_code)]` and `#[allow(clippy::too_many_arguments)]`.

**Step 3: Update all callers** — search for `insert_symbol(` across the codebase and update to pass the struct.

**Step 4: Run clippy + tests**

**Step 5: Commit**

```bash
git commit -am "refactor(tethys): introduce InsertSymbolParams to replace too_many_arguments"
```

---

### Task 11: Fix too_many_arguments — db/references.rs insert_reference

**Files:**
- Modify: `crates/tethys/src/db/references.rs:20-47`
- Modify: all callers of `insert_reference`

Same pattern as Task 10. Create `InsertReferenceParams` struct, update signature, update callers.

**Step 1: Create struct, update method, update callers**

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): introduce InsertReferenceParams to replace too_many_arguments"
```

---

### Task 12: Fix too_many_arguments — parallel.rs ParsedFileData::new and OwnedSymbolData::new

**Files:**
- Modify: `crates/tethys/src/parallel.rs:76-100`
- Modify: `crates/tethys/src/parallel.rs:139-149`

`ParsedFileData::new` takes 7 params and `OwnedSymbolData::new` takes 11 params.

For `ParsedFileData` — this is a simple data carrier. Consider removing the `new()` constructor entirely and using struct literal initialization at call sites (all fields are `pub`). Remove the `#[allow(clippy::too_many_arguments)]`.

For `OwnedSymbolData::new` — same approach. If all fields are pub, callers can construct directly. Remove the constructor and the allow.

**Step 1: Check if fields are pub, remove constructors if so, update callers to use struct literals**

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): remove too_many_arguments constructors, use struct literals"
```

---

### Task 13: Fix too_many_arguments — lib.rs try_resolve_reference

**Files:**
- Modify: `crates/tethys/src/lib.rs:1609-1619`

`try_resolve_reference` takes 8 params. Introduce a `ResolveContext` struct:

```rust
struct ResolveContext<'a> {
    explicit_imports: &'a HashMap<&'a str, (&'a str, &'a str)>,
    glob_imports: &'a [&'a str],
    current_file_path: Option<&'a Path>,
    crate_root: &'a Path,
    file_id: FileId,
}
```

Keep `ref_` and `ref_name` as direct params (they're the "what"), context is the "where".

**Step 1: Create struct, update method, update callers**

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): introduce ResolveContext for try_resolve_reference"
```

---

### Task 14: Fix too_many_lines — cli/stats.rs run()

**Files:**
- Modify: `crates/tethys/src/cli/stats.rs:9`

The `run()` function is long because it formats many output sections. Extract each section into a helper function (e.g., `print_file_stats()`, `print_symbol_stats()`, `print_reference_stats()`).

**Step 1: Identify logical sections in run(), extract into helpers**

**Step 2: Remove `#[allow(clippy::too_many_lines)]`**

**Step 3: Run clippy + tests**

**Step 4: Commit**

```bash
git commit -am "refactor(tethys): split stats run() into section helpers"
```

---

### Task 15: Fix too_many_lines — lib.rs methods (3 instances)

**Files:**
- Modify: `crates/tethys/src/lib.rs:224,1837,2518`

These are `index_with_options`, `resolve_via_lsp`, and `get_callers_with_lsp`. These will be addressed more naturally in Task 22 (lib.rs splitting), so **defer these to Phase 6**. For now, leave the allows in place — they'll be resolved when the methods move to dedicated modules where they can be further decomposed.

Note: Mark this task as skipped/deferred in the commit for Task 22.

---

### Task 16: Fix unnecessary_wraps — languages/mod.rs and languages/rust.rs

**Files:**
- Modify: `crates/tethys/src/languages/mod.rs:31`
- Modify: `crates/tethys/src/languages/rust.rs:503,567`

**mod.rs:31:** `get_language_support` returns `Option` but always returns `Some`. The comment says "intentional for future language stubs." Since `Language` is an enum with only Rust and CSharp, and both are implemented, this always returns `Some`. Change return type to `&'static dyn LanguageSupport` and update callers to remove `.unwrap()` / `?` chains on the result.

**rust.rs:503,567:** `parse_use_wildcard` and `parse_scoped_use_list` always return `Some(...)`. The comment says "may need Option later." Change return type to the inner type and update callers.

**Step 1: Change return types, remove allows, update callers**

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): remove unnecessary_wraps, return concrete types"
```

---

### Task 17: Fix similar_names — db/call_edges.rs

**Files:**
- Modify: `crates/tethys/src/db/call_edges.rs:19,41,66`

The `caller_id`/`callee_id` parameter pairs trigger `clippy::similar_names`. These names are actually the correct domain terms. Options:

1. Rename to `source_id`/`target_id` (more distinct but less domain-accurate)
2. Rename to `caller`/`callee` (shorter, still clear from context)

**Recommended:** Option 2 — rename to `caller`/`callee` since the method names already provide context.

**Step 1: Rename params, remove allows**

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): rename call_edge params for clippy similar_names"
```

---

### Task 18: Fix remaining allows — deprecated, cast_precision_loss, needless_pass_by_value, unused_variables, missing_errors_doc, cast_possible_wrap

**Files:**
- Modify: `crates/tethys/src/lsp/transport.rs:108` — `#[allow(deprecated)]`
- Modify: `crates/tethys/src/cli/stats.rs:140` — `#[allow(clippy::cast_precision_loss)]`
- Modify: `crates/tethys/src/batch_writer.rs:158` — `#[allow(clippy::needless_pass_by_value)]`
- Modify: `crates/tethys/src/lib.rs:141` — `#[allow(unused_variables)]`
- Modify: `crates/tethys/src/lib.rs:101` — `#[allow(clippy::missing_errors_doc)]`
- Modify: `crates/tethys/src/db/symbols.rs:78,129,172` — `#[allow(clippy::cast_possible_wrap)]` and `#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]`

**transport.rs:108:** `root_uri` is deprecated in LSP. Use `workspace_folders` instead, or if that's too invasive, convert the allow to a line comment explaining the deprecation status and keep it (some LSP servers still require `root_uri`). If keeping, change to a targeted `#[expect(deprecated)]` with a reason.

**stats.rs:140:** `cast_precision_loss` on `bytes as f64`. This is intentional — file sizes don't need sub-byte precision. Use `#[expect(clippy::cast_precision_loss, reason = "file size display doesn't need sub-byte precision")]` if the project uses Rust 1.81+, or add a safety comment and keep the allow.

**batch_writer.rs:158:** `needless_pass_by_value` on `Receiver<ParsedFileData>`. The comment says "Receiver is consumed by loop, PathBuf owned by thread." This is correct — the thread owns the receiver. Remove the allow and change `PathBuf` to `&Path` if possible, or use `#[expect(...)]` with reason.

**lib.rs:141:** `unused_variables` on `with_lsp`'s `lsp_command` param. The method ignores the param. Either prefix with underscore `_lsp_command` or remove the allow and use `_`.

**lib.rs:101:** `missing_errors_doc` on the entire `impl Tethys` block. This is a blanket suppression. **Remove it** and add `# Errors` doc sections to the public methods that need them, or add them incrementally.

**db/symbols.rs cast allows:** These are `usize as i64` and `i64 as usize` casts for SQLite interop. These are legitimate — SQLite uses i64, Rust APIs use usize. Use `#[expect(...)]` with safety comments, or use `TryFrom`/`TryInto` for correctness.

**Step 1: Fix each file per the guidance above**

**Step 2: Run clippy + tests**

**Step 3: Commit**

```bash
git commit -am "refactor(tethys): resolve remaining clippy allow directives"
```

---

### Task 19: Fix production expect — tree_sitter_utils.rs:57

**Files:**
- Modify: `crates/tethys/src/languages/tree_sitter_utils.rs:57`

The `.expect("fallback span is always valid")` is on a `Span::new()` call that constructs from tree-sitter positions. The comment argues these are always valid. If `Span::new` returns `Option` or `Result`, use `.unwrap_or_else(|| /* default span */)` or propagate the error.

**Step 1: Check Span::new return type**

**Step 2: Replace with safe fallback or propagate error**

**Step 3: Run tests**

**Step 4: Commit**

```bash
git commit -am "fix(tethys): replace production expect in tree_sitter_utils with safe fallback"
```

---

### Task 20: Fix production let _ = — lsp/transport.rs:610

**Files:**
- Modify: `crates/tethys/src/lsp/transport.rs:610`

`let _ = self.process.wait();` in the `Drop` impl. This is intentional — we're reaping the process to prevent zombies, and there's nothing useful to do with the result in a destructor. **Add a comment:**

```rust
// Reap the child process to prevent zombies. The exit status is
// irrelevant during cleanup — we already attempted kill() above.
let _ = self.process.wait();
```

**Step 1: Add the comment**

**Step 2: Commit**

```bash
git commit -am "docs(tethys): document intentional process.wait() discard in Drop"
```

---

### Task 21: Convert TODO/FIXME to tracked issues

**Files:**
- Modify: `crates/tethys/src/lib.rs:688,699,813,880,1091,1518,2291,2303`
- Modify: `crates/tethys/src/cli/impact.rs:20`
- Modify: `crates/tethys/src/batch_writer.rs:240`
- Modify: `crates/tethys/src/languages/csharp.rs:110,1036,1038`

For each TODO/FIXME:
1. Create a rivets issue tracking the work
2. Replace the inline comment with a reference to the issue ID
3. Or remove the comment entirely if the issue tracker is sufficient

Group related TODOs into single issues:
- `parent_symbol_id` (lib.rs:688, 813) → one issue
- `content hash` (lib.rs:699, 880, batch_writer.rs:240) → one issue
- `crate root detection FIXME` (lib.rs:1091, 1518) → one issue
- `incremental update + staleness` (lib.rs:2291, 2303) → one issue
- `C# enhancements` (csharp.rs:110, 1036, 1038) → one issue
- `depth-limited transitive analysis` (cli/impact.rs:20) → one issue

**Step 1: Create 6 rivets issues**

**Step 2: Replace TODO/FIXME comments with issue references or remove them**

**Step 3: Run tests**

**Step 4: Commit**

```bash
git commit -am "chore(tethys): convert TODO/FIXME comments to tracked issues"
```

---

### Task 22: Split lib.rs into focused modules

**Files:**
- Modify: `crates/tethys/src/lib.rs`
- Create: `crates/tethys/src/indexing.rs`
- Create: `crates/tethys/src/reindex.rs`
- Create: `crates/tethys/src/resolve.rs`

This is the largest task. The goal is to get `lib.rs` under ~1,500 lines.

**Step 1: Identify method groups to extract**

- `indexing.rs`: `index()`, `index_with_options()`, `scan_workspace()`, `parse_file_static()`, `write_parsed_file()`, related helpers
- `reindex.rs`: `update()`, `needs_update()`, `rebuild()`, `rebuild_with_options()`
- `resolve.rs`: `resolve_dependencies()`, `try_resolve_reference()`, `resolve_via_explicit_import()`, `resolve_via_glob_import()`, `resolve_via_lsp()`, `get_callers_with_lsp()`

**Step 2: Move methods into new files as `impl Tethys` blocks**

Each new file:
```rust
use super::*;  // or explicit imports

impl Tethys {
    // moved methods
}
```

**Step 3: Add `mod indexing; mod reindex; mod resolve;` to lib.rs**

**Step 4: Remove the 3 remaining `#[allow(clippy::too_many_lines)]` from the moved methods** (Task 15 deferred items). Decompose the methods further now that they're in dedicated modules.

**Step 5: Verify lib.rs is under 1,500 lines**

**Step 6: Run full test suite**

Run: `cargo nextest run -p tethys`

**Step 7: Run clippy**

Run: `cargo clippy -p tethys --all-features -- -D warnings`

**Step 8: Commit**

```bash
git commit -am "refactor(tethys): split lib.rs into indexing, reindex, and resolve modules"
```

---

### Task 23: Final verification

**Step 1: Run full workspace build**

Run: `cargo build --workspace`

**Step 2: Run full workspace tests**

Run: `cargo nextest run --all-features --workspace`

**Step 3: Run full clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`

**Step 4: Run fmt check**

Run: `cargo fmt --all -- --check`

**Step 5: Verify zero allows remaining in tethys production code**

Run: `grep -rn '#\[allow(' crates/tethys/src/ --include='*.rs' | grep -v '#\[cfg(test)\]' | grep -v '// test'`

Expected: Zero results (or only results inside `#[cfg(test)]` blocks).

**Step 6: Commit any final fixes**
