//! Common display utilities for CLI commands.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use colored::Colorize;
use tethys::Dependent;

const MAX_DISPLAY_ITEMS: usize = 10;

/// Display a list of dependent files with optional truncation.
///
/// Shows up to `MAX_DISPLAY_ITEMS` files with bullet points. If there are more,
/// shows "... and N more". If empty, shows the provided `empty_message`.
pub fn print_dependents(dependents: &[Dependent], empty_message: &str) {
    if dependents.is_empty() {
        println!("    {}", empty_message.dimmed());
        return;
    }

    for dep in dependents.iter().take(MAX_DISPLAY_ITEMS) {
        println!("    {} {}", "•".dimmed(), dep.file.display());
    }

    if dependents.len() > MAX_DISPLAY_ITEMS {
        println!(
            "    {} ... and {} more",
            "•".dimmed(),
            dependents.len() - MAX_DISPLAY_ITEMS
        );
    }
}

/// Group callers by file and display symbols used from each file.
///
/// Groups the given dependents by their source file, deduplicates symbols,
/// and prints them in sorted order for deterministic output.
pub fn print_callers_by_file(callers: &[Dependent]) {
    // Group by file, deduplicating symbols
    let mut by_file: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for caller in callers {
        by_file
            .entry(caller.file.clone())
            .or_default()
            .extend(caller.symbols_used.iter().cloned());
    }

    // Sort files for deterministic output
    let mut sorted_files: Vec<_> = by_file.iter().collect();
    sorted_files.sort_by_key(|(path, _)| *path);

    for (file, symbols) in sorted_files {
        println!("  {}:", file.display().to_string().white().bold());
        let mut sorted_symbols: Vec<_> = symbols.iter().collect();
        sorted_symbols.sort();
        for sym in sorted_symbols {
            println!("    {} {}", "•".dimmed(), sym);
        }
    }
}
