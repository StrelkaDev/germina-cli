mod check;
mod launch;

use anyhow::{Context, anyhow};
use std::path::Path;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

#[derive(clap::Subcommand, Clone, Debug)]
pub(crate) enum CoreCommand {
    /// Validate runtime folder hierarchy and component availability
    Check {
        #[command(flatten)]
        command: check::CheckCommand,
    },
    /// Manage node sessions
    Node {
        #[command(subcommand)]
        command: crate::node::command::NodeCommand,
    },
    /// Manage web servers
    Web {
        #[command(subcommand)]
        command: crate::web::command::WebCommand,
    },
}

pub(crate) struct CoreRequest {
    pub command: CoreCommand,
    pub completion_tx: oneshot::Sender<anyhow::Result<()>>,
}

pub struct Core {
    root_path: PathBuf,
    node_manager: crate::node::NodeManager,
    web_manager: crate::web::WebManager,
    rx: mpsc::Receiver<CoreRequest>,
}

impl Core {
    pub fn new(
        config: crate::config::AppConfig,
    ) -> anyhow::Result<(Self, mpsc::Sender<CoreRequest>)> {
        let root_path = resolve_root_path(&config)?;
        let cli_endpoint = config.cli_endpoint();
        launch::ensure_launch_configs(root_path.as_path(), cli_endpoint.as_str())?;
        let (tx, rx) = mpsc::channel(100);
        let core = Self {
            root_path,
            node_manager: crate::node::NodeManager::new(config.node_bind_addr()),
            web_manager: crate::web::WebManager::new(config.web_bind_addr()),
            rx,
        };
        Ok((core, tx))
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        while let Some(request) = self.rx.recv().await {
            let result = match request.command {
                CoreCommand::Check { command } => command.execute(self.root_path.as_path()).await,
                CoreCommand::Node { command } => command.execute(&mut self.node_manager).await,
                CoreCommand::Web { command } => command.execute(&mut self.web_manager).await,
            };

            let _ = request.completion_tx.send(result);
        }

        self.node_manager.shutdown().await;
        Ok(())
    }

    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        self.node_manager.ensure_listener().await
    }
}

fn resolve_root_path(config: &crate::config::AppConfig) -> anyhow::Result<PathBuf> {
    let raw_root = if let Some(root) = config.root_path() {
        root.to_path_buf()
    } else {
        let exe = std::env::current_exe().context("Failed to determine current executable")?;
        exe.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("Current executable has no parent directory"))?
    };

    std::fs::canonicalize(&raw_root)
        .with_context(|| format!("Failed to resolve root path {}", raw_root.display()))
}
