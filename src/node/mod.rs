use anyhow::Context;
use serde_json::json;
use std::net::SocketAddr;
use tokio::sync::mpsc;

pub mod command;
mod event_mapper;
mod events;
pub mod service;
pub mod session;

#[derive(Clone, Copy, Debug)]
pub(crate) enum NodeStatus {
    Starting,
    Connected,
    Disconnected,
    Failed,
}

pub(crate) struct NodeManager {
    service: service::NodeService,
    listener: Option<session::listener::ListenerHandle>,
    transport_tx: mpsc::Sender<session::events::TransportEvent>,
    transport_rx: mpsc::Receiver<session::events::TransportEvent>,
}

impl NodeManager {
    pub fn new(orchestrator_addr: SocketAddr) -> Self {
        let (transport_tx, transport_rx) = mpsc::channel(1024);
        Self {
            service: service::NodeService::new(orchestrator_addr),
            listener: None,
            transport_tx,
            transport_rx,
        }
    }

    pub async fn ensure_listener(&mut self) -> anyhow::Result<()> {
        if self.listener.is_some() {
            return Ok(());
        }

        let bind_addr = self.service.orchestrator_addr();
        let handle = session::listener::start_listener(bind_addr, self.transport_tx.clone())
            .await
            .context("Failed to start node orchestrator listener")?;
        self.listener = Some(handle);

        println!("Orchestrator QUIC listener started at {}", bind_addr);
        Ok(())
    }

    async fn refresh_events(&mut self) {
        while let Ok(event) = self.transport_rx.try_recv() {
            let domain_event = event_mapper::map_transport_event(event);
            self.handle_event(domain_event);
        }
    }

    fn handle_event(&mut self, event: events::NodeEvent) {
        match event {
            events::NodeEvent::Connected {
                node_id,
                connection,
            } => {
                self.service.mark_connected(node_id, connection);
            }
            events::NodeEvent::RpcStreamReady { node_id, sender } => {
                self.service.set_rpc_sender(node_id, sender);
            }
            events::NodeEvent::RpcIncoming { node_id, message } => {
                if let Some(incoming_call) = self.service.route_incoming_rpc(message) {
                    println!(
                        "[node-{node_id} rpc] incoming call: {}",
                        serde_json::to_string(&incoming_call)
                            .unwrap_or_else(|_| "<invalid rpc json>".to_string())
                    );
                }
            }
            events::NodeEvent::Log { node_id, record } => {
                println!(
                    "[node-{node_id} remote-log {} {} {}] {}",
                    record.timestamp, record.level, record.source, record.message
                );
            }
            events::NodeEvent::Disconnected { node_id, reason } => {
                self.service.mark_disconnected(node_id);
                println!("Node {} disconnected: {}", node_id, reason);
            }
        }
    }

    pub fn list(&mut self) {
        println!("Connected/known nodes:");
        for line in self.service.list_lines() {
            println!("{line}");
        }
    }

    pub async fn set_dev_mode(&mut self, id: u64, state: bool) -> anyhow::Result<()> {
        self.refresh_events().await;
        self.service.set_dev_mode_local(id, state)?;

        let response = self
            .send_rpc_request(id, "set_dev", self.service.build_set_dev_params(state))
            .await;

        match response {
            Ok(Some(msg)) => println!("Node {id} set_dev response: {:?}", msg.result),
            Ok(None) => println!("Node {id} not connected via RPC yet; only local state updated"),
            Err(err) => {
                self.service.mark_failed(id);
                return Err(err);
            }
        }

        Ok(())
    }

    pub async fn info(&mut self, id: u64) -> anyhow::Result<()> {
        self.refresh_events().await;

        println!("{}", self.service.format_info_line(id)?);

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
    ) -> anyhow::Result<Option<session::protocol::RpcMessage>> {
        self.refresh_events().await;

        let response = self.service.send_rpc_request(node_id, method, params).await;

        self.refresh_events().await;
        response
    }

    pub async fn shutdown(&mut self) {
        self.refresh_events().await;
        self.service.shutdown().await;
    }
}
