//! `tethys impact` command implementation.

use std::path::Path;

use colored::Colorize;
use tethys::{Impact, Tethys};

use super::display::print_dependents;

/// Run the impact command.
pub fn run(
    workspace: &Path,
    target: &str,
    is_symbol: bool,
    depth: Option<u32>,
) -> Result<(), tethys::Error> {
    // TODO: Implement depth-limited transitive analysis
    // Currently, transitive analysis explores the full dependency graph.
    // The --depth flag would limit how many levels of indirection to follow.
    if depth.is_some() {
        eprintln!(
            "{}: --depth flag is not yet implemented; full transitive analysis will be used",
            "warning".yellow()
        );
    }

    let tethys = Tethys::new(workspace)?;

    if is_symbol {
        let impact = tethys.get_symbol_impact(target)?;
        println!("Impact analysis for symbol \"{}\":", target.cyan().bold());
        print_impact_analysis(&impact);
    } else {
        let target_path = Path::new(target);
        let impact = tethys.get_impact(target_path)?;
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
