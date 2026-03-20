# Tethys Quality Alignment Design

Bring the tethys crate in line with the discipline standards already achieved in the rivets crate, using patterns observed in the GitComet project as a reference.

## Context

An audit of `crates/tethys/src/` against the GitComet quality checklist revealed:
- 1 production `.expect()` call
- ~36 `#[allow(...)]` directives in production code (14 dead_code, 5 too_many_arguments, 4 too_many_lines, 3 similar_names, 3 unnecessary_wraps, and others)
- 1 production `let _ =` discarded result
- 13 TODO/FIXME comments
- `lib.rs` at 3,316 lines with multiple concerns
- No `rust-toolchain.toml`, `.rustfmt.toml`, or build profiles in workspace

The rivets crate already has zero unwraps, zero expects, and zero allow directives in production code. Tethys should match.

## Phases

### Phase 1: Config wins

New files:
- `rust-toolchain.toml` — pin to 1.94.0, minimal profile, rustfmt + clippy components
- `.rustfmt.toml` — edition 2024

Root `Cargo.toml` changes:
- Workspace edition `"2021"` to `"2024"`
- Add `rust-version = "1.94.0"` to `[workspace.package]`
- Add `[profile.test]` with `opt-level = 1`
- Add `[profile.release]` with `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`
- Add `[profile.release-with-debug]` inheriting release with `debug = 2`, `strip = "none"`

### Phase 2: Remove dead_code allows (~14 directives)

For each `#[allow(dead_code)]`:
- Delete the item if truly unused
- Remove the allow if it is actually used
- Make it `pub` if it is intended API

Files: lib.rs, resolver.rs, languages/common.rs, languages/mod.rs, languages/rust.rs, lsp/transport.rs, db/references.rs, db/symbols.rs, db/call_edges.rs

### Phase 3: Fix structural allows (~9 directives)

**`too_many_arguments`** (5 instances in lib.rs, parallel.rs, db/references.rs, db/symbols.rs):
- Introduce parameter structs (e.g., `InsertSymbolParams`, `InsertReferenceParams`)

**`too_many_lines`** (4 instances in lib.rs, cli/stats.rs):
- Extract helper functions
- Overlaps with Phase 6 (lib.rs split)

### Phase 4: Fix remaining allows (~13 directives)

- `unnecessary_wraps` (3) — change return type from `Result<T>` to `T`
- `similar_names` (3 in call_edges.rs) — rename variables
- `deprecated` (1 in transport.rs) — migrate to non-deprecated API
- `missing_errors_doc` (1) — add `# Errors` doc section
- `cast_precision_loss` (1) — explicit conversion or justified comment
- `needless_pass_by_value` (1) — take `&T` instead of `T`
- `unused_variables` (1) — use or remove the parameter

### Phase 5: Production expect, let _, TODOs

- `tree_sitter_utils.rs:57` — replace `.expect()` with safe fallback or Result propagation
- `lsp/transport.rs:610` — add comment explaining intentional discard of process wait result
- 13 TODO/FIXME comments — convert to tracked rivets issues, remove inline comments

### Phase 6: Split lib.rs

Extract from 3,316-line lib.rs into focused modules:
- `indexing.rs` — file scanning, symbol extraction orchestration
- `reindex.rs` — incremental update logic
- `stats.rs` — index statistics computation

lib.rs retains the `Tethys` struct definition and public API, delegating to new modules.

## Success Criteria

- `cargo clippy --workspace --all-features -- -D warnings` passes with zero `#[allow(...)]` in tethys production code
- Zero `.unwrap()` / `.expect()` in tethys production code
- Zero `let _ =` without explanatory comment
- Zero TODO/FIXME (converted to tracked issues)
- `lib.rs` under 1,500 lines
- All existing tests pass
- Workspace builds on edition 2024 with Rust 1.94.0
