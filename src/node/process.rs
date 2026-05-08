use crate::node::NodeType;
use anyhow::anyhow;
use std::net::SocketAddr;
use tokio::process::Child;

pub(crate) struct SpawnedNodeProcess {
    pub child: Child,
}

pub(crate) fn spawn_node_process(
    _orchestrator_addr: SocketAddr,
    _node_id: u64,
    _node_type: NodeType,
    _dev: bool,
) -> anyhow::Result<SpawnedNodeProcess> {
    Err(anyhow!("Node process launching is temporarily disabled"))
}
