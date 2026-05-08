use crate::node::NodeManager;

#[derive(clap::Subcommand, Clone, Debug)]
pub(crate) enum NodeCommand {
    #[command(alias = "l")]
    List,
    #[command(alias = "s")]
    Start {
        #[arg(short, long)]
        node_type: crate::node::NodeType,
    },
    Dev {
        #[arg(short, long)]
        id: u64,
        #[arg(value_parser = crate::util::parse_on_off)]
        state: bool,
    },
    #[command(alias = "i")]
    Info {
        #[arg(short, long)]
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
