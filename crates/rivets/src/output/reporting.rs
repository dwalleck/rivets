//! Rendering for workspace information and statistics reports.

use std::io::{self, Write};

use crate::reporting::{WorkspaceInformation, WorkspaceStatistics};

use super::{OutputMode, print_json};

/// Print workspace information in the requested output format.
pub fn print_information(information: &WorkspaceInformation, mode: OutputMode) -> io::Result<()> {
    match mode {
        OutputMode::Json => print_json(information),
        OutputMode::Text => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            print_information_text(&mut handle, information)
        }
    }
}

/// Print workspace statistics in the requested output format.
pub fn print_statistics(statistics: &WorkspaceStatistics, mode: OutputMode) -> io::Result<()> {
    match mode {
        OutputMode::Json => print_json(statistics),
        OutputMode::Text => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            print_statistics_text(&mut handle, statistics)
        }
    }
}

fn print_information_text<W: Write>(
    w: &mut W,
    information: &WorkspaceInformation,
) -> io::Result<()> {
    writeln!(w, "Workspace Information")?;
    writeln!(w, "=====================")?;
    writeln!(w)?;
    writeln!(
        w,
        "Workspace:    {}",
        information.workspace_root().display()
    )?;
    writeln!(w, "Database:     {}", information.database_path().display())?;
    writeln!(w, "Config:       {}", information.config_path().display())?;
    writeln!(w, "Storage:      {}", information.storage_backend())?;
    writeln!(w, "Issue prefix: {}", information.issue_prefix())
}

fn print_statistics_text<W: Write>(w: &mut W, statistics: &WorkspaceStatistics) -> io::Result<()> {
    let priority = statistics.by_priority();

    writeln!(w, "Workspace Statistics")?;
    writeln!(w, "====================")?;
    writeln!(w)?;
    writeln!(w, "Total Issues:  {}", statistics.total())?;
    writeln!(w)?;
    writeln!(w, "By Workflow State:")?;
    writeln!(w, "  Open:        {}", statistics.open())?;
    writeln!(w, "  In Progress: {}", statistics.in_progress())?;
    writeln!(w, "  Closed:      {}", statistics.closed())?;
    writeln!(w)?;
    writeln!(w, "Ready to Work: {}", statistics.ready())?;
    writeln!(
        w,
        "Blocked by Dependencies: {}",
        statistics.blocked_by_dependencies()
    )?;
    writeln!(w)?;
    writeln!(w, "By Priority:")?;
    for (label, count) in [
        "P0 (Critical)",
        "P1 (High)",
        "P2 (Medium)",
        "P3 (Low)",
        "P4 (Backlog)",
    ]
    .iter()
    .zip(priority)
    {
        writeln!(w, "  {label}: {count}")?;
    }
    Ok(())
}
