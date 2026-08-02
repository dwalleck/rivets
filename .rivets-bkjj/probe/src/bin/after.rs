//! Probe v2 — post-implementation oracle for rivets-bkjj.
//!
//! Enumerates the REAL domain `ValueEnum` derives (names + aliases) and
//! diffs against `baseline-cli-contract.txt` (the `[cli]` lines recorded
//! from the live Arg mirrors before the change). The CLI accepted set must
//! be unchanged.
//!
//! Also checks FromStr roundtrip + rejection of the formerly-lenient MCP
//! spellings, since MCP now parses via FromStr.

use clap::ValueEnum;
use rivets::domain::{DependencyType, IssueKind, IssueStatus, ResourceRole};

fn cli_lines<T>(label: &str) -> Vec<String>
where
    T: ValueEnum + Copy + std::fmt::Display,
{
    let mut rows = Vec::new();
    for variant in T::value_variants() {
        let possible = variant
            .to_possible_value()
            .expect("every ValueEnum variant has a possible value");
        let canon = variant.to_string();
        let mut names = possible.get_name_and_aliases();
        let name = names.next().expect("at least the canonical name");
        rows.push(format!("[cli] {label} {name} -> {canon}"));
        for alias in names {
            rows.push(format!("[cli] {label} alias {alias} -> {canon}"));
        }
    }
    rows
}

fn main() {
    let mut rows = Vec::new();
    rows.extend(cli_lines::<IssueKind>("IssueKind"));
    rows.extend(cli_lines::<IssueStatus>("IssueStatus"));
    rows.extend(cli_lines::<ResourceRole>("ResourceRole"));
    rows.extend(cli_lines::<DependencyType>("DependencyType"));

    let baseline_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../baseline-cli-contract.txt");
    let baseline_text = std::fs::read_to_string(&baseline_path)
        .expect("baseline-cli-contract.txt must exist (committed by probe v1)");
    let baseline: Vec<String> = baseline_text
        .lines()
        .filter(|line| line.starts_with("[cli]"))
        .map(str::to_string)
        .collect();

    println!("--- domain ValueEnum [cli] table ---");
    for row in &rows {
        println!("{row}");
    }

    if rows != baseline {
        eprintln!("FALSIFIED: domain ValueEnum table differs from baseline");
        for (i, (a, b)) in rows.iter().zip(baseline.iter()).enumerate() {
            if a != b {
                eprintln!("  first diff at {i}: future={a:?} baseline={b:?}");
                break;
            }
        }
        std::process::exit(1);
    }
    println!(
        "AGREES with baseline-cli-contract.txt ({} lines)",
        baseline.len()
    );

    // FromStr contract: canonical roundtrip; formerly-lenient spellings rejected.
    for status in [
        IssueStatus::Open,
        IssueStatus::InProgress,
        IssueStatus::Blocked,
        IssueStatus::Closed,
    ] {
        assert_eq!(status.to_string().parse::<IssueStatus>(), Ok(status));
    }
    for kind in [
        IssueKind::Bug,
        IssueKind::Feature,
        IssueKind::Task,
        IssueKind::Epic,
        IssueKind::Chore,
    ] {
        assert_eq!(kind.to_string().parse::<IssueKind>(), Ok(kind));
    }
    for role in [
        ResourceRole::Implementation,
        ResourceRole::Documentation,
        ResourceRole::Evidence,
        ResourceRole::Successor,
        ResourceRole::Reference,
    ] {
        assert_eq!(role.to_string().parse::<ResourceRole>(), Ok(role));
    }
    for dep in [
        DependencyType::Blocks,
        DependencyType::Related,
        DependencyType::ParentChild,
        DependencyType::DiscoveredFrom,
    ] {
        assert_eq!(dep.to_string().parse::<DependencyType>(), Ok(dep));
    }
    for lenient in [
        "",
        "OPEN",
        "IN_PROGRESS",
        "in-progress",
        "BUG",
        "parent_child",
        "discovered_from",
        "Evidence",
    ] {
        assert!(lenient.parse::<IssueStatus>().is_err(), "{lenient:?}");
        assert!(lenient.parse::<IssueKind>().is_err(), "{lenient:?}");
        assert!(lenient.parse::<DependencyType>().is_err(), "{lenient:?}");
    }
    println!("FromStr: canonical roundtrip OK; lenient spellings rejected");
}
