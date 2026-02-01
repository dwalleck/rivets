# Module Path Computation Design

**Issue:** rivets-cz85
**Date:** 2026-02-01
**Status:** Design Complete

## Problem

The schema has a `module_path` column (indexed) but symbols are stored with empty strings. The existing `resolve_module_path` in `resolver.rs` computes **file paths** from import statements, not **module paths** from file locations.

We need the inverse: given a file path like `src/db/query.rs`, compute its Rust module path like `crate::db::query`.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Module style detection | Auto-detect per file | Handles mixed 2015/2018 styles in same workspace |
| Binary crate prefix | Use binary name | Matches rustdoc behavior (`cli::commands` not `bin::cli::commands`) |
| Computation timing | During indexing | Compute once, store in DB; avoids repeated work on queries |
| Files outside crates | Empty module_path | These aren't part of the module tree; preserves current behavior |

## Algorithm

Given a file path and crate info:

```
Input:  /workspace/crates/tethys/src/db/query.rs
Crate:  { name: "tethys", path: /workspace/crates/tethys, lib_path: src/lib.rs }
Output: crate::db::query
```

### Steps

1. **Find containing crate** - Match file path against `CrateInfo.path` prefixes
2. **Determine entry point** - Is this a lib (`src/lib.rs`) or binary (`src/main.rs`, `src/bin/X/main.rs`)?
3. **Compute relative path** - Strip crate root and entry point directory from file path
4. **Handle module styles**:
   - If file is `mod.rs` → use parent directory name
   - If file is `lib.rs` or `main.rs` → just the prefix (`crate` or binary name)
   - Otherwise → file stem is module name
5. **Build path segments** - Join with `::`, prefix with `crate` (lib) or binary name (with `-` → `_`)

### Edge Cases

| File Path | Module Path |
|-----------|-------------|
| `src/lib.rs` | `crate` |
| `src/main.rs` | `{crate_name}` |
| `src/db.rs` | `crate::db` |
| `src/db/mod.rs` | `crate::db` |
| `src/db/query.rs` | `crate::db::query` |
| `src/bin/cli/main.rs` | `cli` |
| `src/bin/cli/commands.rs` | `cli::commands` |
| `examples/demo.rs` | `` (empty) |
| `benches/perf.rs` | `` (empty) |

## Implementation

### New Code

**`cargo.rs` - add function:**

```rust
/// Compute the Rust module path for a file within a crate.
///
/// Returns `None` if the file is not within the crate's module tree
/// (e.g., examples, benches, or files outside the crate entirely).
pub fn compute_module_path(file_path: &Path, crate_info: &CrateInfo) -> Option<String>
```

**`cargo.rs` - add helper:**

```rust
/// Find which binary (if any) a file belongs to.
///
/// Returns `(binary_name, binary_entry_path)` if the file is under a binary's source tree.
fn find_binary_for_file(file_path: &Path, crate_info: &CrateInfo) -> Option<(String, PathBuf)>
```

**`lib.rs` - add method to `Tethys`:**

```rust
/// Find which crate a file belongs to and compute its module path.
///
/// Returns empty string if file is not part of any crate's module tree.
fn compute_module_path_for_file(&self, file_path: &Path) -> String
```

### Struct Changes

```rust
pub struct Tethys {
    db: TethysDb,
    workspace_root: PathBuf,
    crates: Vec<CrateInfo>,  // NEW: cached crate info
}
```

### Integration Points

1. **`Tethys::open()`** - Call `discover_crates()` to populate `self.crates`
2. **`Tethys::index()`** - Refresh `self.crates` before indexing (Cargo.toml may have changed)
3. **`index_file()` (~line 636)** - Replace `module_path: ""` with computed value

### Algorithm Implementation

```rust
pub fn compute_module_path(file_path: &Path, crate_info: &CrateInfo) -> Option<String> {
    let file_path = file_path.canonicalize().ok()?;

    // 1. Determine if lib or binary, get entry directory and prefix
    let (entry_dir, prefix) = determine_entry_point(&file_path, crate_info)?;

    // 2. Get path relative to entry directory
    let relative = file_path.strip_prefix(&entry_dir).ok()?;

    // 3. Build module segments from path components
    let mut segments = vec![prefix];

    // Add directory components (excluding the file itself)
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component {
                segments.push(name.to_str()?.to_string());
            }
        }
    }

    // 4. Handle file name based on module style
    let file_stem = file_path.file_stem()?.to_str()?;
    match file_stem {
        "mod" | "lib" | "main" => { /* Entry points - no additional segment */ }
        _ => segments.push(file_stem.to_string()),
    }

    Some(segments.join("::"))
}

fn determine_entry_point(file_path: &Path, crate_info: &CrateInfo) -> Option<(PathBuf, String)> {
    // Check if file is under library source tree
    if let Some(lib_path) = &crate_info.lib_path {
        let lib_full = crate_info.path.join(lib_path);
        let entry_dir = lib_full.parent()?;
        if file_path.starts_with(entry_dir) {
            return Some((entry_dir.to_path_buf(), "crate".to_string()));
        }
    }

    // Check if file is under any binary source tree
    for (bin_name, bin_path) in &crate_info.bin_paths {
        let bin_full = crate_info.path.join(bin_path);
        let entry_dir = bin_full.parent()?;
        if file_path.starts_with(entry_dir) {
            // Rust requires snake_case for crate names in paths
            let prefix = bin_name.replace('-', "_");
            return Some((entry_dir.to_path_buf(), prefix));
        }
    }

    None
}
```

## Performance Impact

Negligible. Module path computation is pure string manipulation on data already in memory.

| Operation | Time |
|-----------|------|
| File read + parse | ~5-50ms |
| Crate lookup (linear) | ~100ns |
| Path computation | ~500ns |

Memory: `Vec<CrateInfo>` adds a few KB even for large workspaces.

## Testing Strategy

1. **Unit tests for `compute_module_path`:**
   - Library files (lib.rs, nested modules, mod.rs style)
   - Binary files (main.rs, src/bin/ structure)
   - Edge cases (hyphens in names, deeply nested)

2. **Integration test:**
   - Index the tethys crate itself
   - Verify symbols have correct module_path values
   - Query by module_path to confirm indexing works

3. **Roundtrip invariant:**
   - For any indexed symbol, `module_path + qualified_name` should produce a valid Rust path

## Files to Modify

1. `crates/tethys/src/cargo.rs` - Add `compute_module_path`, `find_binary_for_file`
2. `crates/tethys/src/lib.rs` - Add `crates` field, `compute_module_path_for_file` method, integrate into indexing
3. `crates/tethys/tests/module_path.rs` - New test file for module path computation
