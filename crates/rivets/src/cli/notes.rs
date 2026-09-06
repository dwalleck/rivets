//! Batch handlers for Issue Note commands.

use anyhow::Result;

use super::args::{NoteAction, NoteArgs};
use super::execute::{bail_on_batch_failures, output_batch_result, save_or_record_failure};
use crate::domain::{IssueId, NoteContent};
use crate::output::OutputMode;

/// Execute the `note` command.
pub(super) async fn execute_note(
    app: &mut crate::app::App,
    args: &NoteArgs,
    output_mode: OutputMode,
) -> Result<()> {
    match &args.action {
        NoteAction::Append { issue_ids, content } => {
            let content = NoteContent::new(content.clone())?;
            let mut result = super::types::BatchResult::new();
            for id_str in issue_ids {
                let issue_id = IssueId::new(id_str);
                let storage_result = app
                    .storage_mut()
                    .append_note(&issue_id, content.clone())
                    .await;
                save_or_record_failure(app, &mut result, id_str, storage_result).await;
            }

            output_batch_result(&result, "Appended Note", output_mode)?;
            bail_on_batch_failures(&result, "note append")
        }
    }
}
