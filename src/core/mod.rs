use tokio::sync::mpsc;

#[derive(clap::Subcommand, Clone, Debug)]
pub(crate) enum CoreCommand {
    Node {
        #[command(subcommand)]
        command: crate::node::command::NodeCommand,
    },
    Web {
        #[command(subcommand)]
        command: crate::web::command::WebCommand,
    },
    Exit,
}

pub struct Core {
    node_manager: crate::node::NodeManager,
    web_manager: crate::web::WebManager,
    rx: mpsc::Receiver<CoreCommand>,
}

impl Core {
    pub fn new() -> (Self, mpsc::Sender<CoreCommand>) {
        let (tx, rx) = mpsc::channel(100);
        let core = Self {
            node_manager: crate::node::NodeManager::new(),
            web_manager: crate::web::WebManager::new(),
            rx,
        };
        (core, tx)
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        self.node_manager.ensure_listener().await?;

        while let Some(command) = self.rx.recv().await {
            match command {
                CoreCommand::Node { command } => command.execute(&mut self.node_manager).await?,
                CoreCommand::Web { command } => command.execute(&mut self.web_manager).await?,
                CoreCommand::Exit => break,
            }
        }

        self.node_manager.shutdown().await;
        Ok(())
    }
}
