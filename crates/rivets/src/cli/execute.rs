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

    // Get issue counts in a single pass
    let all_issues = app.storage().list(&IssueFilter::default()).await?;
    let (total, open, in_progress, closed) =
        all_issues
            .iter()
            .fold((0, 0, 0, 0), |(t, o, ip, c), issue| match issue.status {
                IssueStatus::Open => (t + 1, o + 1, ip, c),
                IssueStatus::InProgress => (t + 1, o, ip + 1, c),
                IssueStatus::Closed => (t + 1, o, ip, c + 1),
                IssueStatus::Blocked => (t + 1, o, ip, c),
            });

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
                        "created_at": issue.created_at,
                        "updated_at": issue.updated_at,
                        "closed_at": issue.closed_at,
                        // Dependencies this issue has (issues it depends on)
                        "dependencies": deps,
                        // Issues that depend on this issue
                        "dependents": dependents,
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
/// # Batch Processing
///
/// Each issue is processed independently with save-after-each-success semantics:
/// - Each successful update is immediately saved to disk
/// - Processing continues even if some updates fail
/// - Returns a structured result showing both succeeded and failed operations
/// - Exit code is non-zero if any failures occurred
pub async fn execute_update(
    app: &mut crate::app::App,
    args: &UpdateArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use super::types::BatchResult;
    use crate::domain::{IssueId, IssueUpdate};

    let mut result = BatchResult::new();

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

        let storage_result = app.storage_mut().update(&issue_id, update).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    // Output results
    output_batch_result(&result, "Updated", output_mode)?;

    // Return error if any failures occurred
    if result.has_failures() {
        anyhow::bail!(
            "{} of {} update(s) failed",
            result.failed.len(),
            result.total()
        );
    }

    Ok(())
}

/// Handle save-or-record-failure for batch operations.
///
/// This helper encapsulates the common pattern of:
/// 1. Checking the result of a storage operation
/// 2. Saving to disk on success
/// 3. Reloading on save failure to restore consistency
/// 4. Recording success or failure in the batch result
async fn save_or_record_failure(
    app: &mut crate::app::App,
    result: &mut super::types::BatchResult,
    issue_id: &str,
    storage_result: Result<crate::domain::Issue, crate::error::Error>,
) {
    use super::types::BatchError;

    match storage_result {
        Ok(issue) => {
            if let Err(save_err) = app.save().await {
                if let Err(reload_err) = app.storage_mut().reload().await {
                    eprintln!("Warning: Failed to reload after save error: {}", reload_err);
                }
                result.failed.push(BatchError {
                    issue_id: issue_id.to_string(),
                    error: format!("Save failed: {}", save_err),
                });
            } else {
                result.succeeded.push(issue);
            }
        }
        Err(e) => {
            result.failed.push(BatchError {
                issue_id: issue_id.to_string(),
                error: e.to_string(),
            });
        }
    }
}

/// Output batch operation results in the appropriate format
fn output_batch_result(
    result: &super::types::BatchResult,
    action: &str,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::output;

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(result)?;
        }
        output::OutputMode::Text => {
            // Print successes
            if !result.succeeded.is_empty() {
                let ids: Vec<_> = result.succeeded.iter().map(|i| i.id.to_string()).collect();
                println!(
                    "{} {} issue(s): {}",
                    action,
                    result.succeeded.len(),
                    ids.join(", ")
                );
            }

            // Print failures
            if !result.failed.is_empty() {
                eprintln!("Failed {} issue(s):", result.failed.len());
                for err in &result.failed {
                    eprintln!("  {}: {}", err.issue_id, err.error);
                }
            }
        }
    }

    Ok(())
}

/// Execute the close command
///
/// # Batch Processing
///
/// Each issue is processed independently with save-after-each-success semantics:
/// - Each successful close is immediately saved to disk
/// - Processing continues even if some closes fail
/// - Returns a structured result showing both succeeded and failed operations
/// - Exit code is non-zero if any failures occurred
pub async fn execute_close(
    app: &mut crate::app::App,
    args: &CloseArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use super::types::{BatchError, BatchResult};
    use crate::domain::{IssueId, IssueStatus, IssueUpdate};

    let mut result = BatchResult::new();

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        // Build updated notes: append close reason to existing notes if present
        let new_notes = if args.reason != "Completed" {
            match app.storage().get(&issue_id).await {
                Ok(Some(existing)) => {
                    let close_note = format!("Closed: {}", args.reason);
                    Some(match existing.notes {
                        Some(existing_notes) => format!("{}\n\n{}", existing_notes, close_note),
                        None => close_note,
                    })
                }
                Ok(None) => {
                    result.failed.push(BatchError {
                        issue_id: id_str.clone(),
                        error: format!("Issue not found: {}", id_str),
                    });
                    continue;
                }
                Err(e) => {
                    result.failed.push(BatchError {
                        issue_id: id_str.clone(),
                        error: e.to_string(),
                    });
                    continue;
                }
            }
        } else {
            None
        };

        let update = IssueUpdate {
            status: Some(IssueStatus::Closed),
            notes: new_notes,
            ..Default::default()
        };

        let storage_result = app.storage_mut().update(&issue_id, update).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    // Output results
    output_batch_result(&result, "Closed", output_mode)?;

    // Return error if any failures occurred
    if result.has_failures() {
        anyhow::bail!(
            "{} of {} close(s) failed",
            result.failed.len(),
            result.total()
        );
    }

    Ok(())
}

/// Execute the reopen command
///
/// # Batch Processing
///
/// Each issue is processed independently with save-after-each-success semantics:
/// - Each successful reopen is immediately saved to disk
/// - Processing continues even if some reopens fail
/// - Returns a structured result showing both succeeded and failed operations
/// - Exit code is non-zero if any failures occurred
pub async fn execute_reopen(
    app: &mut crate::app::App,
    args: &ReopenArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use super::types::{BatchError, BatchResult};
    use crate::domain::{IssueId, IssueStatus, IssueUpdate};

    let mut result = BatchResult::new();

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        // Build updated notes: append reopen reason to existing notes if provided
        let new_notes = if let Some(reason) = &args.reason {
            match app.storage().get(&issue_id).await {
                Ok(Some(existing)) => {
                    let reopen_note = format!("Reopened: {}", reason);
                    Some(match existing.notes {
                        Some(existing_notes) => format!("{}\n\n{}", existing_notes, reopen_note),
                        None => reopen_note,
                    })
                }
                Ok(None) => {
                    result.failed.push(BatchError {
                        issue_id: id_str.clone(),
                        error: format!("Issue not found: {}", id_str),
                    });
                    continue;
                }
                Err(e) => {
                    result.failed.push(BatchError {
                        issue_id: id_str.clone(),
                        error: e.to_string(),
                    });
                    continue;
                }
            }
        } else {
            None
        };

        let update = IssueUpdate {
            status: Some(IssueStatus::Open),
            notes: new_notes,
            ..Default::default()
        };

        let storage_result = app.storage_mut().update(&issue_id, update).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    // Output results
    output_batch_result(&result, "Reopened", output_mode)?;

    // Return error if any failures occurred
    if result.has_failures() {
        anyhow::bail!(
            "{} of {} reopen(s) failed",
            result.failed.len(),
            result.total()
        );
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
///
/// # Batch Processing (for Add/Remove)
///
/// Each issue is processed independently with save-after-each-success semantics:
/// - Each successful label operation is immediately saved to disk
/// - Processing continues even if some operations fail
/// - Returns a structured result showing both succeeded and failed operations
/// - Exit code is non-zero if any failures occurred
pub async fn execute_label(
    app: &mut crate::app::App,
    args: &LabelArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use super::types::BatchResult;
    use crate::domain::{IssueFilter, IssueId};
    use crate::output;
    use std::collections::BTreeSet;

    match &args.action {
        LabelAction::Add {
            label,
            issue_id,
            ids,
        } => {
            // Validate: exactly one of issue_id or ids must be provided
            let issue_ids: Vec<String> = match (issue_id, ids.is_empty()) {
                (Some(id), true) => vec![id.clone()],
                (None, false) => ids.clone(),
                (Some(_), false) => {
                    anyhow::bail!(
                        "Cannot use both positional issue ID and --ids flag. Use one or the other."
                    );
                }
                (None, true) => {
                    anyhow::bail!(
                        "Must provide an issue ID (positional) or use --ids flag with one or more IDs."
                    );
                }
            };

            let mut result = BatchResult::new();

            for id_str in &issue_ids {
                let issue_id = IssueId::new(id_str);
                let storage_result = app.storage_mut().add_label(&issue_id, label).await;
                save_or_record_failure(app, &mut result, id_str, storage_result).await;
            }

            // Output results
            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&result)?;
                }
                output::OutputMode::Text => {
                    if !result.succeeded.is_empty() {
                        let ids: Vec<_> =
                            result.succeeded.iter().map(|i| i.id.to_string()).collect();
                        println!(
                            "Added label '{}' to {} issue(s): {}",
                            label,
                            result.succeeded.len(),
                            ids.join(", ")
                        );
                    }
                    if !result.failed.is_empty() {
                        eprintln!("Failed to add label to {} issue(s):", result.failed.len());
                        for err in &result.failed {
                            eprintln!("  {}: {}", err.issue_id, err.error);
                        }
                    }
                }
            }

            if result.has_failures() {
                anyhow::bail!(
                    "{} of {} label add(s) failed",
                    result.failed.len(),
                    result.total()
                );
            }
        }
        LabelAction::Remove {
            label,
            issue_id,
            ids,
        } => {
            // Validate: exactly one of issue_id or ids must be provided
            let issue_ids: Vec<String> = match (issue_id, ids.is_empty()) {
                (Some(id), true) => vec![id.clone()],
                (None, false) => ids.clone(),
                (Some(_), false) => {
                    anyhow::bail!(
                        "Cannot use both positional issue ID and --ids flag. Use one or the other."
                    );
                }
                (None, true) => {
                    anyhow::bail!(
                        "Must provide an issue ID (positional) or use --ids flag with one or more IDs."
                    );
                }
            };

            let mut result = BatchResult::new();

            for id_str in &issue_ids {
                let issue_id = IssueId::new(id_str);
                let storage_result = app.storage_mut().remove_label(&issue_id, label).await;
                save_or_record_failure(app, &mut result, id_str, storage_result).await;
            }

            // Output results
            match output_mode {
                output::OutputMode::Json => {
                    output::print_json(&result)?;
                }
                output::OutputMode::Text => {
                    if !result.succeeded.is_empty() {
                        let ids: Vec<_> =
                            result.succeeded.iter().map(|i| i.id.to_string()).collect();
                        println!(
                            "Removed label '{}' from {} issue(s): {}",
                            label,
                            result.succeeded.len(),
                            ids.join(", ")
                        );
                    }
                    if !result.failed.is_empty() {
                        eprintln!(
                            "Failed to remove label from {} issue(s):",
                            result.failed.len()
                        );
                        for err in &result.failed {
                            eprintln!("  {}: {}", err.issue_id, err.error);
                        }
                    }
                }
            }

            if result.has_failures() {
                anyhow::bail!(
                    "{} of {} label remove(s) failed",
                    result.failed.len(),
                    result.total()
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::types::BatchResult;
    use crate::domain::{Issue, IssueId, IssueStatus, IssueType};
    use crate::error::Error;
    use chrono::Utc;
    use tempfile::TempDir;

    /// Create a test issue with the given ID for use in unit tests.
    fn create_test_issue(id: &str) -> Issue {
        Issue {
            id: IssueId::new(id),
            title: "Test Issue".to_string(),
            description: String::new(),
            status: IssueStatus::Open,
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        }
    }

    #[tokio::test]
    async fn test_save_or_record_failure_success() {
        // Create a temp directory and initialize rivets
        let temp_dir = TempDir::new().unwrap();
        crate::commands::init::init(temp_dir.path(), None)
            .await
            .unwrap();

        let mut app = crate::app::App::from_directory(temp_dir.path())
            .await
            .unwrap();
        let mut result = BatchResult::new();

        let issue = create_test_issue("test-abc");
        let storage_result: Result<Issue, Error> = Ok(issue);

        save_or_record_failure(&mut app, &mut result, "test-abc", storage_result).await;

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.failed.len(), 0);
        assert_eq!(result.succeeded[0].id.as_str(), "test-abc");
    }

    #[tokio::test]
    async fn test_save_or_record_failure_storage_error() {
        // Create a temp directory and initialize rivets
        let temp_dir = TempDir::new().unwrap();
        crate::commands::init::init(temp_dir.path(), None)
            .await
            .unwrap();

        let mut app = crate::app::App::from_directory(temp_dir.path())
            .await
            .unwrap();
        let mut result = BatchResult::new();

        // Simulate a storage error (e.g., issue not found)
        let storage_result: Result<Issue, Error> =
            Err(Error::IssueNotFound(IssueId::new("test-abc")));

        save_or_record_failure(&mut app, &mut result, "test-abc", storage_result).await;

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].issue_id, "test-abc");
        assert!(result.failed[0].error.contains("not found"));
    }

    #[tokio::test]
    async fn test_save_or_record_failure_records_correct_issue_id() {
        // Verify that the issue ID passed to the function is used in the result,
        // not the ID from the issue itself (they should match in practice,
        // but the function uses the passed-in ID for error reporting)
        let temp_dir = TempDir::new().unwrap();
        crate::commands::init::init(temp_dir.path(), None)
            .await
            .unwrap();

        let mut app = crate::app::App::from_directory(temp_dir.path())
            .await
            .unwrap();
        let mut result = BatchResult::new();

        // Create issue with one ID but pass a different ID string
        let issue = create_test_issue("test-xyz");
        let storage_result: Result<Issue, Error> = Ok(issue);

        // The function should record success with the issue from storage_result
        save_or_record_failure(&mut app, &mut result, "test-xyz", storage_result).await;

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].id.as_str(), "test-xyz");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_save_or_record_failure_save_error() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        // Create a temp directory and initialize rivets
        let temp_dir = TempDir::new().unwrap();
        crate::commands::init::init(temp_dir.path(), None)
            .await
            .unwrap();

        let mut app = crate::app::App::from_directory(temp_dir.path())
            .await
            .unwrap();
        let mut result = BatchResult::new();

        // Make the .rivets directory read-only to cause a save failure
        // (save uses atomic write with temp file + rename, so we need to block directory writes)
        let rivets_dir = temp_dir.path().join(".rivets");
        let original_perms = fs::metadata(&rivets_dir).unwrap().permissions();
        let mut perms = original_perms.clone();
        perms.set_mode(0o555); // read + execute only (no write)
        fs::set_permissions(&rivets_dir, perms).unwrap();

        let issue = create_test_issue("test-save-fail");
        let storage_result: Result<Issue, Error> = Ok(issue);

        save_or_record_failure(&mut app, &mut result, "test-save-fail", storage_result).await;

        // Should record as failure due to save error
        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].issue_id, "test-save-fail");
        assert!(result.failed[0].error.contains("Save failed"));

        // Restore permissions for cleanup
        fs::set_permissions(&rivets_dir, original_perms).unwrap();
    }
}
