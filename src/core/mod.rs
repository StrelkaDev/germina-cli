use std::net::SocketAddr;
use tokio::sync::{mpsc, oneshot};

pub(crate) type CommandMsg = (CoreCommand, oneshot::Sender<()>);

#[derive(clap::Subcommand, Clone, Debug)]
pub(crate) enum CoreCommand {
    /// Manage node processes
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

pub struct Core {
    node_manager: crate::node::NodeManager,
    web_manager: crate::web::WebManager,
    rx: mpsc::Receiver<CommandMsg>,
}

impl Core {
    pub fn new(node_addr: SocketAddr, web_addr: SocketAddr) -> (Self, mpsc::Sender<CommandMsg>) {
        let (tx, rx) = mpsc::channel(100);
        let core = Self {
            node_manager: crate::node::NodeManager::new(node_addr),
            web_manager: crate::web::WebManager::new(web_addr),
            rx,
        };
        (core, tx)
    }

    pub async fn run(&mut self, ready_tx: oneshot::Sender<()>) -> anyhow::Result<()> {
        self.node_manager.ensure_listener().await?;
        let _ = ready_tx.send(());

        while let Some((command, done_tx)) = self.rx.recv().await {
            let result = match command {
                CoreCommand::Node { command } => command.execute(&mut self.node_manager).await,
                CoreCommand::Web { command } => command.execute(&mut self.web_manager).await,
            };

            if let Err(err) = result {
                eprintln!("Command failed: {err}");
            }
            let _ = done_tx.send(());
        }

        self.node_manager.shutdown().await;
        Ok(())
    }
}
