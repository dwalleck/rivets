//! `tethys impact` command implementation.

use std::path::Path;

use colored::Colorize;
use tethys::{Impact, Tethys};

use super::display::print_dependents;
use super::ensure_lsp_if_requested;

/// Run the impact command.
pub fn run(
    workspace: &Path,
    target: &str,
    is_symbol: bool,
    depth: Option<u32>,
    lsp: bool,
) -> Result<(), tethys::Error> {
    ensure_lsp_if_requested(lsp)?;

    let tethys = Tethys::new(workspace)?;

    if is_symbol {
        let impact = tethys.get_symbol_impact(target, depth)?;
        println!("Impact analysis for symbol \"{}\":", target.cyan().bold());
        print_impact_analysis(&impact);
    } else {
        let target_path = Path::new(target);
        let impact = tethys.get_impact(target_path, depth)?;
        println!("Impact analysis for {}:", target.cyan().bold());
        print_impact_analysis(&impact);
    }

    Ok(())
}

/// Display impact analysis results.
fn print_impact_analysis(impact: &Impact) {
    println!();

    // Direct dependents
    println!(
        "  {} ({} files):",
        "Direct dependents".white().bold(),
        impact.direct_dependents.len().to_string().green()
    );
    print_dependents(&impact.direct_dependents, "(none)");

    println!();

    // Transitive dependents
    println!(
        "  {} ({} files total):",
        "Transitive dependents".white().bold(),
        impact.transitive_dependents.len().to_string().yellow()
    );
    print_dependents(&impact.transitive_dependents, "(none beyond direct)");
}
