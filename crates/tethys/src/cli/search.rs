//! `tethys search` command implementation.

use std::path::Path;

use colored::Colorize;
use tethys::{SymbolKind, Tethys};

/// Run the search command.
pub fn run(
    workspace: &Path,
    query: &str,
    kind_filter: Option<&str>,
    limit: usize,
) -> Result<(), tethys::Error> {
    let tethys = Tethys::new(workspace)?;

    let mut symbols = tethys.search_symbols(query)?;

    // Filter by kind if specified
    if let Some(kind_str) = kind_filter {
        let target_kind = parse_kind(kind_str).ok_or_else(|| {
            tethys::Error::Config(format!(
                "unknown symbol kind '{kind_str}'. Valid kinds: function, method, struct, class, enum, trait, interface, const, static, module, type_alias, macro"
            ))
        })?;
        symbols.retain(|s| s.kind == target_kind);
    }

    // Limit results
    symbols.truncate(limit);

    if symbols.is_empty() {
        println!("No symbols found matching \"{query}\"");

        // Provide helpful context
        let stats = tethys.get_stats()?;
        if stats.symbol_count == 0 {
            println!(
                "\n{}: The index is empty. Run '{}' to index your workspace.",
                "hint".dimmed(),
                "tethys index".cyan()
            );
        } else if kind_filter.is_some() {
            println!(
                "\n{}: Try searching without the --kind filter, or check available symbol kinds with '{}'.",
                "hint".dimmed(),
                "tethys stats".cyan()
            );
        }
        return Ok(());
    }

    println!(
        "Found {} symbols matching \"{}\":",
        symbols.len().to_string().green().bold(),
        query.cyan()
    );
    println!();

    for sym in &symbols {
        let kind_str = format_kind(sym.kind);

        // Get file path for this symbol
        let file_path = if let Some(f) = tethys.get_file_by_id(sym.file_id)? {
            f.path.display().to_string()
        } else {
            tracing::warn!(
                symbol_id = %sym.id,
                file_id = %sym.file_id,
                symbol_name = %sym.name,
                "Symbol references non-existent file (database inconsistency)"
            );
            "<unknown>".to_string()
        };
        let location = format!("{}:{}", file_path, sym.line);

        // Build qualified name display
        let display_name = if sym.module_path.is_empty() {
            sym.qualified_name.clone()
        } else {
            format!("{}::{}", sym.module_path, sym.qualified_name)
        };

        println!(
            "  {} {} {}",
            display_name.white().bold(),
            format!("({kind_str})").dimmed(),
            format!("- {location}").dimmed()
        );

        // Show signature if available
        if let Some(sig) = &sym.signature {
            println!("    {}", sig.dimmed());
        }
    }

    Ok(())
}

fn parse_kind(s: &str) -> Option<SymbolKind> {
    match s.to_lowercase().as_str() {
        "function" | "fn" => Some(SymbolKind::Function),
        "method" => Some(SymbolKind::Method),
        "struct" => Some(SymbolKind::Struct),
        "class" => Some(SymbolKind::Class),
        "enum" => Some(SymbolKind::Enum),
        "trait" => Some(SymbolKind::Trait),
        "interface" => Some(SymbolKind::Interface),
        "const" => Some(SymbolKind::Const),
        "static" => Some(SymbolKind::Static),
        "module" | "mod" => Some(SymbolKind::Module),
        "type_alias" | "type" => Some(SymbolKind::TypeAlias),
        "macro" => Some(SymbolKind::Macro),
        _ => None,
    }
}

fn format_kind(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Struct => "struct",
        SymbolKind::Class => "class",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Interface => "interface",
        SymbolKind::Const => "const",
        SymbolKind::Static => "static",
        SymbolKind::Module => "module",
        SymbolKind::TypeAlias => "type alias",
        SymbolKind::Macro => "macro",
    }
}
