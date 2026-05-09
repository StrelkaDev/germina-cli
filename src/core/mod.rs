mod check;

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
    node_manager: crate::node::NodeManager,
    web_manager: crate::web::WebManager,
    rx: mpsc::Receiver<CoreRequest>,
}

impl Core {
    pub fn new(config: crate::config::AppConfig) -> (Self, mpsc::Sender<CoreRequest>) {
        let (tx, rx) = mpsc::channel(100);
        let core = Self {
            node_manager: crate::node::NodeManager::new(config.node_bind_addr()),
            web_manager: crate::web::WebManager::new(config.web_bind_addr()),
            rx,
        };
        (core, tx)
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        while let Some(request) = self.rx.recv().await {
            let result = match request.command {
                CoreCommand::Check { command } => command.execute().await,
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
