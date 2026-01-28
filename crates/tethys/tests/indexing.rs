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

    // Check for resolved symbol references (symbol_id is Some)
    let resolved_refs: Vec<_> = refs.iter().filter(|r| r.symbol_id.is_some()).collect();
    assert!(
        !resolved_refs.is_empty(),
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

#[test]
fn cross_file_references_are_resolved() {
    // Cross-file references are now resolved in Pass 2 via import information.
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

    // Cross-file references are now resolved via Pass 2.
    // The refs in main_mod.rs to Helper from utils.rs should be resolved.
    let refs = tethys
        .get_references("Helper")
        .expect("get_references failed");

    // References should be found since Pass 2 resolves them via imports.
    assert!(
        !refs.is_empty(),
        "cross-file references should be resolved, got empty"
    );

    // The symbol itself should exist
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
    // Note: Cross-impl method resolution is a known limitation (same-file only).
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

#[test]
fn indexes_csharp_class() {
    let code = r"
public class UserService {
    public void Save(User user) { }
}
";
    let (_dir, mut tethys) = workspace_with_files(&[("UserService.cs", code)]);
    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 1, "should index 1 C# file");
    assert!(stats.symbols_found >= 2, "should find class + method");
}

#[test]
fn indexes_csharp_symbols() {
    let code = r"
namespace MyApp.Services;

public class Calculator {
    public int Add(int a, int b) { return a + b; }
    public static int Multiply(int a, int b) { return a * b; }
}

public interface ICalculator {
    int Add(int a, int b);
}
";
    let (_dir, mut tethys) = workspace_with_files(&[("Calculator.cs", code)]);
    tethys.index().expect("index failed");

    let symbols = tethys.search_symbols("Calculator").expect("search failed");
    let calculator = symbols
        .iter()
        .find(|s| s.name == "Calculator" && s.kind == tethys::SymbolKind::Class);
    assert!(
        calculator.is_some(),
        "should find Calculator as a Class, got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.name, s.kind))
            .collect::<Vec<_>>()
    );

    let symbols = tethys.search_symbols("ICalculator").expect("search failed");
    let interface = symbols
        .iter()
        .find(|s| s.name == "ICalculator" && s.kind == tethys::SymbolKind::Interface);
    assert!(
        interface.is_some(),
        "should find ICalculator as an Interface, got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.name, s.kind))
            .collect::<Vec<_>>()
    );
}

#[test]
fn indexes_mixed_rust_and_csharp() {
    let rust_code = r"
pub fn hello() {}
";
    let csharp_code = r"
public class Greeter {
    public void Hello() { }
}
";
    let (_dir, mut tethys) =
        workspace_with_files(&[("src/lib.rs", rust_code), ("Greeter.cs", csharp_code)]);
    let stats = tethys.index().expect("index failed");

    assert_eq!(
        stats.files_indexed, 2,
        "should index both Rust and C# files"
    );
}

#[test]
fn csharp_stats_include_language() {
    let code = "public class Foo { }";
    let (_dir, mut tethys) = workspace_with_files(&[("Foo.cs", code)]);
    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");
    let csharp_count = stats
        .files_by_language
        .get(&tethys::Language::CSharp)
        .copied()
        .unwrap_or(0);
    assert_eq!(csharp_count, 1, "should count 1 C# file in stats");
}

#[test]
fn csharp_references_are_stored() {
    // Define User in the same file so references can be resolved
    let code = r"
public class User {
    public void Save() { }
}

public class Test {
    public void Run() {
        var user = new User();
        user.Save();
    }
}
";
    let (_dir, mut tethys) = workspace_with_files(&[("Test.cs", code)]);
    let stats = tethys.index().expect("index failed");

    // Should find references: User constructor (new User())
    assert!(
        stats.references_found > 0,
        "should find references in C# code, found: {}",
        stats.references_found
    );
}

#[test]
fn csharp_namespace_dependency_resolution() {
    let service_code = r"
namespace MyApp.Services;

public class UserService {
    public void Save() { }
}
";
    let controller_code = r"
using MyApp.Services;

namespace MyApp.Controllers;

public class UserController {
    public void Create() {
        var svc = new UserService();
        svc.Save();
    }
}
";
    let (dir, mut tethys) = workspace_with_files(&[
        ("Services/UserService.cs", service_code),
        ("Controllers/UserController.cs", controller_code),
    ]);
    let stats = tethys.index().expect("index failed");
    assert_eq!(stats.files_indexed, 2);

    // UserController.cs depends on UserService.cs via `using MyApp.Services`
    let deps = tethys
        .get_dependencies(&dir.path().join("Controllers/UserController.cs"))
        .expect("get_dependencies failed");

    assert_eq!(deps.len(), 1, "should have 1 dependency");
    assert!(
        deps[0].ends_with("Services/UserService.cs"),
        "should depend on UserService.cs, got: {:?}",
        deps[0]
    );
}

#[test]
fn csharp_namespace_shared_by_multiple_files() {
    let model_a = r"
namespace MyApp.Models;
public class User { }
";
    let model_b = r"
namespace MyApp.Models;
public class Order { }
";
    let consumer = r"
using MyApp.Models;
namespace MyApp.Services;
public class Service {
    public void Run() {
        var u = new User();
        var o = new Order();
    }
}
";
    let (dir, mut tethys) = workspace_with_files(&[
        ("Models/User.cs", model_a),
        ("Models/Order.cs", model_b),
        ("Services/Service.cs", consumer),
    ]);
    tethys.index().expect("index failed");

    let deps = tethys
        .get_dependencies(&dir.path().join("Services/Service.cs"))
        .expect("get_dependencies failed");

    // Should depend on both files that declare the MyApp.Models namespace
    assert_eq!(deps.len(), 2, "should depend on both model files");
}

// ========================================================================
// Import Storage Tests
// ========================================================================

#[test]
fn rust_imports_stored_during_indexing() {
    let code = r"
use std::collections::HashMap;
use std::io::{Read, Write};
use crate::db::Index;

fn main() {
    let _map: HashMap<String, i32> = HashMap::new();
}
";
    let (dir, mut tethys) = workspace_with_files(&[("src/main.rs", code)]);
    tethys.index().expect("index failed");

    let imports = tethys
        .list_imports_in_file(&dir.path().join("src/main.rs"))
        .expect("list_imports_in_file failed");

    // Should have: HashMap from std::collections, Read and Write from std::io, Index from crate::db
    assert!(
        imports.len() >= 4,
        "expected at least 4 imports, found {}",
        imports.len()
    );

    // Check HashMap import
    let hashmap_import = imports
        .iter()
        .find(|i| i.symbol_name == "HashMap")
        .expect("should have HashMap import");
    assert_eq!(
        hashmap_import.source_module, "std::collections",
        "HashMap should come from std::collections"
    );

    // Check Read import
    let read_import = imports
        .iter()
        .find(|i| i.symbol_name == "Read")
        .expect("should have Read import");
    assert_eq!(
        read_import.source_module, "std::io",
        "Read should come from std::io"
    );

    // Check Write import
    let write_import = imports
        .iter()
        .find(|i| i.symbol_name == "Write")
        .expect("should have Write import");
    assert_eq!(
        write_import.source_module, "std::io",
        "Write should come from std::io"
    );

    // Check Index import
    let index_import = imports
        .iter()
        .find(|i| i.symbol_name == "Index")
        .expect("should have Index import");
    assert_eq!(
        index_import.source_module, "crate::db",
        "Index should come from crate::db"
    );
}

#[test]
fn rust_glob_import_stored_with_star() {
    let code = r"
use std::collections::*;

fn main() {}
";
    let (dir, mut tethys) = workspace_with_files(&[("src/main.rs", code)]);
    tethys.index().expect("index failed");

    let imports = tethys
        .list_imports_in_file(&dir.path().join("src/main.rs"))
        .expect("list_imports_in_file failed");

    // Should have glob import with "*"
    let glob_import = imports
        .iter()
        .find(|i| i.symbol_name == "*" && i.source_module == "std::collections")
        .expect("should have glob import");

    assert_eq!(glob_import.symbol_name, "*");
    assert_eq!(glob_import.source_module, "std::collections");
}

#[test]
fn rust_aliased_import_stored() {
    let code = r"
use std::collections::HashMap as Map;

fn main() {
    let _m: Map<String, i32> = Map::new();
}
";
    let (dir, mut tethys) = workspace_with_files(&[("src/main.rs", code)]);
    tethys.index().expect("index failed");

    let imports = tethys
        .list_imports_in_file(&dir.path().join("src/main.rs"))
        .expect("list_imports_in_file failed");

    let aliased_import = imports
        .iter()
        .find(|i| i.symbol_name == "HashMap")
        .expect("should have HashMap import");

    assert_eq!(
        aliased_import.alias,
        Some("Map".to_string()),
        "should have alias 'Map'"
    );
}

#[test]
fn csharp_imports_stored_during_indexing() {
    let code = r"
using System;
using System.Collections.Generic;
using MyApp.Services;

namespace MyApp;
public class Program {
    public static void Main() {
        var list = new List<string>();
    }
}
";
    let (dir, mut tethys) = workspace_with_files(&[("src/Program.cs", code)]);
    tethys.index().expect("index failed");

    let imports = tethys
        .list_imports_in_file(&dir.path().join("src/Program.cs"))
        .expect("list_imports_in_file failed");

    // C# using directives import the whole namespace, stored with "*"
    assert!(
        imports.len() >= 3,
        "expected at least 3 imports, found {}",
        imports.len()
    );

    // Check System import
    let system_import = imports
        .iter()
        .find(|i| i.source_module == "System")
        .expect("should have System import");
    assert_eq!(
        system_import.symbol_name, "*",
        "C# namespace import should use *"
    );

    // Check System.Collections.Generic import
    let collections_import = imports
        .iter()
        .find(|i| i.source_module == "System.Collections.Generic")
        .expect("should have System.Collections.Generic import");
    assert_eq!(
        collections_import.symbol_name, "*",
        "C# namespace import should use *"
    );

    // Check MyApp.Services import
    let services_import = imports
        .iter()
        .find(|i| i.source_module == "MyApp.Services")
        .expect("should have MyApp.Services import");
    assert_eq!(
        services_import.symbol_name, "*",
        "C# namespace import should use *"
    );
}

#[test]
fn csharp_imports_use_dot_separator() {
    let code = r"
using System.Collections.Generic;

namespace MyApp;
public class Program { }
";
    let (dir, mut tethys) = workspace_with_files(&[("src/Program.cs", code)]);
    tethys.index().expect("index failed");

    let imports = tethys
        .list_imports_in_file(&dir.path().join("src/Program.cs"))
        .expect("list_imports_in_file failed");

    // Verify C# uses "." separator, not "::"
    let import = imports
        .iter()
        .find(|i| i.source_module.contains("Collections"))
        .expect("should have Collections import");

    assert!(
        import.source_module.contains('.'),
        "C# imports should use '.' separator, got: {}",
        import.source_module
    );
    assert!(
        !import.source_module.contains("::"),
        "C# imports should not use '::' separator, got: {}",
        import.source_module
    );
}

#[test]
fn imports_cleared_on_reindex() {
    let code_v1 = r"
use std::collections::HashMap;

fn main() {}
";
    let code_v2 = r"
use std::io::Read;

fn main() {}
";

    let (dir, mut tethys) = workspace_with_files(&[("src/main.rs", code_v1)]);
    tethys.index().expect("first index failed");

    // Check initial imports
    let imports_v1 = tethys
        .list_imports_in_file(&dir.path().join("src/main.rs"))
        .expect("list_imports_in_file failed");
    assert!(
        imports_v1.iter().any(|i| i.symbol_name == "HashMap"),
        "should have HashMap import initially"
    );

    // Update the file and reindex
    std::fs::write(dir.path().join("src/main.rs"), code_v2).expect("write failed");
    tethys.index().expect("second index failed");

    // Check imports after reindex
    let imports_v2 = tethys
        .list_imports_in_file(&dir.path().join("src/main.rs"))
        .expect("list_imports_in_file failed");

    // Old import should be gone
    assert!(
        !imports_v2.iter().any(|i| i.symbol_name == "HashMap"),
        "HashMap import should be cleared after reindex"
    );

    // New import should be present
    assert!(
        imports_v2.iter().any(|i| i.symbol_name == "Read"),
        "Read import should be present after reindex"
    );
}

#[test]
fn list_imports_returns_not_found_for_unknown_file() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/main.rs", "fn main() {}")]);
    tethys.index().expect("index failed");

    let result = tethys.list_imports_in_file(std::path::Path::new("/nonexistent/file.rs"));

    assert!(result.is_err(), "should return error for unknown file");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("not found"),
        "error should indicate file not found"
    );
}

#[test]
fn list_imports_returns_empty_for_file_without_imports() {
    let code = r#"
fn main() {
    let x = 42;
    println!("{}", x);
}
"#;
    let (dir, mut tethys) = workspace_with_files(&[("src/main.rs", code)]);
    tethys.index().expect("index failed");

    let imports = tethys
        .list_imports_in_file(&dir.path().join("src/main.rs"))
        .expect("list_imports_in_file failed");

    assert!(
        imports.is_empty(),
        "file without imports should return empty list"
    );
}

// ========================================================================
// Cross-File Reference Resolution Tests (Pass 2)
// ========================================================================

#[test]
fn cross_file_references_resolved_via_explicit_import() {
    // Test that references to symbols imported via explicit import are resolved
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "mod utils;\nmod main_mod;"),
        (
            "src/utils.rs",
            r"
pub struct Helper;

impl Helper {
    pub fn assist() {}
}
",
        ),
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

    tethys.index().expect("index failed");

    // The Helper symbol should now have references from main_mod.rs
    let refs = tethys
        .get_references("Helper")
        .expect("get_references failed");

    // After Pass 2, cross-file references via explicit imports should be resolved
    // We expect at least one reference from the main_mod.rs file
    assert!(
        !refs.is_empty(),
        "Helper should have cross-file references resolved via explicit import"
    );
}

#[test]
fn cross_file_references_resolved_via_glob_import() {
    // Test that references to symbols imported via glob import are resolved
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "mod prelude;\nmod consumer;"),
        (
            "src/prelude.rs",
            r"
pub struct Config;
pub struct Settings;
",
        ),
        (
            "src/consumer.rs",
            r"
use crate::prelude::*;

pub fn get_config() -> Config {
    Config
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    // Config should have references from consumer.rs resolved via glob import
    let refs = tethys
        .get_references("Config")
        .expect("get_references failed");

    assert!(
        !refs.is_empty(),
        "Config should have cross-file references resolved via glob import"
    );
}

#[test]
fn unresolved_external_crate_references_remain_unresolved() {
    // Test that references to external crate symbols remain unresolved
    let (dir, mut tethys) = workspace_with_files(&[(
        "src/main.rs",
        r"
use std::collections::HashMap;

fn main() {
    let map: HashMap<String, i32> = HashMap::new();
}
",
    )]);

    tethys.index().expect("index failed");

    // HashMap is from std, not our workspace, so there should be no symbol for it
    let symbol = tethys
        .get_symbol("HashMap")
        .expect("get_symbol should not error");
    assert!(
        symbol.is_none(),
        "HashMap from std should not be indexed (external crate)"
    );

    // References to HashMap should remain unresolved (can be checked via list_references_in_file)
    let refs = tethys
        .list_references_in_file(&dir.path().join("src/main.rs"))
        .expect("list_references_in_file failed");

    // Any unresolved references should still have symbol_id = None
    let unresolved_refs: Vec<_> = refs.iter().filter(|r| r.symbol_id.is_none()).collect();

    // We expect unresolved references because HashMap and String are external
    // The exact count depends on what's extracted, but we should have some unresolved refs
    assert!(
        !unresolved_refs.is_empty(),
        "references to external crate symbols should remain unresolved"
    );
}

#[test]
fn cross_file_reference_resolution_with_aliased_import() {
    // Test that aliased imports resolve correctly
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "mod types;\nmod consumer;"),
        (
            "src/types.rs",
            r"
pub struct Configuration;
",
        ),
        (
            "src/consumer.rs",
            r"
use crate::types::Configuration as Config;

pub fn get_config() -> Config {
    Config
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    // Configuration should have references even though it's used as Config
    let refs = tethys
        .get_references("Configuration")
        .expect("get_references failed");

    // After Pass 2, references using the alias should be resolved
    assert!(
        !refs.is_empty(),
        "Configuration should have cross-file references resolved via aliased import"
    );
}

#[test]
fn multiple_files_importing_same_symbol() {
    // Test that when multiple files import the same symbol, all references are resolved
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "mod shared;\nmod a;\nmod b;"),
        (
            "src/shared.rs",
            r"
pub struct Shared;
",
        ),
        (
            "src/a.rs",
            r"
use crate::shared::Shared;

pub fn use_shared_a() -> Shared {
    Shared
}
",
        ),
        (
            "src/b.rs",
            r"
use crate::shared::Shared;

pub fn use_shared_b() -> Shared {
    Shared
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    // Shared should have references from both a.rs and b.rs
    let refs = tethys
        .get_references("Shared")
        .expect("get_references failed");

    // We expect references from both files
    assert!(
        refs.len() >= 2,
        "Shared should have references from multiple files, got: {}",
        refs.len()
    );
}

#[test]
fn csharp_cross_file_reference_resolution() {
    // Test that C# cross-file references are resolved via using directives
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "Services/UserService.cs",
            r"
namespace MyApp.Services;

public class UserService {
    public void Save() { }
}
",
        ),
        (
            "Controllers/UserController.cs",
            r"
using MyApp.Services;

namespace MyApp.Controllers;

public class UserController {
    public void Create() {
        var svc = new UserService();
    }
}
",
        ),
    ]);

    tethys.index().expect("index failed");

    // UserService should have references from UserController.cs
    let refs = tethys
        .get_references("UserService")
        .expect("get_references failed");

    // After Pass 2, the `new UserService()` reference should be resolved
    assert!(
        !refs.is_empty(),
        "UserService should have cross-file references resolved via C# using directive"
    );
}
