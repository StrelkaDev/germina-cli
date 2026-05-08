use anyhow::{Context, anyhow};
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{Mutex, mpsc},
};

pub(crate) async fn handle_stream(
    connection: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    event_tx: mpsc::Sender<crate::node::session::events::TransportEvent>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(recv);
    let mut hello_line = String::new();
    if reader.read_line(&mut hello_line).await? == 0 {
        return Ok(());
    }

    let hello: crate::node::session::protocol::StreamHello = serde_json::from_str(hello_line.trim())
        .context("Failed to decode stream hello")?;

    event_tx
        .send(crate::node::session::events::TransportEvent::Connected {
            node_id: hello.node_id,
            connection: connection.clone(),
        })
        .await
        .ok();

    match hello.channel.as_str() {
        "log" => handle_log_stream(hello.node_id, &mut reader, &event_tx).await,
        "rpc" => handle_rpc_stream(hello.node_id, send, &mut reader, &event_tx).await,
        other => Err(anyhow!("Unsupported stream channel: {other}")),
    }
}

async fn handle_log_stream(
    node_id: u64,
    reader: &mut BufReader<quinn::RecvStream>,
    event_tx: &mpsc::Sender<crate::node::session::events::TransportEvent>,
) -> anyhow::Result<()> {
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            break;
        }

        let record = match serde_json::from_str::<crate::node::session::protocol::LogRecord>(line.trim())
        {
            Ok(record) => record,
            Err(_) => crate::node::session::protocol::LogRecord {
                timestamp: crate::node::session::protocol::unix_ts(),
                level: "INFO".to_string(),
                source: "plain".to_string(),
                message: line.trim().to_string(),
            },
        };

        let _ = event_tx.try_send(crate::node::session::events::TransportEvent::Log {
            node_id,
            record,
        });
    }

    Ok(())
}

async fn handle_rpc_stream(
    node_id: u64,
    send: quinn::SendStream,
    reader: &mut BufReader<quinn::RecvStream>,
    event_tx: &mpsc::Sender<crate::node::session::events::TransportEvent>,
) -> anyhow::Result<()> {
    let sender = Arc::new(Mutex::new(send));
    event_tx
        .send(crate::node::session::events::TransportEvent::RpcStreamReady {
            node_id,
            sender,
        })
        .await
        .ok();

    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            break;
        }

        if let Ok(message) = serde_json::from_str::<crate::node::session::protocol::RpcMessage>(line.trim()) {
            event_tx
                .send(crate::node::session::events::TransportEvent::RpcIncoming { node_id, message })
                .await
                .ok();
        }
    }

    event_tx
        .send(crate::node::session::events::TransportEvent::Disconnected {
            node_id,
            reason: "RPC stream closed".to_string(),
        })
        .await
        .ok();

    Ok(())
}
