use anyhow::{Context, anyhow};
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr};
use tokio::sync::oneshot;

pub(crate) struct Node {
    pub id: u64,
    pub status: crate::node::NodeStatus,
    pub dev_mode: bool,
    pub connection: Option<quinn::Connection>,
    pub session: crate::node::session::protocol::NodeSession,
}

pub(crate) struct NodeService {
    nodes: HashMap<u64, Node>,
    rpc_seq: u64,
    pending_rpc: HashMap<u64, oneshot::Sender<crate::node::session::protocol::RpcMessage>>,
    orchestrator_addr: SocketAddr,
}

impl NodeService {
    pub(crate) fn new(orchestrator_addr: SocketAddr) -> Self {
        Self {
            nodes: HashMap::new(),
            rpc_seq: 1,
            pending_rpc: HashMap::new(),
            orchestrator_addr,
        }
    }

    pub(crate) fn orchestrator_addr(&self) -> SocketAddr {
        self.orchestrator_addr
    }

    pub(crate) fn list_lines(&self) -> Vec<String> {
        self.nodes
            .values()
            .map(|node| {
                format!(
                    "ID: {}, Status: {:?}, Dev: {}, RPC: {}",
                    node.id,
                    node.status,
                    node.dev_mode,
                    node.session.rpc_sender.is_some()
                )
            })
            .collect()
    }

    pub(crate) fn set_dev_mode_local(&mut self, id: u64, state: bool) -> anyhow::Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| anyhow!("Node {id} not found"))?;
        node.dev_mode = state;
        Ok(())
    }

    pub(crate) fn format_info_line(&self, id: u64) -> anyhow::Result<String> {
        let node = self
            .nodes
            .get(&id)
            .ok_or_else(|| anyhow!("Node {id} not found"))?;

        Ok(format!(
            "Node {} => status={:?}, dev={}",
            node.id, node.status, node.dev_mode,
        ))
    }

    pub(crate) fn mark_failed(&mut self, id: u64) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.status = crate::node::NodeStatus::Failed;
        }
    }

    pub(crate) fn mark_connected(&mut self, node_id: u64, connection: quinn::Connection) {
        let node = self.nodes.entry(node_id).or_insert_with(|| Node {
            id: node_id,
            status: crate::node::NodeStatus::Starting,
            dev_mode: false,
            connection: None,
            session: crate::node::session::protocol::NodeSession { rpc_sender: None },
        });

        node.connection = Some(connection);
        node.status = crate::node::NodeStatus::Connected;
    }

    pub(crate) fn set_rpc_sender(
        &mut self,
        node_id: u64,
        sender: std::sync::Arc<tokio::sync::Mutex<quinn::SendStream>>,
    ) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.session.rpc_sender = Some(sender);
        }
    }

    pub(crate) fn route_incoming_rpc(
        &mut self,
        message: crate::node::session::protocol::RpcMessage,
    ) -> Option<crate::node::session::protocol::RpcMessage> {
        if message.method.is_none()
            && let Some(request_id) = message.request_id
            && let Some(tx) = self.pending_rpc.remove(&request_id)
        {
            let _ = tx.send(message);
            return None;
        }

        Some(message)
    }

    pub(crate) fn mark_disconnected(&mut self, node_id: u64) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.status = crate::node::NodeStatus::Disconnected;
            node.connection = None;
            node.session.rpc_sender = None;
        }
    }

    pub(crate) fn rpc_sender(
        &self,
        node_id: u64,
    ) -> Option<std::sync::Arc<tokio::sync::Mutex<quinn::SendStream>>> {
        self.nodes
            .get(&node_id)
            .and_then(|n| n.session.rpc_sender.clone())
    }

    pub(crate) fn next_rpc_request_id(&mut self) -> anyhow::Result<u64> {
        let request_id = self.rpc_seq;
        self.rpc_seq = self
            .rpc_seq
            .checked_add(1)
            .ok_or_else(|| anyhow!("RPC request ID overflow"))?;
        Ok(request_id)
    }

    pub(crate) fn build_rpc_request(
        &self,
        request_id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> crate::node::session::protocol::RpcMessage {
        crate::node::session::protocol::RpcMessage {
            request_id: Some(request_id),
            method: Some(method.to_string()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    pub(crate) fn build_set_dev_params(&self, state: bool) -> serde_json::Value {
        json!({ "state": state })
    }

    pub(crate) fn insert_pending_rpc(
        &mut self,
        request_id: u64,
        tx: oneshot::Sender<crate::node::session::protocol::RpcMessage>,
    ) {
        self.pending_rpc.insert(request_id, tx);
    }

    pub(crate) fn remove_pending_rpc(&mut self, request_id: u64) {
        self.pending_rpc.remove(&request_id);
    }

    pub(crate) async fn send_rpc_request(
        &mut self,
        node_id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<Option<crate::node::session::protocol::RpcMessage>> {
        let sender = match self.rpc_sender(node_id) {
            Some(sender) => sender,
            None => return Ok(None),
        };

        let request_id = self.next_rpc_request_id()?;
        let request = self.build_rpc_request(request_id, method, params);

        let (tx, rx) = oneshot::channel();
        self.insert_pending_rpc(request_id, tx);

        {
            let mut guard = sender.lock().await;
            crate::node::session::protocol::write_json_line(&mut guard, &request)
                .await
                .context("Failed to write RPC request")?;
        }

        match tokio::time::timeout(std::time::Duration::from_secs(3), rx).await {
            Ok(result) => match result {
                Ok(message) => Ok(Some(message)),
                Err(_) => Err(anyhow!("RPC response channel closed")),
            },
            Err(_) => {
                self.remove_pending_rpc(request_id);
                Err(anyhow!("RPC timeout for method {method}"))
            }
        }
    }

    pub(crate) async fn shutdown(&mut self) {
        for node in self.nodes.values_mut() {
            node.status = crate::node::NodeStatus::Disconnected;
            node.connection = None;
            node.session.rpc_sender = None;
        }
    }
}
