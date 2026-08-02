//! Cheapest falsifier for the rivets-bkjj design (run BEFORE design approval).
//!
//! Copies of the four domain enums carrying the PROPOSED attribute shapes:
//! serde rename attrs + Display + `#[value(...)]` (only IssueStatus needs
//! name/alias). Enumerates clap's accepted names + aliases via the actual
//! clap derive machinery and compares against the recorded baseline CLI
//! contract (`baseline-cli-contract.txt`, the `[cli]` lines).
//!
//! If the derive produces different names, or serde/clap attrs collide on a
//! variant, the design premise is false.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// Issue is open and ready to work on
    Open,
    /// Issue is currently being worked on
    #[serde(rename = "in_progress")]
    #[value(name = "in_progress", alias = "in-progress")]
    InProgress,
    /// Issue is blocked by dependencies
    Blocked,
    /// Issue has been completed
    Closed,
}

impl fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Blocked => write!(f, "blocked"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum IssueKind {
    /// Bug fix
    Bug,
    /// New feature
    Feature,
    /// General task
    Task,
    /// Epic (parent issue)
    Epic,
    /// Maintenance/chore
    Chore,
}

impl fmt::Display for IssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bug => write!(f, "bug"),
            Self::Feature => write!(f, "feature"),
            Self::Task => write!(f, "task"),
            Self::Epic => write!(f, "epic"),
            Self::Chore => write!(f, "chore"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRole {
    /// Delivers work for the Issue (e.g., an implementation PR).
    Implementation,
    /// Explains the Issue or its context.
    Documentation,
    /// Supports a finding or decision recorded on the Issue.
    Evidence,
    /// Identifies where the Issue continues after migration.
    Successor,
    /// Generic external context; the fallback role.
    Reference,
}

impl fmt::Display for ResourceRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Implementation => write!(f, "implementation"),
            Self::Documentation => write!(f, "documentation"),
            Self::Evidence => write!(f, "evidence"),
            Self::Successor => write!(f, "successor"),
            Self::Reference => write!(f, "reference"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyType {
    /// Hard blocker - prevents work
    Blocks,
    /// Soft link - informational
    Related,
    /// Hierarchical - epic to task
    ParentChild,
    /// Found during work
    DiscoveredFrom,
}

impl fmt::Display for DependencyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocks => write!(f, "blocks"),
            Self::Related => write!(f, "related"),
            Self::ParentChild => write!(f, "parent-child"),
            Self::DiscoveredFrom => write!(f, "discovered-from"),
        }
    }
}

fn cli_lines<T>(label: &str) -> Vec<String>
where
    T: ValueEnum + Copy + fmt::Display,
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

    println!("--- future ValueEnum [cli] table ---");
    for row in &rows {
        println!("{row}");
    }

    if rows != baseline {
        eprintln!("FALSIFIED: proposed ValueEnum table differs from baseline");
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
}
