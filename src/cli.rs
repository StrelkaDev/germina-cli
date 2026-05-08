use crate::core::CoreCommand;
use tokio::sync::mpsc;

#[derive(clap::Parser)]
pub(crate) struct ReplCommand {
    #[command(subcommand)]
    pub command: CoreCommand,
}

pub async fn run_loop(tx: mpsc::Sender<CoreCommand>) -> anyhow::Result<()> {}
