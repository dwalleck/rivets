//! `tethys panic-points` command implementation.

use std::collections::BTreeMap;
use std::path::Path;

use colored::Colorize;
use tethys::{PanicKind, PanicPoint, Tethys};
use tracing::debug;

/// Run the panic-points command.
///
/// Queries the index for panic points (`.unwrap()` and `.expect()` calls) and displays
/// them in either human-readable or JSON format.
pub fn run(
    workspace: &Path,
    include_tests: bool,
    json: bool,
    file_filter: Option<&str>,
) -> Result<(), tethys::Error> {
    debug!(workspace = %workspace.display(), "Opening tethys database");
    let tethys = Tethys::new(workspace)?;

    let (prod_count, test_count) = tethys.count_panic_points()?;
    debug!(
        include_tests = include_tests,
        file_filter = ?file_filter,
        "Querying panic points"
    );
    let panic_points = tethys.get_panic_points(include_tests, file_filter)?;
    debug!(panic_point_count = panic_points.len(), "Found panic points");

    if json {
        output_json(&panic_points, prod_count, test_count, include_tests)?;
    } else {
        output_human(&panic_points, prod_count, test_count, include_tests);
    }

    Ok(())
}

/// Output panic points in JSON format.
fn output_json(
    panic_points: &[PanicPoint],
    prod_count: usize,
    test_count: usize,
    include_tests: bool,
) -> Result<(), tethys::Error> {
    #[derive(serde::Serialize)]
    struct JsonOutput<'a> {
        summary: JsonSummary,
        panic_points: &'a [PanicPoint],
    }

    #[derive(serde::Serialize)]
    struct JsonSummary {
        production_count: usize,
        test_count: usize,
        include_tests: bool,
    }

    let output = JsonOutput {
        summary: JsonSummary {
            production_count: prod_count,
            test_count,
            include_tests,
        },
        panic_points,
    };

    let json = serde_json::to_string_pretty(&output).map_err(|e| {
        tethys::Error::Internal(format!("Failed to serialize panic points to JSON: {e}"))
    })?;
    println!("{json}");

    Ok(())
}

/// Output panic points in human-readable format.
fn output_human(
    panic_points: &[PanicPoint],
    prod_count: usize,
    test_count: usize,
    include_tests: bool,
) {
    println!("{}", "Panic Points Analysis".cyan().bold());
    println!("{}", "=".repeat(63).dimmed());
    println!();

    println!("{}:", "Summary".white().bold());
    println!(
        "  {:<20}{}",
        "Production code:".dimmed(),
        format!("{prod_count} panic points").green()
    );
    if include_tests {
        println!(
            "  {:<20}{}",
            "Test code:".dimmed(),
            format!("{test_count} panic points").yellow()
        );
    } else {
        println!(
            "  {:<20}{}",
            "Test code:".dimmed(),
            format!("{test_count} (use --include-tests to show)").dimmed()
        );
    }
    println!();

    if panic_points.is_empty() {
        println!("{}", "No panic points found matching the filters.".dimmed());
        return;
    }

    let mut by_kind: BTreeMap<PanicKind, Vec<&PanicPoint>> = BTreeMap::new();
    for point in panic_points {
        by_kind.entry(point.kind).or_default().push(point);
    }

    for (kind, points) in &by_kind {
        let code_type = if include_tests {
            "code"
        } else {
            "production code"
        };

        println!("{}", "-".repeat(62).dimmed());
        println!("{} in {}", kind.to_string().yellow().bold(), code_type);
        println!("{}", "-".repeat(62).dimmed());

        for point in points {
            let test_indicator = if point.is_test {
                " [test]".dimmed().to_string()
            } else {
                String::new()
            };

            println!(
                "  {}:{}  in {}(){}",
                point.path.display().to_string().white(),
                point.line.to_string().cyan(),
                point.containing_symbol.green(),
                test_indicator
            );
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_panic_point(
        path: &str,
        line: u32,
        kind: PanicKind,
        symbol: &str,
        is_test: bool,
    ) -> PanicPoint {
        PanicPoint::new(PathBuf::from(path), line, kind, symbol.to_string(), is_test)
    }

    #[test]
    fn output_human_handles_empty_points() {
        // This test just verifies the function doesn't panic on empty input
        output_human(&[], 0, 0, false);
    }

    #[test]
    fn output_json_produces_valid_json() {
        let points = vec![
            make_panic_point("src/lib.rs", 10, PanicKind::Unwrap, "process", false),
            make_panic_point("src/lib.rs", 20, PanicKind::Expect, "validate", false),
        ];

        // Capture would require more infrastructure; just verify it doesn't error
        let result = output_json(&points, 2, 5, false);
        assert!(result.is_ok());
    }
}
