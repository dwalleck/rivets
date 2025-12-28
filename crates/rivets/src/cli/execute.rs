//! Command execution logic.
//!
//! This module contains the implementation of all CLI commands.

use anyhow::Result;

use super::args::{
    BlockedArgs, CloseArgs, CreateArgs, DeleteArgs, DepAction, DepArgs, InfoArgs, InitArgs,
    LabelAction, LabelArgs, ListArgs, ReadyArgs, ReopenArgs, ShowArgs, StaleArgs, StatsArgs,
    UpdateArgs,
};
use super::types::{SortOrderArg, SortPolicyArg};
use crate::output::OutputMode;

/// Execute the init command
pub async fn execute_init(args: &InitArgs) -> Result<()> {
    use crate::commands::init;

    let current_dir = std::env::current_dir()?;

    if !args.quiet {
        println!(
            "Initializing rivets repository{}...",
            args.prefix
                .as_ref()
                .map(|p| format!(" with prefix '{}'", p))
                .unwrap_or_default()
        );
    }

    let result = init::init(&current_dir, args.prefix.as_deref()).await?;

    if !args.quiet {
        println!("Initialized rivets in {}", result.rivets_dir.display());
        println!("  Config: {}", result.config_file.display());
        println!("  Issues: {}", result.issues_file.display());
        println!("  Issue prefix: {}", result.prefix);
    }

    Ok(())
}

/// Execute the info command
pub async fn execute_info(
    app: &crate::app::App,
    _args: &InfoArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueFilter, IssueStatus};
    use crate::output;

    let rivets_dir = app.rivets_dir();
    let database_path = rivets_dir.join("issues.jsonl");
    let issue_prefix = app.prefix();

    // Get issue counts
    let all_issues = app.storage().list(&IssueFilter::default()).await?;
    let total = all_issues.len();
    let open = all_issues
        .iter()
        .filter(|i| i.status == IssueStatus::Open)
        .count();
    let in_progress = all_issues
        .iter()
        .filter(|i| i.status == IssueStatus::InProgress)
        .count();
    let closed = all_issues
        .iter()
        .filter(|i| i.status == IssueStatus::Closed)
        .count();

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&serde_json::json!({
                "database_path": database_path.display().to_string(),
                "issue_prefix": issue_prefix,
                "issues": {
                    "total": total,
                    "open": open,
                    "in_progress": in_progress,
                    "closed": closed
                }
            }))?;
        }
        output::OutputMode::Text => {
            println!("Rivets Repository Information");
            println!("==============================");
            println!();
            println!("Database:     {}", database_path.display());
            println!("Issue prefix: {}", issue_prefix);
            println!();
            println!(
                "Issues: {} total ({} open, {} in progress, {} closed)",
                total, open, in_progress, closed
            );
        }
    }

    Ok(())
}

/// Execute the create command
pub async fn execute_create(
    app: &mut crate::app::App,
    args: &CreateArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{DependencyType as DomainDepType, IssueId, NewIssue};
    use crate::output;

    // Get title (interactive prompt if not provided)
    let title = match &args.title {
        Some(t) => t.clone(),
        None => {
            // Interactive mode: prompt for title
            eprint!("Title: ");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            // Apply same validation as CLI argument parsing
            super::validators::validate_title(input.trim()).map_err(|e| anyhow::anyhow!("{}", e))?
        }
    };

    // Parse dependencies from args
    let mut dependencies: Vec<(IssueId, DomainDepType)> = Vec::new();
    for dep_str in &args.deps {
        // Format: "issue-id" or "type:issue-id"
        if let Some((dep_type_str, issue_id)) = dep_str.split_once(':') {
            let dep_type = match dep_type_str {
                "blocks" => DomainDepType::Blocks,
                "related" => DomainDepType::Related,
                "parent-child" => DomainDepType::ParentChild,
                "discovered-from" => DomainDepType::DiscoveredFrom,
                _ => {
                    anyhow::bail!(
                        "Invalid dependency type '{}'. Valid types: blocks, related, parent-child, discovered-from",
                        dep_type_str
                    );
                }
            };
            dependencies.push((IssueId::new(issue_id), dep_type));
        } else {
            // Default to Blocks dependency
            dependencies.push((IssueId::new(dep_str), DomainDepType::Blocks));
        }
    }

    let new_issue = NewIssue {
        title,
        description: args.description.clone().unwrap_or_default(),
        priority: args.priority,
        issue_type: args.issue_type.into(),
        assignee: args.assignee.clone(),
        labels: args.labels.clone(),
        design: args.design.clone(),
        acceptance_criteria: args.acceptance.clone(),
        notes: None,
        external_ref: args.external_ref.clone(),
        dependencies,
    };

    let issue = app.storage_mut().create(new_issue).await?;
    app.save().await?;

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&issue)?;
        }
        output::OutputMode::Text => {
            println!("Created issue: {}", issue.id);
        }
    }

    Ok(())
}

/// Execute the list command
pub async fn execute_list(
    app: &crate::app::App,
    args: &ListArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueFilter;
    use crate::output;

    // Don't apply limit in filter - we need to sort first, then limit
    let filter = IssueFilter {
        status: args.status.map(|s| s.into()),
        priority: args.priority,
        issue_type: args.issue_type.map(|t| t.into()),
        assignee: args.assignee.clone(),
        label: args.label.clone(),
        limit: None,
    };

    let mut issues = app.storage().list(&filter).await?;

    // Sort before limiting to get correct results
    match args.sort {
        SortOrderArg::Priority => {
            issues.sort_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| b.created_at.cmp(&a.created_at))
            });
        }
        SortOrderArg::Newest => {
            issues.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }
        SortOrderArg::Oldest => {
            issues.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        }
        SortOrderArg::Updated => {
            issues.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        }
    }

    // Apply limit after sorting
    issues.truncate(args.limit);

    output::print_issues(&issues, output_mode)?;

    Ok(())
}

/// Execute the show command
pub async fn execute_show(
    app: &crate::app::App,
    args: &ShowArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

    let mut results = Vec::new();

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        let issue = app
            .storage()
            .get(&issue_id)
            .await?
            .ok_or_else(|| crate::error::Error::IssueNotFound(issue_id.clone()))?;

        let deps = app.storage().get_dependencies(&issue_id).await?;
        let dependents = app.storage().get_dependents(&issue_id).await?;

        results.push((issue, deps, dependents));
    }

    // Output all results
    match output_mode {
        output::OutputMode::Json => {
            // Always return array for consistency in programmatic usage
            let json_results: Vec<_> = results
                .iter()
                .map(|(issue, deps, dependents)| {
                    serde_json::json!({
                        "id": issue.id.to_string(),
                        "title": issue.title,
                        "description": issue.description,
                        "status": format!("{}", issue.status),
                        "priority": issue.priority,
                        "issue_type": format!("{}", issue.issue_type),
                        "assignee": issue.assignee,
                        "labels": issue.labels,
                        "design": issue.design,
                        "acceptance_criteria": issue.acceptance_criteria,
                        "notes": issue.notes,
                        "external_ref": issue.external_ref,
                        "dependencies": deps,
                        "created_at": issue.created_at,
                        "updated_at": issue.updated_at,
                        "closed_at": issue.closed_at,
                        "dependency_details": deps,
                        "dependent_details": dependents,
                    })
                })
                .collect();
            output::print_json(&json_results)?;
        }
        output::OutputMode::Text => {
            for (i, (issue, deps, dependents)) in results.iter().enumerate() {
                if i > 0 {
                    println!();
                    println!("---");
                    println!();
                }
                output::print_issue_details(issue, deps, dependents, output_mode)?;
            }
        }
    }

    Ok(())
}

/// Execute the update command
///
/// # Atomicity
///
/// Changes are applied sequentially in memory, then persisted to disk at the end.
/// If ANY operation fails mid-batch:
/// - Processing stops immediately
/// - In-memory state may contain partial updates (but these are not persisted)
/// - On-disk state remains unchanged (save is never called)
/// - Successfully processed IDs are reported to stderr to help with debugging
///
/// From a persistence perspective, this is all-or-nothing: either all changes
/// are saved to disk, or none are.
pub async fn execute_update(
    app: &mut crate::app::App,
    args: &UpdateArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, IssueUpdate};
    use crate::output;

    let mut updated_issues = Vec::new();

    /// Helper to report successful updates before returning an error
    fn report_partial_success(updated: &[crate::domain::Issue]) {
        if !updated.is_empty() {
            eprintln!(
                "Successfully updated {} issue(s) before error: {}",
                updated.len(),
                updated
                    .iter()
                    .map(|i| i.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        // Build the update (same for all issues)
        let update = IssueUpdate {
            title: args.title.clone(),
            description: args.description.clone(),
            status: args.status.map(|s| s.into()),
            priority: args.priority,
            assignee: args.assignee.clone().map(Some),
            design: args.design.clone(),
            acceptance_criteria: args.acceptance.clone(),
            notes: args.notes.clone(),
            external_ref: args.external_ref.clone(),
            ..Default::default()
        };

        match app.storage_mut().update(&issue_id, update).await {
            Ok(issue) => updated_issues.push(issue),
            Err(e) => {
                report_partial_success(&updated_issues);
                return Err(e.into());
            }
        }
    }

    app.save().await?;

    match output_mode {
        output::OutputMode::Json => {
            // Always return array for consistency in programmatic usage
            output::print_json(&updated_issues)?;
        }
        output::OutputMode::Text => {
            for issue in &updated_issues {
                println!("Updated issue: {}", issue.id);
            }
        }
    }

    Ok(())
}

/// Execute the close command
///
/// # Atomicity
///
/// Changes are applied sequentially in memory, then persisted to disk at the end.
/// If ANY operation fails mid-batch:
/// - Processing stops immediately
/// - In-memory state may contain partial updates (but these are not persisted)
/// - On-disk state remains unchanged (save is never called)
/// - Successfully processed IDs are reported to stderr to help with debugging
///
/// From a persistence perspective, this is all-or-nothing: either all changes
/// are saved to disk, or none are.
pub async fn execute_close(
    app: &mut crate::app::App,
    args: &CloseArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, IssueStatus, IssueUpdate};
    use crate::output;

    let mut closed_issues = Vec::new();

    /// Helper to report successful closures before returning an error
    fn report_partial_success(closed: &[crate::domain::Issue]) {
        if !closed.is_empty() {
            eprintln!(
                "Successfully closed {} issue(s) before error: {}",
                closed.len(),
                closed
                    .iter()
                    .map(|i| i.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        // Build updated notes: append close reason to existing notes if present
        let new_notes = if args.reason != "Completed" {
            let existing = match app.storage().get(&issue_id).await {
                Ok(Some(issue)) => issue,
                Ok(None) => {
                    report_partial_success(&closed_issues);
                    return Err(crate::error::Error::IssueNotFound(issue_id.clone()).into());
                }
                Err(e) => {
                    report_partial_success(&closed_issues);
                    return Err(e.into());
                }
            };

            let close_note = format!("Closed: {}", args.reason);
            Some(match existing.notes {
                Some(existing_notes) => format!("{}\n\n{}", existing_notes, close_note),
                None => close_note,
            })
        } else {
            None
        };

        let update = IssueUpdate {
            status: Some(IssueStatus::Closed),
            notes: new_notes,
            ..Default::default()
        };

        match app.storage_mut().update(&issue_id, update).await {
            Ok(issue) => closed_issues.push(issue),
            Err(e) => {
                report_partial_success(&closed_issues);
                return Err(e.into());
            }
        }
    }

    app.save().await?;

    match output_mode {
        output::OutputMode::Json => {
            // Always return array for consistency in programmatic usage
            output::print_json(&closed_issues)?;
        }
        output::OutputMode::Text => {
            for issue in &closed_issues {
                println!("Closed issue: {} ({})", issue.id, args.reason);
            }
        }
    }

    Ok(())
}

/// Execute the reopen command
///
/// # Atomicity
///
/// Changes are applied sequentially in memory, then persisted to disk at the end.
/// If ANY operation fails mid-batch:
/// - Processing stops immediately
/// - In-memory state may contain partial updates (but these are not persisted)
/// - On-disk state remains unchanged (save is never called)
/// - Successfully processed IDs are reported to stderr to help with debugging
///
/// From a persistence perspective, this is all-or-nothing: either all changes
/// are saved to disk, or none are.
pub async fn execute_reopen(
    app: &mut crate::app::App,
    args: &ReopenArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, IssueStatus, IssueUpdate};
    use crate::output;

    let mut reopened_issues = Vec::new();

    /// Helper to report successful reopens before returning an error
    fn report_partial_success(reopened: &[crate::domain::Issue]) {
        if !reopened.is_empty() {
            eprintln!(
                "Successfully reopened {} issue(s) before error: {}",
                reopened.len(),
                reopened
                    .iter()
                    .map(|i| i.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        // Build updated notes: append reopen reason to existing notes if provided
        let new_notes = if let Some(reason) = &args.reason {
            let existing = match app.storage().get(&issue_id).await {
                Ok(Some(issue)) => issue,
                Ok(None) => {
                    report_partial_success(&reopened_issues);
                    return Err(crate::error::Error::IssueNotFound(issue_id.clone()).into());
                }
                Err(e) => {
                    report_partial_success(&reopened_issues);
                    return Err(e.into());
                }
            };

            let reopen_note = format!("Reopened: {}", reason);
            Some(match existing.notes {
                Some(existing_notes) => format!("{}\n\n{}", existing_notes, reopen_note),
                None => reopen_note,
            })
        } else {
            None
        };

        let update = IssueUpdate {
            status: Some(IssueStatus::Open),
            notes: new_notes,
            ..Default::default()
        };

        match app.storage_mut().update(&issue_id, update).await {
            Ok(issue) => reopened_issues.push(issue),
            Err(e) => {
                report_partial_success(&reopened_issues);
                return Err(e.into());
            }
        }
    }

    app.save().await?;

    match output_mode {
        output::OutputMode::Json => {
            // Always return array for consistency in programmatic usage
            output::print_json(&reopened_issues)?;
        }
        output::OutputMode::Text => {
            let reason_msg = args
                .reason
                .as_ref()
                .map(|r| format!(" ({})", r))
                .unwrap_or_default();
            for issue in &reopened_issues {
                println!("Reopened issue: {}{}", issue.id, reason_msg);
            }
        }
    }

    Ok(())
}

/// Execute the delete command
pub async fn execute_delete(
    app: &mut crate::app::App,
    args: &DeleteArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

    let issue_id = IssueId::new(&args.issue_id);

    // Verify the issue exists first
    let issue = app
        .storage()
        .get(&issue_id)
        .await?
        .ok_or_else(|| crate::error::Error::IssueNotFound(issue_id.clone()))?;

    // Confirm deletion unless --force is used
    if !args.force {
        eprint!("Delete issue '{}' ({})? [y/N]: ", issue.id, issue.title);
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let response = input.trim().to_lowercase();
        if response != "y" && response != "yes" {
            println!("Deletion cancelled.");
            return Ok(());
        }
    }

    app.storage_mut().delete(&issue_id).await?;
    app.save().await?;

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&serde_json::json!({
                "deleted": args.issue_id,
                "status": "success"
            }))?;
        }
        output::OutputMode::Text => {
            println!("Deleted issue: {}", args.issue_id);
        }
    }

    Ok(())
}

/// Execute the ready command
pub async fn execute_ready(
    app: &crate::app::App,
    args: &ReadyArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueFilter, SortPolicy};
    use crate::output;

    // Only create filter if we have filtering criteria; limit is applied after via truncate
    let filter = if args.assignee.is_some() || args.priority.is_some() {
        Some(IssueFilter {
            assignee: args.assignee.clone(),
            priority: args.priority,
            ..Default::default()
        })
    } else {
        None
    };

    let sort_policy = match args.sort {
        SortPolicyArg::Hybrid => SortPolicy::Hybrid,
        SortPolicyArg::Priority => SortPolicy::Priority,
        SortPolicyArg::Oldest => SortPolicy::Oldest,
    };

    let mut issues = app
        .storage()
        .ready_to_work(filter.as_ref(), Some(sort_policy))
        .await?;

    // Apply limit
    issues.truncate(args.limit);

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&issues)?;
        }
        output::OutputMode::Text => {
            if issues.is_empty() {
                println!("No ready issues found.");
            } else {
                println!("Ready to work ({} issue(s)):", issues.len());
                println!();
                for issue in &issues {
                    output::print_issue(issue, output_mode)?;
                }
            }
        }
    }

    Ok(())
}

/// Execute the dep command
pub async fn execute_dep(
    app: &mut crate::app::App,
    args: &DepArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

    match &args.action {
        DepAction::Add { from, to, dep_type } => {
            let from_id = IssueId::new(from);
            let to_id = IssueId::new(to);

            app.storage_mut()
                .add_dependency(&from_id, &to_id, (*dep_type).into())
                .await?;
            app.save().await?;

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&serde_json::json!({
                        "action": "add",
                        "from": from,
                        "to": to,
                        "type": format!("{}", dep_type),
                        "status": "success"
                    }))?;
                }
                output::OutputMode::Text => {
                    println!("Added dependency: {} --[{}]--> {}", from, dep_type, to);
                }
            }
        }
        DepAction::Remove { from, to } => {
            let from_id = IssueId::new(from);
            let to_id = IssueId::new(to);

            app.storage_mut()
                .remove_dependency(&from_id, &to_id)
                .await?;
            app.save().await?;

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&serde_json::json!({
                        "action": "remove",
                        "from": from,
                        "to": to,
                        "status": "success"
                    }))?;
                }
                output::OutputMode::Text => {
                    println!("Removed dependency: {} --> {}", from, to);
                }
            }
        }
        DepAction::List { issue_id, reverse } => {
            let id = IssueId::new(issue_id);

            let deps = if *reverse {
                app.storage().get_dependents(&id).await?
            } else {
                app.storage().get_dependencies(&id).await?
            };

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&deps)?;
                }
                output::OutputMode::Text => {
                    if deps.is_empty() {
                        if *reverse {
                            println!("↑ No issues depend on {}", issue_id);
                        } else {
                            println!("↓ {} has no dependencies", issue_id);
                        }
                    } else if *reverse {
                        println!("↑ Issues depending on {} ({}):", issue_id, deps.len());
                        for dep in &deps {
                            println!("  └── {} ({})", dep.depends_on_id, dep.dep_type);
                        }
                    } else {
                        println!("↓ Dependencies of {} ({}):", issue_id, deps.len());
                        for dep in &deps {
                            println!("  └── {} ({})", dep.depends_on_id, dep.dep_type);
                        }
                    }
                }
            }
        }
        DepAction::Tree { issue_id, depth } => {
            let id = IssueId::new(issue_id);

            // Get the issue to verify it exists
            let issue = app
                .storage()
                .get(&id)
                .await?
                .ok_or_else(|| crate::error::Error::IssueNotFound(id.clone()))?;

            // Convert depth: 0 means unlimited (None), otherwise Some(depth)
            let max_depth = if *depth == 0 { None } else { Some(*depth) };

            // Get dependency tree
            let tree = app.storage().get_dependency_tree(&id, max_depth).await?;

            // Also get dependents (reverse tree)
            let dependents = app.storage().get_dependents(&id).await?;

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&serde_json::json!({
                        "issue_id": issue_id,
                        "title": issue.title,
                        "dependencies": tree.iter().map(|(dep, depth)| {
                            serde_json::json!({
                                "depends_on_id": dep.depends_on_id.to_string(),
                                "dep_type": format!("{}", dep.dep_type),
                                "depth": depth
                            })
                        }).collect::<Vec<_>>(),
                        "dependents": dependents.iter().map(|dep| {
                            serde_json::json!({
                                "depends_on_id": dep.depends_on_id.to_string(),
                                "dep_type": format!("{}", dep.dep_type)
                            })
                        }).collect::<Vec<_>>()
                    }))?;
                }
                output::OutputMode::Text => {
                    println!("Dependency tree for: {} ({})", issue_id, issue.title);
                    println!();

                    // Print dependents (what depends on this issue)
                    if dependents.is_empty() {
                        println!("  ↑ No issues depend on this");
                    } else {
                        println!("  ↑ Depended on by ({}):", dependents.len());
                        for dep in &dependents {
                            println!("    {} ({})", dep.depends_on_id, dep.dep_type);
                        }
                    }

                    println!("  │");
                    println!("  ◆ {} [P{}]", issue_id, issue.priority);
                    println!("  │");

                    // Print dependencies (what this issue depends on)
                    if tree.is_empty() {
                        println!("  ↓ No dependencies");
                    } else {
                        println!("  ↓ Depends on ({}):", tree.len());
                        const MAX_VISUAL_DEPTH: usize = 10;
                        for (dep, dep_depth) in &tree {
                            // Cap visual indentation at MAX_VISUAL_DEPTH to prevent extremely wide output
                            let visual_depth = (*dep_depth).min(MAX_VISUAL_DEPTH);
                            let indent = "  ".repeat(visual_depth);
                            let depth_indicator = if *dep_depth > MAX_VISUAL_DEPTH {
                                format!(" [depth: {}]", dep_depth)
                            } else {
                                String::new()
                            };
                            println!(
                                "    {}└── {} ({}){}",
                                indent, dep.depends_on_id, dep.dep_type, depth_indicator
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Execute the label command
pub async fn execute_label(
    app: &mut crate::app::App,
    args: &LabelArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueFilter, IssueId};
    use crate::output;
    use std::collections::BTreeSet;

    match &args.action {
        LabelAction::Add { issue_ids, label } => {
            // Use atomic add_label operation to avoid TOCTOU race conditions
            let mut updated_issues = Vec::new();

            for id_str in issue_ids {
                let issue_id = IssueId::new(id_str);
                let updated = app.storage_mut().add_label(&issue_id, label).await?;
                updated_issues.push(updated);
            }

            app.save().await?;

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&serde_json::json!({
                        "action": "add",
                        "label": label,
                        "issues": updated_issues.iter().map(|i| i.id.to_string()).collect::<Vec<_>>(),
                        "status": "success"
                    }))?;
                }
                output::OutputMode::Text => {
                    for issue in &updated_issues {
                        println!("Added label '{}' to {}", label, issue.id);
                    }
                }
            }
        }
        LabelAction::Remove { issue_ids, label } => {
            // Use atomic remove_label operation to avoid TOCTOU race conditions
            let mut updated_issues = Vec::new();

            for id_str in issue_ids {
                let issue_id = IssueId::new(id_str);
                let updated = app.storage_mut().remove_label(&issue_id, label).await?;
                updated_issues.push(updated);
            }

            app.save().await?;

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&serde_json::json!({
                        "action": "remove",
                        "label": label,
                        "issues": updated_issues.iter().map(|i| i.id.to_string()).collect::<Vec<_>>(),
                        "status": "success"
                    }))?;
                }
                output::OutputMode::Text => {
                    for issue in &updated_issues {
                        println!("Removed label '{}' from {}", label, issue.id);
                    }
                }
            }
        }
        LabelAction::List { issue_id } => {
            let id = IssueId::new(issue_id);
            let issue = app
                .storage()
                .get(&id)
                .await?
                .ok_or_else(|| crate::error::Error::IssueNotFound(id.clone()))?;

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&issue.labels)?;
                }
                output::OutputMode::Text => {
                    if issue.labels.is_empty() {
                        println!("{} has no labels", issue_id);
                    } else {
                        println!("Labels for {} ({}):", issue_id, issue.labels.len());
                        for label in &issue.labels {
                            println!("  {}", label);
                        }
                    }
                }
            }
        }
        LabelAction::ListAll => {
            let all_issues = app.storage().list(&IssueFilter::default()).await?;

            // Collect all unique labels
            let all_labels: BTreeSet<String> = all_issues
                .iter()
                .flat_map(|i| i.labels.iter().cloned())
                .collect();

            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&all_labels.iter().collect::<Vec<_>>())?;
                }
                output::OutputMode::Text => {
                    if all_labels.is_empty() {
                        println!("No labels found in any issues.");
                    } else {
                        println!("All labels ({}):", all_labels.len());
                        for label in &all_labels {
                            println!("  {}", label);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Execute the stale command
///
/// By default, closed issues are excluded from staleness checks (since they're done).
/// Use `--status closed` to explicitly find stale closed issues if needed.
pub async fn execute_stale(
    app: &crate::app::App,
    args: &StaleArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueFilter, IssueStatus};
    use crate::output;
    use chrono::{Duration, Utc};

    let cutoff = Utc::now() - Duration::days(i64::from(args.days));

    // Build filter based on status if provided
    let filter = IssueFilter {
        status: args.status.map(|s| s.into()),
        ..Default::default()
    };

    let all_issues = app.storage().list(&filter).await?;

    // Filter to stale issues (not updated since cutoff)
    // When no status filter is provided, exclude closed issues by default
    // When a status filter IS provided (e.g., --status closed), respect it
    let mut stale_issues: Vec<_> = all_issues
        .into_iter()
        .filter(|i| {
            let is_stale = i.updated_at < cutoff;
            let include_issue = args.status.is_some() || i.status != IssueStatus::Closed;
            is_stale && include_issue
        })
        .collect();

    // Sort by updated_at (oldest first)
    stale_issues.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));

    // Apply limit
    stale_issues.truncate(args.limit);

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&stale_issues)?;
        }
        output::OutputMode::Text => {
            if stale_issues.is_empty() {
                println!("No stale issues found (not updated in {} days).", args.days);
            } else {
                println!(
                    "Stale issues ({} not updated in {} days):",
                    stale_issues.len(),
                    args.days
                );
                println!();
                for issue in &stale_issues {
                    let days_stale = (Utc::now() - issue.updated_at).num_days();
                    println!(
                        "  {} [P{}] {} ({} days stale)",
                        issue.id, issue.priority, issue.title, days_stale
                    );
                }
            }
        }
    }

    Ok(())
}

/// Execute the blocked command
pub async fn execute_blocked(
    app: &crate::app::App,
    _args: &BlockedArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::output;

    let blocked = app.storage().blocked_issues().await?;

    output::print_blocked_issues(&blocked, output_mode)?;

    Ok(())
}

/// Execute the stats command
pub async fn execute_stats(
    app: &crate::app::App,
    args: &StatsArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueFilter, IssueStatus};
    use crate::output;

    // Get all issues
    let all_issues = app.storage().list(&IssueFilter::default()).await?;

    // Calculate statistics
    let total = all_issues.len();
    let open = all_issues
        .iter()
        .filter(|i| i.status == IssueStatus::Open)
        .count();
    let in_progress = all_issues
        .iter()
        .filter(|i| i.status == IssueStatus::InProgress)
        .count();
    let blocked = all_issues
        .iter()
        .filter(|i| i.status == IssueStatus::Blocked)
        .count();
    let closed = all_issues
        .iter()
        .filter(|i| i.status == IssueStatus::Closed)
        .count();

    // Ready issues (not blocked by dependencies)
    let ready = app.storage().ready_to_work(None, None).await?.len();

    // Blocked issues (by dependencies)
    let blocked_by_deps = app.storage().blocked_issues().await?.len();

    match output_mode {
        output::OutputMode::Json => {
            let mut stats = serde_json::json!({
                "total": total,
                "by_status": {
                    "open": open,
                    "in_progress": in_progress,
                    "blocked": blocked,
                    "closed": closed
                },
                "ready": ready,
                "blocked_by_dependencies": blocked_by_deps
            });

            if args.detailed {
                // Add priority breakdown
                let by_priority: Vec<usize> = (0..=4)
                    .map(|p| all_issues.iter().filter(|i| i.priority == p).count())
                    .collect();

                stats["by_priority"] = serde_json::json!({
                    "p0_critical": by_priority[0],
                    "p1_high": by_priority[1],
                    "p2_medium": by_priority[2],
                    "p3_low": by_priority[3],
                    "p4_backlog": by_priority[4]
                });
            }

            output::print_json(&stats)?;
        }
        output::OutputMode::Text => {
            println!("Project Statistics");
            println!("==================");
            println!();
            println!("Total Issues:  {}", total);
            println!();
            println!("By Status:");
            println!("  Open:        {}", open);
            println!("  In Progress: {}", in_progress);
            println!("  Blocked:     {}", blocked);
            println!("  Closed:      {}", closed);
            println!();
            println!("Ready to Work: {}", ready);
            println!("Blocked by Dependencies: {}", blocked_by_deps);

            if args.detailed {
                println!();
                println!("By Priority:");
                for p in 0..=4 {
                    let count = all_issues.iter().filter(|i| i.priority == p).count();
                    let label = match p {
                        0 => "P0 (Critical)",
                        1 => "P1 (High)",
                        2 => "P2 (Medium)",
                        3 => "P3 (Low)",
                        4 => "P4 (Backlog)",
                        _ => unreachable!(),
                    };
                    println!("  {}: {}", label, count);
                }
            }
        }
    }

    Ok(())
}
