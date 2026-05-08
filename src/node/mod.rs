use anyhow::{Context, anyhow};
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr, time::Duration};
use tokio::sync::{mpsc, oneshot};

pub mod command;
pub mod process;
pub mod session;

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub(crate) enum NodeType {
    Client,
    Server,
    Tools,
}

impl NodeType {
    pub(crate) fn to_string(self) -> &'static str {
        match self {
            NodeType::Client => "client",
            NodeType::Server => "server",
            NodeType::Tools => "tools",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum NodeStatus {
    Starting,
    Connected,
    Disconnected,
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) struct NodeRuntimeConfig {
    pub id: u64,
    pub node_type: NodeType,
    pub name: String,
    pub dev_mode: bool,
    pub orchestrator_addr: SocketAddr,
}

pub(crate) struct Node {
    pub config: NodeRuntimeConfig,
    pub status: NodeStatus,
    pub process: Option<process::SpawnedNodeProcess>,
    pub connection: Option<quinn::Connection>,
    pub session: session::NodeSession,
}

pub(crate) struct NodeManager {
    nodes: HashMap<u64, Node>,
    next_id: u64,
    rpc_seq: u64,
    pending_rpc: HashMap<u64, oneshot::Sender<session::RpcMessage>>,
    orchestrator_addr: SocketAddr,
    listener: Option<session::ListenerHandle>,
    event_tx: mpsc::Sender<session::NodeEvent>,
    event_rx: mpsc::Receiver<session::NodeEvent>,
}

impl NodeManager {
    pub fn new(orchestrator_addr: SocketAddr) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            nodes: HashMap::new(),
            next_id: 1,
            rpc_seq: 1,
            pending_rpc: HashMap::new(),
            orchestrator_addr,
            listener: None,
            event_tx,
            event_rx,
        }
    }

    pub async fn ensure_listener(&mut self) -> anyhow::Result<()> {
        if self.listener.is_some() {
            return Ok(());
        }

        let handle = session::start_listener(self.orchestrator_addr, self.event_tx.clone())
            .await
            .context("Failed to start node orchestrator listener")?;
        self.listener = Some(handle);

        println!(
            "Orchestrator QUIC listener started at {}",
            self.orchestrator_addr
        );
        Ok(())
    }

    async fn refresh_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: session::NodeEvent) {
        match event {
            session::NodeEvent::Connected {
                node_id,
                connection,
            } => {
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    node.connection = Some(connection);
                    node.status = NodeStatus::Connected;
                }
            }
            session::NodeEvent::RpcStreamReady { node_id, sender } => {
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    node.session.rpc_sender = Some(sender);
                }
            }
            session::NodeEvent::RpcIncoming { node_id, message } => {
                if message.method.is_none() {
                    if let Some(request_id) = message.request_id
                        && let Some(tx) = self.pending_rpc.remove(&request_id)
                    {
                        let _ = tx.send(message);
                    }
                } else {
                    println!(
                        "[node-{node_id} rpc] incoming call: {}",
                        serde_json::to_string(&message)
                            .unwrap_or_else(|_| "<invalid rpc json>".to_string())
                    );
                }
            }
            session::NodeEvent::Log { node_id, record } => {
                println!(
                    "[node-{node_id} remote-log {} {} {}] {}",
                    record.timestamp, record.level, record.source, record.message
                );
            }
            session::NodeEvent::Disconnected { node_id, reason } => {
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    node.status = NodeStatus::Disconnected;
                    node.connection = None;
                    node.session.rpc_sender = None;
                    println!("Node {} disconnected: {}", node_id, reason);
                }
            }
        }
    }

    pub fn list(&mut self) {
        println!("Connected/known nodes:");
        for node in self.nodes.values() {
            println!(
                "ID: {}, Type: {:?}, Name: {}, Status: {:?}, Dev: {}, RPC: {}",
                node.config.id,
                node.config.node_type,
                node.config.name,
                node.status,
                node.config.dev_mode,
                node.session.rpc_sender.is_some()
            );
        }
    }

    pub async fn start(&mut self, node_type: NodeType) -> anyhow::Result<()> {
        self.ensure_listener().await?;
        self.refresh_events().await;

        let node_id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| anyhow!("Node ID overflow"))?;

        if self.nodes.contains_key(&node_id) {
            return Err(anyhow!("Node with id {node_id} already exists"));
        }

        let config = NodeRuntimeConfig {
            id: node_id,
            node_type,
            name: format!("node-{node_id}"),
            dev_mode: false,
            orchestrator_addr: self.orchestrator_addr,
        };

        let process = process::spawn_node_process(
            config.orchestrator_addr,
            config.id,
            config.node_type,
            config.dev_mode,
        )?;

        let pid = process
            .child
            .id()
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        self.nodes.insert(
            node_id,
            Node {
                config,
                status: NodeStatus::Starting,
                process: Some(process),
                connection: None,
                session: session::NodeSession { rpc_sender: None },
            },
        );

        println!("Started node {node_id} ({node_type:?}), pid={pid}");
        Ok(())
    }

    pub async fn set_dev_mode(&mut self, id: u64, state: bool) -> anyhow::Result<()> {
        self.refresh_events().await;

        {
            let node = self
                .nodes
                .get_mut(&id)
                .ok_or_else(|| anyhow!("Node {id} not found"))?;
            node.config.dev_mode = state;
        }

        let response = self
            .send_rpc_request(id, "set_dev", json!({ "state": state }))
            .await;

        match response {
            Ok(Some(msg)) => println!("Node {id} set_dev response: {:?}", msg.result),
            Ok(None) => println!("Node {id} not connected via RPC yet; only local state updated"),
            Err(err) => {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.status = NodeStatus::Failed;
                }
                return Err(err);
            }
        }

        Ok(())
    }

    pub async fn info(&mut self, id: u64) -> anyhow::Result<()> {
        self.refresh_events().await;

        let node = self
            .nodes
            .get(&id)
            .ok_or_else(|| anyhow!("Node {id} not found"))?;

        println!(
            "Node {} => type={:?}, name={}, status={:?}, dev={}, pid={}",
            node.config.id,
            node.config.node_type,
            node.config.name,
            node.status,
            node.config.dev_mode,
            node.process
                .as_ref()
                .and_then(|p| p.child.id())
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );

        match self.send_rpc_request(id, "info", json!({})).await {
            Ok(Some(msg)) => {
                println!(
                    "Node {} rpc info: {}",
                    id,
                    serde_json::to_string_pretty(&msg.result.unwrap_or(json!(null)))?
                );
            }
            Ok(None) => println!("Node {} rpc info unavailable: no RPC stream", id),
            Err(err) => println!("Node {} rpc info error: {err}", id),
        }

        Ok(())
    }

    async fn send_rpc_request(
        &mut self,
        node_id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<Option<session::RpcMessage>> {
        self.refresh_events().await;

        let sender = match self
            .nodes
            .get(&node_id)
            .and_then(|n| n.session.rpc_sender.clone())
        {
            Some(sender) => sender,
            None => return Ok(None),
        };

        let request_id = self.rpc_seq;
        self.rpc_seq = self.rpc_seq.saturating_add(1);

        let request = session::RpcMessage {
            request_id: Some(request_id),
            method: Some(method.to_string()),
            params: Some(params),
            result: None,
            error: None,
        };

        let (tx, rx) = oneshot::channel();
        self.pending_rpc.insert(request_id, tx);

        {
            let mut guard = sender.lock().await;
            session::write_json_line(&mut guard, &request)
                .await
                .context("Failed to write RPC request")?;
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut rx = rx;

        loop {
            tokio::select! {
                result = &mut rx => {
                    return match result {
                        Ok(message) => Ok(Some(message)),
                        Err(_) => Err(anyhow!("RPC response channel closed")),
                    };
                }
                maybe_event = self.event_rx.recv() => {
                    if let Some(event) = maybe_event {
                        self.handle_event(event);
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    self.pending_rpc.remove(&request_id);
                    return Err(anyhow!("RPC timeout for method {method}"));
                }
            }
        }
    }

    pub async fn shutdown(&mut self) {
        self.refresh_events().await;
        for node in self.nodes.values_mut() {
            if let Some(process) = node.process.as_mut() {
                let _ = process.child.start_kill();
                let _ = process.child.wait().await;
            }
            node.status = NodeStatus::Disconnected;
            node.connection = None;
            node.session.rpc_sender = None;
        }
    }
}
