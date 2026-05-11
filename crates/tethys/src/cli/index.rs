//! `tethys index` command implementation.

use std::io::{self, Write};
use std::path::Path;

use colored::Colorize;
use tethys::{ArchPhaseResult, IndexOptions, Tethys};

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

    let total_lsp_resolved = stats.total_lsp_resolved();
    if total_lsp_resolved > 0 {
        println!(
            "{}: {total_lsp_resolved} references via LSP",
            "LSP resolved".cyan(),
        );
    }

    print_arch_phase_result(&mut io::stdout().lock(), stats.arch_phase.as_ref())
        .map_err(tethys::Error::Io)?;
    print_lsp_session_errors(&stats.lsp_sessions);

    Ok(())
}

/// Print architecture-phase outcome to `out`, if any. Success path is silent.
///
/// Takes a `Write` sink so callers can unit-test all three output paths
/// (Failed → warning, Completed → silent, None → silent) without capturing
/// stdout.
fn print_arch_phase_result<W: Write>(
    out: &mut W,
    arch_phase: Option<&ArchPhaseResult>,
) -> io::Result<()> {
    match arch_phase {
        Some(ArchPhaseResult::Completed(arch)) => {
            // Keep the success case silent — rivets-tuph tracks surfacing
            // the package count in `tethys index` output. We don't want to
            // drop the no-output behavior callers may scrape.
            tracing::debug!(
                packages = arch.packages_recorded,
                files = arch.files_assigned,
                "architecture phase summary"
            );
        }
        Some(ArchPhaseResult::Failed(err)) => {
            writeln!(out)?;
            writeln!(
                out,
                "  {}: architecture phase failed — coupling metrics unavailable",
                "Warning".yellow().bold()
            )?;
            writeln!(out, "  {}", err.dimmed())?;
        }
        None => {
            // Phase didn't run (e.g., default state) — nothing to print.
        }
    }
    Ok(())
}

/// Print LSP session errors, if any.
fn print_lsp_session_errors(sessions: &[tethys::LspSessionResult]) {
    for session in sessions {
        if session.has_errors() {
            println!();
            match &session.outcome {
                tethys::LspOutcome::ServerUnavailable {
                    reason,
                    install_hint,
                } => {
                    println!(
                        "{}: {} - {reason}",
                        "LSP error".red(),
                        session.language.as_str()
                    );
                    println!("  {}: {install_hint}", "hint".dimmed());
                }
                tethys::LspOutcome::Completed(s) => {
                    for err in &s.errors {
                        println!(
                            "{}: {} - {err}",
                            "LSP error".red(),
                            session.language.as_str()
                        );
                    }
                }
                tethys::LspOutcome::NothingToResolve => {}
            }
        }
    }
}

#[cfg(test)]
mod arch_phase_print_tests {
    use super::*;
    use tethys::{ArchPhaseResult, ArchStats};

    #[test]
    fn failed_path_writes_warning_with_error_text() {
        colored::control::set_override(false);
        let mut buf: Vec<u8> = Vec::new();
        let result = ArchPhaseResult::Failed("simulated db corruption".into());
        print_arch_phase_result(&mut buf, Some(&result)).expect("write");
        let out = String::from_utf8(buf).expect("utf-8");
        assert!(out.contains("Warning"), "should include the Warning label");
        assert!(
            out.contains("simulated db corruption"),
            "should include the error Display form"
        );
    }

    #[test]
    fn completed_path_writes_nothing_to_stdout() {
        let mut buf: Vec<u8> = Vec::new();
        let result = ArchPhaseResult::Completed(ArchStats::default());
        print_arch_phase_result(&mut buf, Some(&result)).expect("write");
        assert!(
            buf.is_empty(),
            "completed path should be silent on stdout (rivets-tuph follow-up)"
        );
    }

    #[test]
    fn none_writes_nothing() {
        let mut buf: Vec<u8> = Vec::new();
        print_arch_phase_result(&mut buf, None).expect("write");
        assert!(buf.is_empty());
    }
}
