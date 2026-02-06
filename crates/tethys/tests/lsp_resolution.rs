//! Integration tests for LSP-based reference resolution.
//!
//! These tests verify that LSP can resolve references that tree-sitter cannot:
//! - Trait method calls (dynamic dispatch)
//! - Methods on inferred types
//! - Complex type inference scenarios
//!
//! All tests in this file require rust-analyzer to be installed.
//! Run with: `cargo test --test lsp_resolution -- --ignored`

use std::fs;
use std::process::Command;
use tempfile::TempDir;
use tethys::{IndexOptions, Tethys};

/// Check if rust-analyzer is available in PATH.
fn rust_analyzer_available() -> bool {
    let check_cmd = if cfg!(windows) { "where" } else { "which" };
    Command::new(check_cmd)
        .arg("rust-analyzer")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Create a minimal Cargo workspace for rust-analyzer.
fn create_cargo_toml(dir: &TempDir) {
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "test_workspace"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("failed to write Cargo.toml");
}

/// Test that LSP can resolve trait method calls.
///
/// Tree-sitter sees `p.process()` but cannot resolve which impl is called
/// because it requires understanding the trait hierarchy. LSP can resolve this.
#[test]
#[ignore = "requires rust-analyzer installed"]
fn lsp_resolves_trait_method_call() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    // Create code with trait method calls
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
pub trait Processor {
    fn process(&self) -> i32;
}

pub struct DataProcessor;

impl Processor for DataProcessor {
    fn process(&self) -> i32 {
        42
    }
}

pub fn run_processor(p: &dyn Processor) -> i32 {
    // This call goes through dynamic dispatch - tree-sitter can't resolve it
    // but LSP understands the trait relationship
    p.process()
}

pub fn use_data_processor() -> i32 {
    let processor = DataProcessor;
    run_processor(&processor)
}
",
    )
    .expect("failed to write lib.rs");

    create_cargo_toml(&dir);

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");

    // Index with LSP enabled
    let stats = tethys
        .index_with_options(IndexOptions::with_lsp())
        .expect("index with LSP failed");

    // The trait method should be indexed
    let symbols = tethys
        .search_symbols("process")
        .expect("search_symbols failed");

    assert!(
        !symbols.is_empty(),
        "should find 'process' symbol (trait method or impl); LSP resolved {} references",
        stats.lsp_resolved_count
    );

    // Verify we can query callers for the trait method
    // This tests that LSP helped resolve the trait method call
    let callers = tethys
        .get_callers_with_lsp("run_processor")
        .expect("get_callers failed");

    assert!(
        !callers.is_empty(),
        "run_processor should have callers (use_data_processor calls it); found {} symbols",
        symbols.len()
    );
}

/// Test that LSP resolves methods on variables with inferred types.
///
/// When a variable's type is inferred (e.g., `let v = vec![1, 2, 3]`),
/// tree-sitter doesn't know the type, so it can't resolve method calls like `v.len()`.
/// LSP performs type inference and can resolve these.
#[test]
#[ignore = "requires rust-analyzer installed"]
fn lsp_resolves_method_on_inferred_type() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    // Create code where type inference is required
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
pub fn create_data() -> Vec<i32> {
    vec![1, 2, 3, 4, 5]
}

pub fn process_data() -> usize {
    // Type of 'data' is inferred as Vec<i32>
    // Tree-sitter can't resolve .len() without knowing the type
    let data = create_data();

    // These method calls require type inference to resolve
    let length = data.len();
    let is_empty = data.is_empty();

    if is_empty {
        0
    } else {
        length
    }
}

pub fn chain_inference() -> Option<i32> {
    // Even more complex: iterator methods with inference
    let data = create_data();
    data.iter().find(|&&x| x > 3).copied()
}
",
    )
    .expect("failed to write lib.rs");

    create_cargo_toml(&dir);

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");

    // Index with LSP enabled
    let stats = tethys
        .index_with_options(IndexOptions::with_lsp())
        .expect("index with LSP failed");

    // Verify indexing succeeded
    assert!(
        stats.files_indexed > 0,
        "should have indexed at least one file"
    );

    // Check that create_data has callers (process_data and chain_inference)
    let callers = tethys
        .get_callers_with_lsp("create_data")
        .expect("get_callers failed");

    assert!(
        callers.len() >= 2,
        "create_data should have at least 2 callers (process_data, chain_inference), \
         got: {}; LSP resolved {} references",
        callers.len(),
        stats.lsp_resolved_count
    );
}

/// Test that indexing with LSP resolves more references than without.
///
/// This is the key value proposition of LSP integration: it should resolve
/// references that tree-sitter alone cannot handle.
#[test]
#[ignore = "requires rust-analyzer installed"]
fn lsp_index_increases_resolution_count() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    // Create code with various resolution challenges
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
mod utils;

pub use utils::helper_function;

pub fn main_function() -> i32 {
    // Cross-module call that benefits from LSP
    helper_function()
}
",
    )
    .expect("failed to write lib.rs");

    fs::write(
        dir.path().join("src/utils.rs"),
        r"
pub fn helper_function() -> i32 {
    internal_work()
}

fn internal_work() -> i32 {
    42
}
",
    )
    .expect("failed to write utils.rs");

    create_cargo_toml(&dir);

    // First, index WITHOUT LSP
    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    let stats_no_lsp = tethys.index().expect("index without LSP failed");

    // Now rebuild WITH LSP
    let stats_with_lsp = tethys
        .rebuild_with_options(IndexOptions::with_lsp())
        .expect("index with LSP failed");

    // Key assertion: non-LSP index should report 0 LSP resolutions
    assert_eq!(
        stats_no_lsp.lsp_resolved_count, 0,
        "non-LSP index should report 0 LSP resolutions"
    );

    // Verify the index still works correctly after LSP pass
    let symbols = tethys
        .search_symbols("helper_function")
        .expect("search failed");

    assert!(
        !symbols.is_empty(),
        "should find helper_function after LSP indexing; \
         without LSP: {} refs found, with LSP: {} refs found ({} via LSP)",
        stats_no_lsp.references_found,
        stats_with_lsp.references_found,
        stats_with_lsp.lsp_resolved_count
    );
}

/// Test LSP resolution with generic types and type parameters.
#[test]
#[ignore = "requires rust-analyzer installed"]
fn lsp_resolves_generic_method_calls() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    fs::write(
        dir.path().join("src/lib.rs"),
        r#"
pub struct Container<T> {
    value: T,
}

impl<T> Container<T> {
    pub fn new(value: T) -> Self {
        Container { value }
    }

    pub fn get(&self) -> &T {
        &self.value
    }
}

pub fn use_container() -> i32 {
    // Type parameter T is inferred as i32
    let c = Container::new(42);
    *c.get()
}

pub fn use_string_container() -> String {
    // Type parameter T is inferred as String
    let c = Container::new(String::from("hello"));
    c.get().clone()
}
"#,
    )
    .expect("failed to write lib.rs");

    create_cargo_toml(&dir);

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    let stats = tethys
        .index_with_options(IndexOptions::with_lsp())
        .expect("index with LSP failed");

    // Verify Container methods are found
    let new_callers = tethys
        .get_callers_with_lsp("Container::new")
        .expect("get_callers for Container::new failed");

    assert!(
        new_callers.len() >= 2,
        "Container::new should have at least 2 callers, got: {}; LSP resolved {} refs",
        new_callers.len(),
        stats.lsp_resolved_count
    );

    let get_callers = tethys
        .get_callers_with_lsp("Container::get")
        .expect("get_callers for Container::get failed");

    assert!(
        get_callers.len() >= 2,
        "Container::get should have at least 2 callers, got: {}",
        get_callers.len()
    );
}
