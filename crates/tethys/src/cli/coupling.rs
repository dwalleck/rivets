//! `tethys coupling` command implementation.
//!
//! Renders per-crate coupling metrics (Ca, Ce, instability) as a table, a
//! single-package detail view (`--package`), or JSON (`--json`).

use std::io::{self, Write};
use std::path::Path;

use clap::ValueEnum;
use colored::Colorize;
use tethys::{CouplingMetrics, CouplingSort, PackageSource, Tethys};

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
/// Propagates any `tethys::Error` from indexing or DB queries.
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

#[allow(dead_code, clippy::unnecessary_wraps)] // implemented in Task 16
fn run_detail(_tethys: &Tethys, _name: &str, _json: bool) -> Result<(), tethys::Error> {
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

fn instability_color(value: f64) -> impl Fn(&str) -> colored::ColoredString + Copy {
    move |s: &str| {
        if value <= 0.40 {
            s.green()
        } else if value <= 0.70 {
            s.yellow()
        } else {
            s.red()
        }
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
        "PACKAGE              Ca   Ce   INSTABILITY".white().dimmed()
    )?;

    let max_name_len = metrics
        .iter()
        .map(|m| m.package.name.len())
        .max()
        .unwrap_or(0)
        .max(20);

    for m in metrics {
        let bar = render_bar(m.instability);
        let color = instability_color(m.instability);
        writeln!(
            out,
            "  {name:width$}  {ca:>3}  {ce:>3}   {bar}  {value:>4}",
            name = m.package.name,
            width = max_name_len,
            ca = m.afferent,
            ce = m.efferent,
            bar = color(&bar),
            value = format!("{:.2}", m.instability),
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
            "source": match m.package.source {
                PackageSource::Manifest => "manifest",
                PackageSource::Directory => "directory",
            },
            "afferent": m.afferent,
            "efferent": m.efferent,
            "instability": round_to_4(m.instability),
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
                instability: 1.0,
            },
            CouplingMetrics {
                package: pkg("beta"),
                afferent: 2,
                efferent: 1,
                instability: 0.33,
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
            instability: 1.0,
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
