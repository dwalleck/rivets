# Cargo.toml Parsing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Parse Cargo.toml files to dynamically detect crate roots instead of hardcoding `workspace_root/src/`.

**Architecture:** Add a `cargo.rs` module that discovers crate structure by parsing Cargo.toml files (using `cargo_toml` crate). Store discovered `CrateInfo` in `Tethys` struct. Provide methods to resolve which crate a file belongs to and get that crate's root directory.

**Tech Stack:** Rust, cargo_toml crate, existing tethys infrastructure

**Design Doc:** `docs/plans/2026-02-01-cargo-toml-parsing-design.md`

---

### Task 1: Add cargo_toml Dependency

**Files:**
- Modify: `crates/tethys/Cargo.toml`

**Step 1: Add the dependency**

Add to `[dependencies]` section:

```toml
cargo_toml = "0.20"
```

**Step 2: Verify it compiles**

Run: `cargo check -p tethys`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add crates/tethys/Cargo.toml
git commit -m "build(tethys): add cargo_toml dependency for manifest parsing"
```

---

### Task 2: Add CrateInfo Type

**Files:**
- Modify: `crates/tethys/src/types.rs`
- Test: `crates/tethys/src/types.rs` (inline tests)

**Step 1: Write the failing test**

Add to the `#[cfg(test)]` module at the bottom of `types.rs`:

```rust
#[test]
fn crate_info_default_lib_path() {
    let info = CrateInfo {
        name: "my_crate".to_string(),
        path: PathBuf::from("/workspace/crates/my_crate"),
        lib_path: None,
        bin_paths: vec![],
    };
    assert_eq!(info.name, "my_crate");
    assert!(info.lib_path.is_none());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tethys crate_info_default_lib_path`
Expected: FAIL with "cannot find type `CrateInfo`"

**Step 3: Write the type definition**

Add after the `Language` enum (around line 150) in `types.rs`:

```rust
/// Information about a Rust crate discovered from Cargo.toml.
///
/// Used to determine crate roots for module path resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateInfo {
    /// Crate name from `[package].name`
    pub name: String,
    /// Path to the crate directory (contains Cargo.toml)
    pub path: PathBuf,
    /// Library entry point relative to crate path (e.g., `src/lib.rs`)
    pub lib_path: Option<PathBuf>,
    /// Binary entry points: (name, path relative to crate)
    pub bin_paths: Vec<(String, PathBuf)>,
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p tethys crate_info_default_lib_path`
Expected: PASS

**Step 5: Export the type**

Add to `crates/tethys/src/lib.rs` exports (around line 47):

```rust
pub use types::{
    CrateInfo, Cycle, DatabaseStats, ...  // Add CrateInfo to existing list
};
```

**Step 6: Verify it compiles**

Run: `cargo check -p tethys`
Expected: Compiles successfully

**Step 7: Commit**

```bash
git add crates/tethys/src/types.rs crates/tethys/src/lib.rs
git commit -m "feat(tethys): add CrateInfo type for Cargo.toml parsing"
```

---

### Task 3: Create cargo.rs Module with discover_crates

**Files:**
- Create: `crates/tethys/src/cargo.rs`
- Modify: `crates/tethys/src/lib.rs` (add `mod cargo;`)
- Test: `crates/tethys/tests/cargo_discovery.rs`

**Step 1: Write the failing test**

Create `crates/tethys/tests/cargo_discovery.rs`:

```rust
//! Tests for Cargo.toml discovery and parsing.

use std::fs;
use tempfile::TempDir;
use tethys::CrateInfo;

mod common;

#[test]
fn discover_single_crate_with_default_lib() {
    let dir = TempDir::new().expect("create temp dir");

    // Create a minimal Cargo.toml
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write Cargo.toml");

    // Create src/lib.rs so default lib path exists
    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/lib.rs"), "// lib").expect("write lib.rs");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1);
    assert_eq!(crates[0].name, "test_crate");
    assert_eq!(crates[0].lib_path, Some(std::path::PathBuf::from("src/lib.rs")));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tethys --test cargo_discovery discover_single_crate`
Expected: FAIL with "cannot find function `discover_crates`"

**Step 3: Create the cargo module**

Create `crates/tethys/src/cargo.rs`:

```rust
//! Cargo.toml discovery and parsing.
//!
//! This module handles discovering Rust crate structure by parsing
//! Cargo.toml manifest files. It supports workspaces, single crates,
//! and virtual workspaces.

use std::path::{Path, PathBuf};

use cargo_toml::Manifest;
use tracing::{debug, warn};

use crate::CrateInfo;

/// Discover all crates in a workspace by parsing Cargo.toml files.
///
/// Handles three cases:
/// 1. Virtual workspace - `[workspace]` without `[package]`
/// 2. Workspace with root crate - Both `[workspace]` and `[package]`
/// 3. Single crate - Just `[package]`, no workspace
///
/// Returns empty vec if no Cargo.toml found (non-Rust project).
pub fn discover_crates(workspace_root: &Path) -> Vec<CrateInfo> {
    let manifest_path = workspace_root.join("Cargo.toml");

    let manifest = match Manifest::from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            debug!(
                path = %manifest_path.display(),
                error = %e,
                "No valid Cargo.toml found, treating as non-Rust project"
            );
            return Vec::new();
        }
    };

    let mut crates = Vec::new();

    // Handle workspace members
    if let Some(workspace) = &manifest.workspace {
        for member in &workspace.members {
            let member_path = workspace_root.join(member);

            // Handle glob patterns (e.g., "crates/*")
            if member.contains('*') {
                if let Ok(entries) = glob_member(workspace_root, member) {
                    for entry in entries {
                        if let Some(info) = parse_crate(&entry) {
                            crates.push(info);
                        }
                    }
                }
            } else if let Some(info) = parse_crate(&member_path) {
                crates.push(info);
            }
        }
    }

    // Handle root package (if present)
    if manifest.package.is_some() {
        if let Some(info) = parse_crate_from_manifest(workspace_root, &manifest) {
            crates.push(info);
        }
    }

    debug!(
        workspace = %workspace_root.display(),
        crate_count = crates.len(),
        "Discovered crates"
    );

    crates
}

/// Parse a single crate's Cargo.toml.
fn parse_crate(crate_path: &Path) -> Option<CrateInfo> {
    let manifest_path = crate_path.join("Cargo.toml");
    let manifest = Manifest::from_path(&manifest_path).ok()?;
    parse_crate_from_manifest(crate_path, &manifest)
}

/// Extract CrateInfo from a parsed manifest.
fn parse_crate_from_manifest(crate_path: &Path, manifest: &Manifest) -> Option<CrateInfo> {
    let package = manifest.package.as_ref()?;

    // Determine library path
    let lib_path = if let Some(lib) = &manifest.lib {
        lib.path.as_ref().map(PathBuf::from)
    } else {
        // Check for default lib.rs location
        let default_lib = crate_path.join("src/lib.rs");
        if default_lib.exists() {
            Some(PathBuf::from("src/lib.rs"))
        } else {
            None
        }
    };

    // Determine binary paths
    let mut bin_paths = Vec::new();

    // Explicit [[bin]] entries
    for bin in &manifest.bin {
        let name = bin.name.clone().unwrap_or_else(|| package.name.clone());
        let path = bin
            .path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("src/bin/{name}.rs")));
        bin_paths.push((name, path));
    }

    // Default main.rs if no explicit bins and file exists
    if bin_paths.is_empty() {
        let default_main = crate_path.join("src/main.rs");
        if default_main.exists() {
            bin_paths.push((package.name.clone(), PathBuf::from("src/main.rs")));
        }
    }

    Some(CrateInfo {
        name: package.name.clone(),
        path: crate_path.to_path_buf(),
        lib_path,
        bin_paths,
    })
}

/// Expand a glob pattern to matching directories.
fn glob_member(workspace_root: &Path, pattern: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut results = Vec::new();

    // Simple glob: only handle "prefix/*" pattern
    if let Some(prefix) = pattern.strip_suffix("/*") {
        let search_dir = workspace_root.join(prefix);
        if search_dir.is_dir() {
            for entry in std::fs::read_dir(&search_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() && path.join("Cargo.toml").exists() {
                    results.push(path);
                }
            }
        }
    } else {
        warn!(
            pattern = pattern,
            "Unsupported glob pattern, only 'prefix/*' supported"
        );
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_member_expands_simple_pattern() {
        // This test uses the actual rivets workspace structure
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let results = glob_member(workspace, "crates/*").expect("glob should work");

        // Should find at least tethys
        assert!(!results.is_empty());
        assert!(results.iter().any(|p| p.ends_with("tethys")));
    }
}
```

**Step 4: Add the module to lib.rs**

Add after line 44 in `crates/tethys/src/lib.rs`:

```rust
mod cargo;
```

And add to exports (around line 68):

```rust
pub use cargo::discover_crates;
```

**Step 5: Run test to verify it passes**

Run: `cargo test -p tethys --test cargo_discovery discover_single_crate`
Expected: PASS

**Step 6: Run all tests**

Run: `cargo test -p tethys`
Expected: All tests pass

**Step 7: Commit**

```bash
git add crates/tethys/src/cargo.rs crates/tethys/src/lib.rs crates/tethys/tests/cargo_discovery.rs
git commit -m "feat(tethys): add cargo.rs module with discover_crates function"
```

---

### Task 4: Add More Discovery Tests

**Files:**
- Modify: `crates/tethys/tests/cargo_discovery.rs`

**Step 1: Add workspace discovery test**

```rust
#[test]
fn discover_workspace_with_multiple_members() {
    let dir = TempDir::new().expect("create temp dir");

    // Create workspace Cargo.toml
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[workspace]
members = ["crates/foo", "crates/bar"]
"#,
    )
    .expect("write workspace Cargo.toml");

    // Create member crates
    for name in ["foo", "bar"] {
        let crate_dir = dir.path().join("crates").join(name);
        fs::create_dir_all(crate_dir.join("src")).expect("create crate dir");

        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#
            ),
        )
        .expect("write member Cargo.toml");

        fs::write(crate_dir.join("src/lib.rs"), "// lib").expect("write lib.rs");
    }

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 2);

    let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"foo"));
    assert!(names.contains(&"bar"));
}

#[test]
fn discover_workspace_with_glob_members() {
    let dir = TempDir::new().expect("create temp dir");

    // Create workspace with glob pattern
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[workspace]
members = ["crates/*"]
"#,
    )
    .expect("write workspace Cargo.toml");

    // Create member crates
    for name in ["alpha", "beta"] {
        let crate_dir = dir.path().join("crates").join(name);
        fs::create_dir_all(crate_dir.join("src")).expect("create crate dir");

        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#
            ),
        )
        .expect("write member Cargo.toml");

        fs::write(crate_dir.join("src/lib.rs"), "// lib").expect("write lib.rs");
    }

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 2);
}

#[test]
fn discover_crate_with_custom_lib_path() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "custom_lib"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/mylib.rs"
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/mylib.rs"), "// custom lib").expect("write mylib.rs");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1);
    assert_eq!(
        crates[0].lib_path,
        Some(std::path::PathBuf::from("src/mylib.rs"))
    );
}

#[test]
fn discover_binary_crate() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "my_cli"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/main.rs"), "fn main() {}").expect("write main.rs");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1);
    assert!(crates[0].lib_path.is_none());
    assert_eq!(crates[0].bin_paths.len(), 1);
    assert_eq!(crates[0].bin_paths[0].0, "my_cli");
    assert_eq!(
        crates[0].bin_paths[0].1,
        std::path::PathBuf::from("src/main.rs")
    );
}

#[test]
fn discover_non_rust_project_returns_empty() {
    let dir = TempDir::new().expect("create temp dir");

    // No Cargo.toml
    let crates = tethys::discover_crates(dir.path());

    assert!(crates.is_empty());
}
```

**Step 2: Run tests**

Run: `cargo test -p tethys --test cargo_discovery`
Expected: All tests pass

**Step 3: Commit**

```bash
git add crates/tethys/tests/cargo_discovery.rs
git commit -m "test(tethys): add comprehensive Cargo.toml discovery tests"
```

---

### Task 5: Add crates Field and Resolution Methods to Tethys

**Files:**
- Modify: `crates/tethys/src/lib.rs`
- Test: `crates/tethys/tests/cargo_discovery.rs`

**Step 1: Write the failing test**

Add to `cargo_discovery.rs`:

```rust
#[test]
fn tethys_resolves_crate_for_file() {
    let dir = TempDir::new().expect("create temp dir");

    // Create workspace with two crates
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[workspace]
members = ["crates/foo", "crates/bar"]
"#,
    )
    .expect("write workspace Cargo.toml");

    for name in ["foo", "bar"] {
        let crate_dir = dir.path().join("crates").join(name);
        fs::create_dir_all(crate_dir.join("src")).expect("create crate dir");
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(r#"
[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#),
        ).expect("write Cargo.toml");
        fs::write(crate_dir.join("src/lib.rs"), "// lib").expect("write lib.rs");
    }

    let tethys = tethys::Tethys::new(dir.path()).expect("create Tethys");

    // File in foo crate should resolve to foo
    let foo_file = dir.path().join("crates/foo/src/lib.rs");
    let crate_info = tethys.get_crate_for_file(&foo_file);
    assert!(crate_info.is_some());
    assert_eq!(crate_info.unwrap().name, "foo");

    // File in bar crate should resolve to bar
    let bar_file = dir.path().join("crates/bar/src/lib.rs");
    let crate_info = tethys.get_crate_for_file(&bar_file);
    assert!(crate_info.is_some());
    assert_eq!(crate_info.unwrap().name, "bar");
}

#[test]
fn tethys_gets_crate_root_for_file() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src/utils")).expect("create src/utils");
    fs::write(dir.path().join("src/lib.rs"), "mod utils;").expect("write lib.rs");
    fs::write(dir.path().join("src/utils/mod.rs"), "// utils").expect("write utils/mod.rs");

    let tethys = tethys::Tethys::new(dir.path()).expect("create Tethys");

    // File deep in src should resolve to src/ as crate root
    let nested_file = dir.path().join("src/utils/mod.rs");
    let crate_root = tethys.get_crate_root_for_file(&nested_file);

    assert!(crate_root.is_some());
    assert_eq!(crate_root.unwrap(), dir.path().join("src"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tethys --test cargo_discovery tethys_resolves`
Expected: FAIL with "no method named `get_crate_for_file`"

**Step 3: Add crates field to Tethys struct**

In `lib.rs`, modify the `Tethys` struct (around line 88):

```rust
pub struct Tethys {
    workspace_root: PathBuf,
    db_path: PathBuf,
    db: Index,
    parser: tree_sitter::Parser,
    crates: Vec<CrateInfo>,
}
```

**Step 4: Update Tethys::new to discover crates**

In `Tethys::new` (around line 100), add after creating the parser:

```rust
let crates = cargo::discover_crates(workspace_root);
```

And update the struct instantiation:

```rust
Ok(Self {
    workspace_root: workspace_root.to_path_buf(),
    db_path,
    db,
    parser,
    crates,
})
```

**Step 5: Add resolution methods**

Add these methods to the `impl Tethys` block:

```rust
/// Get information about the crate a file belongs to.
///
/// Returns the crate with the longest matching path prefix (handles nested crates).
/// Returns `None` if the file isn't in any discovered crate.
#[must_use]
pub fn get_crate_for_file(&self, file_path: &Path) -> Option<&CrateInfo> {
    self.crates
        .iter()
        .filter(|c| file_path.starts_with(&c.path))
        .max_by_key(|c| c.path.components().count())
}

/// Get the crate root directory (containing lib.rs/main.rs) for a file.
///
/// Returns the parent directory of the crate's entry point (e.g., `crate_path/src/`).
/// Returns `None` if the file isn't in a discovered crate or the crate has no entry point.
#[must_use]
pub fn get_crate_root_for_file(&self, file_path: &Path) -> Option<PathBuf> {
    let crate_info = self.get_crate_for_file(file_path)?;

    // Use lib path if available, otherwise first bin path
    let entry_point = crate_info
        .lib_path
        .as_ref()
        .or_else(|| crate_info.bin_paths.first().map(|(_, p)| p))?;

    // Crate root is the parent of the entry point (e.g., src/)
    entry_point
        .parent()
        .map(|p| crate_info.path.join(p))
}
```

**Step 6: Run tests to verify they pass**

Run: `cargo test -p tethys --test cargo_discovery`
Expected: All tests pass

**Step 7: Commit**

```bash
git add crates/tethys/src/lib.rs crates/tethys/tests/cargo_discovery.rs
git commit -m "feat(tethys): add crates field and resolution methods to Tethys"
```

---

### Task 6: Update FIXME Site 1 (derive_file_deps_from_call_edges)

**Files:**
- Modify: `crates/tethys/src/lib.rs:1023-1025`

**Step 1: Read the current code**

The current code at line 1023-1025:

```rust
// FIXME: Assumes crate root is workspace_root/src/. Does not detect actual
// main/lib location from Cargo.toml. Needs Cargo.toml parsing support.
let crate_root = self.workspace_root.join("src");
```

**Step 2: Update to use dynamic resolution**

Replace with:

```rust
let crate_root = self
    .get_crate_root_for_file(current_file)
    .unwrap_or_else(|| self.workspace_root.join("src"));
```

**Step 3: Run tests**

Run: `cargo test -p tethys`
Expected: All tests pass

**Step 4: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "fix(tethys): use dynamic crate root in derive_file_deps_from_call_edges"
```

---

### Task 7: Update FIXME Site 2 (resolve_refs_via_imports)

**Files:**
- Modify: `crates/tethys/src/lib.rs:1450-1455`

**Step 1: Read the current code**

The current code computes crate_root once before the loop:

```rust
// FIXME: Assumes crate root is workspace_root/src/. Does not detect actual
// main/lib location from Cargo.toml. Needs Cargo.toml parsing support.
let crate_root = self.workspace_root.join("src");

for (file_id, refs) in by_file {
    resolved_count += self.resolve_refs_for_file(file_id, refs, &crate_root)?;
}
```

**Step 2: Update to compute crate root per-file**

This is trickier because we need to look up the file path for each file_id. Modify to:

```rust
for (file_id, refs) in by_file {
    // Determine crate root for this specific file
    let crate_root = if let Some(file) = self.db.get_file_by_id(file_id)? {
        let file_path = self.workspace_root.join(&file.path);
        self.get_crate_root_for_file(&file_path)
            .unwrap_or_else(|| self.workspace_root.join("src"))
    } else {
        self.workspace_root.join("src")
    };

    resolved_count += self.resolve_refs_for_file(file_id, refs, &crate_root)?;
}
```

Also remove the old FIXME comment and the old `let crate_root = ...` line.

**Step 3: Run tests**

Run: `cargo test -p tethys`
Expected: All tests pass

**Step 4: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "fix(tethys): use per-file crate root in resolve_refs_via_imports"
```

---

### Task 8: Add Integration Test with Real Workspace

**Files:**
- Modify: `crates/tethys/tests/cargo_discovery.rs`

**Step 1: Add test using rivets workspace**

```rust
#[test]
fn discover_rivets_workspace() {
    // Test against the actual rivets workspace
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let crates = tethys::discover_crates(workspace);

    // Should find at least these crates
    let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"tethys"), "should find tethys crate");
    assert!(names.contains(&"rivets"), "should find rivets crate");
    assert!(names.contains(&"rivets-jsonl"), "should find rivets-jsonl crate");

    // Tethys should have lib_path
    let tethys_crate = crates.iter().find(|c| c.name == "tethys").unwrap();
    assert_eq!(
        tethys_crate.lib_path,
        Some(std::path::PathBuf::from("src/lib.rs"))
    );
}

#[test]
fn tethys_resolves_files_in_rivets_workspace() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let tethys = tethys::Tethys::new(workspace).expect("create Tethys");

    // A file in tethys crate should resolve to tethys
    let tethys_lib = workspace.join("crates/tethys/src/lib.rs");
    let crate_info = tethys.get_crate_for_file(&tethys_lib);
    assert!(crate_info.is_some());
    assert_eq!(crate_info.unwrap().name, "tethys");

    // Crate root should be crates/tethys/src
    let crate_root = tethys.get_crate_root_for_file(&tethys_lib);
    assert_eq!(crate_root, Some(workspace.join("crates/tethys/src")));
}
```

**Step 2: Run tests**

Run: `cargo test -p tethys --test cargo_discovery rivets`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/tethys/tests/cargo_discovery.rs
git commit -m "test(tethys): add integration tests with rivets workspace"
```

---

### Task 9: Final Verification and Cleanup

**Step 1: Run all tests**

Run: `cargo test -p tethys`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -p tethys -- -D warnings`
Expected: No warnings

**Step 3: Run fmt check**

Run: `cargo fmt -p tethys -- --check`
Expected: No formatting issues

**Step 4: Verify the fix works on rivets itself**

Run: `cargo run -p tethys -- index` (in the worktree)
Expected: Indexes successfully

**Step 5: Final commit if any cleanup needed**

```bash
git add -A
git commit -m "chore(tethys): cleanup after Cargo.toml parsing implementation"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Add cargo_toml dependency | Cargo.toml |
| 2 | Add CrateInfo type | types.rs, lib.rs |
| 3 | Create cargo.rs module | cargo.rs, lib.rs, cargo_discovery.rs |
| 4 | Add more discovery tests | cargo_discovery.rs |
| 5 | Add crates field + resolution methods | lib.rs, cargo_discovery.rs |
| 6 | Update FIXME site 1 | lib.rs |
| 7 | Update FIXME site 2 | lib.rs |
| 8 | Add integration tests | cargo_discovery.rs |
| 9 | Final verification | - |
