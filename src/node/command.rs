use crate::node::NodeManager;

#[derive(clap::Subcommand, Clone, Debug)]
#[command(about = "Node session commands")]
pub(crate) enum NodeCommand {
    /// List known node sessions and their states
    #[command(alias = "l")]
    List,
    /// Change node dev mode (on/off)
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
    /// Show detailed information for a node
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
