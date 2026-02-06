//! `tethys callers` command implementation.

use std::collections::HashSet;
use std::path::Path;

use colored::Colorize;
use tethys::Tethys;

use super::display::print_callers_by_file;
use super::ensure_lsp_if_requested;

/// Run the callers command.
pub fn run(
    workspace: &Path,
    symbol: &str,
    transitive: bool,
    lsp: bool,
) -> Result<(), tethys::Error> {
    ensure_lsp_if_requested(lsp)?;

    let tethys = Tethys::new(workspace)?;

    if transitive {
        // Use get_symbol_impact for transitive callers
        let impact = tethys.get_symbol_impact(symbol, None)?;

        if impact.direct_dependents.is_empty() && impact.transitive_dependents.is_empty() {
            println!("No callers found for \"{}\"", symbol.cyan());
            return Ok(());
        }

        println!(
            "Callers of \"{}\" (including transitive):",
            symbol.cyan().bold()
        );
        println!();

        // Combine direct and transitive for display
        let all_callers: Vec<_> = impact
            .direct_dependents
            .iter()
            .chain(impact.transitive_dependents.iter())
            .cloned()
            .collect();
        let unique_files: HashSet<_> = all_callers.iter().map(|c| &c.file).collect();
        print_callers_by_file(&all_callers);

        let total = impact.direct_dependents.len() + impact.transitive_dependents.len();
        println!();
        println!(
            "{}: {} direct, {} transitive",
            "Total".dimmed(),
            impact.direct_dependents.len().to_string().green(),
            impact.transitive_dependents.len().to_string().yellow()
        );
        println!(
            "{}: {} callers across {} files",
            "Summary".dimmed(),
            total,
            unique_files.len()
        );
    } else {
        // Direct callers only - use LSP if requested
        let callers = if lsp {
            tethys.get_callers_with_lsp(symbol)?
        } else {
            tethys.get_callers(symbol)?
        };

        if callers.is_empty() {
            println!("No callers found for \"{}\"", symbol.cyan());
            return Ok(());
        }

        let mode_suffix = if lsp { " (with LSP)" } else { "" };
        println!("Callers of \"{}\"{}:", symbol.cyan().bold(), mode_suffix);
        println!();

        let unique_files: HashSet<_> = callers.iter().map(|c| &c.file).collect();
        print_callers_by_file(&callers);

        println!();
        println!(
            "{}: {} direct callers across {} files",
            "Total".dimmed(),
            callers.len().to_string().green(),
            unique_files.len()
        );
    }

    Ok(())
}
