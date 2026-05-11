//! `tethys coupling` command implementation.
//!
//! Renders per-crate coupling metrics (Ca, Ce, instability) as a table, a
//! single-package detail view (`--package`), or JSON (`--json`).

use std::io::{self, Write};
use std::path::Path;

use clap::ValueEnum;
use colored::Colorize;
use tethys::{CouplingDetail, CouplingMetrics, CouplingSort, PackageDependency, Tethys};

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
        write_table_json(&mut out, &metrics, sort)
    } else {
        write_table_text(&mut out, &metrics, sort)
    }
    .or_else(super::ignore_broken_pipe)
    .map_err(tethys::Error::Io)
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

    // main.rs prints the standard "error: not found: ..." line for us; this
    // function only adds the suggestion list so the user gets both.
    let write_result = match (&detail, json) {
        (Some(d), true) => write_detail_json(out, d),
        (Some(d), false) => write_detail_text(out, d),
        (None, true) => writeln!(out, "null"),
        (None, false) => Ok(()),
    };
    write_result
        .or_else(super::ignore_broken_pipe)
        .map_err(tethys::Error::Io)?;

    if detail.is_some() {
        return Ok(());
    }
    print_not_found_suggestions(tethys, name);
    Err(tethys::Error::NotFound(format!("package: {name}")))
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

fn write_deps_section<W: Write>(
    out: &mut W,
    title: &str,
    deps: &[PackageDependency],
) -> io::Result<()> {
    if deps.is_empty() {
        return Ok(());
    }
    writeln!(out, "  {}", title.white().bold())?;
    let name_width = deps
        .iter()
        .map(|d| d.package.name.len())
        .max()
        .unwrap_or(0)
        .max(MIN_PACKAGE_COL_WIDTH);
    for dep in deps {
        let label = if dep.dep_count == 1 { "edge" } else { "edges" };
        writeln!(
            out,
            "    {name:<name_width$} {count} {label}",
            name = dep.package.name,
            count = dep.dep_count,
        )?;
    }
    writeln!(out)
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
        "    Instability:     {}  {instability:.2}",
        instability_color(instability, &bar),
    )?;
    writeln!(out)?;

    write_deps_section(out, "Depends on (outgoing):", &d.outgoing)?;
    write_deps_section(out, "Depended on by (incoming):", &d.incoming)?;
    Ok(())
}

// NOTE: the JSON shape is hand-rolled. The architecture types don't yet derive
// `Serialize` (tracked: rivets-4srr) — until that lands, new fields on
// `CouplingDetail` / `CouplingMetrics` / `Package` will NOT auto-propagate here.
pub(crate) fn write_detail_json<W: Write>(out: &mut W, d: &CouplingDetail) -> io::Result<()> {
    let dep_json = |p: &PackageDependency| {
        serde_json::json!({
            "name": p.package.name,
            "dep_count": p.dep_count,
        })
    };
    let value = serde_json::json!({
        "package": {
            "name": d.metrics.package.name,
            "path": d.metrics.package.path.to_string_lossy(),
            "source": d.metrics.package.source.as_str(),
        },
        "afferent": d.metrics.afferent,
        "efferent": d.metrics.efferent,
        "instability": round_to_4(d.metrics.instability()),
        "outgoing": d.outgoing.iter().map(&dep_json).collect::<Vec<_>>(),
        "incoming": d.incoming.iter().map(&dep_json).collect::<Vec<_>>(),
    });
    serde_json::to_writer_pretty(&mut *out, &value).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(())
}

const BAR_WIDTH: usize = 10;

/// Minimum width of the PACKAGE column in the table view, so the header is never
/// pinched against the Ca column on very short workspace names.
const MIN_PACKAGE_COL_WIDTH: usize = 20;

/// Render an N-character bar where the filled portion uses round-half-up of `value * N`.
fn render_bar(value: f64) -> String {
    let clamped = value.clamp(0.0, 1.0);
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "BAR_WIDTH fits in f64 exactly; clamped*BAR_WIDTH+0.5 ∈ [0, BAR_WIDTH+0.5], non-negative and within usize range"
    )]
    let fill = (clamped * BAR_WIDTH as f64 + 0.5) as usize;
    let fill = fill.min(BAR_WIDTH);
    let filled: String = "█".repeat(fill);
    let empty: String = "░".repeat(BAR_WIDTH - fill);
    format!("{filled}{empty}")
}

/// Color the instability bar by Martin's informal coupling zones:
/// stable (≤0.40), transitional (≤0.70), unstable (>0.70).
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

    let max_name_len = metrics
        .iter()
        .map(|m| m.package.name.len())
        .max()
        .unwrap_or(0)
        .max(MIN_PACKAGE_COL_WIDTH);

    let header = format!(
        "{:<width$}  {:>3}  {:>3}   INSTABILITY",
        "PACKAGE",
        "Ca",
        "Ce",
        width = max_name_len
    );
    writeln!(out, "  {}", header.white().dimmed())?;

    for m in metrics {
        let instability = m.instability();
        let bar = render_bar(instability);
        writeln!(
            out,
            "  {name:width$}  {ca:>3}  {ce:>3}   {bar}  {instability:>4.2}",
            name = m.package.name,
            width = max_name_len,
            ca = m.afferent,
            ce = m.efferent,
            bar = instability_color(instability, &bar),
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

// NOTE: hand-rolled JSON shape — see comment on `write_detail_json` (rivets-4srr).
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
mod test_support {
    use tethys::{Package, PackageId, PackageSource};

    pub(super) fn pkg(name: &str) -> Package {
        Package {
            id: PackageId::new(1),
            name: name.into(),
            path: name.into(),
            source: PackageSource::Manifest,
        }
    }
}

#[cfg(test)]
mod table_tests {
    use super::test_support::pkg;
    use super::*;
    use rstest::rstest;
    use tethys::CouplingMetrics;

    #[rstest]
    #[case::zero(0.00, "░░░░░░░░░░")]
    #[case::quarter_rounds_up(0.25, "███░░░░░░░")]
    #[case::half(0.50, "█████░░░░░")]
    #[case::three_quarters_rounds_up(0.75, "████████░░")]
    #[case::full(1.00, "██████████")]
    fn render_bar_uses_round_half_up(#[case] value: f64, #[case] expected: &str) {
        assert_eq!(render_bar(value), expected);
    }

    #[rstest]
    #[case::negative_clamps_to_zero(-0.5, "░░░░░░░░░░")]
    #[case::above_one_clamps_to_full(1.5, "██████████")]
    fn render_bar_clamps_out_of_range(#[case] value: f64, #[case] expected: &str) {
        assert_eq!(render_bar(value), expected);
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
        // Exact-precision substrings (not just "0.3") so a future format change
        // that drops to 1-decimal precision would fail the test instead of
        // silently still matching a shorter prefix.
        assert!(s.contains(" 1.00"));
        assert!(s.contains(" 0.33"));
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

    #[rstest]
    #[case::instability(SortFlag::Instability, "instability")]
    #[case::ca(SortFlag::Ca, "ca")]
    #[case::ce(SortFlag::Ce, "ce")]
    #[case::name(SortFlag::Name, "name")]
    fn sort_key_str_covers_every_variant(#[case] sort: SortFlag, #[case] expected: &str) {
        assert_eq!(sort_key_str(sort), expected);
    }

    #[test]
    fn table_text_header_aligns_with_long_package_names() {
        let long_name = "a-really-rather-long-package-name-41chars";
        assert_eq!(
            long_name.len(),
            41,
            "test setup: name length must exceed 20"
        );

        let metrics = vec![CouplingMetrics {
            package: pkg(long_name),
            afferent: 7,
            efferent: 9,
        }];

        let mut buf = Vec::new();
        write_table_text(&mut buf, &metrics, SortFlag::Instability).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");

        let header_line = s
            .lines()
            .find(|l| l.contains("PACKAGE"))
            .expect("header line");
        let data_line = s
            .lines()
            .find(|l| l.contains(long_name))
            .expect("data line");

        // Search only the post-name region of the data line so a numeric digit
        // in a future package name can't sneak into the assertion.
        let data_tail_start =
            data_line.find(long_name).expect("name in data line") + long_name.len();
        let data_tail = &data_line[data_tail_start..];

        let afferent_label_end = header_line.find("Ca").expect("Ca in header") + 1;
        let afferent_value_col =
            data_tail_start + data_tail.find('7').expect("ca=7 after name in data row");
        assert_eq!(
            afferent_value_col, afferent_label_end,
            "ca digit ({afferent_value_col}) must align with the 'a' of 'Ca' ({afferent_label_end})"
        );

        let efferent_label_end = header_line.find("Ce").expect("Ce in header") + 1;
        let efferent_value_col =
            data_tail_start + data_tail.find('9').expect("ce=9 after name in data row");
        assert_eq!(
            efferent_value_col, efferent_label_end,
            "ce digit ({efferent_value_col}) must align with the 'e' of 'Ce' ({efferent_label_end})"
        );
    }
}

#[cfg(test)]
mod detail_tests {
    use super::test_support::pkg;
    use super::*;
    use tethys::{CouplingDetail, CouplingMetrics, PackageDependency};

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
    fn detail_text_widens_dep_name_column_for_long_names() {
        let long_name = "a-really-rather-long-package-name-41chars";
        assert!(long_name.len() > 18, "test setup precondition");

        let detail = CouplingDetail {
            metrics: CouplingMetrics {
                package: pkg("target"),
                afferent: 0,
                efferent: 2,
            },
            outgoing: vec![
                PackageDependency {
                    package: pkg(long_name),
                    dep_count: 7,
                },
                PackageDependency {
                    package: pkg("short"),
                    dep_count: 99,
                },
            ],
            incoming: vec![],
        };

        let mut buf = Vec::new();
        write_detail_text(&mut buf, &detail).expect("write");
        let s = String::from_utf8(buf).expect("utf-8");

        let long_line = s
            .lines()
            .find(|l| l.contains(long_name))
            .expect("long-name dep line");
        let short_line = s
            .lines()
            .find(|l| l.contains("short"))
            .expect("short dep line");

        let long_count_col = long_line.find('7').expect("count on long line");
        let short_count_col = short_line.find("99").expect("count on short line");
        assert_eq!(
            long_count_col, short_count_col,
            "dep_count column must align across names of different length"
        );
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

    #[test]
    fn suggestions_capped_at_five_returns_first_five_by_input_order() {
        let names: Vec<_> = (0..10).map(|i| format!("auth-{i}")).collect();
        let s = collect_suggestions("auth", &names);
        assert_eq!(s.len(), 5);
        assert_eq!(
            s,
            vec![
                "auth-0".to_string(),
                "auth-1".to_string(),
                "auth-2".to_string(),
                "auth-3".to_string(),
                "auth-4".to_string()
            ],
            "with input in alphabetical order, cap should preserve order"
        );
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

    fn single_crate_workspace(name: &str) -> (tempfile::TempDir, Tethys) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            format!("[workspace]\nmembers = [\"{name}\"]\nresolver = \"2\"\n"),
        )
        .expect("workspace toml");
        fs::create_dir_all(root.join(format!("{name}/src"))).expect("mkdir");
        fs::write(
            root.join(format!("{name}/Cargo.toml")),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .expect("crate toml");
        fs::write(root.join(format!("{name}/src/lib.rs")), "pub fn x() {}\n").expect("lib");

        let mut tethys = Tethys::new(root).expect("new");
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
        assert!(
            buf.is_empty(),
            "text-mode stdout must be empty on not-found"
        );
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
        let (_dir, tethys) = single_crate_workspace("only");
        let mut buf: Vec<u8> = Vec::new();
        run_detail_to(&tethys, "only", false, &mut buf).expect("should succeed");
        let s = String::from_utf8(buf).expect("utf-8");
        assert!(s.contains("only"), "output should mention package name");
    }

    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
        }
    }

    #[test]
    fn run_detail_text_swallows_broken_pipe_when_package_exists() {
        let (_dir, tethys) = single_crate_workspace("only");
        let mut out = BrokenPipeWriter;
        run_detail_to(&tethys, "only", false, &mut out)
            .expect("BrokenPipe on stdout must not surface as a command failure");
    }

    #[test]
    fn run_detail_json_notfound_swallows_broken_pipe_and_still_returns_not_found() {
        let (_dir, tethys) = empty_workspace();
        let mut out = BrokenPipeWriter;
        let err = run_detail_to(&tethys, "no-such-pkg", true, &mut out)
            .expect_err("not-found should still surface even if stdout is closed");
        assert!(
            err.to_string().contains("no-such-pkg"),
            "BrokenPipe on the `null` write must not mask the NotFound error: got {err}"
        );
    }
}
