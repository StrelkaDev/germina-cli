use crate::node::NodeManager;

#[derive(clap::Subcommand, Clone, Debug)]
pub(crate) enum NodeCommand {
    /// List all known nodes
    #[command(alias = "l")]
    List,
    /// Spawn a new node process
    #[command(alias = "s")]
    Start {
        #[arg(short = 't', long = "type")]
        node_type: crate::node::NodeType,
    },
    /// Toggle dev mode on a node
    Dev {
        #[arg(short, long, value_name = "ID", help = "node id")]
        id: u64,
        #[arg(
            action = clap::ArgAction::Set,
            value_parser = crate::util::parse_on_off,
            value_name = "STATE",
            help = "(on/off, true/false, 1/0)"
        )]
        state: bool,
    },
    /// Show detailed info about a node
    #[command(alias = "i")]
    Info {
        #[arg(short, long, value_name = "ID", help = "node id")]
        id: u64,
    },
}

impl NodeCommand {
    pub(crate) async fn execute(&self, manager: &mut NodeManager) -> anyhow::Result<()> {
        match self {
            NodeCommand::List => {
                manager.list();
            }
            NodeCommand::Start { node_type } => {
                manager.start(*node_type).await?;
            }
            NodeCommand::Dev { id, state } => {
                manager.set_dev_mode(*id, *state).await?;
            }
            NodeCommand::Info { id } => {
                manager.info(*id).await?;
            }
        }
        Ok(())
    }
}
