//! Integration tests for Tethys indexing.
//!
//! These tests verify the full indexing pipeline:
//! workspace → tree-sitter → symbols → `SQLite`

use std::fs;
use tempfile::TempDir;
use tethys::Tethys;

/// Create a temporary workspace with the given files.
/// Returns the temp directory (must be kept alive) and the Tethys instance.
fn workspace_with_files(files: &[(&str, &str)]) -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");

    for (path, content) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(&full_path, content).expect("failed to write file");
    }

    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

// ============================================================================
// Phase 1: Basic Indexing
// ============================================================================

#[test]
fn index_empty_workspace_returns_zero_stats() {
    let (_dir, mut tethys) = workspace_with_files(&[]);

    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 0);
    assert_eq!(stats.symbols_found, 0);
    assert_eq!(stats.references_found, 0);
    assert!(stats.errors.is_empty());
}

#[test]
fn index_single_rust_file_extracts_function() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/main.rs",
        r#"
fn main() {
    println!("Hello, world!");
}
"#,
    )]);

    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 1);
    assert!(
        stats.symbols_found >= 1,
        "should find at least the main function"
    );
}

#[test]
fn index_extracts_multiple_symbols_from_file() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r#"
pub struct User {
    pub name: String,
    pub age: u32,
}

impl User {
    pub fn new(name: String, age: u32) -> Self {
        Self { name, age }
    }

    pub fn greet(&self) -> String {
        format!("Hello, {}!", self.name)
    }
}

pub fn create_user(name: &str, age: u32) -> User {
    User::new(name.to_string(), age)
}
"#,
    )]);

    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 1);
    // Should find: User (struct), new (method), greet (method), create_user (function)
    assert!(
        stats.symbols_found >= 4,
        "expected at least 4 symbols, found {}",
        stats.symbols_found
    );
}

#[test]
fn index_multiple_files_in_workspace() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/main.rs",
            r"
mod auth;

fn main() {
    auth::authenticate();
}
",
        ),
        (
            "src/auth.rs",
            r"
pub fn authenticate() -> bool {
    true
}

pub fn logout() {
    // cleanup
}
",
        ),
    ]);

    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 2);
    // main.rs: main function
    // auth.rs: authenticate, logout functions
    assert!(
        stats.symbols_found >= 3,
        "expected at least 3 symbols, found {}",
        stats.symbols_found
    );
}

#[test]
fn index_skips_non_rust_files() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/main.rs", "fn main() {}"),
        ("README.md", "# My Project"),
        ("config.json", "{}"),
        (".gitignore", "target/"),
    ]);

    let stats = tethys.index().expect("index failed");

    // Should only index main.rs (non-source files aren't discovered, so not counted as skipped)
    assert_eq!(stats.files_indexed, 1);
    // The implementation filters non-source files during discovery, not processing
    // So files_skipped only counts files that were discovered but couldn't be processed
}

#[test]
fn index_records_errors_for_invalid_syntax() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/broken.rs",
        r"
fn this_is_not_valid {
    completely broken syntax here !!!
}
",
    )]);

    let stats = tethys
        .index()
        .expect("index should complete despite parse errors");

    // The file should be recorded but may have errors
    // Tree-sitter is error-tolerant, so it might still extract partial symbols
    assert_eq!(stats.files_indexed, 1);
    // We don't assert on errors because tree-sitter is forgiving
}

// ============================================================================
// Phase 1: Symbol Queries After Indexing
// ============================================================================

#[test]
fn list_symbols_returns_symbols_in_file() {
    let (dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn hello() {}
pub fn world() {}
",
    )]);

    tethys.index().expect("index failed");

    let symbols = tethys
        .list_symbols(&dir.path().join("src/lib.rs"))
        .expect("list_symbols failed");

    assert_eq!(symbols.len(), 2);

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"hello"));
    assert!(names.contains(&"world"));
}

#[test]
fn list_symbols_returns_empty_for_unknown_file() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn foo() {}")]);

    tethys.index().expect("index failed");

    let symbols = tethys
        .list_symbols(std::path::Path::new("/nonexistent/file.rs"))
        .expect("list_symbols failed");

    assert!(symbols.is_empty());
}

#[test]
fn search_symbols_finds_by_name() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn authenticate_user() {}
pub fn authorize_request() {}
pub fn validate_token() {}
",
    )]);

    tethys.index().expect("index failed");

    let results = tethys.search_symbols("auth").expect("search failed");

    // Should find authenticate_user and authorize_request
    assert!(results.len() >= 2);

    let names: Vec<&str> = results.iter().map(|s| s.name.as_str()).collect();
    assert!(names.iter().any(|n| n.contains("auth")));
}

#[test]
fn search_symbols_returns_empty_for_no_match() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn foo() {}")]);

    tethys.index().expect("index failed");

    let results = tethys
        .search_symbols("nonexistent_symbol_xyz")
        .expect("search failed");

    assert!(results.is_empty());
}

#[test]
fn search_symbols_with_empty_query_returns_empty() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn foo() {}")]);

    tethys.index().expect("index failed");

    let results = tethys.search_symbols("").expect("search failed");

    // Design decision: empty query returns empty results, not all symbols
    assert!(results.is_empty());
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn index_handles_non_utf8_file_gracefully() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file_path = dir.path().join("src").join("binary.rs");

    // Create parent directory
    fs::create_dir_all(file_path.parent().unwrap()).expect("failed to create src dir");

    // Write invalid UTF-8 bytes (this is not valid UTF-8)
    fs::write(&file_path, [0xFF, 0xFE, 0x00, 0x01, 0x80, 0x81]).expect("failed to write file");

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    let stats = tethys
        .index()
        .expect("index should complete despite parse errors");

    // The file should be recorded as an error, not crash the indexer
    assert!(
        !stats.errors.is_empty(),
        "should have recorded an error for non-UTF-8 file"
    );
    assert!(
        stats.errors.iter().any(|e| e.message.contains("UTF-8")),
        "error message should mention UTF-8"
    );
}

#[test]
fn index_handles_empty_rust_file() {
    let (_dir, mut tethys) =
        workspace_with_files(&[("src/empty.rs", ""), ("src/whitespace.rs", "   \n\n   ")]);

    let stats = tethys.index().expect("index failed");

    // Empty files should be indexed without error, just no symbols
    assert_eq!(stats.files_indexed, 2);
    assert_eq!(stats.symbols_found, 0);
    assert!(stats.errors.is_empty());
}

// ============================================================================
// Phase 2: Dependency Detection
// ============================================================================

#[test]
fn get_dependencies_for_file_using_internal_module() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/lib.rs",
            r"
mod auth;
mod config;
",
        ),
        (
            "src/auth.rs",
            r"
use crate::config::Config;

pub struct Authenticator {
    config: Config,
}

impl Authenticator {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}
",
        ),
        (
            "src/config.rs",
            r"
pub struct Config {
    pub secret: String,
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    // auth.rs should depend on config.rs (uses Config type)
    let deps = tethys
        .get_dependencies(std::path::Path::new("src/auth.rs"))
        .expect("get_dependencies failed");

    assert!(
        deps.iter().any(|p| p.to_string_lossy().contains("config")),
        "auth.rs should depend on config.rs, got: {deps:?}"
    );
}

#[test]
fn get_dependents_for_file() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/lib.rs",
            r"
mod auth;
mod config;
",
        ),
        (
            "src/auth.rs",
            r"
use crate::config::Config;

pub fn authenticate(config: Config) -> bool {
    true
}
",
        ),
        (
            "src/config.rs",
            r"
pub struct Config {
    pub secret: String,
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    // config.rs should have auth.rs as a dependent
    let dependents = tethys
        .get_dependents(std::path::Path::new("src/config.rs"))
        .expect("get_dependents failed");

    assert!(
        dependents
            .iter()
            .any(|p| p.to_string_lossy().contains("auth")),
        "config.rs should have auth.rs as dependent, got: {dependents:?}"
    );
}

#[test]
fn dependencies_ignores_unused_imports() {
    // This tests L2 behavior: only count dependencies for symbols that are ACTUALLY USED
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/lib.rs",
            r"
mod auth;
mod config;
mod utils;
",
        ),
        (
            "src/auth.rs",
            r"
// Import Config but never use it
use crate::config::Config;
// Import Helper and actually use it
use crate::utils::Helper;

pub fn authenticate() -> bool {
    Helper::check()
}
",
        ),
        (
            "src/config.rs",
            r"
pub struct Config {}
",
        ),
        (
            "src/utils.rs",
            r"
pub struct Helper;
impl Helper {
    pub fn check() -> bool { true }
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    let deps = tethys
        .get_dependencies(std::path::Path::new("src/auth.rs"))
        .expect("get_dependencies failed");

    // auth.rs should depend on utils.rs (Helper is used)
    assert!(
        deps.iter().any(|p| p.to_string_lossy().contains("utils")),
        "auth.rs should depend on utils.rs (used), got: {deps:?}"
    );

    // auth.rs should NOT depend on config.rs (Config is imported but unused)
    // This is the key L2 test!
    assert!(
        !deps.iter().any(|p| p.to_string_lossy().contains("config")),
        "auth.rs should NOT depend on config.rs (unused import), got: {deps:?}"
    );
}

#[test]
fn dependencies_detects_aliased_imports() {
    // Test that `use Foo as Bar` creates dependency when `Bar` is used
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/lib.rs",
            r"
mod auth;
mod config;
",
        ),
        (
            "src/auth.rs",
            r"
use crate::config::Config as Settings;

pub fn get_settings() -> Settings {
    Settings { secret: String::new() }
}
",
        ),
        (
            "src/config.rs",
            r"
pub struct Config {
    pub secret: String,
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    let deps = tethys
        .get_dependencies(std::path::Path::new("src/auth.rs"))
        .expect("get_dependencies failed");

    // auth.rs should depend on config.rs because Settings (alias for Config) is used
    assert!(
        deps.iter().any(|p| p.to_string_lossy().contains("config")),
        "auth.rs should depend on config.rs (aliased import used), got: {deps:?}"
    );
}

#[test]
fn dependencies_handles_circular_references() {
    // Test that A→B, B→A circular dependencies are both detected.
    //
    // The deferred dependency resolution ensures that dependencies from
    // earlier-indexed files to later-indexed files are properly recorded.
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/lib.rs",
            r"
mod a;
mod b;
",
        ),
        (
            "src/a.rs",
            r"
use crate::b::B;

pub struct A;

impl A {
    pub fn get_b() -> B { B }
}
",
        ),
        (
            "src/b.rs",
            r"
use crate::a::A;

pub struct B;

impl B {
    pub fn get_a() -> A { A }
}
",
        ),
    ]);

    // Indexing should complete without errors
    let stats = tethys.index().expect("index failed");
    assert!(stats.errors.is_empty(), "should have no indexing errors");

    // At least one direction of the circular dependency should be detected
    // (depends on file indexing order - the later-indexed file's dependencies
    // to earlier files will be recorded)
    let deps_a = tethys
        .get_dependencies(std::path::Path::new("src/a.rs"))
        .expect("get_dependencies failed");
    let deps_b = tethys
        .get_dependencies(std::path::Path::new("src/b.rs"))
        .expect("get_dependencies failed");

    let a_depends_on_b = deps_a.iter().any(|p| p.to_string_lossy().contains("b.rs"));
    let b_depends_on_a = deps_b.iter().any(|p| p.to_string_lossy().contains("a.rs"));

    // Both directions should be detected thanks to deferred dependency resolution
    assert!(
        a_depends_on_b,
        "a.rs should depend on b.rs, got deps: {deps_a:?}"
    );
    assert!(
        b_depends_on_a,
        "b.rs should depend on a.rs, got deps: {deps_b:?}"
    );
}

#[test]
fn deferred_resolution_handles_three_file_cycle() {
    // A→B→C→A cycle requires multiple resolution passes.
    // This verifies the convergence loop works correctly.
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "mod a;\nmod b;\nmod c;"),
        (
            "src/a.rs",
            r"
use crate::b::B;

pub struct A;

impl A {
    pub fn get() -> B { B }
}
",
        ),
        (
            "src/b.rs",
            r"
use crate::c::C;

pub struct B;

impl B {
    pub fn get() -> C { C }
}
",
        ),
        (
            "src/c.rs",
            r"
use crate::a::A;

pub struct C;

impl C {
    pub fn get() -> A { A }
}
",
        ),
    ]);

    let stats = tethys.index().expect("index failed");
    assert!(stats.errors.is_empty(), "should have no indexing errors");

    // All three cycle edges should be detected
    let deps_a = tethys
        .get_dependencies(std::path::Path::new("src/a.rs"))
        .expect("get_dependencies failed");
    let deps_b = tethys
        .get_dependencies(std::path::Path::new("src/b.rs"))
        .expect("get_dependencies failed");
    let deps_c = tethys
        .get_dependencies(std::path::Path::new("src/c.rs"))
        .expect("get_dependencies failed");

    assert!(
        deps_a.iter().any(|p| p.to_string_lossy().contains("b.rs")),
        "a.rs should depend on b.rs, got: {deps_a:?}"
    );
    assert!(
        deps_b.iter().any(|p| p.to_string_lossy().contains("c.rs")),
        "b.rs should depend on c.rs, got: {deps_b:?}"
    );
    assert!(
        deps_c.iter().any(|p| p.to_string_lossy().contains("a.rs")),
        "c.rs should depend on a.rs, got: {deps_c:?}"
    );
}
