//! Probe v1 — enumerate the current string tables for the four mirrored enums
//! (IssueKind, IssueStatus, ResourceRole, DependencyType) at the three sites
//! that write them:
//!
//!   1. CLI: clap `ValueEnum` names + aliases of the Arg mirrors in
//!      `cli/types.rs` (what the CLI accepts today, and what it must keep
//!      accepting after the change).
//!   2. MCP: `parse_status` / `parse_dep_type` / `McpIssueKind` in
//!      `rivets-mcp/src/models.rs` (what MCP accepts today, including the
//!      case-insensitive and hyphen/underscore leniencies the issue removes).
//!   3. Domain: `Display` + serde rename output of the domain enums (the
//!      canonical vocabulary the design unifies on).
//!
//! Smallest question: does the CLI-accepted set equal the domain canonical
//! set (plus the documented `in-progress` alias), and which lenient forms
//! does MCP add on top?
//!
//! Oracle: `oracle.py` extracts the same three tables from source text with
//! regexes (no clap/serde machinery). Probe and oracle must agree line for
//! line (after sorting).
//!
//! v2 (post-implementation, run by checkpointed-build): same probe source
//! pointed at the domain `ValueEnum` derives + `FromStr`; its CLI table must
//! equal this v1 baseline.

use clap::ValueEnum;
use rivets::cli::{DependencyTypeArg, IssueKindArg, IssueStatusArg, ResourceRoleArg};
use rivets::domain::{DependencyType, IssueKind, IssueStatus, ResourceRole};
use rivets_mcp::models::{McpIssueKind, parse_dep_type, parse_status};

/// Print the clap-accepted strings (name + aliases) for an Arg mirror enum,
/// mapped to the canonical domain Display string via the existing `From`.
fn cli_table<T, C>(label: &str, canonical: C)
where
    T: ValueEnum + Copy,
    C: Fn(T) -> String,
{
    for variant in T::value_variants() {
        let possible = variant
            .to_possible_value()
            .expect("every ValueEnum variant has a possible value");
        let canon = canonical(*variant);
        let mut names = possible.get_name_and_aliases();
        let name = names.next().expect("at least the canonical name");
        println!("[cli] {label} {name} -> {canon}");
        for alias in names {
            println!("[cli] {label} alias {alias} -> {canon}");
        }
    }
}

/// Alternate-case spelling of a literal, mirroring the oracle's candidate
/// generation for the case-insensitive MCP parsers.
fn mixed_case(s: &str) -> String {
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 0 {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect()
}

fn main() {
    // ---- Table 1: CLI accepted strings -> canonical domain Display. ----
    cli_table::<IssueKindArg, _>("IssueKind", |v| IssueKind::from(v).to_string());
    cli_table::<IssueStatusArg, _>("IssueStatus", |v| IssueStatus::from(v).to_string());
    cli_table::<ResourceRoleArg, _>("ResourceRole", |v| ResourceRole::from(v).to_string());
    cli_table::<DependencyTypeArg, _>("DependencyType", |v| DependencyType::from(v).to_string());

    // ---- Table 2: MCP accepted strings -> canonical domain Display. ----
    for candidate in [
        "open",
        "OPEN",
        "in_progress",
        "IN_PROGRESS",
        "in-progress",
        "IN-PROGRESS",
        "blocked",
        "BLOCKED",
        "closed",
        "CLOSED",
        "invalid",
        "",
    ] {
        if let Some(status) = parse_status(candidate) {
            println!("[mcp] status {candidate} -> {status}");
        }
    }
    for candidate in [
        "blocks",
        "BLOCKS",
        "related",
        "RELATED",
        "parent-child",
        "PARENT-CHILD",
        "parent_child",
        "PARENT_CHILD",
        "discovered-from",
        "DISCOVERED-FROM",
        "discovered_from",
        "DISCOVERED_FROM",
        "invalid",
        "",
    ] {
        if let Some(dep) = parse_dep_type(candidate) {
            println!("[mcp] dep_type {candidate} -> {dep}");
        }
    }
    for literal in ["bug", "feature", "task", "epic", "chore"] {
        for candidate in [
            literal,
            &literal.to_ascii_uppercase(),
            &title_case(literal),
            &mixed_case(literal),
        ] {
            match serde_json::from_str::<McpIssueKind>(&format!("\"{candidate}\"")) {
                Ok(kind) => println!("[mcp] kind {candidate} -> {}", IssueKind::from(kind)),
                Err(_) => {}
            }
        }
    }

    // ---- Table 3: canonical domain Display + serde strings. ----
    for status in [
        IssueStatus::Open,
        IssueStatus::InProgress,
        IssueStatus::Blocked,
        IssueStatus::Closed,
    ] {
        println!(
            "[domain] IssueStatus {} serde {}",
            status,
            serde_json::to_string(&status).expect("serde serialization")
        );
    }
    for kind in [
        IssueKind::Bug,
        IssueKind::Feature,
        IssueKind::Task,
        IssueKind::Epic,
        IssueKind::Chore,
    ] {
        println!(
            "[domain] IssueKind {} serde {}",
            kind,
            serde_json::to_string(&kind).expect("serde serialization")
        );
    }
    for role in [
        ResourceRole::Implementation,
        ResourceRole::Documentation,
        ResourceRole::Evidence,
        ResourceRole::Successor,
        ResourceRole::Reference,
    ] {
        println!(
            "[domain] ResourceRole {} serde {}",
            role,
            serde_json::to_string(&role).expect("serde serialization")
        );
    }
    for dep in [
        DependencyType::Blocks,
        DependencyType::Related,
        DependencyType::ParentChild,
        DependencyType::DiscoveredFrom,
    ] {
        println!(
            "[domain] DependencyType {} serde {}",
            dep,
            serde_json::to_string(&dep).expect("serde serialization")
        );
    }
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
