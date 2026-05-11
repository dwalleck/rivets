//! `tethys coupling` command implementation.
//!
//! Renders per-crate coupling metrics (Ca, Ce, instability) as a table, a
//! single-package detail view (`--package`), or JSON (`--json`).

use std::io::{self, Write};
use std::path::Path;

use clap::ValueEnum;
use colored::Colorize;
use tethys::{CouplingDetail, CouplingMetrics, CouplingSort, Tethys};

/// CLI sort flag, converted to the API's `CouplingSort` via `From`.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum SortFlag {
    /// Sort by instability (descending). Default.
    #[default]
    Instability,
    /// Sort by afferent coupling (descending).
    Ca,
    /// Sort by efferent coupling (descending).
    Ce,
    /// Alphabetical by name (ascending).
    Name,
}

impl From<SortFlag> for CouplingSort {
    fn from(f: SortFlag) -> Self {
        match f {
            SortFlag::Instability => CouplingSort::Instability,
            SortFlag::Ca => CouplingSort::Afferent,
            SortFlag::Ce => CouplingSort::Efferent,
            SortFlag::Name => CouplingSort::Name,
        }
    }
}

/// Run the coupling command.
///
/// # Errors
/// Propagates any `tethys::Error` from `Tethys::new` (workspace initialization)
/// or the underlying DB queries (`get_coupling_metrics`, `get_package_coupling`).
pub fn run(
    workspace: &Path,
    sort: SortFlag,
    package: Option<String>,
    json: bool,
) -> Result<(), tethys::Error> {
    let tethys = Tethys::new(workspace)?;

    if let Some(name) = package {
        run_detail(&tethys, &name, json)
    } else {
        run_table(&tethys, sort, json)
    }
}

fn run_table(tethys: &Tethys, sort: SortFlag, json: bool) -> Result<(), tethys::Error> {
    let metrics = tethys.get_coupling_metrics(sort.into())?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if json {
        write_table_json(&mut out, &metrics, sort).map_err(tethys::Error::Io)
    } else {
        write_table_text(&mut out, &metrics, sort).map_err(tethys::Error::Io)
    }
}

fn run_detail(tethys: &Tethys, name: &str, json: bool) -> Result<(), tethys::Error> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    run_detail_to(tethys, name, json, &mut out)
}

fn run_detail_to<W: Write>(
    tethys: &Tethys,
    name: &str,
    json: bool,
    out: &mut W,
) -> Result<(), tethys::Error> {
    let detail = tethys.get_package_coupling(name)?;

    // For not-found cases we let main.rs print the standard "error: not found: ..." line
    // and only print the suggestions here. This avoids a redundant eprintln! in this function.
    match (detail, json) {
        (Some(d), true) => write_detail_json(out, &d).map_err(tethys::Error::Io),
        (Some(d), false) => write_detail_text(out, &d).map_err(tethys::Error::Io),
        (None, true) => {
            writeln!(out, "null").map_err(tethys::Error::Io)?;
            print_not_found_suggestions(tethys, name);
            Err(tethys::Error::NotFound(format!("package: {name}")))
        }
        (None, false) => {
            print_not_found_suggestions(tethys, name);
            Err(tethys::Error::NotFound(format!("package: {name}")))
        }
    }
}

const MAX_SUGGESTIONS: usize = 5;

fn collect_suggestions(name: &str, all_names: &[String]) -> Vec<String> {
    let needle = name.to_lowercase();
    all_names
        .iter()
        .filter(|n| n.to_lowercase().contains(&needle))
        .take(MAX_SUGGESTIONS)
        .cloned()
        .collect()
}

/// Print package name suggestions to stderr. Best-effort: errors from the
/// suggestion fetch are logged at debug and silently ignored so that we never
/// swallow the caller's primary `NotFound` error.
fn print_not_found_suggestions(tethys: &Tethys, name: &str) {
    match tethys.get_packages() {
        Ok(pkgs) => {
            let names: Vec<String> = pkgs.into_iter().map(|p| p.name).collect();
            let suggestions = collect_suggestions(name, &names);
            if !suggestions.is_empty() {
                eprintln!();
                eprintln!("Did you mean: {}?", suggestions.join(", "));
            }
        }
        Err(e) => {
            tracing::debug!(error = %e, "could not fetch packages for suggestion list");
        }
    }
}

pub(crate) fn write_detail_text<W: Write>(out: &mut W, d: &CouplingDetail) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "Package: {}", d.metrics.package.name.cyan().bold())?;
    writeln!(out, "  Path:    {}", d.metrics.package.path.display())?;
    writeln!(out, "  Source:  {}", d.metrics.package.source.as_str())?;
    writeln!(out)?;
    writeln!(out, "  {}", "Coupling".white().bold())?;
    writeln!(out, "    Afferent (Ca):   {}", d.metrics.afferent)?;
    writeln!(out, "    Efferent (Ce):   {}", d.metrics.efferent)?;
    let instability = d.metrics.instability();
    let bar = render_bar(instability);
    writeln!(
        out,
        "    Instability:     {bar}  {value:.2}",
        bar = instability_color(instability, &bar),
        value = instability
    )?;
    writeln!(out)?;

    if !d.outgoing.is_empty() {
        writeln!(out, "  {}", "Depends on (outgoing):".white().bold())?;
        for dep in &d.outgoing {
            let label = if dep.dep_count == 1 { "edge" } else { "edges" };
            writeln!(
                out,
                "    {:<18} {} {}",
                dep.package.name, dep.dep_count, label
            )?;
        }
        writeln!(out)?;
    }
    if !d.incoming.is_empty() {
        writeln!(out, "  {}", "Depended on by (incoming):".white().bold())?;
        for dep in &d.incoming {
            let label = if dep.dep_count == 1 { "edge" } else { "edges" };
            writeln!(
                out,
                "    {:<18} {} {}",
                dep.package.name, dep.dep_count, label
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

pub(crate) fn write_detail_json<W: Write>(out: &mut W, d: &CouplingDetail) -> io::Result<()> {
    let value = serde_json::json!({
        "package": {
            "name": d.metrics.package.name,
            "path": d.metrics.package.path.to_string_lossy(),
            "source": d.metrics.package.source.as_str(),
        },
        "afferent": d.metrics.afferent,
        "efferent": d.metrics.efferent,
        "instability": round_to_4(d.metrics.instability()),
        "outgoing": d.outgoing.iter().map(|p| serde_json::json!({
            "name": p.package.name,
            "dep_count": p.dep_count,
        })).collect::<Vec<_>>(),
        "incoming": d.incoming.iter().map(|p| serde_json::json!({
            "name": p.package.name,
            "dep_count": p.dep_count,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_writer_pretty(&mut *out, &value).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(())
}

const BAR_WIDTH: usize = 10;

/// Render an N-character bar where the filled portion uses round-half-up of `value * N`.
fn render_bar(value: f64) -> String {
    let clamped = value.clamp(0.0, 1.0);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let fill = (clamped * BAR_WIDTH as f64 + 0.5) as usize;
    let fill = fill.min(BAR_WIDTH);
    let filled: String = "█".repeat(fill);
    let empty: String = "░".repeat(BAR_WIDTH - fill);
    format!("{filled}{empty}")
}

fn instability_color(value: f64, s: &str) -> colored::ColoredString {
    if value <= 0.40 {
        s.green()
    } else if value <= 0.70 {
        s.yellow()
    } else {
        s.red()
    }
}

fn sort_label(sort: SortFlag) -> &'static str {
    match sort {
        SortFlag::Instability => "instability (descending)",
        SortFlag::Ca => "Ca (descending)",
        SortFlag::Ce => "Ce (descending)",
        SortFlag::Name => "name (ascending)",
    }
}

pub(crate) fn write_table_text<W: Write>(
    out: &mut W,
    metrics: &[CouplingMetrics],
    sort: SortFlag,
) -> io::Result<()> {
    if metrics.is_empty() {
        writeln!(out)?;
        writeln!(out, "  No packages discovered.")?;
        writeln!(
            out,
            "  '{}' requires a Cargo workspace.",
            "tethys coupling".dimmed()
        )?;
        writeln!(out)?;
        return Ok(());
    }

    writeln!(out)?;
    writeln!(out, "{}", "Tethys Coupling Metrics".cyan().bold())?;
    writeln!(out)?;
    writeln!(
        out,
        "  {}",
        "PACKAGE              Ca   Ce   INSTABILITY"
            .white()
            .dimmed()
    )?;

    let max_name_len = metrics
        .iter()
        .map(|m| m.package.name.len())
        .max()
        .unwrap_or(0)
        .max(20);

    for m in metrics {
        let instability = m.instability();
        let bar = render_bar(instability);
        writeln!(
            out,
            "  {name:width$}  {ca:>3}  {ce:>3}   {bar}  {value:>4}",
            name = m.package.name,
            width = max_name_len,
            ca = m.afferent,
            ce = m.efferent,
            bar = instability_color(instability, &bar),
            value = format!("{:.2}", instability),
        )?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "  {}",
        format!(
            "{} packages — sorted by {}",
            metrics.len(),
            sort_label(sort)
        )
        .dimmed()
    )?;
    writeln!(out)?;
    Ok(())
}

pub(crate) fn write_table_json<W: Write>(
    out: &mut W,
    metrics: &[CouplingMetrics],
    sort: SortFlag,
) -> io::Result<()> {
    let value = serde_json::json!({
        "sort": sort_key_str(sort),
        "count": metrics.len(),
        "packages": metrics.iter().map(|m| serde_json::json!({
            "name": m.package.name,
            "path": m.package.path.to_string_lossy(),
            "source": m.package.source.as_str(),
            "afferent": m.afferent,
            "efferent": m.efferent,
            "instability": round_to_4(m.instability()),
        })).collect::<Vec<_>>(),
    });
    serde_json::to_writer_pretty(&mut *out, &value).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(())
}

fn sort_key_str(sort: SortFlag) -> &'static str {
    match sort {
        SortFlag::Instability => "instability",
        SortFlag::Ca => "ca",
        SortFlag::Ce => "ce",
        SortFlag::Name => "name",
    }
}

fn round_to_4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use tethys::{CouplingMetrics, Package, PackageId, PackageSource};

    fn pkg(name: &str) -> Package {
        Package {
            id: PackageId::from(1),
            name: name.into(),
            path: name.into(),
            source: PackageSource::Manifest,
        }
    }

    #[test]
    fn render_bar_uses_round_half_up() {
        assert_eq!(render_bar(0.00), "░░░░░░░░░░");
        assert_eq!(render_bar(0.25), "███░░░░░░░", "0.25 rounds up to 3");
        assert_eq!(render_bar(0.50), "█████░░░░░");
        assert_eq!(render_bar(0.75), "████████░░", "0.75 rounds up to 8");
        assert_eq!(render_bar(1.00), "██████████");
    }

    #[test]
    fn table_text_contains_all_packages_and_values() {
        let metrics = vec![
            CouplingMetrics {
                package: pkg("alpha"),
                afferent: 0,
                efferent: 1,
            },
            CouplingMetrics {
                package: pkg("beta"),
                afferent: 2,
                efferent: 1,
            },
        ];

        let mut buf = Vec::new();
        write_table_text(&mut buf, &metrics, SortFlag::Instability).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");

        assert!(s.contains("alpha"));
        assert!(s.contains("beta"));
        assert!(s.contains("1.00"));
        assert!(s.contains("0.33"));
        assert!(s.contains("2 packages"));
    }

    #[test]
    fn table_json_serializes_full_shape() {
        let metrics = vec![CouplingMetrics {
            package: pkg("alpha"),
            afferent: 0,
            efferent: 1,
        }];
        let mut buf = Vec::new();
        write_table_json(&mut buf, &metrics, SortFlag::Instability).expect("write json");
        let s = String::from_utf8(buf).expect("utf-8");
        let v: serde_json::Value = serde_json::from_str(&s).expect("parse json");

        assert_eq!(v["sort"], "instability");
        assert_eq!(v["count"], 1);
        assert_eq!(v["packages"][0]["name"], "alpha");
        assert_eq!(v["packages"][0]["afferent"], 0);
        assert_eq!(v["packages"][0]["efferent"], 1);
        assert_eq!(v["packages"][0]["instability"], 1.0);
        assert_eq!(v["packages"][0]["source"], "manifest");
    }

    #[test]
    fn table_text_for_empty_metrics_prints_friendly_message() {
        let mut buf = Vec::new();
        write_table_text(&mut buf, &[], SortFlag::Instability).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");
        assert!(s.contains("No packages discovered"));
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;
    use tethys::{
        CouplingDetail, CouplingMetrics, Package, PackageDependency, PackageId, PackageSource,
    };

    fn pkg(name: &str) -> Package {
        Package {
            id: PackageId::from(1),
            name: name.into(),
            path: name.into(),
            source: PackageSource::Manifest,
        }
    }

    fn sample_detail() -> CouplingDetail {
        CouplingDetail {
            metrics: CouplingMetrics {
                package: pkg("rivets-mcp"),
                afferent: 3,
                efferent: 1,
            },
            outgoing: vec![PackageDependency {
                package: pkg("rivets"),
                dep_count: 5,
            }],
            incoming: vec![
                PackageDependency {
                    package: pkg("cli-binary"),
                    dep_count: 3,
                },
                PackageDependency {
                    package: pkg("rivets-test"),
                    dep_count: 2,
                },
                PackageDependency {
                    package: pkg("rivets-bench"),
                    dep_count: 1,
                },
            ],
        }
    }

    #[test]
    fn detail_text_includes_metrics_and_neighbors() {
        let mut buf = Vec::new();
        write_detail_text(&mut buf, &sample_detail()).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");

        assert!(s.contains("rivets-mcp"));
        assert!(s.contains("Afferent (Ca):   3"));
        assert!(s.contains("Efferent (Ce):   1"));
        assert!(s.contains("0.25"));
        assert!(s.contains("rivets"));
        assert!(s.contains("cli-binary"));
        assert!(s.contains("5 edges"));
    }

    #[test]
    fn detail_json_serializes_full_shape() {
        let mut buf = Vec::new();
        write_detail_json(&mut buf, &sample_detail()).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");
        let v: serde_json::Value = serde_json::from_str(&s).expect("parse");

        assert_eq!(v["package"]["name"], "rivets-mcp");
        assert_eq!(v["afferent"], 3);
        assert_eq!(v["efferent"], 1);
        assert_eq!(v["instability"], 0.25);
        assert_eq!(v["outgoing"][0]["name"], "rivets");
        assert_eq!(v["outgoing"][0]["dep_count"], 5);
        assert_eq!(
            v["incoming"]
                .as_array()
                .expect("incoming is a JSON array")
                .len(),
            3
        );
    }
}

#[cfg(test)]
mod suggestion_tests {
    use super::*;

    #[test]
    fn suggestions_for_substring_match_only() {
        let names = vec![
            "auth-server".to_string(),
            "auth-client".to_string(),
            "billing".to_string(),
        ];
        let s = collect_suggestions("auth", &names);
        assert!(s.contains(&"auth-server".to_string()));
        assert!(s.contains(&"auth-client".to_string()));
        assert!(!s.contains(&"billing".to_string()));
    }

    #[test]
    fn suggestions_empty_when_nothing_matches() {
        let names = vec!["alpha".to_string(), "beta".to_string()];
        assert!(collect_suggestions("zzz", &names).is_empty());
    }

    #[test]
    fn suggestions_capped_at_five() {
        let names: Vec<_> = (0..10).map(|i| format!("auth-{i}")).collect();
        let s = collect_suggestions("auth", &names);
        assert_eq!(s.len(), 5);
    }
}

#[cfg(test)]
mod run_detail_tests {
    use super::*;
    use std::fs;
    use tethys::Tethys;

    fn empty_workspace() -> (tempfile::TempDir, Tethys) {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut tethys = Tethys::new(dir.path()).expect("new");
        // No Cargo.toml → no packages indexed after index().
        tethys.index().expect("index");
        (dir, tethys)
    }

    #[test]
    fn run_detail_text_mode_returns_not_found_err() {
        let (_dir, tethys) = empty_workspace();
        let mut buf: Vec<u8> = Vec::new();
        let result = run_detail_to(&tethys, "no-such-pkg", false, &mut buf);
        let err = result.expect_err("should return Err for missing package");
        let msg = err.to_string();
        assert!(
            msg.contains("not found") && msg.contains("no-such-pkg"),
            "error message should describe the missing package, got: {msg}"
        );
        // Text-mode stdout should be empty (suggestions go to stderr).
        assert!(buf.is_empty(), "text-mode stdout must be empty on not-found");
    }

    #[test]
    fn run_detail_json_mode_writes_null_then_returns_not_found_err() {
        let (_dir, tethys) = empty_workspace();
        let mut buf: Vec<u8> = Vec::new();
        let result = run_detail_to(&tethys, "no-such-pkg", true, &mut buf);
        let err = result.expect_err("should return Err for missing package");
        assert!(
            err.to_string().contains("no-such-pkg"),
            "error should mention the package name"
        );
        let stdout = String::from_utf8(buf).expect("utf-8");
        assert_eq!(
            stdout, "null\n",
            "json-mode stdout must be exactly 'null\\n' on not-found"
        );
    }

    #[test]
    fn run_detail_text_mode_succeeds_when_package_exists() {
        // Build a minimal Cargo workspace with one crate and verify the happy path
        // returns Ok and writes detail to stdout.
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"only\"]\nresolver = \"2\"\n",
        )
        .expect("workspace toml");
        fs::create_dir_all(root.join("only/src")).expect("mkdir");
        fs::write(
            root.join("only/Cargo.toml"),
            "[package]\nname = \"only\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("crate toml");
        fs::write(root.join("only/src/lib.rs"), "pub fn x() {}\n").expect("lib");

        let mut tethys = Tethys::new(root).expect("new");
        tethys.index().expect("index");

        let mut buf: Vec<u8> = Vec::new();
        run_detail_to(&tethys, "only", false, &mut buf).expect("should succeed");
        let s = String::from_utf8(buf).expect("utf-8");
        assert!(s.contains("only"), "output should mention package name");
    }
}
