//! `tethys index` command implementation.

use std::path::Path;

use colored::Colorize;
use tethys::Tethys;

/// Run the index command.
pub fn run(workspace: &Path, rebuild: bool) -> Result<(), tethys::Error> {
    println!("{} {}...", "Indexing".cyan().bold(), workspace.display());

    let mut tethys = Tethys::new(workspace)?;

    let stats = if rebuild {
        println!("{}", "Rebuilding index from scratch".yellow());
        tethys.rebuild()?
    } else {
        tethys.index()?
    };

    // Display results
    println!();
    println!(
        "{} {} files, found {} symbols, {} references",
        "Indexed".green().bold(),
        stats.files_indexed,
        stats.symbols_found,
        stats.references_found
    );
    println!("{}: {:.2?}", "Duration".dimmed(), stats.duration);

    if stats.files_skipped > 0 {
        println!(
            "{}: {} files (unsupported language)",
            "Skipped".yellow(),
            stats.files_skipped
        );
    }

    if !stats.directories_skipped.is_empty() {
        println!(
            "{}: {} directories (permission denied)",
            "Skipped".yellow(),
            stats.directories_skipped.len()
        );
    }

    if !stats.errors.is_empty() {
        println!();
        println!("{} ({}):", "Errors".red().bold(), stats.errors.len());
        for err in stats.errors.iter().take(5) {
            println!("  {} {}: {}", "•".red(), err.path.display(), err.message);
        }
        if stats.errors.len() > 5 {
            println!("  ... and {} more", stats.errors.len() - 5);
        }
    }

    if !stats.unresolved_dependencies.is_empty() {
        println!();
        println!(
            "{}: {} (likely external crates)",
            "Unresolved dependencies".dimmed(),
            stats.unresolved_dependencies.len()
        );
    }

    Ok(())
}
