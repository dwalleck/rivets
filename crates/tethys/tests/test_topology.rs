//! Integration tests for test topology feature.
//!
//! Tests the detection of test functions and the "affected tests" query.

// Allow clippy pedantic warnings that are acceptable in test code
#![allow(
    clippy::needless_raw_string_hashes,
    clippy::doc_markdown,
    clippy::uninlined_format_args
)]

use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use tethys::Tethys;

/// Create a temporary workspace with the given files.
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

mod rust_test_detection {
    use super::*;

    #[test]
    fn detects_standard_test_attribute() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 1, "should find 1 test");
        assert_eq!(tests[0].name, "test_add");
        assert!(tests[0].is_test, "should be marked as test");
    }

    #[test]
    fn detects_tokio_test_attribute() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
async fn fetch_data() -> String {
    "data".to_string()
}

#[tokio::test]
async fn test_fetch_data() {
    let data = fetch_data().await;
    assert_eq!(data, "data");
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 1, "should find 1 test");
        assert_eq!(tests[0].name, "test_fetch_data");
        assert!(tests[0].is_test, "should be marked as test");
    }

    #[test]
    fn detects_rstest_attribute() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

#[rstest]
fn test_multiply() {
    assert_eq!(multiply(2, 3), 6);
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 1, "should find 1 test");
        assert_eq!(tests[0].name, "test_multiply");
        assert!(tests[0].is_test, "should be marked as test");
    }

    #[test]
    fn non_test_functions_not_marked() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
pub fn regular_function() -> i32 {
    42
}

pub fn another_function() {
    // nothing
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert!(tests.is_empty(), "should find no tests");
    }

    #[test]
    fn detects_multiple_tests_in_file() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }
pub fn sub(a: i32, b: i32) -> i32 { a - b }

#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}

#[test]
fn test_sub() {
    assert_eq!(sub(5, 3), 2);
}

#[test]
fn test_combined() {
    assert_eq!(add(sub(5, 3), 1), 3);
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 3, "should find 3 tests");
        let test_names: Vec<&str> = tests.iter().map(|t| t.name.as_str()).collect();
        assert!(test_names.contains(&"test_add"));
        assert!(test_names.contains(&"test_sub"));
        assert!(test_names.contains(&"test_combined"));
    }

    #[test]
    fn detects_tests_across_multiple_files() {
        let (_dir, mut tethys) = workspace_with_files(&[
            (
                "src/math.rs",
                r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}
"#,
            ),
            (
                "src/strings.rs",
                r#"
pub fn concat(a: &str, b: &str) -> String {
    format!("{}{}", a, b)
}

#[test]
fn test_concat() {
    assert_eq!(concat("hello", "world"), "helloworld");
}
"#,
            ),
        ]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 2, "should find 2 tests");
    }
}

mod csharp_test_detection {
    use super::*;

    #[test]
    fn detects_nunit_test_attribute() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/Tests.cs",
            r#"
using NUnit.Framework;

public class CalculatorTests
{
    [Test]
    public void TestAdd()
    {
        Assert.AreEqual(5, 2 + 3);
    }
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 1, "should find 1 test");
        assert_eq!(tests[0].name, "TestAdd");
        assert!(tests[0].is_test, "should be marked as test");
    }

    #[test]
    fn detects_xunit_fact_attribute() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/Tests.cs",
            r#"
using Xunit;

public class StringTests
{
    [Fact]
    public void TestConcat()
    {
        Assert.Equal("ab", "a" + "b");
    }
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 1, "should find 1 test");
        assert_eq!(tests[0].name, "TestConcat");
        assert!(tests[0].is_test, "should be marked as test");
    }

    #[test]
    fn detects_xunit_theory_attribute() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/Tests.cs",
            r#"
using Xunit;

public class MathTests
{
    [Theory]
    public void TestMultiply(int a, int b, int expected)
    {
        Assert.Equal(expected, a * b);
    }
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 1, "should find 1 test");
        assert_eq!(tests[0].name, "TestMultiply");
        assert!(tests[0].is_test, "should be marked as test");
    }

    #[test]
    fn detects_mstest_testmethod_attribute() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/Tests.cs",
            r#"
using Microsoft.VisualStudio.TestTools.UnitTesting;

[TestClass]
public class ServiceTests
{
    [TestMethod]
    public void TestService()
    {
        Assert.IsTrue(true);
    }
}
"#,
        )]);

        tethys.index().expect("index failed");
        let tests = tethys.get_test_symbols().expect("get_test_symbols failed");

        assert_eq!(tests.len(), 1, "should find 1 test");
        assert_eq!(tests[0].name, "TestService");
        assert!(tests[0].is_test, "should be marked as test");
    }
}

mod affected_tests {
    use super::*;

    #[test]
    fn returns_empty_for_no_changes() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}
"#,
        )]);

        tethys.index().expect("index failed");
        let affected = tethys
            .get_affected_tests(&[])
            .expect("get_affected_tests failed");

        assert!(affected.is_empty(), "should have no affected tests");
    }

    #[test]
    fn returns_empty_for_unknown_file() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}
"#,
        )]);

        tethys.index().expect("index failed");
        let affected = tethys
            .get_affected_tests(&[PathBuf::from("nonexistent.rs")])
            .expect("get_affected_tests failed");

        assert!(
            affected.is_empty(),
            "should have no affected tests for unknown file"
        );
    }

    #[test]
    fn finds_tests_in_changed_file() {
        let (_dir, mut tethys) = workspace_with_files(&[(
            "src/lib.rs",
            r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}
"#,
        )]);

        tethys.index().expect("index failed");
        let affected = tethys
            .get_affected_tests(&[PathBuf::from("src/lib.rs")])
            .expect("get_affected_tests failed");

        // The test is in the changed file, so it should be affected
        // Note: this depends on the file_depends_on_any logic
        // If the test file itself is changed, any tests in it are affected
        assert_eq!(affected.len(), 1, "should find 1 affected test");
        assert_eq!(affected[0].name, "test_add");
    }

    /// Test that transitive dependencies are detected.
    ///
    /// Dependency chain: core.rs → helpers.rs → test_helpers.rs
    /// When core.rs changes, tests in test_helpers.rs should be affected
    /// because test_helpers.rs transitively depends on core.rs.
    #[test]
    fn finds_tests_with_transitive_dependencies() {
        let (_dir, mut tethys) = workspace_with_files(&[
            // lib.rs with module declarations - required for crate:: imports to resolve
            (
                "src/lib.rs",
                r#"
pub mod core;
pub mod helpers;
pub mod test_helpers;
"#,
            ),
            // Core module - the file we'll mark as changed
            (
                "src/core.rs",
                r#"
pub fn core_add(a: i32, b: i32) -> i32 {
    a + b
}
"#,
            ),
            // Helpers module - imports and uses core
            (
                "src/helpers.rs",
                r#"
use crate::core::core_add;

pub fn helper_sum(values: &[i32]) -> i32 {
    values.iter().fold(0, |acc, &x| core_add(acc, x))
}
"#,
            ),
            // Test module - imports helpers (transitive dependency on core)
            // Note: We must call the imported function outside macros for tree-sitter
            // to recognize it as a reference (macros are not expanded during parsing)
            (
                "src/test_helpers.rs",
                r#"
use crate::helpers::helper_sum;

#[test]
fn test_helper_sum() {
    let result = helper_sum(&[1, 2, 3]);
    assert!(result == 6);
}

#[test]
fn test_helper_sum_empty() {
    let result = helper_sum(&[]);
    assert!(result == 0);
}
"#,
            ),
        ]);

        tethys.index().expect("index failed");

        // DEBUG: Check that file dependencies were created
        let helpers_deps = tethys
            .get_dependencies(std::path::Path::new("src/helpers.rs"))
            .expect("get_dependencies for helpers.rs");
        let test_helpers_deps = tethys
            .get_dependencies(std::path::Path::new("src/test_helpers.rs"))
            .expect("get_dependencies for test_helpers.rs");

        // helpers.rs should depend on core.rs
        assert!(
            helpers_deps
                .iter()
                .any(|p| p.to_string_lossy().contains("core.rs")),
            "helpers.rs should depend on core.rs, got: {:?}",
            helpers_deps
        );

        // test_helpers.rs should depend on helpers.rs
        assert!(
            test_helpers_deps
                .iter()
                .any(|p| p.to_string_lossy().contains("helpers.rs")),
            "test_helpers.rs should depend on helpers.rs, got: {:?}",
            test_helpers_deps
        );

        // Change core.rs - tests in test_helpers.rs should be affected
        // because test_helpers.rs → helpers.rs → core.rs
        let affected = tethys
            .get_affected_tests(&[PathBuf::from("src/core.rs")])
            .expect("get_affected_tests failed");

        // Should find both tests in test_helpers.rs
        assert_eq!(
            affected.len(),
            2,
            "should find 2 tests affected by transitive dependency"
        );

        let test_names: Vec<&str> = affected.iter().map(|t| t.name.as_str()).collect();
        assert!(
            test_names.contains(&"test_helper_sum"),
            "test_helper_sum should be affected"
        );
        assert!(
            test_names.contains(&"test_helper_sum_empty"),
            "test_helper_sum_empty should be affected"
        );
    }

    /// Test that only relevant tests are affected (not all tests).
    #[test]
    fn does_not_affect_unrelated_tests() {
        let (_dir, mut tethys) = workspace_with_files(&[
            // lib.rs with module declarations - required for crate:: imports to resolve
            (
                "src/lib.rs",
                r#"
pub mod module_a;
pub mod module_b;
pub mod test_a;
pub mod test_b;
"#,
            ),
            // Module A - will be changed
            (
                "src/module_a.rs",
                r#"
pub fn func_a() -> i32 { 1 }
"#,
            ),
            // Module B - independent of A
            (
                "src/module_b.rs",
                r#"
pub fn func_b() -> i32 { 2 }
"#,
            ),
            // Test for A - should be affected when A changes
            // Note: Call outside macro for tree-sitter to detect reference
            (
                "src/test_a.rs",
                r#"
use crate::module_a::func_a;

#[test]
fn test_func_a() {
    let result = func_a();
    assert!(result == 1);
}
"#,
            ),
            // Test for B - should NOT be affected when A changes
            (
                "src/test_b.rs",
                r#"
use crate::module_b::func_b;

#[test]
fn test_func_b() {
    let result = func_b();
    assert!(result == 2);
}
"#,
            ),
        ]);

        tethys.index().expect("index failed");

        // Change module_a.rs - only test_a tests should be affected
        let affected = tethys
            .get_affected_tests(&[PathBuf::from("src/module_a.rs")])
            .expect("get_affected_tests failed");

        assert_eq!(affected.len(), 1, "should find exactly 1 affected test");
        assert_eq!(
            affected[0].name, "test_func_a",
            "only test_func_a should be affected"
        );
    }
}
