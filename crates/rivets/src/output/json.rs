//! JSON output formatting for CLI commands.

use crate::domain::{BlockingDependency, Issue};
use serde::Serialize;
use std::io::{self, Write};

pub(crate) fn print_issue_json<W: Write>(w: &mut W, issue: &Issue) -> io::Result<()> {
    let json = serde_json::to_string_pretty(issue)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}

pub(crate) fn print_issues_json<W: Write>(w: &mut W, issues: &[Issue]) -> io::Result<()> {
    let json = serde_json::to_string_pretty(issues)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}

#[derive(Serialize)]
pub(crate) struct IssueDetails<'a> {
    #[serde(flatten)]
    pub issue: &'a Issue,
    pub blocking_prerequisites: Vec<&'a BlockingDependency>,
    pub blocking_dependents: Vec<&'a BlockingDependency>,
}

pub(crate) fn print_issue_details_json<W: Write>(
    w: &mut W,
    issue: &Issue,
    prerequisites: &[BlockingDependency],
    dependents: &[BlockingDependency],
) -> io::Result<()> {
    let details = IssueDetails {
        issue,
        blocking_prerequisites: prerequisites.iter().collect(),
        blocking_dependents: dependents.iter().collect(),
    };

    let json = serde_json::to_string_pretty(&details)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}

#[derive(Serialize)]
struct BlockedIssue<'a> {
    issue: &'a Issue,
    blocked_by: Vec<&'a Issue>,
}

pub(crate) fn print_blocked_json<W: Write>(
    w: &mut W,
    blocked: &[(Issue, Vec<Issue>)],
) -> io::Result<()> {
    let items: Vec<BlockedIssue> = blocked
        .iter()
        .map(|(issue, blockers)| BlockedIssue {
            issue,
            blocked_by: blockers.iter().collect(),
        })
        .collect();

    let json = serde_json::to_string_pretty(&items)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}
