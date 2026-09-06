//! Batch handlers for workflow lifecycle commands.

use anyhow::Result;

use super::args::{CloseArgs, ReopenArgs, ReturnToOpenArgs, StartArgs};
use super::execute::{bail_on_batch_failures, output_batch_result, save_or_record_failure};
use crate::domain::{IssueId, LifecycleAction, NoteContent};
use crate::output::OutputMode;

async fn execute_transition_batch<F>(
    app: &mut crate::app::App,
    issue_ids: &[String],
    action: F,
    output_label: &str,
    error_label: &str,
    output_mode: OutputMode,
) -> Result<()>
where
    F: Fn() -> LifecycleAction,
{
    let mut result = super::types::BatchResult::new();
    for id_str in issue_ids {
        let issue_id = IssueId::new(id_str);
        let storage_result = app.storage_mut().transition(&issue_id, action()).await;
        save_or_record_failure(app, &mut result, id_str, storage_result).await;
    }

    output_batch_result(&result, output_label, output_mode)?;
    bail_on_batch_failures(&result, error_label)
}

/// Execute the `start` command.
pub(super) async fn execute_start(
    app: &mut crate::app::App,
    args: &StartArgs,
    output_mode: OutputMode,
) -> Result<()> {
    execute_transition_batch(
        app,
        &args.issue_ids,
        || LifecycleAction::Start,
        "Started",
        "start",
        output_mode,
    )
    .await
}

/// Execute the `return-to-open` command.
pub(super) async fn execute_return_to_open(
    app: &mut crate::app::App,
    args: &ReturnToOpenArgs,
    output_mode: OutputMode,
) -> Result<()> {
    execute_transition_batch(
        app,
        &args.issue_ids,
        || LifecycleAction::ReturnToOpen,
        "Returned to Open",
        "return-to-open",
        output_mode,
    )
    .await
}

/// Execute the `close` command.
pub(super) async fn execute_close(
    app: &mut crate::app::App,
    args: &CloseArgs,
    output_mode: OutputMode,
) -> Result<()> {
    let reason = args
        .reason
        .as_deref()
        .map(NoteContent::closing_reason)
        .transpose()?;
    execute_transition_batch(
        app,
        &args.issue_ids,
        || LifecycleAction::Close {
            reason: reason.clone(),
        },
        "Closed",
        "close",
        output_mode,
    )
    .await
}

/// Execute the `reopen` command.
pub(super) async fn execute_reopen(
    app: &mut crate::app::App,
    args: &ReopenArgs,
    output_mode: OutputMode,
) -> Result<()> {
    let reason = args
        .reason
        .as_deref()
        .map(NoteContent::reopening_reason)
        .transpose()?;
    execute_transition_batch(
        app,
        &args.issue_ids,
        || LifecycleAction::Reopen {
            reason: reason.clone(),
        },
        "Reopened",
        "reopen",
        output_mode,
    )
    .await
}
