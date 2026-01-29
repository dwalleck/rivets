//! `tethys reachable` command implementation.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use colored::Colorize;
use tethys::{ReachabilityDirection, ReachabilityResult, ReachablePath, Tethys};

/// Maximum symbols to display per depth level.
const MAX_PER_DEPTH: usize = 15;

/// Run the reachable command.
pub fn run(
    workspace: &Path,
    symbol: &str,
    direction: &str,
    max_depth: Option<usize>,
) -> Result<(), tethys::Error> {
    let tethys = Tethys::new(workspace)?;

    let result = match direction.to_lowercase().as_str() {
        "forward" | "f" => tethys.get_forward_reachable(symbol, max_depth)?,
        "backward" | "b" => tethys.get_backward_reachable(symbol, max_depth)?,
        _ => {
            return Err(tethys::Error::Config(format!(
                "Invalid direction '{direction}'. Use 'forward' (or 'f') or 'backward' (or 'b')."
            )));
        }
    };

    print_reachability_result(&result);

    Ok(())
}

/// Print reachability analysis results.
fn print_reachability_result(result: &ReachabilityResult) {
    let direction_desc = match result.direction {
        ReachabilityDirection::Forward => "can reach",
        ReachabilityDirection::Backward => "can be reached from",
    };

    if result.is_empty() {
        println!(
            "No symbols {} \"{}\" (max depth: {})",
            direction_desc,
            result.source.qualified_name.cyan(),
            result.max_depth
        );
        return;
    }

    let direction_title = match result.direction {
        ReachabilityDirection::Forward => "Forward reachability",
        ReachabilityDirection::Backward => "Backward reachability",
    };

    println!(
        "{} from \"{}\":",
        direction_title.white().bold(),
        result.source.qualified_name.cyan().bold()
    );
    println!();

    // Group by depth for clearer output
    print_reachable_by_depth(&result.reachable, result.max_depth);

    // Summary
    println!();
    let unique_files: HashSet<_> = result.reachable.iter().map(|r| &r.target.file_id).collect();
    println!(
        "{}: {} symbols across {} files (max depth: {})",
        "Summary".dimmed(),
        result.reachable_count().to_string().green(),
        unique_files.len(),
        result.max_depth
    );
}

/// Print reachable symbols grouped by depth level.
fn print_reachable_by_depth(reachable: &[ReachablePath], max_depth: usize) {
    // Group by depth
    let mut by_depth: HashMap<usize, Vec<&ReachablePath>> = HashMap::new();
    for r in reachable {
        by_depth.entry(r.depth).or_default().push(r);
    }

    // Print by depth, starting from 1
    for depth in 1..=max_depth {
        if let Some(paths) = by_depth.get(&depth) {
            let depth_label = if depth == 1 { "direct" } else { "transitive" };
            println!(
                "  {} {} ({}):",
                format!("Depth {depth}").yellow(),
                depth_label.dimmed(),
                paths.len()
            );

            // Sort by qualified name for deterministic output
            let mut sorted_paths: Vec<_> = paths.iter().collect();
            sorted_paths.sort_by_key(|p| &p.target.qualified_name);

            for path in sorted_paths.iter().take(MAX_PER_DEPTH) {
                // Show symbol and its file
                println!(
                    "    {} {} {}",
                    "•".dimmed(),
                    path.target.qualified_name.white(),
                    format!("({}:{})", path.target.file_id, path.target.line).dimmed()
                );
            }

            if paths.len() > MAX_PER_DEPTH {
                println!(
                    "    {} ... and {} more at depth {depth}",
                    "•".dimmed(),
                    paths.len() - MAX_PER_DEPTH
                );
            }
        }
    }
}
