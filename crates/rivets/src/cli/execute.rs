//! Command execution logic.
//!
//! This module contains the implementation of all CLI commands.

use std::io::Write;

use anyhow::{Context, Result};

use super::args::{
    AssignmentArgs, BlockedArgs, BlockingDependencyAction, BlockingDependencyArgs, CloseArgs,
    CreateArgs, DeleteArgs, DiscoveryAction, DiscoveryArgs, InfoArgs, InitArgs, LabelAction,
    LabelArgs, ListArgs, ParentAction, ParentArgs, ReadyArgs, RelatedAction, RelatedArgs,
    ReopenArgs, ResourceAction, ResourceArgs, ShowArgs, StaleArgs, StatsArgs, UpdateArgs,
};
use super::types::{SortOrderArg, SortPolicyArg};
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
                Some(super::validators::validate_prefix(trimmed).map_err(|e| {
                    crate::error::Error::Validation {
                        field: "prefix",
                        reason: e,
                    }
                })?)
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
                "Issues: {} total ({} open, {} in progress, {} closed)",
                counts.total, counts.open, counts.in_progress, counts.closed
            );
        }
    }

    Ok(())
}

/// Resolve a create title, including interactive input, before a mutation lock.
pub(super) fn resolve_create_title(args: &CreateArgs) -> Result<String> {
    match &args.title {
        Some(title) => Ok(title.clone()),
        None => {
            eprint!("Title: ");
            std::io::stderr()
                .flush()
                .context("Failed to flush prompt to stderr")?;
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .context("Failed to read title from stdin")?;
            super::validators::validate_title(input.trim()).map_err(|reason| {
                crate::error::Error::Validation {
                    field: "title",
                    reason,
                }
                .into()
            })
        }
    }
}

/// Execute the create command
pub async fn execute_create(
    app: &mut crate::app::App,
    args: &CreateArgs,
    title: String,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, NewIssue, NoteContent};
    use crate::output;

    let prerequisites = args
        .prerequisites
        .iter()
        .cloned()
        .map(IssueId::new)
        .collect();

    let new_issue = NewIssue {
        title,
        description: args.description.clone().unwrap_or_default(),
        priority: args.priority,
        issue_kind: args.issue_kind,
        assignee: args.assignee.clone(),
        labels: args.labels.clone(),
        design: args.design.clone(),
        acceptance_criteria: args.acceptance.clone(),
        initial_note: args.notes.clone().map(NoteContent::new).transpose()?,
        prerequisites,
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
        status: args.status,
        priority: args.priority,
        issue_kind: args.issue_kind,
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

        let prerequisites = app.storage().blocking_prerequisites(&issue_id).await?;
        let dependents = app.storage().blocking_dependents(&issue_id).await?;

        results.push((issue, prerequisites, dependents));
    }

    // Output all results
    match output_mode {
        output::OutputMode::Json => {
            // Always return array for consistency in programmatic usage
            let json_results: Vec<_> = results
                .iter()
                .map(|(issue, prerequisites, dependents)| {
                    serde_json::json!({
                        "id": issue.id.to_string(),
                        "title": issue.title,
                        "description": issue.description,
                        "status": format!("{}", issue.status),
                        "priority": issue.priority,
                        "issue_kind": format!("{}", issue.issue_kind),
                        "assignee": issue.assignee,
                        "labels": issue.labels,
                        "design": issue.design,
                        "acceptance_criteria": issue.acceptance_criteria,
                        "notes": issue.notes(),
                        "resources": issue.resources(),
                        "created_at": issue.created_at,
                        "updated_at": issue.updated_at,
                        "closed_at": issue.closed_at,
                        "blocking_prerequisites": prerequisites,
                        "blocking_dependents": dependents,
                    })
                })
                .collect();
            output::print_json(&json_results)?;
        }
        output::OutputMode::Text => {
            for (i, (issue, prerequisites, dependents)) in results.iter().enumerate() {
                if i > 0 {
                    println!();
                    println!("---");
                    println!();
                }
                output::print_issue_details(issue, prerequisites, dependents, output_mode)?;
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
    use crate::domain::{IssueId, IssueUpdate, NoteContent};

    if !args.has_updates() {
        anyhow::bail!(
            "No fields specified to update. Use one or more of:\n  {}\n\n\
             Example: rivets update ISSUE-ID --title 'New title' --priority 1",
            UpdateArgs::available_flags_help()
        );
    }

    let mut result = BatchResult::new();
    let note = args.notes.clone().map(NoteContent::new).transpose()?;

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);

        // Build the update (same for all issues)
        let update = IssueUpdate {
            title: args.title.clone(),
            description: args.description.clone(),
            status: args.status,
            priority: args.priority,
            issue_kind: args.issue_kind,
            design: args.design.clone(),
            acceptance_criteria: args.acceptance.clone(),
            note: note.clone(),
            ..Default::default()
        };

        let storage_result = app.storage_mut().update(&issue_id, update).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    output_batch_result(&result, "Updated", output_mode)?;
    bail_on_batch_failures(&result, "update")
}
#[derive(Clone, Copy)]
enum AssignmentOperation {
    Claim,
    Release,
}

async fn execute_assignment(
    app: &mut crate::app::App,
    args: &AssignmentArgs,
    output_mode: OutputMode,
    operation: AssignmentOperation,
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

    let issue_id = IssueId::new(&args.issue_id);
    let issue = match operation {
        AssignmentOperation::Claim => app.storage_mut().claim(&issue_id, &args.assignee).await?,
        AssignmentOperation::Release => {
            app.storage_mut().release(&issue_id, &args.assignee).await?
        }
    };
    app.save().await?;

    match output_mode {
        OutputMode::Json => output::print_json(&issue)?,
        OutputMode::Text => match operation {
            AssignmentOperation::Claim => {
                println!("Claimed issue {} for {}", issue.id, args.assignee);
            }
            AssignmentOperation::Release => {
                println!("Released issue {} from {}", issue.id, args.assignee);
            }
        },
    }
    Ok(())
}

/// Atomically claim one Open, unblocked Issue.
pub async fn execute_claim(
    app: &mut crate::app::App,
    args: &AssignmentArgs,
    output_mode: OutputMode,
) -> Result<()> {
    execute_assignment(app, args, output_mode, AssignmentOperation::Claim).await
}

/// Atomically release one Open Issue from its exact Assignee.
pub async fn execute_release(
    app: &mut crate::app::App,
    args: &AssignmentArgs,
    output_mode: OutputMode,
) -> Result<()> {
    execute_assignment(app, args, output_mode, AssignmentOperation::Release).await
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
                IssueStatus::Closed => counts.closed += 1,
            }
            counts
        })
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
    let bytes_read = std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read confirmation from stdin")?;
    // EOF (e.g. Ctrl+D or piped input) is treated as "no"
    if bytes_read == 0 {
        eprintln!();
        return Ok(false);
    }
    let response = input.trim().to_lowercase();
    Ok(response == "y" || response == "yes")
}

/// Confirm a batch operation affecting multiple issues.
///
/// Returns `Ok(true)` to proceed, `Ok(false)` if the user cancelled.
/// Skips the prompt when `skip_confirm` is true or only one issue is affected.
pub(super) fn confirm_batch(action: &str, count: usize, skip_confirm: bool) -> Result<bool> {
    if count > 1 && !skip_confirm {
        let prompt = format!("{action} {count} issues?");
        if !confirm_action(&prompt)? {
            println!("{action} cancelled.");
            return Ok(false);
        }
    }
    Ok(true)
}

/// Resolve an interactive delete confirmation before a mutation lock.
pub(super) async fn confirm_delete(
    app: &crate::app::App,
    args: &DeleteArgs,
    skip_confirm: bool,
) -> Result<bool> {
    use crate::domain::IssueId;

    if args.force || skip_confirm {
        return Ok(true);
    }

    let issue_id = IssueId::new(&args.issue_id);
    let issue = app
        .storage()
        .get(&issue_id)
        .await?
        .ok_or(crate::error::Error::IssueNotFound(issue_id))?;
    let prompt = format!("Delete issue '{}' ({})?", issue.id, issue.title);
    if !confirm_action(&prompt)? {
        println!("Deletion cancelled.");
        return Ok(false);
    }
    Ok(true)
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
    use super::types::BatchResult;
    use crate::domain::{IssueId, IssueStatus, IssueUpdate, NoteContent};

    let note = args
        .reason
        .as_deref()
        .map(NoteContent::closing_reason)
        .transpose()?;

    let mut result = BatchResult::new();

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);
        let update = IssueUpdate {
            status: Some(IssueStatus::Closed),
            note: note.clone(),
            ..Default::default()
        };

        // Missing issues and invalid transitions are rejected by the
        // storage/domain seam (ADR-0005); no adapter-local checks here.
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
) -> Result<()> {
    use super::types::BatchResult;
    use crate::domain::{IssueId, IssueStatus, IssueUpdate, NoteContent};

    let note = args
        .reason
        .as_deref()
        .map(NoteContent::reopening_reason)
        .transpose()?;

    let mut result = BatchResult::new();

    for id_str in &args.issue_ids {
        let issue_id = IssueId::new(id_str);
        let update = IssueUpdate {
            status: Some(IssueStatus::Open),
            note: note.clone(),
            ..Default::default()
        };

        let storage_result = match app.storage().get(&issue_id).await {
            Ok(Some(issue)) => match issue.status.validate_reopen() {
                Ok(()) => app.storage_mut().update(&issue_id, update).await,
                Err(source) => Err(crate::error::Error::Storage(
                    crate::error::StorageError::InvalidStatusTransition(source),
                )),
            },
            Ok(None) => Err(crate::error::Error::IssueNotFound(issue_id.clone())),
            Err(error) => Err(error),
        };
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
) -> Result<()> {
    use crate::domain::IssueId;
    use crate::output;

    let issue_id = IssueId::new(&args.issue_id);

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
    use crate::domain::{ReadyAssignmentFilter, ReadyFilter, SortPolicy};
    use crate::output;

    let assignment = if args.all_assignees {
        ReadyAssignmentFilter::All
    } else if let Some(assignee) = &args.assignee {
        ReadyAssignmentFilter::Assignee(assignee.clone())
    } else {
        ReadyAssignmentFilter::Unassigned
    };
    let filter = ReadyFilter {
        priority: args.priority,
        issue_kind: args.issue_kind,
        assignment,
        label: args.label.clone(),
        limit: Some(args.limit),
    };

    let sort_policy = match args.sort {
        SortPolicyArg::Hybrid => SortPolicy::Hybrid,
        SortPolicyArg::Priority => SortPolicy::Priority,
        SortPolicyArg::Oldest => SortPolicy::Oldest,
    };

    let issues = app
        .storage()
        .ready_to_work(&filter, Some(sort_policy))
        .await?;

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

/// Execute one canonical Blocking Dependency operation.
pub async fn execute_blocking_dependency(
    app: &mut crate::app::App,
    args: &BlockingDependencyArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{BlockingDependency, IssueId};
    use crate::output;

    match &args.action {
        BlockingDependencyAction::Add {
            dependent,
            prerequisite,
        } => {
            let dependency =
                BlockingDependency::new(IssueId::new(dependent), IssueId::new(prerequisite))?;
            app.storage_mut()
                .add_blocking_dependency(dependency.clone())
                .await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "add",
                    "relationship": "blocking_dependency",
                    "dependent_id": dependency.dependent_id(),
                    "prerequisite_id": dependency.prerequisite_id(),
                    "status": "success"
                }))?,
                OutputMode::Text => println!(
                    "{} depends on {}",
                    dependency.dependent_id(),
                    dependency.prerequisite_id()
                ),
            }
        }
        BlockingDependencyAction::Remove {
            dependent,
            prerequisite,
        } => {
            let dependency =
                BlockingDependency::new(IssueId::new(dependent), IssueId::new(prerequisite))?;
            app.storage_mut()
                .remove_blocking_dependency(&dependency)
                .await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "remove",
                    "relationship": "blocking_dependency",
                    "dependent_id": dependency.dependent_id(),
                    "prerequisite_id": dependency.prerequisite_id(),
                    "status": "success"
                }))?,
                OutputMode::Text => println!(
                    "Removed: {} no longer depends on {}",
                    dependency.dependent_id(),
                    dependency.prerequisite_id()
                ),
            }
        }
        BlockingDependencyAction::List(query) => {
            let dependencies = match (&query.dependent, &query.prerequisite) {
                (Some(dependent), None) => {
                    app.storage()
                        .blocking_prerequisites(&IssueId::new(dependent))
                        .await?
                }
                (None, Some(prerequisite)) => {
                    app.storage()
                        .blocking_dependents(&IssueId::new(prerequisite))
                        .await?
                }
                (Some(_), Some(_)) | (None, None) => {
                    anyhow::bail!("provide exactly one of --dependent or --prerequisite")
                }
            };
            match output_mode {
                OutputMode::Json => output::print_json(&dependencies)?,
                OutputMode::Text if dependencies.is_empty() => {
                    println!("No Blocking Dependencies found");
                }
                OutputMode::Text => {
                    for dependency in dependencies {
                        println!(
                            "{} depends on {}",
                            dependency.dependent_id(),
                            dependency.prerequisite_id()
                        );
                    }
                }
            }
        }
        BlockingDependencyAction::Tree { dependent, depth } => {
            let max_depth = (*depth != 0).then_some(*depth);
            let tree = app
                .storage()
                .blocking_dependency_tree(&IssueId::new(dependent), max_depth)
                .await?;
            match output_mode {
                OutputMode::Json => {
                    let rows = tree
                        .iter()
                        .map(|(dependency, depth)| {
                            serde_json::json!({
                                "dependent_id": dependency.dependent_id(),
                                "prerequisite_id": dependency.prerequisite_id(),
                                "depth": depth
                            })
                        })
                        .collect::<Vec<_>>();
                    output::print_json(&serde_json::json!({
                        "root_dependent_id": dependent,
                        "prerequisites": rows
                    }))?;
                }
                OutputMode::Text if tree.is_empty() => {
                    println!("{dependent} has no Blocking prerequisites");
                }
                OutputMode::Text => {
                    println!("Blocking prerequisites of {dependent}:");
                    for (dependency, depth) in tree {
                        println!(
                            "{}{} depends on {}",
                            "  ".repeat(depth),
                            dependency.dependent_id(),
                            dependency.prerequisite_id()
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Execute one Related Association operation.
pub async fn execute_related(
    app: &mut crate::app::App,
    args: &RelatedArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, RelatedAssociation};
    use crate::output;

    match &args.action {
        RelatedAction::Add { issue, related } => {
            let association = RelatedAssociation::new(IssueId::new(issue), IssueId::new(related))?;
            app.storage_mut()
                .add_related_association(association.clone())
                .await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "add",
                    "relationship": "related",
                    "left_issue_id": association.left_issue_id(),
                    "right_issue_id": association.right_issue_id(),
                    "status": "success"
                }))?,
                OutputMode::Text => println!(
                    "{} is related to {}",
                    association.left_issue_id(),
                    association.right_issue_id()
                ),
            }
        }
        RelatedAction::Remove { issue, related } => {
            let association = RelatedAssociation::new(IssueId::new(issue), IssueId::new(related))?;
            app.storage_mut()
                .remove_related_association(&association)
                .await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "remove",
                    "relationship": "related",
                    "left_issue_id": association.left_issue_id(),
                    "right_issue_id": association.right_issue_id(),
                    "status": "success"
                }))?,
                OutputMode::Text => println!(
                    "Removed: {} is no longer related to {}",
                    association.left_issue_id(),
                    association.right_issue_id()
                ),
            }
        }
        RelatedAction::List { issue } => {
            let associations = app
                .storage()
                .related_associations(&IssueId::new(issue))
                .await?;
            match output_mode {
                OutputMode::Json => output::print_json(&associations)?,
                OutputMode::Text if associations.is_empty() => {
                    println!("No Related Associations found");
                }
                OutputMode::Text => {
                    for association in associations {
                        println!(
                            "{} is related to {}",
                            association.left_issue_id(),
                            association.right_issue_id()
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Execute one Discovery Origin operation.
pub async fn execute_discovery(
    app: &mut crate::app::App,
    args: &DiscoveryArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{DiscoveryOrigin, IssueId};
    use crate::output;

    match &args.action {
        DiscoveryAction::Add { discovered, source } => {
            let origin = DiscoveryOrigin::new(IssueId::new(discovered), IssueId::new(source))?;
            app.storage_mut().add_discovery_origin(origin).await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "add",
                    "relationship": "discovery_origin",
                    "discovered_issue_id": discovered,
                    "source_issue_id": source,
                    "status": "success"
                }))?,
                OutputMode::Text => println!("{discovered} was discovered from {source}"),
            }
        }
        DiscoveryAction::Remove { discovered, source } => {
            let origin = DiscoveryOrigin::new(IssueId::new(discovered), IssueId::new(source))?;
            app.storage_mut().remove_discovery_origin(&origin).await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "remove",
                    "relationship": "discovery_origin",
                    "discovered_issue_id": discovered,
                    "source_issue_id": source,
                    "status": "success"
                }))?,
                OutputMode::Text => {
                    println!("Removed: {discovered} was discovered from {source}");
                }
            }
        }
        DiscoveryAction::List { discovered } => {
            let origins = app
                .storage()
                .discovery_origins(&IssueId::new(discovered))
                .await?;
            match output_mode {
                OutputMode::Json => output::print_json(&origins)?,
                OutputMode::Text if origins.is_empty() => {
                    println!("No Discovery Origins found");
                }
                OutputMode::Text => {
                    for origin in origins {
                        println!(
                            "{} was discovered from {}",
                            origin.discovered_issue_id(),
                            origin.source_issue_id()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Execute one canonical Parentage operation.
pub async fn execute_parent(
    app: &mut crate::app::App,
    args: &ParentArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, Parentage};
    use crate::output;

    match &args.action {
        ParentAction::Set { child, parent } => {
            let parentage = Parentage::new(IssueId::new(child), IssueId::new(parent))?;
            app.storage_mut().set_parent(parentage.clone()).await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "set",
                    "relationship": "parentage",
                    "child_id": parentage.child_id(),
                    "parent_id": parentage.parent_id(),
                    "status": "success"
                }))?,
                OutputMode::Text => println!(
                    "Set parent: {} -> {}",
                    parentage.child_id(),
                    parentage.parent_id()
                ),
            }
        }
        ParentAction::Clear { child } => {
            let removed = app.storage_mut().clear_parent(&IssueId::new(child)).await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "clear",
                    "relationship": "parentage",
                    "child_id": removed.child_id(),
                    "parent_id": removed.parent_id(),
                    "status": "success"
                }))?,
                OutputMode::Text => println!(
                    "Cleared parent: {} was owned by {}",
                    removed.child_id(),
                    removed.parent_id()
                ),
            }
        }
        ParentAction::Move { child, parent } => {
            let parentage = Parentage::new(IssueId::new(child), IssueId::new(parent))?;
            let previous = app.storage_mut().move_parent(parentage.clone()).await?;
            app.save().await?;
            match output_mode {
                OutputMode::Json => output::print_json(&serde_json::json!({
                    "action": "move",
                    "relationship": "parentage",
                    "child_id": parentage.child_id(),
                    "previous_parent_id": previous.parent_id(),
                    "parent_id": parentage.parent_id(),
                    "status": "success"
                }))?,
                OutputMode::Text => println!(
                    "Moved parent: {} from {} to {}",
                    parentage.child_id(),
                    previous.parent_id(),
                    parentage.parent_id()
                ),
            }
        }
        ParentAction::Show { child } => {
            let child_id = IssueId::new(child);
            let parentage = app.storage().parent_of(&child_id).await?;
            match (output_mode, parentage) {
                (OutputMode::Json, Some(parentage)) => {
                    output::print_json(&serde_json::json!({
                        "relationship": "parentage",
                        "child_id": parentage.child_id(),
                        "parent_id": parentage.parent_id()
                    }))?;
                }
                (OutputMode::Json, None) => output::print_json(&serde_json::json!({
                    "relationship": "parentage",
                    "child_id": child_id,
                    "parent_id": null
                }))?,
                (OutputMode::Text, Some(parentage)) => println!(
                    "{} has parent {}",
                    parentage.child_id(),
                    parentage.parent_id()
                ),
                (OutputMode::Text, None) => println!("{child_id} has no parent"),
            }
        }
    }

    Ok(())
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
    label: &crate::domain::Label,
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
    label: &crate::domain::Label,
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
    let all_labels: BTreeSet<crate::domain::Label> = all_issues
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

/// Parse at most one of `--url`/`--path` into a Resource Target.
///
/// The four-arm match is the single canonical url/path classification for
/// the CLI; `Add` layers its "exactly one" requirement on the `None` case.
fn parse_target_flags(
    url: Option<&str>,
    path: Option<&str>,
) -> Result<Option<crate::domain::ResourceTarget>> {
    use crate::domain::{ResourceTarget, WebUrl, WorkspacePath};
    match (url, path) {
        (Some(url), None) => Ok(Some(ResourceTarget::web(WebUrl::new(url)?))),
        (None, Some(path)) => Ok(Some(ResourceTarget::path(WorkspacePath::new(path)?))),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => anyhow::bail!("only one of --url or --path may be given"),
    }
}

/// Execute an Associated Resource command.
pub async fn execute_resource(
    app: &mut crate::app::App,
    args: &ResourceArgs,
    output_mode: OutputMode,
) -> Result<()> {
    use crate::domain::{IssueId, NewResource, ResourceId, ResourceLabel, ResourceUpdate};
    use crate::output;

    match &args.action {
        ResourceAction::Add {
            issue_id,
            url,
            path,
            role,
            label,
        } => {
            let target = parse_target_flags(url.as_deref(), path.as_deref())?
                .ok_or_else(|| anyhow::anyhow!("exactly one of --url or --path is required"))?;
            let resource = NewResource {
                target,
                role: *role,
                label: label.clone().map(ResourceLabel::new).transpose()?,
            };
            let issue = app
                .storage_mut()
                .add_resource(&IssueId::new(issue_id), resource)
                .await?;
            app.save().await?;

            match output_mode {
                output::OutputMode::Json => output::print_json(&issue)?,
                output::OutputMode::Text => {
                    println!("Added resource to {}", issue.id);
                }
            }
        }
        ResourceAction::Update {
            issue_id,
            resource,
            url,
            path,
            role,
            label,
            no_label,
        } => {
            let target = parse_target_flags(url.as_deref(), path.as_deref())?;
            let label = match (label, no_label) {
                (Some(label), false) => Some(Some(ResourceLabel::new(label)?)),
                (None, true) => Some(None),
                (None, false) => None,
                (Some(_), true) => {
                    anyhow::bail!("only one of --label or --no-label may be given")
                }
            };
            let update = ResourceUpdate {
                target,
                role: *role,
                label,
            };
            let issue = app
                .storage_mut()
                .update_resource(&IssueId::new(issue_id), &ResourceId::new(resource)?, update)
                .await?;
            app.save().await?;

            match output_mode {
                output::OutputMode::Json => output::print_json(&issue)?,
                output::OutputMode::Text => {
                    println!("Updated resource {resource} on {}", issue.id);
                }
            }
        }
        ResourceAction::Remove { issue_id, resource } => {
            let issue = app
                .storage_mut()
                .remove_resource(&IssueId::new(issue_id), &ResourceId::new(resource)?)
                .await?;
            app.save().await?;

            match output_mode {
                output::OutputMode::Json => output::print_json(&issue)?,
                output::OutputMode::Text => {
                    println!("Removed resource {resource} from {}", issue.id);
                }
            }
        }
        ResourceAction::List { issue_id } => {
            let id = IssueId::new(issue_id);
            let issue = app
                .storage()
                .get(&id)
                .await?
                .ok_or_else(|| crate::error::Error::IssueNotFound(id.clone()))?;

            match output_mode {
                output::OutputMode::Json => output::print_json(&issue.resources())?,
                output::OutputMode::Text => {
                    if issue.resources().is_empty() {
                        println!("{issue_id} has no associated resources");
                    } else {
                        println!("Resources for {} ({}):", issue_id, issue.resources().len());
                        for resource in issue.resources() {
                            println!("  {resource}");
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
        status: args.status,
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
    use crate::domain::{IssueFilter, ReadyFilter};
    use crate::output;

    // Get all issues and count by status
    let all_issues = app.storage().list(&IssueFilter::default()).await?;
    let counts = count_by_status(&all_issues);

    // Ready defaults to unassigned Issues.
    let ready = app
        .storage()
        .ready_to_work(&ReadyFilter::default(), None)
        .await?
        .len();

    // Blocked issues (by dependencies)
    let blocked_by_deps = app.storage().blocked_issues().await?.len();

    match output_mode {
        output::OutputMode::Json => {
            let mut stats = serde_json::json!({
                "total": counts.total,
                "by_status": {
                    "open": counts.open,
                    "in_progress": counts.in_progress,
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
    use crate::domain::{Issue, IssueId, IssueKind, IssueStatus};
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
            issue_kind: IssueKind::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: vec![],
            resources: vec![],
            next_resource_id: 1,
            dependencies: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        }
    }

    #[tokio::test]
    async fn parent_move_rejects_self_before_parent_lookup() {
        use crate::domain::ParentageError;
        use crate::output::OutputMode;

        let directory = TempDir::new().expect("temporary Workspace should exist");
        crate::commands::init::init(directory.path(), Some("test"))
            .await
            .expect("Workspace should initialize");
        let mut app = crate::app::App::from_directory_for_mutation(directory.path())
            .await
            .expect("Workspace mutation guard should load");
        app.storage_mut()
            .import_issues(vec![create_test_issue("test-existing")])
            .await
            .expect("unparented Issue should import");
        app.save().await.expect("fixture should persist");
        let path = directory.path().join(".rivets/issues.jsonl");
        let before = std::fs::read(&path).expect("fixture should be readable");

        for child in ["test-existing", "test-missing"] {
            let args = ParentArgs {
                action: ParentAction::Move {
                    child: child.to_string(),
                    parent: child.to_string(),
                },
            };
            let error = execute_parent(&mut app, &args, OutputMode::Json)
                .await
                .expect_err("a self-parent request must be rejected");
            assert!(
                matches!(
                    error.downcast_ref::<ParentageError>(),
                    Some(ParentageError::SelfReference { issue_id }) if issue_id.as_str() == child
                ),
                "wrong rejection: {error:?}"
            );
            assert_eq!(
                std::fs::read(&path).expect("records should remain readable"),
                before
            );
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
            issues[3].status = IssueStatus::InProgress;
            issues[4].status = IssueStatus::Closed;
            issues[5].status = IssueStatus::Closed;

            let counts = count_by_status(&issues);
            assert_eq!(counts.total, 6);
            assert_eq!(counts.open, 2);
            assert_eq!(counts.in_progress, 2);
            assert_eq!(counts.closed, 2);
        }

        #[test]
        fn test_all_same_status() {
            let issues: Vec<_> = (1..=5)
                .map(|i| {
                    let mut issue = create_test_issue(&format!("test-{}", i));
                    issue.status = IssueStatus::InProgress;
                    issue
                })
                .collect();

            let counts = count_by_status(&issues);
            assert_eq!(counts.total, 5);
            assert_eq!(counts.open, 0);
            assert_eq!(counts.in_progress, 5);
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
        use super::super::{UpdateArgs, execute_update};
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
                issue_kind: None,
                design: None,
                acceptance: None,
                notes: None,
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
        use super::super::{CloseArgs, execute_close};
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

            let result = execute_close(&mut app, &args, OutputMode::Text).await;

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
        use super::super::{ReopenArgs, execute_reopen};
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

            let result = execute_reopen(&mut app, &args, OutputMode::Text).await;

            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(
                error_msg.contains("failed"),
                "Error should indicate failure, got: {}",
                error_msg
            );
        }

        #[tokio::test]
        async fn test_reopen_in_progress_issue_is_rejected_without_mutation() {
            use crate::domain::{IssueStatus, IssueUpdate};

            let temp_dir = TempDir::new().unwrap();
            crate::commands::init::init(temp_dir.path(), Some("test"))
                .await
                .unwrap();

            let mut app = crate::app::App::from_directory(temp_dir.path())
                .await
                .unwrap();

            let new_issue = NewIssue {
                title: "Test issue".to_string(),
                assignee: Some("alice".to_string()),
                ..Default::default()
            };
            let issue = app.storage_mut().create(new_issue).await.unwrap();
            app.storage_mut()
                .update(
                    &issue.id,
                    IssueUpdate {
                        status: Some(IssueStatus::InProgress),
                        ..Default::default()
                    },
                )
                .await
                .expect("assigned Issue should enter In Progress");
            app.save().await.unwrap();

            let args = ReopenArgs {
                issue_ids: vec![issue.id.to_string()],
                reason: None,
            };
            execute_reopen(&mut app, &args, OutputMode::Text)
                .await
                .expect_err("In Progress is not a Closed Issue");

            let reopened = app
                .storage()
                .get(&issue.id)
                .await
                .expect("issue lookup should succeed")
                .expect("Issue should remain");
            assert_eq!(reopened.status, IssueStatus::InProgress);
            assert_eq!(reopened.assignee.as_deref(), Some("alice"));
        }
    }
}
