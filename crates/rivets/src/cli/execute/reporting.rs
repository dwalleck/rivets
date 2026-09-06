//! Orchestration for workspace information and statistics commands.

use anyhow::Result;

use super::super::args::InfoArgs;
use crate::app::App;
use crate::output::{self, OutputMode};

/// Execute the info command from the initialized application snapshot.
pub(in crate::cli) async fn execute_info(
    app: &App,
    _args: &InfoArgs,
    output_mode: OutputMode,
) -> Result<()> {
    output::print_information(app.information(), output_mode)?;
    Ok(())
}

/// Execute the stats command from one storage statistics snapshot.
pub(in crate::cli) async fn execute_stats(app: &App, output_mode: OutputMode) -> Result<()> {
    let statistics = app.storage().statistics().await?;
    output::print_statistics(&statistics, output_mode)?;
    Ok(())
}
