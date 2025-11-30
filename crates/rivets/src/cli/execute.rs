//! Command execution logic.
//!
//! This module contains the implementation of all CLI commands.

use anyhow::Result;

use super::args::{
    BlockedArgs, CloseArgs, CreateArgs, DeleteArgs, DepAction, DepArgs, InitArgs, ListArgs,
    ReadyArgs, ShowArgs, StatsArgs, UpdateArgs,
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

    let issue_id = IssueId::new(&args.issue_id);

    let issue = app
        .storage()
        .get(&issue_id)
        .await?
        .ok_or_else(|| crate::error::Error::IssueNotFound(issue_id.clone()))?;

    let deps = app.storage().get_dependencies(&issue_id).await?;
    let dependents = app.storage().get_dependents(&issue_id).await?;

    output::print_issue_details(&issue, &deps, &dependents, output_mode)?;

    Ok(())
}

/// Execute the update command
pub async fn execute_update(
    app: &mut crate::app::App,
    args: &UpdateArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, IssueUpdate};
    use crate::output;

    let issue_id = IssueId::new(&args.issue_id);

    // Build the update
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
    };

    let issue = app.storage_mut().update(&issue_id, update).await?;
    app.save().await?;

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&issue)?;
        }
        output::OutputMode::Text => {
            println!("Updated issue: {}", issue.id);
        }
    }

    Ok(())
}

/// Execute the close command
pub async fn execute_close(
    app: &mut crate::app::App,
    args: &CloseArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, IssueStatus, IssueUpdate};
    use crate::output;

    let issue_id = IssueId::new(&args.issue_id);

    // Build updated notes: append close reason to existing notes if present
    //
    // NOTE: There is a TOCTOU (time-of-check-time-of-use) window between reading
    // the existing notes and updating the issue. In a multi-process scenario,
    // another process could modify notes between these operations. This is
    // acceptable for a single-user CLI tool. For concurrent access, consider
    // adding an atomic "append_notes" operation to the storage trait.
    let new_notes = if args.reason != "Completed" {
        let existing = app
            .storage()
            .get(&issue_id)
            .await?
            .ok_or_else(|| crate::error::Error::IssueNotFound(issue_id.clone()))?;

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

    let issue = app.storage_mut().update(&issue_id, update).await?;
    app.save().await?;

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&issue)?;
        }
        output::OutputMode::Text => {
            println!("Closed issue: {} ({})", issue.id, args.reason);
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
                            println!("No issues depend on {}", issue_id);
                        } else {
                            println!("{} has no dependencies", issue_id);
                        }
                    } else {
                        if *reverse {
                            println!("Issues depending on {} ({}):", issue_id, deps.len());
                        } else {
                            println!("Dependencies of {} ({}):", issue_id, deps.len());
                        }
                        for dep in &deps {
                            println!("  {} ({})", dep.depends_on_id, dep.dep_type);
                        }
                    }
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
