//! `tethys index` command implementation.

use std::path::Path;

use colored::Colorize;
use tethys::{IndexOptions, Tethys};

use super::ensure_lsp_if_requested;

/// Run the index command.
pub fn run(
    workspace: &Path,
    rebuild: bool,
    lsp: bool,
    lsp_timeout: Option<u64>,
) -> Result<(), tethys::Error> {
    ensure_lsp_if_requested(lsp)?;

    println!("{} {}...", "Indexing".cyan().bold(), workspace.display());

    let mut tethys = Tethys::new(workspace)?;

    // Build options - with_lsp() reads TETHYS_LSP_TIMEOUT env var by default,
    // but CLI arg takes precedence if provided
    let options = if lsp {
        let mut opts = IndexOptions::with_lsp();
        if let Some(timeout) = lsp_timeout {
            opts = opts.lsp_timeout(timeout);
        }
        opts
    } else {
        IndexOptions::default()
    };

    let stats = if rebuild {
        println!("{}", "Rebuilding index from scratch".yellow());
        tethys.rebuild_with_options(options)?
    } else {
        tethys.index_with_options(options)?
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

    if stats.lsp_resolved_count > 0 {
        println!(
            "{}: {} references via LSP",
            "LSP resolved".cyan(),
            stats.lsp_resolved_count
        );
    }

    Ok(())
}
