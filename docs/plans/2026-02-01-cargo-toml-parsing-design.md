# Design: Cargo.toml Parsing for Crate Root Detection

**Issue**: `rivets-m4wt`
**Date**: 2026-02-01
**Status**: Approved

## Problem

Tethys hardcodes `workspace_root/src/` as the crate root in two places (`lib.rs:1025` and `lib.rs:1452`). This breaks for:

- Workspaces with custom `[lib].path` in Cargo.toml
- Binary crates vs library crates
- Multi-crate workspaces (like rivets itself)

## Solution

Parse Cargo.toml files to discover crate structure and dynamically determine crate roots.

## Data Model

```rust
/// Information about a Rust crate discovered from Cargo.toml
#[derive(Debug, Clone)]
pub struct CrateInfo {
    /// Crate name from [package].name
    pub name: String,
    /// Path to the crate directory (contains Cargo.toml)
    pub path: PathBuf,
    /// Library entry point, if this crate has a lib target
    /// Defaults to src/lib.rs if [lib] exists without explicit path
    pub lib_path: Option<PathBuf>,
    /// Binary entry points (name -> path)
    /// Defaults to src/main.rs for single binary
    pub bin_paths: Vec<(String, PathBuf)>,
}
```

Add to `Tethys` struct:

```rust
pub struct Tethys {
    workspace_root: PathBuf,
    db_path: PathBuf,
    db: Index,
    parser: tree_sitter::Parser,
    crates: Vec<CrateInfo>,  // NEW
}
```

## Cargo.toml Discovery

Handle three workspace types:

1. **Virtual workspace** - `[workspace]` exists, no `[package]` at root (like rivets)
2. **Workspace with root crate** - Both `[workspace]` and `[package]` at root
3. **Single crate** - Just `[package]`, no workspace

### Discovery Algorithm

```
discover_crates(workspace_root) -> Vec<CrateInfo>:
    manifest = parse(workspace_root/Cargo.toml)

    if manifest.workspace exists:
        for member_path in expand_globs(manifest.workspace.members):
            crate_info = parse_crate(workspace_root/member_path)
            if crate_info: add to results

    if manifest.package exists:
        crate_info = parse_crate_from_manifest(workspace_root, manifest)
        if crate_info: add to results

    return results

parse_crate(crate_path) -> Option<CrateInfo>:
    manifest = parse(crate_path/Cargo.toml)
    return parse_crate_from_manifest(crate_path, manifest)

parse_crate_from_manifest(crate_path, manifest) -> Option<CrateInfo>:
    if not manifest.package: return None

    lib_path = manifest.lib.path or default_lib_path(crate_path)
    bin_paths = extract_bin_paths(manifest, crate_path)

    return CrateInfo {
        name: manifest.package.name,
        path: crate_path,
        lib_path,
        bin_paths,
    }
```

### Default Paths (Cargo conventions)

- Library: `src/lib.rs` if exists
- Single binary: `src/main.rs` if exists
- Named binaries: `src/bin/<name>.rs` or `src/bin/<name>/main.rs`

## File-to-Crate Resolution

```rust
impl Tethys {
    /// Find which crate a file belongs to (longest path match)
    fn get_crate_for_file(&self, file_path: &Path) -> Option<&CrateInfo> {
        self.crates
            .iter()
            .filter(|c| file_path.starts_with(&c.path))
            .max_by_key(|c| c.path.components().count())
    }

    /// Get the crate root directory (containing lib.rs/main.rs) for a file
    fn get_crate_root_for_file(&self, file_path: &Path) -> Option<PathBuf> {
        let crate_info = self.get_crate_for_file(file_path)?;

        // Use lib path if available, otherwise first bin path
        let entry_point = crate_info.lib_path.as_ref()
            .or_else(|| crate_info.bin_paths.first().map(|(_, p)| p))?;

        // Crate root is the parent of the entry point (e.g., src/)
        entry_point.parent().map(|p| crate_info.path.join(p))
    }
}
```

## FIXME Site Changes

### Site 1: `lib.rs:1025` (derive_file_deps_from_call_edges)

```rust
// Before:
let crate_root = self.workspace_root.join("src");

// After:
let crate_root = self.get_crate_root_for_file(current_file)
    .unwrap_or_else(|| self.workspace_root.join("src"));
```

### Site 2: `lib.rs:1452` (resolve_refs_via_imports)

```rust
// Before:
let crate_root = self.workspace_root.join("src");

// After:
// Need to get the file path first, then determine crate root per-file in the loop
```

## Initialization

Crate discovery happens eagerly in `Tethys::new()`:

```rust
impl Tethys {
    pub fn new(workspace_root: &Path) -> Result<Self> {
        // ... existing setup ...

        let crates = discover_crates(workspace_root);

        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
            db_path,
            db,
            parser,
            crates,
        })
    }
}
```

## Error Handling

- **Missing Cargo.toml**: Not a Rust project, `crates` stays empty, fallback to `src/`
- **Malformed Cargo.toml**: Log warning, skip that crate, continue with others
- **Glob expansion fails**: Log warning, skip that pattern
- **Missing entry points**: Skip that target, crate may still have other targets

## Dependencies

Add to `crates/tethys/Cargo.toml`:

```toml
cargo_toml = "0.20"
```

## Implementation Tasks

1. Add `cargo_toml` dependency
2. Add `CrateInfo` to `types.rs`
3. Add `cargo.rs` module with `discover_crates()` function
4. Add `crates` field and resolution methods to `Tethys`
5. Update FIXME site 1 (`derive_file_deps_from_call_edges`)
6. Update FIXME site 2 (`resolve_refs_via_imports`)
7. Add tests for workspace discovery
8. Add tests for file-to-crate resolution

## Test Cases

1. Single crate with default `src/lib.rs`
2. Single crate with custom `[lib].path`
3. Workspace with multiple members
4. Virtual workspace (no root package)
5. Workspace with glob patterns (`crates/*`)
6. Binary-only crate
7. Mixed lib + bin crate
8. Non-Rust project (graceful fallback)
