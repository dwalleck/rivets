//! `tethys cycles` command implementation.

use std::path::Path;

use colored::Colorize;
use tethys::Tethys;

/// Run the cycles command.
pub fn run(workspace: &Path) -> Result<(), tethys::Error> {
    let tethys = Tethys::new(workspace)?;

    let cycles = tethys.detect_cycles()?;

    if cycles.is_empty() {
        println!("{}", "No circular dependencies detected.".green());
        return Ok(());
    }

    println!(
        "Found {} circular dependencies:",
        cycles.len().to_string().red().bold()
    );
    println!();

    for (i, cycle) in cycles.iter().enumerate() {
        println!("  {} {}:", "Cycle".yellow().bold(), i + 1);

        // Display cycle as: a -> b -> c -> a
        let mut path_str = cycle
            .files
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(" → ");

        // Add the first file again to show the cycle closes
        if let Some(first) = cycle.files.first() {
            path_str.push_str(" → ");
            path_str.push_str(&first.display().to_string());
        }

        println!("    {}", path_str.dimmed());
    }

    Ok(())
}
