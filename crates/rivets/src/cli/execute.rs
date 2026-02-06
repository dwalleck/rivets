//! Command execution logic.
//!
//! This module contains the implementation of all CLI commands.

use std::io::Write;

use anyhow::{Context, Result};

use super::args::{
    BlockedArgs, CloseArgs, CreateArgs, DeleteArgs, DepAction, DepArgs, InfoArgs, InitArgs,
    LabelAction, LabelArgs, ListArgs, ReadyArgs, ReopenArgs, ShowArgs, StaleArgs, StatsArgs,
    UpdateArgs,
};
use super::types::{DependencyTypeArg, SortOrderArg, SortPolicyArg};
use crate::output::OutputMode;

/// Execute the init command
pub async fn execute_init(args: &InitArgs) -> Result<()> {
    use crate::commands::init;

    let current_dir = std::env::current_dir()?;

    // Get prefix (interactive prompt if not provided and not in quiet mode)
    let prefix = match &args.prefix {
        Some(p) => Some(p.clone()),
        None if !args.quiet => {
            // Interactive mode: prompt for prefix
            eprint!("Issue ID prefix (e.g., 'myproj' for 'myproj-abc'): ");
            std::io::stderr()
                .flush()
                .context("Failed to flush prompt to stderr")?;
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .context("Failed to read prefix from stdin")?;
            let trimmed = input.trim();
            if trimmed.is_empty() {
                None // Use default prefix
            } else {
                // Validate the input
                Some(
                    super::validators::validate_prefix(trimmed)
                        .map_err(|e| anyhow::anyhow!("{}", e))?,
                )
            }
        }
        None => None, // Quiet mode: use default prefix
    };

    if !args.quiet {
        println!(
            "Initializing rivets repository{}...",
            prefix
                .as_ref()
                .map(|p| format!(" with prefix '{}'", p))
                .unwrap_or_default()
        );
    }

    let result = init::init(&current_dir, prefix.as_deref()).await?;

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
    use crate::domain::IssueFilter;
    use crate::output;

    let rivets_dir = app.rivets_dir();
    let database_path = rivets_dir.join("issues.jsonl");
    let issue_prefix = app.prefix();

    // Get issue counts in a single pass
    let all_issues = app.storage().list(&IssueFilter::default()).await?;
    let counts = count_by_status(&all_issues);

    match output_mode {
        output::OutputMode::Json => {
            output::print_json(&serde_json::json!({
                "database_path": database_path.display().to_string(),
                "issue_prefix": issue_prefix,
                "issues": {
                    "total": counts.total,
                    "open": counts.open,
                    "in_progress": counts.in_progress,
                    "blocked": counts.blocked,
                    "closed": counts.closed
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
                "Issues: {} total ({} open, {} in progress, {} blocked, {} closed)",
                counts.total, counts.open, counts.in_progress, counts.blocked, counts.closed
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
            std::io::stderr()
                .flush()
                .context("Failed to flush prompt to stderr")?;
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .context("Failed to read title from stdin")?;
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

    if !args.has_updates() {
        anyhow::bail!(
            "No fields specified to update. Use one or more of:\n  {}\n\n\
             Example: rivets update ISSUE-ID --title 'New title' --priority 1",
            UpdateArgs::available_flags_help()
        );
    }

    let mut result = BatchResult::new();

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        // Build the update (same for all issues)
        let update = IssueUpdate {
            title: args.title.clone(),
            description: args.description.clone(),
            status: args.status.map(|s| s.into()),
            priority: args.priority,
            assignee: if args.no_assignee {
                Some(None) // Clear the assignee
            } else {
                args.assignee.clone().map(Some)
            },
            design: args.design.clone(),
            acceptance_criteria: args.acceptance.clone(),
            notes: args.notes.clone(),
            external_ref: args.external_ref.clone(),
            ..Default::default()
        };

        let storage_result = app.storage_mut().update(&issue_id, update).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    output_batch_result(&result, "Updated", output_mode)?;
    bail_on_batch_failures(&result, "update")
}

/// Handle save-or-record-failure for batch operations.
///
/// This helper encapsulates the common pattern of:
/// 1. Checking the result of a storage operation
/// 2. Saving to disk on success
/// 3. Reloading on save failure to restore consistency and prevent partial state
/// 4. Recording success or failure in the batch result
///
/// # Arguments
/// * `app` - Application instance with storage
/// * `result` - Batch result to record success/failure
/// * `issue_id` - Issue identifier for error reporting
/// * `storage_result` - Result from the storage operation
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
                // Try to reload to restore consistent state
                let error_msg = if let Err(reload_err) = app.storage_mut().reload().await {
                    tracing::error!(
                        save_error = %save_err,
                        reload_error = %reload_err,
                        issue_id = %issue_id,
                        "Failed to reload after save error - state may be inconsistent"
                    );
                    format!(
                        "Save failed: {} (reload also failed: {} - state may be inconsistent. \
                         Run 'rivets list' to verify current state)",
                        save_err, reload_err
                    )
                } else {
                    format!("Save failed: {}", save_err)
                };
                result.failed.push(BatchError {
                    issue_id: issue_id.to_string(),
                    error: error_msg,
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

/// Append a new note to existing notes, separated by blank line.
///
/// Used by close/reopen commands to append reason notes to existing issue notes.
/// Treats empty existing notes the same as None (returns just the new note).
fn append_note(existing: Option<&str>, new_note: &str) -> String {
    match existing {
        Some(notes) if !notes.is_empty() => format!("{}\n\n{}", notes, new_note),
        _ => new_note.to_string(),
    }
}

/// Build a reason note for close/reopen operations.
///
/// If a reason is provided, formats it with the given prefix (e.g., "Closed" or "Reopened")
/// and appends it to existing notes. Returns None if no reason provided.
fn build_reason_note(
    existing_notes: Option<&str>,
    reason: Option<&str>,
    prefix: &str,
) -> Option<String> {
    reason.map(|r| append_note(existing_notes, &format!("{}: {}", prefix, r)))
}

/// Return an error if a batch operation had any failures.
fn bail_on_batch_failures(result: &super::types::BatchResult, action: &str) -> Result<()> {
    if result.has_failures() {
        anyhow::bail!(
            "{} of {} {}(s) failed",
            result.failed.len(),
            result.total(),
            action
        );
    }
    Ok(())
}

/// Issue counts grouped by status.
#[derive(Default)]
struct StatusCounts {
    total: usize,
    open: usize,
    in_progress: usize,
    blocked: usize,
    closed: usize,
}

/// Count issues by status in a single pass.
fn count_by_status(issues: &[crate::domain::Issue]) -> StatusCounts {
    use crate::domain::IssueStatus;

    issues
        .iter()
        .fold(StatusCounts::default(), |mut counts, issue| {
            counts.total += 1;
            match issue.status {
                IssueStatus::Open => counts.open += 1,
                IssueStatus::InProgress => counts.in_progress += 1,
                IssueStatus::Blocked => counts.blocked += 1,
                IssueStatus::Closed => counts.closed += 1,
            }
            counts
        })
}

/// Fetch an issue for a batch operation, recording failures if not found.
///
/// Returns `Some(issue)` if found, `None` if not found or error (failure recorded in result).
async fn get_issue_for_batch_op(
    app: &crate::app::App,
    result: &mut super::types::BatchResult,
    id_str: &str,
) -> Option<crate::domain::Issue> {
    use super::types::BatchError;
    use crate::domain::IssueId;

    let issue_id = IssueId::new(id_str);
    match app.storage().get(&issue_id).await {
        Ok(Some(issue)) => Some(issue),
        Ok(None) => {
            result.failed.push(BatchError {
                issue_id: id_str.to_string(),
                error: format!("Issue not found: {}", id_str),
            });
            None
        }
        Err(e) => {
            result.failed.push(BatchError {
                issue_id: id_str.to_string(),
                error: e.to_string(),
            });
            None
        }
    }
}

/// Validate that an issue can transition to a target status.
///
/// # Valid Transitions
///
/// - Any non-closed status → Closed (close operation)
/// - Closed → Open (reopen operation)
/// - Any other transition is allowed by default
///
/// # Invalid Transitions
///
/// - Closed → Closed: Cannot close an already closed issue
/// - Open/InProgress/Blocked → Open: Cannot reopen a non-closed issue
///
/// Returns `Ok(())` if the transition is valid, or an error message describing why not.
fn validate_status_transition(
    current: crate::domain::IssueStatus,
    target: crate::domain::IssueStatus,
) -> Result<(), String> {
    use crate::domain::IssueStatus;

    match (current, target) {
        // Close: must not already be closed
        (IssueStatus::Closed, IssueStatus::Closed) => {
            Err(format!("Issue is already closed (status: {})", current))
        }
        // Reopen: must be closed
        (status, IssueStatus::Open) if status != IssueStatus::Closed => {
            Err(format!("Issue is not closed (status: {})", current))
        }
        // Valid transitions
        _ => Ok(()),
    }
}

/// Prompt the user for confirmation and return whether they accepted.
///
/// Prints `prompt` to stderr, reads a line from stdin, and returns `true`
/// if the response is "y" or "yes" (case-insensitive).
fn confirm_action(prompt: &str) -> Result<bool> {
    eprint!("{} [y/N]: ", prompt);
    std::io::stderr()
        .flush()
        .context("Failed to flush prompt to stderr")?;
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read confirmation from stdin")?;
    let response = input.trim().to_lowercase();
    Ok(response == "y" || response == "yes")
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
    skip_confirm: bool,
) -> Result<()> {
    use super::types::{BatchError, BatchResult};
    use crate::domain::{IssueId, IssueStatus, IssueUpdate};

    // Confirm batch close when multiple issues are being closed
    if args.issue_ids.len() > 1 && !skip_confirm {
        let prompt = format!("Close {} issues?", args.issue_ids.len());
        if !confirm_action(&prompt)? {
            println!("Close cancelled.");
            return Ok(());
        }
    }

    let mut result = BatchResult::new();

    for id_str in &args.issue_ids {
        let Some(existing) = get_issue_for_batch_op(app, &mut result, id_str).await else {
            continue;
        };

        // Validate status transition
        if let Err(err) = validate_status_transition(existing.status, IssueStatus::Closed) {
            result.failed.push(BatchError {
                issue_id: id_str.clone(),
                error: err,
            });
            continue;
        }

        let issue_id = IssueId::new(id_str);
        let update = IssueUpdate {
            status: Some(IssueStatus::Closed),
            notes: build_reason_note(existing.notes.as_deref(), args.reason.as_deref(), "Closed"),
            ..Default::default()
        };

        let storage_result = app.storage_mut().update(&issue_id, update).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    output_batch_result(&result, "Closed", output_mode)?;
    bail_on_batch_failures(&result, "close")
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
    skip_confirm: bool,
) -> Result<()> {
    use super::types::{BatchError, BatchResult};
    use crate::domain::{IssueId, IssueStatus, IssueUpdate};

    // Confirm batch reopen when multiple issues are being reopened
    if args.issue_ids.len() > 1 && !skip_confirm {
        let prompt = format!("Reopen {} issues?", args.issue_ids.len());
        if !confirm_action(&prompt)? {
            println!("Reopen cancelled.");
            return Ok(());
        }
    }

    let mut result = BatchResult::new();

    for id_str in &args.issue_ids {
        let Some(existing) = get_issue_for_batch_op(app, &mut result, id_str).await else {
            continue;
        };

        // Validate status transition
        if let Err(err) = validate_status_transition(existing.status, IssueStatus::Open) {
            result.failed.push(BatchError {
                issue_id: id_str.clone(),
                error: err,
            });
            continue;
        }

        let issue_id = IssueId::new(id_str);
        let update = IssueUpdate {
            status: Some(IssueStatus::Open),
            notes: build_reason_note(
                existing.notes.as_deref(),
                args.reason.as_deref(),
                "Reopened",
            ),
            ..Default::default()
        };

        let storage_result = app.storage_mut().update(&issue_id, update).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    output_batch_result(&result, "Reopened", output_mode)?;
    bail_on_batch_failures(&result, "reopen")
}

/// Execute the delete command
pub async fn execute_delete(
    app: &mut crate::app::App,
    args: &DeleteArgs,
    output_mode: OutputMode,
    skip_confirm: bool,
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

    // Confirm deletion unless --force or --yes is used
    if !args.force && !skip_confirm {
        let prompt = format!("Delete issue '{}' ({})?", issue.id, issue.title);
        if !confirm_action(&prompt)? {
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

/// Add a dependency between two issues.
async fn execute_dep_add(
    app: &mut crate::app::App,
    from: &str,
    to: &str,
    dep_type: DependencyTypeArg,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

    let from_id = IssueId::new(from);
    let to_id = IssueId::new(to);

    app.storage_mut()
        .add_dependency(&from_id, &to_id, dep_type.into())
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

    Ok(())
}

/// Remove a dependency between two issues.
async fn execute_dep_remove(
    app: &mut crate::app::App,
    from: &str,
    to: &str,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

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

    Ok(())
}

/// List dependencies for an issue.
async fn execute_dep_list(
    app: &crate::app::App,
    issue_id: &str,
    reverse: bool,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

    let id = IssueId::new(issue_id);

    let deps = if reverse {
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
                if reverse {
                    println!("↑ No issues depend on {}", issue_id);
                } else {
                    println!("↓ {} has no dependencies", issue_id);
                }
            } else if reverse {
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

    Ok(())
}

/// Display dependency tree for an issue.
async fn execute_dep_tree(
    app: &crate::app::App,
    issue_id: &str,
    depth: usize,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output::{self, DepTreeNode};
    use std::collections::HashSet;

    let id = IssueId::new(issue_id);

    // Verify the issue exists and get its details
    let issue = app
        .storage()
        .get(&id)
        .await?
        .ok_or_else(|| crate::error::Error::IssueNotFound(id.clone()))?;

    // Convert depth: 0 means unlimited (None), otherwise Some(depth)
    let max_depth = if depth == 0 { None } else { Some(depth) };

    // Build tree recursively using DFS
    let mut visited = HashSet::new();
    visited.insert(id.clone());
    let children = build_dep_tree_children(app, &id, max_depth, 0, &mut visited).await?;

    let root = DepTreeNode {
        id: issue_id.to_string(),
        dep_type: None,
        status: Some(issue.status),
        title: Some(issue.title.clone()),
        priority: Some(issue.priority),
        children,
    };

    // Get dependents (reverse dependencies)
    let dependents = app.storage().get_dependents(&id).await?;

    match output_mode {
        output::OutputMode::Json => {
            let json = output::dep_tree_to_json_public(&root, &dependents);
            output::print_json(&json)?;
        }
        output::OutputMode::Text => {
            output::print_dep_tree(&root, output_mode)?;

            if !dependents.is_empty() {
                let config = output::OutputConfig::from_env();
                let stdout = std::io::stdout();
                let mut handle = stdout.lock();
                output::print_dep_tree_dependents(&mut handle, &dependents, &config)?;
            }
        }
    }

    Ok(())
}

/// Recursively build dependency tree children via DFS.
///
/// Uses a visited set to prevent cycles, and respects the max depth limit.
async fn build_dep_tree_children(
    app: &crate::app::App,
    parent_id: &crate::domain::IssueId,
    max_depth: Option<usize>,
    current_depth: usize,
    visited: &mut std::collections::HashSet<crate::domain::IssueId>,
) -> Result<Vec<crate::output::DepTreeNode>> {
    use crate::output::DepTreeNode;

    // Check depth limit
    if let Some(max) = max_depth {
        if current_depth >= max {
            return Ok(vec![]);
        }
    }

    let deps = app.storage().get_dependencies(parent_id).await?;
    let mut children = Vec::new();

    for dep in deps {
        let child_id = dep.depends_on_id.clone();

        // Skip already-visited nodes to prevent cycles
        if !visited.insert(child_id.clone()) {
            continue;
        }

        // Fetch status for the child node
        let status = app
            .storage()
            .get(&child_id)
            .await
            .ok()
            .flatten()
            .map(|i| i.status);

        // Recurse into children
        let grandchildren = Box::pin(build_dep_tree_children(
            app,
            &child_id,
            max_depth,
            current_depth + 1,
            visited,
        ))
        .await?;

        children.push(DepTreeNode {
            id: child_id.to_string(),
            dep_type: Some(dep.dep_type),
            status,
            title: None,
            priority: None,
            children: grandchildren,
        });
    }

    Ok(children)
}

/// Execute the dep command
pub async fn execute_dep(
    app: &mut crate::app::App,
    args: &DepArgs,
    output_mode: OutputMode,
) -> Result<()> {
    match &args.action {
        DepAction::Add { from, to, dep_type } => {
            execute_dep_add(app, from, to, *dep_type, output_mode).await
        }
        DepAction::Remove { from, to } => execute_dep_remove(app, from, to, output_mode).await,
        DepAction::List { issue_id, reverse } => {
            execute_dep_list(app, issue_id, *reverse, output_mode).await
        }
        DepAction::Tree { issue_id, depth } => {
            execute_dep_tree(app, issue_id, *depth, output_mode).await
        }
    }
}

/// Resolve issue IDs from either a single ID or a list of IDs.
///
/// Validates that exactly one of issue_id or ids is provided.
fn resolve_label_issue_ids(issue_id: &Option<String>, ids: &[String]) -> Result<Vec<String>> {
    match (issue_id, ids.is_empty()) {
        (Some(id), true) => Ok(vec![id.clone()]),
        (None, false) => Ok(ids.to_vec()),
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
    }
}

/// Add a label to one or more issues.
async fn execute_label_add(
    app: &mut crate::app::App,
    label: &str,
    issue_id: &Option<String>,
    ids: &[String],
    output_mode: OutputMode,
) -> Result<()> {
    use super::types::BatchResult;
    use crate::domain::IssueId;
    use crate::output;

    let issue_ids = resolve_label_issue_ids(issue_id, ids)?;
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
                let ids: Vec<_> = result.succeeded.iter().map(|i| i.id.to_string()).collect();
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

    bail_on_batch_failures(&result, "label add")
}

/// Remove a label from one or more issues.
async fn execute_label_remove(
    app: &mut crate::app::App,
    label: &str,
    issue_id: &Option<String>,
    ids: &[String],
    output_mode: OutputMode,
) -> Result<()> {
    use super::types::BatchResult;
    use crate::domain::IssueId;
    use crate::output;

    let issue_ids = resolve_label_issue_ids(issue_id, ids)?;
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
                let ids: Vec<_> = result.succeeded.iter().map(|i| i.id.to_string()).collect();
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

    bail_on_batch_failures(&result, "label remove")
}

/// List labels for a specific issue.
async fn execute_label_list(
    app: &crate::app::App,
    issue_id: &str,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

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

    Ok(())
}

/// List all labels used across all issues.
async fn execute_label_list_all(app: &crate::app::App, output_mode: OutputMode) -> Result<()> {
    use crate::domain::IssueFilter;
    use crate::output;
    use std::collections::BTreeSet;

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
    match &args.action {
        LabelAction::Add {
            label,
            issue_id,
            ids,
        } => execute_label_add(app, label, issue_id, ids, output_mode).await,
        LabelAction::Remove {
            label,
            issue_id,
            ids,
        } => execute_label_remove(app, label, issue_id, ids, output_mode).await,
        LabelAction::List { issue_id } => execute_label_list(app, issue_id, output_mode).await,
        LabelAction::ListAll => execute_label_list_all(app, output_mode).await,
    }
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
                let config = output::OutputConfig::from_env();
                for issue in &stale_issues {
                    let days_stale = (Utc::now() - issue.updated_at).num_days();
                    output::print_issue(issue, output_mode)?;
                    println!(
                        "  {} {} days",
                        output::warning("Stale:", &config),
                        days_stale
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
    use crate::domain::IssueFilter;
    use crate::output;

    // Get all issues and count by status
    let all_issues = app.storage().list(&IssueFilter::default()).await?;
    let counts = count_by_status(&all_issues);

    // Ready issues (not blocked by dependencies)
    let ready = app.storage().ready_to_work(None, None).await?.len();

    // Blocked issues (by dependencies)
    let blocked_by_deps = app.storage().blocked_issues().await?.len();

    match output_mode {
        output::OutputMode::Json => {
            let mut stats = serde_json::json!({
                "total": counts.total,
                "by_status": {
                    "open": counts.open,
                    "in_progress": counts.in_progress,
                    "blocked": counts.blocked,
                    "closed": counts.closed
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
            println!("Total Issues:  {}", counts.total);
            println!();
            println!("By Status:");
            println!("  Open:        {}", counts.open);
            println!("  In Progress: {}", counts.in_progress);
            println!("  Blocked:     {}", counts.blocked);
            println!("  Closed:      {}", counts.closed);
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
    use rstest::rstest;
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

    #[rstest]
    #[case::success(true, 1, 0)]
    #[case::storage_error(false, 0, 1)]
    #[tokio::test]
    async fn test_save_or_record_failure_outcomes(
        #[case] is_success: bool,
        #[case] expected_succeeded: usize,
        #[case] expected_failed: usize,
    ) {
        let temp_dir = TempDir::new().unwrap();
        crate::commands::init::init(temp_dir.path(), None)
            .await
            .unwrap();

        let mut app = crate::app::App::from_directory(temp_dir.path())
            .await
            .unwrap();
        let mut result = BatchResult::new();

        let storage_result: Result<Issue, Error> = if is_success {
            Ok(create_test_issue("test-abc"))
        } else {
            Err(Error::IssueNotFound(IssueId::new("test-abc")))
        };

        save_or_record_failure(&mut app, &mut result, "test-abc", storage_result).await;

        assert_eq!(result.succeeded.len(), expected_succeeded);
        assert_eq!(result.failed.len(), expected_failed);
    }

    #[tokio::test]
    async fn test_save_or_record_failure_success_records_issue() {
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

        assert_eq!(result.succeeded[0].id.as_str(), "test-abc");
    }

    #[tokio::test]
    async fn test_save_or_record_failure_error_contains_message() {
        let temp_dir = TempDir::new().unwrap();
        crate::commands::init::init(temp_dir.path(), None)
            .await
            .unwrap();

        let mut app = crate::app::App::from_directory(temp_dir.path())
            .await
            .unwrap();
        let mut result = BatchResult::new();

        let storage_result: Result<Issue, Error> =
            Err(Error::IssueNotFound(IssueId::new("test-abc")));

        save_or_record_failure(&mut app, &mut result, "test-abc", storage_result).await;

        assert_eq!(result.failed[0].issue_id, "test-abc");
        assert!(result.failed[0].error.contains("not found"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_save_or_record_failure_save_error() {
        use std::fs::{self, Permissions};
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        /// RAII guard that restores directory permissions on drop.
        /// Ensures cleanup happens even if assertions panic.
        struct PermissionGuard {
            path: PathBuf,
            original: Permissions,
        }

        impl Drop for PermissionGuard {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.path, self.original.clone());
            }
        }

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

        // Create guard to restore permissions even if test panics
        let _guard = PermissionGuard {
            path: rivets_dir.clone(),
            original: original_perms,
        };

        let mut perms = fs::metadata(&rivets_dir).unwrap().permissions();
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

        // Guard will restore permissions on drop
    }

    mod resolve_label_issue_ids_tests {
        use super::super::resolve_label_issue_ids;

        #[test]
        fn test_single_positional_id() {
            let result = resolve_label_issue_ids(&Some("test-abc".to_string()), &[]);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), vec!["test-abc".to_string()]);
        }

        #[test]
        fn test_multiple_ids_via_flag() {
            let ids = vec!["test-1".to_string(), "test-2".to_string()];
            let result = resolve_label_issue_ids(&None, &ids);
            assert!(result.is_ok());
            assert_eq!(
                result.unwrap(),
                vec!["test-1".to_string(), "test-2".to_string()]
            );
        }

        #[test]
        fn test_both_positional_and_flag_fails() {
            let ids = vec!["test-2".to_string()];
            let result = resolve_label_issue_ids(&Some("test-1".to_string()), &ids);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("Cannot use both"));
        }

        #[test]
        fn test_neither_positional_nor_flag_fails() {
            let result = resolve_label_issue_ids(&None, &[]);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("Must provide"));
        }
    }

    mod append_note_tests {
        use super::super::append_note;
        use rstest::rstest;

        #[rstest]
        #[case::no_existing_notes(None, "New note", "New note")]
        #[case::with_existing_notes(
            Some("Existing notes"),
            "New note",
            "Existing notes\n\nNew note"
        )]
        #[case::empty_existing_notes(Some(""), "New note", "New note")]
        #[case::multiline_existing(
            Some("Line 1\nLine 2"),
            "New note",
            "Line 1\nLine 2\n\nNew note"
        )]
        fn test_append_note(
            #[case] existing: Option<&str>,
            #[case] new_note: &str,
            #[case] expected: &str,
        ) {
            let result = append_note(existing, new_note);
            assert_eq!(result, expected);
        }

        #[test]
        fn test_append_note_close_reason() {
            let close_note = format!("Closed: {}", "Fixed the bug");
            let result = append_note(Some("Initial description"), &close_note);
            assert_eq!(result, "Initial description\n\nClosed: Fixed the bug");
        }

        #[test]
        fn test_append_note_reopen_reason() {
            let reopen_note = format!("Reopened: {}", "Bug still present");
            let result = append_note(Some("Closed: Fixed the bug"), &reopen_note);
            assert_eq!(
                result,
                "Closed: Fixed the bug\n\nReopened: Bug still present"
            );
        }
    }

    mod validate_status_transition_tests {
        use super::super::validate_status_transition;
        use crate::domain::IssueStatus;
        use rstest::rstest;

        #[rstest]
        #[case::open_to_closed(IssueStatus::Open, IssueStatus::Closed, true)]
        #[case::in_progress_to_closed(IssueStatus::InProgress, IssueStatus::Closed, true)]
        #[case::blocked_to_closed(IssueStatus::Blocked, IssueStatus::Closed, true)]
        #[case::closed_to_closed(IssueStatus::Closed, IssueStatus::Closed, false)]
        #[case::closed_to_open(IssueStatus::Closed, IssueStatus::Open, true)]
        #[case::open_to_open(IssueStatus::Open, IssueStatus::Open, false)]
        #[case::in_progress_to_open(IssueStatus::InProgress, IssueStatus::Open, false)]
        #[case::blocked_to_open(IssueStatus::Blocked, IssueStatus::Open, false)]
        fn test_status_transitions(
            #[case] current: IssueStatus,
            #[case] target: IssueStatus,
            #[case] should_succeed: bool,
        ) {
            let result = validate_status_transition(current, target);
            assert_eq!(
                result.is_ok(),
                should_succeed,
                "Transition {:?} -> {:?} expected success={}, got {:?}",
                current,
                target,
                should_succeed,
                result
            );
        }

        #[test]
        fn test_closed_to_closed_error_message() {
            let result = validate_status_transition(IssueStatus::Closed, IssueStatus::Closed);
            assert!(result.is_err());
            let error = result.unwrap_err();
            assert!(
                error.contains("already closed"),
                "Error should mention 'already closed', got: {}",
                error
            );
        }

        #[test]
        fn test_open_to_open_error_message() {
            let result = validate_status_transition(IssueStatus::Open, IssueStatus::Open);
            assert!(result.is_err());
            let error = result.unwrap_err();
            assert!(
                error.contains("not closed"),
                "Error should mention 'not closed', got: {}",
                error
            );
        }

        #[rstest]
        #[case::open_to_in_progress(IssueStatus::Open, IssueStatus::InProgress, true)]
        #[case::open_to_blocked(IssueStatus::Open, IssueStatus::Blocked, true)]
        #[case::in_progress_to_blocked(IssueStatus::InProgress, IssueStatus::Blocked, true)]
        #[case::blocked_to_in_progress(IssueStatus::Blocked, IssueStatus::InProgress, true)]
        fn test_general_status_transitions_allowed(
            #[case] current: IssueStatus,
            #[case] target: IssueStatus,
            #[case] should_succeed: bool,
        ) {
            let result = validate_status_transition(current, target);
            assert_eq!(
                result.is_ok(),
                should_succeed,
                "Transition {:?} -> {:?} expected success={}, got {:?}",
                current,
                target,
                should_succeed,
                result
            );
        }
    }

    mod build_reason_note_tests {
        use super::super::build_reason_note;
        use rstest::rstest;

        #[rstest]
        #[case::no_reason(None, None, "Closed", None)]
        #[case::with_reason_no_existing(
            None,
            Some("Fixed the bug"),
            "Closed",
            Some("Closed: Fixed the bug")
        )]
        #[case::with_reason_and_existing(
            Some("Initial notes"),
            Some("Fixed"),
            "Closed",
            Some("Initial notes\n\nClosed: Fixed")
        )]
        #[case::empty_existing_treated_as_none(
            Some(""),
            Some("Needs work"),
            "Reopened",
            Some("Reopened: Needs work")
        )]
        #[case::reopen_prefix(
            None,
            Some("Not actually fixed"),
            "Reopened",
            Some("Reopened: Not actually fixed")
        )]
        fn test_build_reason_note(
            #[case] existing: Option<&str>,
            #[case] reason: Option<&str>,
            #[case] prefix: &str,
            #[case] expected: Option<&str>,
        ) {
            let result = build_reason_note(existing, reason, prefix);
            assert_eq!(result.as_deref(), expected);
        }
    }

    mod count_by_status_tests {
        use super::super::count_by_status;
        use super::create_test_issue;
        use crate::domain::IssueStatus;

        #[test]
        fn test_empty_list() {
            let counts = count_by_status(&[]);
            assert_eq!(counts.total, 0);
            assert_eq!(counts.open, 0);
            assert_eq!(counts.in_progress, 0);
            assert_eq!(counts.blocked, 0);
            assert_eq!(counts.closed, 0);
        }

        #[test]
        fn test_single_status() {
            let mut issue = create_test_issue("test-1");
            issue.status = IssueStatus::InProgress;
            let counts = count_by_status(&[issue]);
            assert_eq!(counts.total, 1);
            assert_eq!(counts.open, 0);
            assert_eq!(counts.in_progress, 1);
            assert_eq!(counts.blocked, 0);
            assert_eq!(counts.closed, 0);
        }

        #[test]
        fn test_mixed_statuses() {
            let mut issues = vec![
                create_test_issue("test-1"),
                create_test_issue("test-2"),
                create_test_issue("test-3"),
                create_test_issue("test-4"),
                create_test_issue("test-5"),
                create_test_issue("test-6"),
            ];
            issues[0].status = IssueStatus::Open;
            issues[1].status = IssueStatus::Open;
            issues[2].status = IssueStatus::InProgress;
            issues[3].status = IssueStatus::Blocked;
            issues[4].status = IssueStatus::Closed;
            issues[5].status = IssueStatus::Closed;

            let counts = count_by_status(&issues);
            assert_eq!(counts.total, 6);
            assert_eq!(counts.open, 2);
            assert_eq!(counts.in_progress, 1);
            assert_eq!(counts.blocked, 1);
            assert_eq!(counts.closed, 2);
        }

        #[test]
        fn test_all_same_status() {
            let issues: Vec<_> = (1..=5)
                .map(|i| {
                    let mut issue = create_test_issue(&format!("test-{}", i));
                    issue.status = IssueStatus::Blocked;
                    issue
                })
                .collect();

            let counts = count_by_status(&issues);
            assert_eq!(counts.total, 5);
            assert_eq!(counts.open, 0);
            assert_eq!(counts.in_progress, 0);
            assert_eq!(counts.blocked, 5);
            assert_eq!(counts.closed, 0);
        }
    }

    mod bail_on_batch_failures_tests {
        use super::super::bail_on_batch_failures;
        use super::create_test_issue;
        use crate::cli::types::{BatchError, BatchResult};

        #[test]
        fn test_no_failures_returns_ok() {
            let result = BatchResult::new();
            assert!(bail_on_batch_failures(&result, "update").is_ok());
        }

        #[test]
        fn test_with_successes_only_returns_ok() {
            let mut result = BatchResult::new();
            result.succeeded.push(create_test_issue("test-1"));
            result.succeeded.push(create_test_issue("test-2"));
            assert!(bail_on_batch_failures(&result, "close").is_ok());
        }

        #[test]
        fn test_all_failures_returns_error() {
            let mut result = BatchResult::new();
            result.failed.push(BatchError {
                issue_id: "test-1".to_string(),
                error: "Not found".to_string(),
            });
            result.failed.push(BatchError {
                issue_id: "test-2".to_string(),
                error: "Invalid".to_string(),
            });

            let err = bail_on_batch_failures(&result, "update").unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("2 of 2"), "Should show '2 of 2', got: {}", msg);
            assert!(
                msg.contains("update"),
                "Should contain 'update', got: {}",
                msg
            );
        }

        #[test]
        fn test_partial_failures_returns_error() {
            let mut result = BatchResult::new();
            result.succeeded.push(create_test_issue("test-ok"));
            result.failed.push(BatchError {
                issue_id: "test-fail".to_string(),
                error: "Error".to_string(),
            });

            let err = bail_on_batch_failures(&result, "close").unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("1 of 2"), "Should show '1 of 2', got: {}", msg);
            assert!(
                msg.contains("close"),
                "Should contain 'close', got: {}",
                msg
            );
        }

        #[test]
        fn test_error_message_format() {
            let mut result = BatchResult::new();
            result.failed.push(BatchError {
                issue_id: "test-1".to_string(),
                error: "Error".to_string(),
            });

            let err = bail_on_batch_failures(&result, "label add").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("label add(s) failed"),
                "Should format action correctly, got: {}",
                msg
            );
        }
    }

    mod execute_update_tests {
        use super::super::{execute_update, UpdateArgs};
        use crate::output::OutputMode;
        use tempfile::TempDir;

        #[tokio::test]
        async fn test_update_with_no_fields_returns_error() {
            let temp_dir = TempDir::new().unwrap();
            crate::commands::init::init(temp_dir.path(), Some("test"))
                .await
                .unwrap();

            let mut app = crate::app::App::from_directory(temp_dir.path())
                .await
                .unwrap();

            let args = UpdateArgs {
                issue_ids: vec!["test-abc".to_string()],
                title: None,
                description: None,
                status: None,
                priority: None,
                assignee: None,
                no_assignee: false,
                design: None,
                acceptance: None,
                notes: None,
                external_ref: None,
            };

            let result = execute_update(&mut app, &args, OutputMode::Text).await;

            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(
                error_msg.contains("No fields specified"),
                "Error should mention no fields specified, got: {}",
                error_msg
            );
            assert!(
                error_msg.contains("--title"),
                "Error should list available options, got: {}",
                error_msg
            );
        }
    }

    mod execute_close_tests {
        use super::super::{execute_close, CloseArgs};
        use crate::domain::{IssueStatus, IssueUpdate, NewIssue};
        use crate::output::OutputMode;
        use tempfile::TempDir;

        #[tokio::test]
        async fn test_close_already_closed_issue_returns_error() {
            let temp_dir = TempDir::new().unwrap();
            crate::commands::init::init(temp_dir.path(), Some("test"))
                .await
                .unwrap();

            let mut app = crate::app::App::from_directory(temp_dir.path())
                .await
                .unwrap();

            // Create an issue
            let new_issue = NewIssue {
                title: "Test issue".to_string(),
                ..Default::default()
            };
            let issue = app.storage_mut().create(new_issue).await.unwrap();
            app.save().await.unwrap();

            // Close it first
            let update = IssueUpdate {
                status: Some(IssueStatus::Closed),
                ..Default::default()
            };
            app.storage_mut().update(&issue.id, update).await.unwrap();
            app.save().await.unwrap();

            // Try to close it again
            let args = CloseArgs {
                issue_ids: vec![issue.id.to_string()],
                reason: None,
            };

            let result = execute_close(&mut app, &args, OutputMode::Text, true).await;

            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(
                error_msg.contains("failed"),
                "Error should indicate failure, got: {}",
                error_msg
            );
        }
    }

    mod execute_reopen_tests {
        use super::super::{execute_reopen, ReopenArgs};
        use crate::domain::NewIssue;
        use crate::output::OutputMode;
        use tempfile::TempDir;

        #[tokio::test]
        async fn test_reopen_already_open_issue_returns_error() {
            let temp_dir = TempDir::new().unwrap();
            crate::commands::init::init(temp_dir.path(), Some("test"))
                .await
                .unwrap();

            let mut app = crate::app::App::from_directory(temp_dir.path())
                .await
                .unwrap();

            // Create an issue (starts as Open)
            let new_issue = NewIssue {
                title: "Test issue".to_string(),
                ..Default::default()
            };
            let issue = app.storage_mut().create(new_issue).await.unwrap();
            app.save().await.unwrap();

            // Try to reopen an already open issue
            let args = ReopenArgs {
                issue_ids: vec![issue.id.to_string()],
                reason: None,
            };

            let result = execute_reopen(&mut app, &args, OutputMode::Text, true).await;

            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(
                error_msg.contains("failed"),
                "Error should indicate failure, got: {}",
                error_msg
            );
        }

        #[tokio::test]
        async fn test_reopen_in_progress_issue_returns_error() {
            use crate::domain::{IssueStatus, IssueUpdate};

            let temp_dir = TempDir::new().unwrap();
            crate::commands::init::init(temp_dir.path(), Some("test"))
                .await
                .unwrap();

            let mut app = crate::app::App::from_directory(temp_dir.path())
                .await
                .unwrap();

            // Create an issue and set it to in_progress
            let new_issue = NewIssue {
                title: "Test issue".to_string(),
                ..Default::default()
            };
            let issue = app.storage_mut().create(new_issue).await.unwrap();

            let update = IssueUpdate {
                status: Some(IssueStatus::InProgress),
                ..Default::default()
            };
            app.storage_mut().update(&issue.id, update).await.unwrap();
            app.save().await.unwrap();

            // Try to reopen an in_progress issue
            let args = ReopenArgs {
                issue_ids: vec![issue.id.to_string()],
                reason: None,
            };

            let result = execute_reopen(&mut app, &args, OutputMode::Text, true).await;

            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(
                error_msg.contains("failed"),
                "Error should indicate failure, got: {}",
                error_msg
            );
        }
    }
}
