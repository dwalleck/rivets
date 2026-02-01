# Module Path Computation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Compute Rust module paths (e.g., `crate::db::query`) from file paths during indexing.

**Architecture:** Add `compute_module_path()` to `cargo.rs`, cache `Vec<CrateInfo>` in `Tethys` struct, integrate into indexing loop to populate the `module_path` field for all symbols.

**Tech Stack:** Rust, existing `cargo_toml` crate, tree-sitter for parsing

**Worktree:** `/home/dwalleck/repos/rivets/.worktrees/module-path`

**Design Doc:** `docs/plans/2026-02-01-module-path-computation.md`

---

## Task 1: Add `compute_module_path` function to cargo.rs

**Files:**
- Modify: `crates/tethys/src/cargo.rs`
- Test: `crates/tethys/src/cargo.rs` (inline tests)

**Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `cargo.rs`:

```rust
#[test]
fn compute_module_path_lib_root() {
    let crate_info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    };
    let result = compute_module_path(
        Path::new("/workspace/my_crate/src/lib.rs"),
        &crate_info,
    );
    assert_eq!(result, Some("crate".to_string()));
}

#[test]
fn compute_module_path_nested_module() {
    let crate_info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    };
    let result = compute_module_path(
        Path::new("/workspace/my_crate/src/db/query.rs"),
        &crate_info,
    );
    assert_eq!(result, Some("crate::db::query".to_string()));
}

#[test]
fn compute_module_path_mod_rs_style() {
    let crate_info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    };
    let result = compute_module_path(
        Path::new("/workspace/my_crate/src/db/mod.rs"),
        &crate_info,
    );
    assert_eq!(result, Some("crate::db".to_string()));
}

#[test]
fn compute_module_path_binary_main() {
    let crate_info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: None,
        bin_paths: vec![("cli".to_string(), PathBuf::from("src/main.rs"))],
    };
    let result = compute_module_path(
        Path::new("/workspace/my_crate/src/main.rs"),
        &crate_info,
    );
    assert_eq!(result, Some("cli".to_string()));
}

#[test]
fn compute_module_path_binary_nested() {
    let crate_info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: None,
        bin_paths: vec![("my-cli".to_string(), PathBuf::from("src/bin/my-cli/main.rs"))],
    };
    let result = compute_module_path(
        Path::new("/workspace/my_crate/src/bin/my-cli/commands.rs"),
        &crate_info,
    );
    // Hyphens become underscores in Rust crate names
    assert_eq!(result, Some("my_cli::commands".to_string()));
}

#[test]
fn compute_module_path_outside_crate() {
    let crate_info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    };
    let result = compute_module_path(
        Path::new("/workspace/other_crate/src/lib.rs"),
        &crate_info,
    );
    assert_eq!(result, None);
}

#[test]
fn compute_module_path_examples_returns_none() {
    let crate_info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    };
    let result = compute_module_path(
        Path::new("/workspace/my_crate/examples/demo.rs"),
        &crate_info,
    );
    assert_eq!(result, None);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p tethys compute_module_path --lib`
Expected: FAIL with "cannot find function `compute_module_path`"

**Step 3: Write the implementation**

Add before the `#[cfg(test)]` block in `cargo.rs`:

```rust
/// Compute the Rust module path for a file within a crate.
///
/// Given a file path and crate info, returns the module path as it would appear
/// in Rust code (e.g., `crate::db::query` for `src/db/query.rs`).
///
/// # Returns
///
/// - `Some(path)` for files in the crate's module tree
/// - `None` for files outside the crate (examples, benches, tests, other crates)
///
/// # Examples
///
/// ```ignore
/// // src/lib.rs -> "crate"
/// // src/db.rs -> "crate::db"
/// // src/db/query.rs -> "crate::db::query"
/// // src/db/mod.rs -> "crate::db"
/// // src/bin/cli/main.rs -> "cli"
/// // src/bin/cli/commands.rs -> "cli::commands"
/// ```
#[must_use]
pub fn compute_module_path(file_path: &Path, crate_info: &CrateInfo) -> Option<String> {
    // Determine entry point (lib or binary) and get prefix
    let (entry_dir, prefix) = determine_entry_point(file_path, crate_info)?;

    // Get path relative to entry directory
    let relative = file_path.strip_prefix(&entry_dir).ok()?;

    // Build module segments
    let mut segments = vec![prefix];

    // Add directory components (parent directories become module path segments)
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component {
                segments.push(name.to_str()?.to_string());
            }
        }
    }

    // Handle file name based on module style
    let file_stem = file_path.file_stem()?.to_str()?;
    match file_stem {
        // Entry points don't add a segment
        "mod" | "lib" | "main" => {}
        // Regular files add their stem as a module segment
        _ => segments.push(file_stem.to_string()),
    }

    Some(segments.join("::"))
}

/// Determine which entry point (lib or binary) a file belongs to.
///
/// Returns `(entry_directory, prefix)` where:
/// - `entry_directory` is the directory containing the entry point (e.g., `src/`)
/// - `prefix` is the module path prefix (`"crate"` for lib, binary name for bins)
fn determine_entry_point(file_path: &Path, crate_info: &CrateInfo) -> Option<(PathBuf, String)> {
    // Check if file is under library source tree
    if let Some(lib_path) = &crate_info.lib_path {
        let lib_full = crate_info.path.join(lib_path);
        if let Some(entry_dir) = lib_full.parent() {
            if file_path.starts_with(entry_dir) {
                return Some((entry_dir.to_path_buf(), "crate".to_string()));
            }
        }
    }

    // Check if file is under any binary source tree
    for (bin_name, bin_path) in &crate_info.bin_paths {
        let bin_full = crate_info.path.join(bin_path);
        if let Some(entry_dir) = bin_full.parent() {
            if file_path.starts_with(entry_dir) {
                // Rust requires snake_case for crate names in paths
                let prefix = bin_name.replace('-', "_");
                return Some((entry_dir.to_path_buf(), prefix));
            }
        }
    }

    None
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p tethys compute_module_path --lib`
Expected: PASS (7 tests)

**Step 5: Commit**

```bash
git add crates/tethys/src/cargo.rs
git commit -m "feat(tethys): add compute_module_path function

Computes Rust module paths from file paths for symbols.
Handles lib.rs, mod.rs, binary crates, and nested modules."
```

---

## Task 2: Add `get_crate_for_file` helper function

**Files:**
- Modify: `crates/tethys/src/cargo.rs`
- Test: `crates/tethys/src/cargo.rs` (inline tests)

**Step 1: Write the failing test**

Add to tests in `cargo.rs`:

```rust
#[test]
fn get_crate_for_file_finds_matching_crate() {
    let crates = vec![
        CrateInfo {
            name: "crate_a".to_string(),
            path: PathBuf::from("/workspace/crates/a"),
            lib_path: Some(PathBuf::from("src/lib.rs")),
            bin_paths: vec![],
        },
        CrateInfo {
            name: "crate_b".to_string(),
            path: PathBuf::from("/workspace/crates/b"),
            lib_path: Some(PathBuf::from("src/lib.rs")),
            bin_paths: vec![],
        },
    ];

    let result = get_crate_for_file(Path::new("/workspace/crates/b/src/lib.rs"), &crates);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "crate_b");
}

#[test]
fn get_crate_for_file_returns_none_for_no_match() {
    let crates = vec![CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/my_crate"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    }];

    let result = get_crate_for_file(Path::new("/other/path/file.rs"), &crates);
    assert!(result.is_none());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p tethys get_crate_for_file --lib`
Expected: FAIL with "cannot find function `get_crate_for_file`"

**Step 3: Write the implementation**

Add after `compute_module_path` in `cargo.rs`:

```rust
/// Find which crate a file belongs to.
///
/// Returns a reference to the `CrateInfo` whose path is a prefix of the file path.
/// Returns `None` if the file is not within any known crate.
#[must_use]
pub fn get_crate_for_file<'a>(file_path: &Path, crates: &'a [CrateInfo]) -> Option<&'a CrateInfo> {
    crates.iter().find(|c| file_path.starts_with(&c.path))
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p tethys get_crate_for_file --lib`
Expected: PASS (2 tests)

**Step 5: Commit**

```bash
git add crates/tethys/src/cargo.rs
git commit -m "feat(tethys): add get_crate_for_file helper

Finds which crate a file belongs to by path prefix matching."
```

---

## Task 3: Add `crates` field to Tethys struct

**Files:**
- Modify: `crates/tethys/src/lib.rs`

**Step 1: Locate the Tethys struct definition**

Find the struct around line 100-150 in `lib.rs`. It should look like:

```rust
pub struct Tethys {
    db: TethysDb,
    workspace_root: PathBuf,
}
```

**Step 2: Add the crates field**

Modify to:

```rust
pub struct Tethys {
    db: TethysDb,
    workspace_root: PathBuf,
    /// Cached crate info for module path computation.
    crates: Vec<CrateInfo>,
}
```

**Step 3: Update Tethys::open() to populate crates**

Find `Tethys::open()` (around line 200-250). Add the `discover_crates` call:

```rust
pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self> {
    let workspace_root = workspace_root.as_ref().to_path_buf();
    let db_path = workspace_root.join(DEFAULT_DB_PATH);
    let db = TethysDb::open(&db_path)?;
    let crates = crate::cargo::discover_crates(&workspace_root);

    debug!(
        workspace = %workspace_root.display(),
        crate_count = crates.len(),
        "Opened Tethys with discovered crates"
    );

    Ok(Self {
        db,
        workspace_root,
        crates,
    })
}
```

**Step 4: Update any other constructors**

Search for other places that construct `Tethys` (e.g., `create()`, test helpers). Each needs the `crates` field. Example pattern:

```rust
Self {
    db,
    workspace_root,
    crates: crate::cargo::discover_crates(&workspace_root),
}
```

**Step 5: Run tests to verify compilation**

Run: `cargo test -p tethys --lib`
Expected: PASS (all existing tests should still pass)

**Step 6: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): add crates field to Tethys struct

Caches discovered CrateInfo for module path computation during indexing."
```

---

## Task 4: Add helper method to compute module path for any file

**Files:**
- Modify: `crates/tethys/src/lib.rs`

**Step 1: Add the helper method to impl Tethys**

Find the `impl Tethys` block and add:

```rust
/// Compute the module path for a file in this workspace.
///
/// Returns an empty string if the file is not part of any crate's module tree.
fn compute_module_path_for_file(&self, file_path: &Path) -> String {
    // Canonicalize for consistent matching (CrateInfo paths are canonicalized)
    let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());

    crate::cargo::get_crate_for_file(&canonical, &self.crates)
        .and_then(|crate_info| crate::cargo::compute_module_path(&canonical, crate_info))
        .unwrap_or_default()
}
```

**Step 2: Run tests to verify compilation**

Run: `cargo test -p tethys --lib`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): add compute_module_path_for_file method

Helper that finds the crate and computes module path in one call."
```

---

## Task 5: Integrate module path computation into indexing

**Files:**
- Modify: `crates/tethys/src/lib.rs` (around line 636)

**Step 1: Find the TODO comment**

Search for `module_path: "", // TODO` in `lib.rs`. It should be around line 636 in the `index_file` method.

**Step 2: Replace the empty string with computed value**

Change from:

```rust
let symbol_data: Vec<SymbolData> = extracted
    .iter()
    .zip(qualified_names.iter())
    .map(|(sym, qn)| SymbolData {
        name: &sym.name,
        module_path: "", // TODO: compute module_path
        // ...
    })
```

To:

```rust
// Compute module path once for all symbols in this file
let module_path = self.compute_module_path_for_file(path);

let symbol_data: Vec<SymbolData> = extracted
    .iter()
    .zip(qualified_names.iter())
    .map(|(sym, qn)| SymbolData {
        name: &sym.name,
        module_path: &module_path,
        // ...
    })
```

**Step 3: Run tests to verify compilation**

Run: `cargo test -p tethys --lib`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): integrate module path computation into indexing

Symbols now have proper module_path values computed from file location."
```

---

## Task 6: Add integration test for module path indexing

**Files:**
- Create: `crates/tethys/tests/module_path_integration.rs`

**Step 1: Write the integration test**

Create the new test file:

```rust
//! Integration tests for module path computation during indexing.

use std::path::PathBuf;
use tethys::Tethys;

/// Test that indexing the tethys crate itself produces correct module paths.
#[test]
fn index_tethys_crate_has_module_paths() {
    // Use the actual tethys crate as test input
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tethys should be in crates/")
        .parent()
        .expect("crates/ should be in workspace root")
        .to_path_buf();

    let tethys = Tethys::create(&workspace).expect("create should succeed");
    let _stats = tethys.index().expect("index should succeed");

    // Query for a known symbol in tethys itself
    let symbols = tethys
        .search_symbols("CrateInfo")
        .expect("search should succeed");

    let crate_info_symbol = symbols
        .iter()
        .find(|s| s.name == "CrateInfo" && s.kind == tethys::SymbolKind::Struct)
        .expect("CrateInfo struct should be indexed");

    // CrateInfo is in crates/tethys/src/types.rs -> crate::types
    assert_eq!(
        crate_info_symbol.module_path, "crate::types",
        "CrateInfo should have module_path 'crate::types'"
    );
}

/// Test that symbols in nested modules have correct paths.
#[test]
fn nested_module_paths_are_correct() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tethys should be in crates/")
        .parent()
        .expect("crates/ should be in workspace root")
        .to_path_buf();

    let tethys = Tethys::create(&workspace).expect("create should succeed");
    let _stats = tethys.index().expect("index should succeed");

    // Query for TethysDb which is in db/mod.rs or db.rs
    let symbols = tethys
        .search_symbols("TethysDb")
        .expect("search should succeed");

    let db_symbol = symbols
        .iter()
        .find(|s| s.name == "TethysDb")
        .expect("TethysDb should be indexed");

    // TethysDb is in crates/tethys/src/db/mod.rs -> crate::db
    assert!(
        db_symbol.module_path.starts_with("crate::db"),
        "TethysDb should have module_path starting with 'crate::db', got '{}'",
        db_symbol.module_path
    );
}

/// Test that files outside crates have empty module paths.
#[test]
fn files_outside_crates_have_empty_module_path() {
    // This test would require setting up a file outside the crate structure
    // For now, we verify the function returns empty string for non-matching paths
    // by testing the cargo module functions directly

    use tethys::cargo::{compute_module_path, CrateInfo};
    use std::path::Path;

    let crate_info = CrateInfo {
        name: "test".to_string(),
        path: PathBuf::from("/workspace/crates/test"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    };

    // File in examples/ is outside module tree
    let result = compute_module_path(
        Path::new("/workspace/crates/test/examples/demo.rs"),
        &crate_info,
    );
    assert_eq!(result, None, "examples should not have module paths");
}
```

**Step 2: Run the integration test**

Run: `cargo test -p tethys --test module_path_integration`
Expected: PASS (3 tests)

**Step 3: Commit**

```bash
git add crates/tethys/tests/module_path_integration.rs
git commit -m "test(tethys): add integration tests for module path computation

Verifies symbols are indexed with correct module_path values."
```

---

## Task 7: Export cargo module functions in lib.rs

**Files:**
- Modify: `crates/tethys/src/lib.rs`

**Step 1: Check current exports**

Look for `pub mod cargo` or `pub use cargo::` in `lib.rs`. If `cargo` is not public, the integration tests won't compile.

**Step 2: Make cargo module public (if needed)**

If `mod cargo;` exists but isn't public, change to:

```rust
pub mod cargo;
```

Or add selective re-exports:

```rust
pub use cargo::{compute_module_path, discover_crates, get_crate_for_file, CrateInfo};
```

Note: `CrateInfo` is already in `types.rs` and likely re-exported. Check if it needs to be moved or if `cargo` functions should use the existing type.

**Step 3: Run all tests**

Run: `cargo test -p tethys`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): export cargo module for module path utilities"
```

---

## Task 8: Final verification and cleanup

**Files:**
- Review all modified files

**Step 1: Run full test suite**

Run: `cargo test -p tethys`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -p tethys -- -D warnings`
Expected: No warnings

**Step 3: Run formatter**

Run: `cargo fmt -p tethys`

**Step 4: Final commit (if any formatting changes)**

```bash
git add -A
git commit -m "chore(tethys): format and clippy fixes"
```

**Step 5: Summarize changes**

The implementation is complete when:
- [ ] `compute_module_path()` works for lib, binary, nested, and mod.rs files
- [ ] `get_crate_for_file()` finds the correct crate
- [ ] `Tethys` struct caches `Vec<CrateInfo>`
- [ ] Indexing populates `module_path` for all symbols
- [ ] Integration tests verify real crate indexing works
- [ ] All tests pass, clippy clean, formatted

---

## Summary

| Task | Description | Tests |
|------|-------------|-------|
| 1 | Add `compute_module_path` function | 7 unit tests |
| 2 | Add `get_crate_for_file` helper | 2 unit tests |
| 3 | Add `crates` field to Tethys | Compilation |
| 4 | Add `compute_module_path_for_file` method | Compilation |
| 5 | Integrate into indexing loop | Compilation |
| 6 | Add integration tests | 3 integration tests |
| 7 | Export cargo module | Compilation |
| 8 | Final verification | Full suite |

**Total new tests:** 12
**Estimated time:** 30-45 minutes
