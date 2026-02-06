//! `tethys affected-tests` command implementation.

use std::path::{Path, PathBuf};

use colored::Colorize;
use tethys::Tethys;
use tracing::{debug, warn};

/// Run the affected-tests command.
pub fn run(workspace: &Path, files: &[String], names_only: bool) -> Result<(), tethys::Error> {
    debug!(workspace = %workspace.display(), "Opening tethys database");
    let tethys = Tethys::new(workspace)?;

    // Convert file strings to PathBuf
    let changed_files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();

    if changed_files.is_empty() {
        warn!("No files specified for affected-tests query");
        eprintln!("{}: no files specified", "warning".yellow());
        return Ok(());
    }

    debug!(
        file_count = changed_files.len(),
        files = ?changed_files,
        "Querying affected tests"
    );
    let affected = tethys.get_affected_tests(&changed_files)?;
    debug!(affected_count = affected.len(), "Found affected tests");

    if affected.is_empty() {
        if !names_only {
            println!("No tests affected by changes to the specified files.");
        }
        return Ok(());
    }

    if names_only {
        // Machine-readable output: one test name per line
        for test in &affected {
            println!("{}", test.qualified_name);
        }
    } else {
        // Human-readable output
        println!(
            "Tests affected by changes to {} file(s):",
            changed_files.len().to_string().cyan()
        );
        println!();

        // Group tests by file for nicer display
        let mut tests_by_file: std::collections::HashMap<tethys::FileId, Vec<&tethys::Symbol>> =
            std::collections::HashMap::new();
        for test in &affected {
            tests_by_file.entry(test.file_id).or_default().push(test);
        }

        let file_count = tests_by_file.len();

        // Get file paths for display
        for (file_id, tests) in &tests_by_file {
            let file_display = match tethys.get_file_by_id(*file_id) {
                Ok(Some(file)) => file.path.display().to_string(),
                Ok(None) => {
                    warn!(file_id = %file_id, "File not found in database");
                    format!("(unknown file_id: {file_id})")
                }
                Err(e) => {
                    warn!(file_id = %file_id, error = %e, "Failed to look up file");
                    format!("(file_id: {file_id})")
                }
            };
            println!("  {}:", file_display.white().bold());
            for test in tests {
                println!("    {} {}", "-".dimmed(), test.qualified_name.green());
            }
            println!();
        }

        println!(
            "{}: {} test(s) across {} file(s)",
            "Total".dimmed(),
            affected.len().to_string().green(),
            file_count
        );
    }

    Ok(())
}
