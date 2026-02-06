//! Error path integration tests for Tethys indexing.
//!
//! Tests that the parser handles malformed, empty, and edge-case files
//! without panicking and returns proper error information.

use std::fs;
use tempfile::TempDir;
use tethys::{IndexErrorKind, Tethys};

/// Create a temporary workspace with the given files.
fn workspace_with_files(files: &[(&str, &str)]) -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("should create temp dir");

    for (path, content) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("should create parent dirs");
        }
        fs::write(&full_path, content).expect("should write file");
    }

    let tethys = Tethys::new(dir.path()).expect("should create Tethys");
    (dir, tethys)
}

// === Empty file tests ===

#[test]
fn empty_rust_file_indexes_with_zero_symbols() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/empty.rs", "")]);

    let stats = tethys.index().expect("index should succeed");

    assert_eq!(stats.files_indexed, 1, "empty file should still be indexed");
    assert_eq!(stats.symbols_found, 0, "empty file has no symbols");
    assert!(
        stats.errors.is_empty(),
        "empty file should not produce errors"
    );
}

#[test]
fn whitespace_only_rust_file_indexes_with_zero_symbols() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/blank.rs", "   \n\n  \t\n")]);

    let stats = tethys.index().expect("index should succeed");

    assert_eq!(stats.files_indexed, 1);
    assert_eq!(stats.symbols_found, 0);
    assert!(stats.errors.is_empty());
}

// === Malformed Rust file tests ===

#[test]
fn rust_file_with_syntax_errors_does_not_panic() {
    let malformed = r"
fn incomplete(
    // missing closing paren and body
struct Orphan {
    field: !!!invalid_type,
}
";
    let (_dir, mut tethys) = workspace_with_files(&[("src/bad.rs", malformed)]);

    // Should not panic - tree-sitter handles malformed input gracefully
    let stats = tethys
        .index()
        .expect("index should succeed even with malformed files");

    // Tree-sitter may extract partial symbols from malformed input
    assert_eq!(
        stats.files_indexed, 1,
        "malformed file should still be indexed"
    );
}

#[test]
fn truncated_rust_file_does_not_panic() {
    // Abruptly truncated in the middle of a function
    let truncated = r#"
pub fn process(data: &[u8]) -> Result<Vec<String>, Error> {
    let mut results = Vec::new();
    for item in data {
        if *item > 0 {
            results.push(format!("val: {}", item));
"#;
    let (_dir, mut tethys) = workspace_with_files(&[("src/truncated.rs", truncated)]);

    let stats = tethys
        .index()
        .expect("index should succeed with truncated files");

    // The file is still indexable; tree-sitter recovers from errors
    assert_eq!(stats.files_indexed, 1);
}

#[test]
fn rust_file_with_deeply_nested_syntax_errors() {
    let deeply_broken = r"
mod outer {
    mod inner {
        fn broken() -> {{{{}}}}} {
            let x = @#$%;
        }
    }
}
pub fn valid() -> i32 { 42 }
";
    let (_dir, mut tethys) = workspace_with_files(&[("src/nested_errors.rs", deeply_broken)]);

    let stats = tethys.index().expect("index should succeed");

    // Should at least index the file and possibly extract `valid`
    assert_eq!(stats.files_indexed, 1);
}

// === Non-UTF-8 content tests ===

#[test]
fn non_utf8_rust_file_reports_encoding_error() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let file_path = dir.path().join("src/binary.rs");
    fs::create_dir_all(file_path.parent().unwrap()).expect("should create dirs");

    // Write raw bytes that are not valid UTF-8
    let invalid_utf8: Vec<u8> = vec![
        0x66, 0x6E, 0x20, 0x62, 0x61, 0x64, 0x28, 0x29, // "fn bad()"
        0x20, 0x7B, 0x0A, // " {\n"
        0xFF, 0xFE, 0x80, 0x81, // invalid UTF-8 bytes
        0x0A, 0x7D, // "\n}"
    ];
    fs::write(&file_path, invalid_utf8).expect("should write binary data");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys
        .index()
        .expect("index should succeed (errors are non-fatal)");

    // The file should fail to parse due to encoding, but indexing continues
    assert!(
        !stats.errors.is_empty(),
        "non-UTF-8 file should produce an error"
    );

    // Non-UTF-8 content produces Error::Parser which maps to ParseFailed
    let parse_errors: Vec<_> = stats
        .errors
        .iter()
        .filter(|e| e.kind == IndexErrorKind::ParseFailed)
        .collect();
    assert!(
        !parse_errors.is_empty(),
        "should report a parse error for non-UTF-8 file, got: {:?}",
        stats.errors.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
}

// === Multiple error files mixed with valid files ===

#[test]
fn mixed_valid_and_invalid_files_indexes_valid_ones() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // Write a valid Rust file
    let valid_path = dir.path().join("src/good.rs");
    fs::create_dir_all(valid_path.parent().unwrap()).expect("should create dirs");
    fs::write(&valid_path, "pub fn hello() -> &'static str { \"hi\" }\n")
        .expect("should write valid file");

    // Write a non-UTF-8 file
    let bad_path = dir.path().join("src/bad.rs");
    fs::write(&bad_path, [0xFF, 0xFE, 0x80]).expect("should write bad file");

    // Write another valid file
    let valid2_path = dir.path().join("src/also_good.rs");
    fs::write(&valid2_path, "pub struct Config { pub name: String }\n")
        .expect("should write valid file");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    // The two valid files should be indexed successfully
    assert!(
        stats.files_indexed >= 2,
        "at least 2 valid files should be indexed, got {}",
        stats.files_indexed
    );

    // The bad file should produce an error
    assert!(
        !stats.errors.is_empty(),
        "non-UTF-8 file should produce an error"
    );

    // The valid symbols should still be findable
    assert!(
        stats.symbols_found >= 2,
        "should find symbols from valid files, got {}",
        stats.symbols_found
    );
}

// === Comment-only file ===

#[test]
fn comment_only_rust_file_indexes_with_zero_symbols() {
    let comment_only = r"
// This file has only comments
// No actual code symbols

/*
 * Block comment too
 */

/// Doc comment without any following item
";
    let (_dir, mut tethys) = workspace_with_files(&[("src/comments.rs", comment_only)]);

    let stats = tethys.index().expect("index should succeed");

    assert_eq!(stats.files_indexed, 1);
    assert_eq!(stats.symbols_found, 0, "comment-only file has no symbols");
    assert!(stats.errors.is_empty());
}

// === Very large symbol name ===

#[test]
fn rust_file_with_very_long_identifier() {
    let long_name = "a".repeat(1000);
    let source = format!("pub fn {long_name}() {{}}\n");
    let (_dir, mut tethys) = workspace_with_files(&[("src/long.rs", &source)]);

    let stats = tethys.index().expect("index should succeed");

    assert_eq!(stats.files_indexed, 1);
    assert_eq!(
        stats.symbols_found, 1,
        "should extract the long-named function"
    );
}

// === File with only attributes and no items ===

#[test]
fn rust_file_with_only_attributes() {
    let attrs_only = r"
#![allow(dead_code)]
#![cfg_attr(test, allow(unused))]
";
    let (_dir, mut tethys) = workspace_with_files(&[("src/attrs.rs", attrs_only)]);

    let stats = tethys.index().expect("index should succeed");

    assert_eq!(stats.files_indexed, 1);
    assert_eq!(
        stats.symbols_found, 0,
        "attributes-only file has no symbols"
    );
    assert!(stats.errors.is_empty());
}
