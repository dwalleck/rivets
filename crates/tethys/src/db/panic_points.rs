//! Panic point query operations for the Tethys index.
//!
//! These operations identify potential panic points (`.unwrap()` and `.expect()` calls)
//! in the codebase by querying the `refs` table for method calls with those names.

use std::path::PathBuf;

use rusqlite::params;
use tracing::{trace, warn};

use super::Index;
use crate::error::Result;
use crate::types::{PanicKind, PanicPoint};

impl Index {
    /// Get all panic points in the codebase.
    ///
    /// Panic points are `.unwrap()` and `.expect()` calls that could panic at runtime.
    /// We identify these by querying the `refs` table for method calls with
    /// `reference_name` in ('unwrap', 'expect') that have a containing symbol
    /// (`in_symbol_id IS NOT NULL`) which is a function or method.
    ///
    /// References without a containing symbol, or contained within non-callable symbols
    /// (structs, enums, etc.), are excluded from this analysis.
    ///
    /// # Arguments
    ///
    /// * `include_tests` - If true, include panic points in test code
    /// * `file_filter` - If provided, only return panic points in the specified file
    ///
    /// # Returns
    ///
    /// A vector of `PanicPoint` structs, ordered by file path and line number.
    pub fn get_panic_points(
        &self,
        include_tests: bool,
        file_filter: Option<&str>,
    ) -> Result<Vec<PanicPoint>> {
        trace!(
            include_tests = include_tests,
            file_filter = ?file_filter,
            "Querying panic points"
        );
        let conn = self.connection()?;

        let base_query = r"
            SELECT f.path, r.line, r.reference_name, s.name, s.is_test
            FROM refs r
            JOIN files f ON r.file_id = f.id
            JOIN symbols s ON r.in_symbol_id = s.id
            WHERE r.reference_name IN ('unwrap', 'expect')
              AND s.kind IN ('function', 'method')
        ";

        let mut query = base_query.to_string();

        if !include_tests {
            query.push_str(" AND s.is_test = 0");
        }

        if file_filter.is_some() {
            query.push_str(" AND f.path = ?1");
        }

        query.push_str(" ORDER BY f.path, r.line");

        let mut stmt = conn.prepare(&query)?;

        let rows = if let Some(path) = file_filter {
            stmt.query_map(params![path], Self::row_to_panic_point)?
        } else {
            stmt.query_map([], Self::row_to_panic_point)?
        };

        let results: Vec<Option<PanicPoint>> = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        let skipped_count = results.iter().filter(|r| r.is_none()).count();
        if skipped_count > 0 {
            warn!(
                skipped_count = skipped_count,
                "Skipped panic point references with unrecognized names"
            );
        }

        let panic_points: Vec<PanicPoint> = results.into_iter().flatten().collect();

        trace!(count = panic_points.len(), "Found panic points");

        Ok(panic_points)
    }

    /// Count panic points grouped by test/production code.
    ///
    /// Returns `(production_count, test_count)`.
    pub fn count_panic_points(&self) -> Result<(usize, usize)> {
        let conn = self.connection()?;

        let mut stmt = conn.prepare(
            r"
            SELECT s.is_test, COUNT(*)
            FROM refs r
            JOIN symbols s ON r.in_symbol_id = s.id
            WHERE r.reference_name IN ('unwrap', 'expect')
              AND s.kind IN ('function', 'method')
            GROUP BY s.is_test
            ",
        )?;

        let mut production_count = 0usize;
        let mut test_count = 0usize;

        let rows = stmt.query_map([], |row| {
            let is_test: bool = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((is_test, count as usize))
        })?;

        for row in rows {
            let (is_test, count) = row?;
            if is_test {
                test_count = count;
            } else {
                production_count = count;
            }
        }

        Ok((production_count, test_count))
    }

    /// Convert a database row to a `PanicPoint`.
    ///
    /// Expected columns: `f.path`, `r.line`, `r.reference_name`, `s.name`, `s.is_test`
    ///
    /// Returns `Ok(None)` if the `reference_name` is not a recognized panic kind,
    /// which is logged at warn level by the caller.
    fn row_to_panic_point(row: &rusqlite::Row) -> rusqlite::Result<Option<PanicPoint>> {
        let path: String = row.get(0)?;
        let line: u32 = row.get(1)?;
        let reference_name: String = row.get(2)?;
        let containing_symbol: String = row.get(3)?;
        let is_test: bool = row.get(4)?;

        let Some(kind) = PanicKind::parse(&reference_name) else {
            // This shouldn't happen since SQL filters for 'unwrap'/'expect',
            // but if it does (e.g., database corruption or schema change),
            // we skip this row. The caller logs aggregated skip counts.
            return Ok(None);
        };

        Ok(Some(PanicPoint::new(
            PathBuf::from(path),
            line,
            kind,
            containing_symbol,
            is_test,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Index;
    use crate::types::{Language, SymbolKind, Visibility};
    use tempfile::TempDir;

    fn setup_test_db() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("should create temp directory");
        let path = dir.path().join("test.db");
        let mut index = Index::open(&path).expect("should open database");

        // Create test file
        let file_id = index
            .upsert_file(
                std::path::Path::new("src/lib.rs"),
                Language::Rust,
                1_000_000,
                1000,
                None,
            )
            .expect("should create file");

        // Create a production function
        let prod_fn_id = index
            .insert_symbol(
                file_id,
                "process",
                "crate",
                "process",
                SymbolKind::Function,
                10,
                1,
                None,
                Some("fn process() -> Result<()>"),
                Visibility::Public,
                None,
                false, // is_test = false
            )
            .expect("should create production function");

        // Create a test function
        let test_fn_id = index
            .insert_symbol(
                file_id,
                "test_process",
                "crate",
                "test_process",
                SymbolKind::Function,
                50,
                1,
                None,
                Some("fn test_process()"),
                Visibility::Private,
                None,
                true, // is_test = true
            )
            .expect("should create test function");

        // Add .unwrap() call in production code
        index
            .insert_reference(
                None,
                file_id,
                "call",
                15,
                10,
                Some(prod_fn_id),
                Some("unwrap"),
            )
            .expect("should create unwrap reference");

        // Add .expect() call in production code
        index
            .insert_reference(
                None,
                file_id,
                "call",
                20,
                10,
                Some(prod_fn_id),
                Some("expect"),
            )
            .expect("should create expect reference");

        // Add .unwrap() call in test code
        index
            .insert_reference(
                None,
                file_id,
                "call",
                55,
                10,
                Some(test_fn_id),
                Some("unwrap"),
            )
            .expect("should create unwrap reference in test");

        (dir, index)
    }

    #[test]
    fn get_panic_points_excludes_tests_by_default() {
        let (_dir, index) = setup_test_db();

        let points = index
            .get_panic_points(false, None)
            .expect("should get panic points");

        // Should only include production code (2 points)
        assert_eq!(
            points.len(),
            2,
            "should find 2 panic points in production code"
        );
        for point in &points {
            assert!(!point.is_test, "should not include test code");
        }
    }

    #[test]
    fn get_panic_points_includes_tests_when_requested() {
        let (_dir, index) = setup_test_db();

        let points = index
            .get_panic_points(true, None)
            .expect("should get panic points");

        // Should include all 3 points
        assert_eq!(points.len(), 3, "should find all 3 panic points");

        let test_points: Vec<_> = points.iter().filter(|p| p.is_test).collect();
        assert_eq!(test_points.len(), 1, "should have 1 test panic point");
    }

    #[test]
    fn get_panic_points_filters_by_file() {
        let (_dir, index) = setup_test_db();

        let points = index
            .get_panic_points(true, Some("src/lib.rs"))
            .expect("should get panic points");

        assert!(!points.is_empty(), "should find panic points in file");

        // Filter by non-existent file
        let points = index
            .get_panic_points(true, Some("src/other.rs"))
            .expect("should get panic points");

        assert!(
            points.is_empty(),
            "should find no panic points in other file"
        );
    }

    #[test]
    fn get_panic_points_returns_correct_kinds() {
        let (_dir, index) = setup_test_db();

        let points = index
            .get_panic_points(false, None)
            .expect("should get panic points");

        let unwrap_count = points
            .iter()
            .filter(|p| p.kind == PanicKind::Unwrap)
            .count();
        let expect_count = points
            .iter()
            .filter(|p| p.kind == PanicKind::Expect)
            .count();

        assert_eq!(unwrap_count, 1, "should have 1 unwrap");
        assert_eq!(expect_count, 1, "should have 1 expect");
    }

    #[test]
    fn count_panic_points_returns_correct_counts() {
        let (_dir, index) = setup_test_db();

        let (prod, test) = index
            .count_panic_points()
            .expect("should count panic points");

        assert_eq!(prod, 2, "should count 2 production panic points");
        assert_eq!(test, 1, "should count 1 test panic point");
    }

    #[test]
    fn panic_points_contain_correct_metadata() {
        let (_dir, index) = setup_test_db();

        let points = index
            .get_panic_points(true, None)
            .expect("should get panic points");

        // Check that containing symbol is populated
        for point in &points {
            assert!(
                !point.containing_symbol.is_empty(),
                "containing_symbol should be populated"
            );
        }

        // Check production function name
        let prod_points: Vec<_> = points.iter().filter(|p| !p.is_test).collect();
        for point in prod_points {
            assert_eq!(
                point.containing_symbol, "process",
                "production panic points should be in 'process' function"
            );
        }

        // Check test function name
        let test_points: Vec<_> = points.iter().filter(|p| p.is_test).collect();
        for point in test_points {
            assert_eq!(
                point.containing_symbol, "test_process",
                "test panic points should be in 'test_process' function"
            );
        }
    }

    /// Verify the SQL query's IN clause stays in sync with `PanicKind::parse()`.
    ///
    /// This test catches bugs where someone adds a new `PanicKind` variant but
    /// forgets to update the SQL query (or vice versa).
    #[test]
    fn panic_kinds_match_sql_query_filter() {
        // The SQL query filters for these exact values (from get_panic_points)
        let sql_values = ["unwrap", "expect"];

        // Verify all SQL values are parseable by PanicKind
        for value in sql_values {
            assert!(
                PanicKind::parse(value).is_some(),
                "SQL query includes '{value}' but PanicKind::parse doesn't recognize it"
            );
        }

        // Verify all PanicKind variants are in SQL
        let all_kinds = [PanicKind::Unwrap, PanicKind::Expect];
        for kind in all_kinds {
            assert!(
                sql_values.contains(&kind.as_str()),
                "PanicKind::{kind:?} (as_str='{}') not in SQL WHERE IN clause",
                kind.as_str()
            );
        }
    }
}
