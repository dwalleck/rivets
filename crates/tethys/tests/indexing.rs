//! Integration tests for Tethys indexing.
//!
//! These tests verify the full indexing pipeline:
//! workspace → tree-sitter → symbols → `SQLite`

use std::fs;
use tempfile::TempDir;
use tethys::{SymbolId, Tethys};

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
    assert_eq!(stats.symbols_found, 1, "should find the main function");
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
    assert_eq!(
        stats.symbols_found, 4,
        "expected 4 symbols (User, new, greet, create_user), found {}",
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
    // main.rs: main function + mod declaration
    // auth.rs: authenticate, logout functions
    assert_eq!(
        stats.symbols_found, 4,
        "expected 4 symbols (main, auth mod, authenticate, logout), found {}",
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
fn list_symbols_returns_not_found_for_unknown_file() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn foo() {}")]);

    tethys.index().expect("index failed");

    let result = tethys.list_symbols(std::path::Path::new("/nonexistent/file.rs"));

    assert!(
        result.is_err(),
        "should return NotFound error for unknown file"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, tethys::Error::NotFound(_)),
        "expected NotFound error, got: {err:?}"
    );
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
    assert_eq!(
        results.len(),
        2,
        "should find authenticate_user and authorize_request"
    );

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

// ============================================================================
// Phase 2: Reference Storage and Queries
// ============================================================================

#[test]
fn index_stores_references_for_same_file_symbols() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct User {
    pub name: String,
}

impl User {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

pub fn create_user(name: String) -> User {
    User::new(name)
}
",
    )]);

    let stats = tethys.index().expect("index failed");

    // Should find references within the file:
    // - User in return type of create_user
    // - User::new() call in create_user
    // - Self references (which resolve to User)
    assert!(
        stats.references_found > 0,
        "should store references, found: {}",
        stats.references_found
    );
}

#[test]
fn list_references_in_file_returns_references() {
    let (dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Config {
    pub value: u32,
}

pub fn get_config() -> Config {
    Config { value: 42 }
}
",
    )]);

    tethys.index().expect("index failed");

    let refs = tethys
        .list_references_in_file(&dir.path().join("src/lib.rs"))
        .expect("list_references_in_file failed");

    // Should find Config references:
    // - Config in return type
    // - Config { ... } constructor
    assert!(
        !refs.is_empty(),
        "should have references in file, got: {refs:?}"
    );

    // All references should be to Config
    let config_refs: Vec<_> = refs.iter().filter(|r| r.symbol_id.as_i64() > 0).collect();
    assert!(
        !config_refs.is_empty(),
        "should have resolved symbol references"
    );
}

#[test]
fn get_references_returns_references_to_symbol() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Helper;

impl Helper {
    pub fn assist() {}
}

pub fn use_helper() {
    Helper::assist();
}

pub fn another_helper_use() -> Helper {
    Helper
}
",
    )]);

    tethys.index().expect("index failed");

    // Get references to the Helper struct
    let refs = tethys
        .get_references("Helper")
        .expect("get_references failed");

    // Should find multiple references to Helper:
    // - In impl Helper
    // - Helper::assist() call
    // - Helper return type
    // - Helper constructor
    assert_eq!(
        refs.len(),
        2,
        "should have 2 references to Helper, got: {refs:?}"
    );
}

#[test]
fn get_symbol_by_qualified_name() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn authenticate() -> bool {
    true
}
",
    )]);

    tethys.index().expect("index failed");

    let symbol = tethys
        .get_symbol("authenticate")
        .expect("get_symbol failed");

    assert!(symbol.is_some(), "should find authenticate symbol");
    let sym = symbol.unwrap();
    assert_eq!(sym.name, "authenticate");
}

#[test]
fn get_symbol_returns_none_for_unknown() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn foo() {}")]);

    tethys.index().expect("index failed");

    let symbol = tethys
        .get_symbol("nonexistent_symbol")
        .expect("get_symbol failed");

    assert!(symbol.is_none(), "should not find nonexistent symbol");
}

#[test]
fn get_symbol_by_id_works() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn my_function() {}
",
    )]);

    tethys.index().expect("index failed");

    // First get the symbol to find its ID
    let symbol = tethys
        .get_symbol("my_function")
        .expect("get_symbol failed")
        .expect("should find symbol");

    // Now look it up by ID
    let by_id = tethys
        .get_symbol_by_id(symbol.id)
        .expect("get_symbol_by_id failed");

    assert!(by_id.is_some(), "should find symbol by ID");
    assert_eq!(by_id.unwrap().name, "my_function");
}

#[test]
fn references_track_containing_symbol() {
    let (dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Data;

pub fn process() -> Data {
    Data
}

pub fn another() -> Data {
    Data
}
",
    )]);

    tethys.index().expect("index failed");

    let refs = tethys
        .list_references_in_file(&dir.path().join("src/lib.rs"))
        .expect("list_references_in_file failed");

    // Filter to Data references with in_symbol_id set
    let refs_with_containing: Vec<_> = refs.iter().filter(|r| r.in_symbol_id.is_some()).collect();

    // References inside process() and another() should have in_symbol_id
    assert!(
        !refs_with_containing.is_empty(),
        "some references should track containing symbol, got: {refs:?}"
    );
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

#[test]
fn get_references_returns_not_found_for_nonexistent_symbol() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn real_function() {}
",
    )]);

    tethys.index().expect("index failed");

    let result = tethys.get_references("symbol_that_does_not_exist");

    assert!(
        result.is_err(),
        "should return NotFound error for nonexistent symbol"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, tethys::Error::NotFound(_)),
        "expected NotFound error, got: {err:?}"
    );
}

#[test]
fn get_symbol_by_id_returns_none_for_invalid_id() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn foo() {}")]);

    tethys.index().expect("index failed");

    let symbol = tethys
        .get_symbol_by_id(SymbolId::from(999_999))
        .expect("get_symbol_by_id should not error");

    assert!(symbol.is_none(), "should return None for non-existent ID");
}

#[test]
fn get_references_returns_empty_for_unreferenced_symbol() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
// Symbol exists but is never referenced anywhere
pub struct UnusedType;

pub fn unrelated_function() -> i32 {
    42
}
",
    )]);

    tethys.index().expect("index failed");

    let refs = tethys
        .get_references("UnusedType")
        .expect("get_references should not error");

    // UnusedType is defined but never used, so no references
    assert!(
        refs.is_empty(),
        "unreferenced symbol should have no references, got: {refs:?}"
    );
}

#[test]
fn list_references_in_file_returns_not_found_for_unknown_file() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn foo() {}")]);

    tethys.index().expect("index failed");

    let result = tethys.list_references_in_file(std::path::Path::new("src/nonexistent.rs"));

    assert!(
        result.is_err(),
        "should return NotFound error for unknown file"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, tethys::Error::NotFound(_)),
        "expected NotFound error, got: {err:?}"
    );
}

#[test]
fn list_references_in_file_returns_empty_for_file_with_no_references() {
    let (dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
// File with only definitions, no references to other symbols
pub const VALUE: i32 = 42;
",
    )]);

    tethys.index().expect("index failed");

    let refs = tethys
        .list_references_in_file(&dir.path().join("src/lib.rs"))
        .expect("list_references_in_file should not error");

    // A file with only a constant definition has no outgoing references
    // (primitive types like i32 are not tracked as symbol references)
    assert!(
        refs.is_empty(),
        "file with no symbol references should return empty, got: {refs:?}"
    );
}

#[test]
fn references_preserve_reference_kind() {
    use tethys::ReferenceKind;

    let (dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct User {
    pub name: String,
}

impl User {
    pub fn new() -> Self {
        Self { name: String::new() }
    }
}

pub fn create() -> User {
    User::new()
}
",
    )]);

    tethys.index().expect("index failed");

    let refs = tethys
        .list_references_in_file(&dir.path().join("src/lib.rs"))
        .expect("list_references_in_file failed");

    // Should have different reference kinds
    let kinds: std::collections::HashSet<_> = refs.iter().map(|r| &r.kind).collect();

    // We expect at least type references (User in return type)
    // and possibly call references (User::new())
    assert!(
        kinds.contains(&ReferenceKind::Type) || kinds.contains(&ReferenceKind::Call),
        "should have typed references, got kinds: {kinds:?}"
    );
}

#[test]
fn references_distinguish_construct_vs_call() {
    use tethys::ReferenceKind;

    let (dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn origin() -> Self {
        Point { x: 0, y: 0 }  // Construct reference
    }
}

pub fn make_point() -> Point {
    Point::new(1, 2)  // Call reference
}
",
    )]);

    tethys.index().expect("index failed");

    let refs = tethys
        .list_references_in_file(&dir.path().join("src/lib.rs"))
        .expect("list_references_in_file failed");

    // Find construct and call references
    let construct_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.kind == ReferenceKind::Construct)
        .collect();
    let call_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.kind == ReferenceKind::Call)
        .collect();

    // Point { x: 0, y: 0 } should be Construct
    assert!(
        !construct_refs.is_empty(),
        "should have Construct references for struct literals"
    );

    // Point::new(1, 2) should be Call
    assert!(
        !call_refs.is_empty(),
        "should have Call references for function calls"
    );
}

// ============================================================================
// Phase 2 Limitations and Edge Cases
// ============================================================================

#[test]
fn cross_file_references_not_stored_in_phase_2() {
    // Phase 2 only stores references where the target symbol is in the same file.
    // Cross-file references will be resolved in Phase 3+.
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "mod utils;\nmod main_mod;"),
        ("src/utils.rs", "pub struct Helper;"),
        (
            "src/main_mod.rs",
            r"
use crate::utils::Helper;

pub fn use_helper() -> Helper {
    Helper
}
",
        ),
    ]);

    let stats = tethys.index().expect("index failed");

    // Cross-file references are NOT stored in Phase 2
    // The refs in main_mod.rs to Helper from utils.rs should not be in refs table
    let refs = tethys
        .get_references("Helper")
        .expect("get_references failed");

    // No refs should be found since Helper is in utils.rs but referenced from main_mod.rs
    // This documents the current Phase 2 limitation
    assert!(
        refs.is_empty(),
        "cross-file references should not be stored in Phase 2, got: {refs:?}"
    );

    // But the symbol itself should exist
    let symbol = tethys.get_symbol("Helper").expect("get_symbol failed");
    assert!(symbol.is_some(), "Helper symbol should be indexed");

    // And the indexer should still work (no errors)
    assert!(stats.errors.is_empty(), "should have no indexing errors");
}

#[test]
fn index_stats_references_found_is_accurate() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Foo;

pub fn a() -> Foo { Foo }
pub fn b() -> Foo { Foo }
",
    )]);

    let stats = tethys.index().expect("index failed");

    // The reference count should be consistent and non-zero.
    // Currently tracks: type references (Foo in return type) and constructors (Foo in body).
    // The exact count may vary based on extraction implementation.
    assert_eq!(
        stats.references_found, 2,
        "should find 2 references (one Foo constructor per function body), got: {}",
        stats.references_found
    );

    // Verify the count matches what we can query
    let refs = tethys.get_references("Foo").expect("get_references failed");
    assert_eq!(
        stats.references_found,
        refs.len(),
        "stats.references_found should match actual queryable references"
    );
}

#[test]
fn duplicate_symbol_names_are_indexed_separately() {
    // When multiple symbols have the same name in different contexts (e.g., impl blocks),
    // they should be indexed as separate symbols with distinct qualified names.
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct A;
pub struct B;

impl A {
    pub fn process() -> i32 { 1 }
}

impl B {
    pub fn process() -> i32 { 2 }
}

pub fn caller() {
    A::process();
    B::process();
}
",
    )]);

    tethys.index().expect("index failed");

    // Both A::process and B::process should be indexed as separate symbols
    let symbols = tethys.search_symbols("process").expect("search failed");
    assert_eq!(
        symbols.len(),
        2,
        "should have two 'process' symbols, got: {symbols:?}"
    );

    // Verify both symbols have qualified names
    let qualified_names: Vec<_> = symbols.iter().map(|s| s.qualified_name.as_str()).collect();
    assert!(
        qualified_names.contains(&"A::process"),
        "should have A::process, got: {qualified_names:?}"
    );
    assert!(
        qualified_names.contains(&"B::process"),
        "should have B::process, got: {qualified_names:?}"
    );

    // Get references using qualified names - each should work independently
    // Note: Cross-impl method resolution is a known limitation in Phase 2.
    // References may or may not be found depending on how scoped calls are extracted.
    for sym in &symbols {
        let refs_result = tethys.get_references(&sym.qualified_name);
        assert!(
            refs_result.is_ok(),
            "get_references for {} should succeed",
            sym.qualified_name
        );
    }
}

// ============================================================================
// Database Statistics Tests
// ============================================================================

#[test]
fn get_stats_on_empty_database() {
    let (_dir, tethys) = workspace_with_files(&[]);

    let stats = tethys.get_stats().expect("get_stats failed");

    assert_eq!(stats.file_count, 0);
    assert_eq!(stats.symbol_count, 0);
    assert_eq!(stats.reference_count, 0);
    assert_eq!(stats.file_dependency_count, 0);
    assert!(stats.files_by_language.is_empty());
    assert!(stats.symbols_by_kind.is_empty());
}

#[test]
fn get_stats_returns_correct_file_count() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "pub fn hello() {}"),
        ("src/utils.rs", "pub fn util() {}"),
    ]);

    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");

    assert_eq!(stats.file_count, 2, "should have 2 indexed files");
}

#[test]
fn get_stats_counts_files_by_language() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "pub fn hello() {}"),
        ("src/utils.rs", "pub fn util() {}"),
    ]);

    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");

    let rust_count = stats
        .files_by_language
        .get(&tethys::Language::Rust)
        .copied()
        .unwrap_or(0);
    assert_eq!(rust_count, 2, "should have 2 Rust files");
}

#[test]
fn get_stats_counts_symbols_by_kind() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn my_function() {}
pub struct MyStruct;
pub enum MyEnum { A, B }
",
    )]);

    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");

    let fn_count = stats
        .symbols_by_kind
        .get(&tethys::SymbolKind::Function)
        .copied()
        .unwrap_or(0);
    let struct_count = stats
        .symbols_by_kind
        .get(&tethys::SymbolKind::Struct)
        .copied()
        .unwrap_or(0);
    let enum_count = stats
        .symbols_by_kind
        .get(&tethys::SymbolKind::Enum)
        .copied()
        .unwrap_or(0);

    assert_eq!(fn_count, 1, "should have exactly 1 function");
    assert_eq!(struct_count, 1, "should have exactly 1 struct");
    assert_eq!(enum_count, 1, "should have exactly 1 enum");
}

#[test]
fn get_stats_counts_references() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Config;

pub fn get_config() -> Config {
    Config
}
",
    )]);

    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");

    // Should have references: Config in return type, Config constructor
    assert_eq!(
        stats.reference_count, 1,
        "should have exactly 1 reference (Config constructor)"
    );
}

#[test]
fn get_stats_file_count_equals_language_sum() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "pub fn a() {}"),
        ("src/utils.rs", "pub fn b() {}"),
        ("src/helpers.rs", "pub fn c() {}"),
    ]);

    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");

    let language_sum: usize = stats.files_by_language.values().sum();
    assert_eq!(
        stats.file_count,
        language_sum + stats.skipped_unknown_languages,
        "file_count should equal sum of files_by_language + skipped"
    );
}

#[test]
fn get_stats_symbol_count_equals_kind_sum() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn func() {}
pub struct S;
pub enum E { A }
pub trait T {}
",
    )]);

    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");

    let kind_sum: usize = stats.symbols_by_kind.values().sum();
    assert_eq!(
        stats.symbol_count,
        kind_sum + stats.skipped_unknown_kinds,
        "symbol_count should equal sum of symbols_by_kind + skipped"
    );
}

// ============================================================================
// Transaction Atomicity Tests
// ============================================================================

#[test]
fn reindex_preserves_data_when_file_becomes_unreadable() {
    // Test that a successful index followed by a failed re-index doesn't corrupt
    // existing data. The index_file_atomic method uses a SQLite transaction, so
    // if re-indexing fails, the original file and symbols should remain intact.
    let (dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn original_function() -> i32 {
    42
}

pub struct OriginalStruct {
    pub value: String,
}
",
    )]);

    // First index should succeed
    let stats = tethys.index().expect("first index should succeed");
    assert_eq!(stats.files_indexed, 1, "should index 1 file");
    assert!(
        stats.symbols_found >= 2,
        "should find at least 2 symbols (function + struct)"
    );

    // Verify symbols are queryable
    let symbols_before = tethys
        .list_symbols(&dir.path().join("src/lib.rs"))
        .expect("list_symbols should work after first index");
    let names_before: Vec<&str> = symbols_before.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names_before.contains(&"original_function"),
        "should find original_function before re-index"
    );
    assert!(
        names_before.contains(&"OriginalStruct"),
        "should find OriginalStruct before re-index"
    );

    // Now corrupt the file with invalid UTF-8 so re-indexing fails
    fs::write(
        dir.path().join("src/lib.rs"),
        [0xFF, 0xFE, 0x00, 0x01, 0x80, 0x81],
    )
    .expect("failed to write corrupt file");

    // Re-index should complete (errors are collected, not propagated)
    let stats2 = tethys
        .index()
        .expect("re-index should complete despite errors");
    assert!(
        !stats2.errors.is_empty(),
        "re-index should report errors for corrupt file"
    );

    // The original symbols should still be queryable because the failed re-index
    // should not have modified the database (transaction rollback).
    // Note: index_file_atomic is only reached if UTF-8 parsing succeeds, so
    // the file entry from the first index remains untouched.
    let file = tethys
        .get_file(&dir.path().join("src/lib.rs"))
        .expect("get_file should not error");
    assert!(
        file.is_some(),
        "file entry should still exist after failed re-index"
    );

    let symbols_after = tethys
        .list_symbols(&dir.path().join("src/lib.rs"))
        .expect("list_symbols should work after failed re-index");
    let names_after: Vec<&str> = symbols_after.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names_after.contains(&"original_function"),
        "original_function should survive failed re-index, got: {names_after:?}"
    );
    assert!(
        names_after.contains(&"OriginalStruct"),
        "OriginalStruct should survive failed re-index, got: {names_after:?}"
    );
    assert_eq!(
        symbols_before.len(),
        symbols_after.len(),
        "symbol count should be unchanged after failed re-index"
    );
}

// C# Indexing Tests

#[test]
fn index_single_csharp_file_extracts_class() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/Program.cs",
        r#"
using System;

public class Program {
    public static void Main() {
        Console.WriteLine("Hello");
    }
}
"#,
    )]);

    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 1);
    // Should find: Program (class), Main (method)
    assert_eq!(
        stats.symbols_found, 2,
        "should find 2 symbols (Program class + Main method), found {}",
        stats.symbols_found
    );
    assert!(stats.errors.is_empty(), "should have no indexing errors");
}

#[test]
fn get_stats_counts_csharp_files_by_language() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "pub fn hello() {}"),
        ("src/utils.rs", "pub fn util() {}"),
        (
            "src/Program.cs",
            r#"
using System;

public class Program {
    public static void Main() {
        Console.WriteLine("Hello");
    }
}
"#,
        ),
        (
            "src/Helper.cs",
            r"
public class Helper {
    public void Assist() { }
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");

    let rust_count = stats
        .files_by_language
        .get(&tethys::Language::Rust)
        .copied()
        .unwrap_or(0);
    let csharp_count = stats
        .files_by_language
        .get(&tethys::Language::CSharp)
        .copied()
        .unwrap_or(0);

    assert_eq!(rust_count, 2, "should have 2 Rust files");
    assert_eq!(csharp_count, 2, "should have 2 C# files");
    assert_eq!(stats.file_count, 4, "should have 4 total files");

    // Verify invariant: file_count == sum(files_by_language) + skipped
    let language_sum: usize = stats.files_by_language.values().sum();
    assert_eq!(
        stats.file_count,
        language_sum + stats.skipped_unknown_languages,
        "file_count should equal sum of files_by_language + skipped"
    );
}

#[test]
fn list_symbols_returns_csharp_symbols() {
    let (dir, mut tethys) = workspace_with_files(&[(
        "src/UserService.cs",
        r"
using System;

public class UserService {
    public void Save(string name) {
        Console.WriteLine(name);
    }

    public void Delete(int id) {
    }
}
",
    )]);

    tethys.index().expect("index failed");

    let symbols = tethys
        .list_symbols(&dir.path().join("src/UserService.cs"))
        .expect("list_symbols failed");

    // Should find: UserService (class), Save (method), Delete (method)
    assert_eq!(
        symbols.len(),
        3,
        "should find 3 symbols (UserService, Save, Delete), found {}",
        symbols.len()
    );

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"UserService"),
        "should find UserService class"
    );
    assert!(names.contains(&"Save"), "should find Save method");
    assert!(names.contains(&"Delete"), "should find Delete method");

    // Verify methods have correct parent
    let save_sym = symbols
        .iter()
        .find(|s| s.name == "Save")
        .expect("should find Save symbol");
    assert_eq!(
        save_sym.kind,
        tethys::SymbolKind::Method,
        "Save should be a Method"
    );
}
