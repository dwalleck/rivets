//! `tethys stats` command implementation.

use std::path::Path;

use colored::Colorize;
use tethys::{SymbolKind, Tethys};

/// Run the stats command.
#[allow(clippy::too_many_lines)]
pub fn run(workspace: &Path) -> Result<(), tethys::Error> {
    let tethys = Tethys::new(workspace)?;

    // Get database size
    let db_path = tethys.db_path();
    let db_size_str = match std::fs::metadata(db_path) {
        Ok(meta) => format_size(meta.len()),
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                tracing::debug!("Database file not found");
                "not created".to_string()
            }
            std::io::ErrorKind::PermissionDenied => {
                tracing::warn!(path = %db_path.display(), "Permission denied reading database");
                "permission denied".to_string()
            }
            _ => {
                tracing::debug!(error = %e, "Failed to get database file size");
                "size unknown".to_string()
            }
        },
    };

    // Get stats from the database
    let stats = tethys.get_stats()?;

    println!("{}", "Tethys Index Statistics".cyan().bold());
    println!();

    // Database info
    println!(
        "  {}: {} ({})",
        "Database".white().bold(),
        db_path.display(),
        db_size_str
    );
    println!();

    // File counts
    println!(
        "  {}: {} total",
        "Files".white().bold(),
        stats.file_count.to_string().green()
    );
    // Sort languages for deterministic output
    let mut sorted_langs: Vec<_> = stats.files_by_language.iter().collect();
    sorted_langs.sort_by_key(|(lang, _)| match lang {
        tethys::Language::Rust => "Rust",
        tethys::Language::CSharp => "C#",
    });

    for (lang, count) in sorted_langs {
        let lang_name = match lang {
            tethys::Language::Rust => "Rust",
            tethys::Language::CSharp => "C#",
        };
        println!("    {}: {}", lang_name.dimmed(), count);
    }
    println!();

    // Symbol counts
    println!(
        "  {}: {} total",
        "Symbols".white().bold(),
        stats.symbol_count.to_string().green()
    );

    // Sort by count descending, then by kind for deterministic output
    let mut kind_counts: Vec<_> = stats.symbols_by_kind.into_iter().collect();
    kind_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    for (kind, count) in kind_counts {
        let kind_name = match kind {
            SymbolKind::Function => "Functions",
            SymbolKind::Method => "Methods",
            SymbolKind::Struct => "Structs",
            SymbolKind::Class => "Classes",
            SymbolKind::Enum => "Enums",
            SymbolKind::Trait => "Traits",
            SymbolKind::Interface => "Interfaces",
            SymbolKind::Const => "Constants",
            SymbolKind::Static => "Statics",
            SymbolKind::Module => "Modules",
            SymbolKind::TypeAlias => "Type Aliases",
            SymbolKind::Macro => "Macros",
        };
        println!("    {}: {}", kind_name.dimmed(), count);
    }
    println!();

    // Reference and dependency counts
    println!(
        "  {}: {}",
        "References".white().bold(),
        stats.reference_count.to_string().green()
    );
    println!(
        "  {}: {}",
        "File Dependencies".white().bold(),
        stats.file_dependency_count.to_string().green()
    );

    // Warn about skipped entries (possible version mismatch)
    if stats.skipped_unknown_languages > 0 || stats.skipped_unknown_kinds > 0 {
        println!();
        println!(
            "  {}: Database contains unrecognized entries",
            "Warning".yellow().bold()
        );
        if stats.skipped_unknown_languages > 0 {
            println!(
                "    {} files with unknown language",
                stats.skipped_unknown_languages.to_string().yellow()
            );
        }
        if stats.skipped_unknown_kinds > 0 {
            println!(
                "    {} symbols with unknown kind",
                stats.skipped_unknown_kinds.to_string().yellow()
            );
        }
        println!(
            "    {}",
            "Database may be from a newer Tethys version. Consider reindexing.".dimmed()
        );
    }

    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
