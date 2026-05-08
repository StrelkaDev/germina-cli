use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{Mutex, mpsc},
};

const CERT_DER: &[u8] = include_bytes!("certs/orchestrator_cert.der");
const KEY_DER: &[u8] = include_bytes!("certs/orchestrator_key.der");

#[derive(Clone, Debug)]
pub(crate) struct NodeSession {
    pub rpc_sender: Option<Arc<Mutex<quinn::SendStream>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StreamHello {
    pub node_id: u64,
    pub channel: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LogRecord {
    pub timestamp: String,
    pub level: String,
    pub source: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RpcMessage {
    pub request_id: Option<u64>,
    pub method: Option<String>,
    pub params: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum NodeEvent {
    Connected {
        node_id: u64,
        connection: quinn::Connection,
    },
    RpcStreamReady {
        node_id: u64,
        sender: Arc<Mutex<quinn::SendStream>>,
    },
    RpcIncoming {
        node_id: u64,
        message: RpcMessage,
    },
    Log {
        node_id: u64,
        record: LogRecord,
    },
    Disconnected {
        node_id: u64,
        reason: String,
    },
}

pub(crate) struct ListenerHandle {
    _endpoint: quinn::Endpoint,
    _task: tokio::task::JoinHandle<()>,
}

pub(crate) fn server_config() -> anyhow::Result<quinn::ServerConfig> {
    let cert = rustls::pki_types::CertificateDer::from(CERT_DER.to_vec());
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        KEY_DER.to_vec(),
    ));

    let mut config = quinn::ServerConfig::with_single_cert(vec![cert], key)
        .context("Failed to create QUIC server config")?;
    config.transport = Arc::new(quinn::TransportConfig::default());
    Ok(config)
}

pub(crate) fn client_config() -> anyhow::Result<quinn::ClientConfig> {
    let cert = rustls::pki_types::CertificateDer::from(CERT_DER.to_vec());
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(cert)
        .map_err(|e| anyhow!("Failed to add root cert: {e}"))?;

    let crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
        .map_err(|e| anyhow!("Failed to build QUIC client crypto: {e}"))?;

    Ok(quinn::ClientConfig::new(Arc::new(quic_crypto)))
}

pub(crate) async fn write_json_line<T: Serialize>(
    send: &mut quinn::SendStream,
    value: &T,
) -> anyhow::Result<()> {
    let payload = serde_json::to_string(value)?;
    send.write_all(payload.as_bytes()).await?;
    send.write_all(b"\n").await?;
    send.flush().await?;
    Ok(())
}

pub(crate) async fn start_listener(
    bind_addr: SocketAddr,
    event_tx: mpsc::Sender<NodeEvent>,
) -> anyhow::Result<ListenerHandle> {
    let endpoint = quinn::Endpoint::server(server_config()?, bind_addr)
        .context("Failed to start QUIC listener")?;

    let acceptor = endpoint.clone();
    let task = tokio::spawn(async move {
        while let Some(incoming) = acceptor.accept().await {
            let tx = event_tx.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        if let Err(err) = handle_connection(connection, tx).await {
                            eprintln!("Connection handling error: {err}");
                        }
                    }
                    Err(err) => {
                        eprintln!("Incoming connection failed: {err}");
                    }
                }
            });
        }
    });

    Ok(ListenerHandle {
        _endpoint: endpoint,
        _task: task,
    })
}

async fn handle_connection(
    connection: quinn::Connection,
    event_tx: mpsc::Sender<NodeEvent>,
) -> anyhow::Result<()> {
    loop {
        let next = connection.accept_bi().await;
        let (send, recv) = match next {
            Ok(streams) => streams,
            Err(err) => {
                eprintln!("Connection stream accept error: {err}");
                break;
            }
        };

        let tx = event_tx.clone();
        let conn = connection.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_stream(conn, send, recv, tx).await {
                eprintln!("Stream handling error: {err}");
            }
        });
    }

    Ok(())
}

async fn handle_stream(
    connection: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    event_tx: mpsc::Sender<NodeEvent>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(recv);
    let mut hello_line = String::new();
    if reader.read_line(&mut hello_line).await? == 0 {
        return Ok(());
    }

    let hello: StreamHello = serde_json::from_str(hello_line.trim())
        .context("Failed to decode stream hello")?;

    event_tx
        .send(NodeEvent::Connected {
            node_id: hello.node_id,
            connection: connection.clone(),
        })
        .await
        .ok();

    match hello.channel.as_str() {
        "log" => {
            loop {
                let mut line = String::new();
                let read = reader.read_line(&mut line).await?;
                if read == 0 {
                    break;
                }
                let record = match serde_json::from_str::<LogRecord>(line.trim()) {
                    Ok(record) => record,
                    Err(_) => LogRecord {
                        timestamp: chrono_like_timestamp(),
                        level: "INFO".to_string(),
                        source: "plain".to_string(),
                        message: line.trim().to_string(),
                    },
                };

                if event_tx
                    .try_send(NodeEvent::Log {
                        node_id: hello.node_id,
                        record,
                    })
                    .is_err()
                {
                    eprintln!("Dropping remote log due to backpressure for node {}", hello.node_id);
                }
            }
        }
        "rpc" => {
            let sender = Arc::new(Mutex::new(send));
            event_tx
                .send(NodeEvent::RpcStreamReady {
                    node_id: hello.node_id,
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
                if let Ok(message) = serde_json::from_str::<RpcMessage>(line.trim()) {
                    event_tx
                        .send(NodeEvent::RpcIncoming {
                            node_id: hello.node_id,
                            message,
                        })
                        .await
                        .ok();
                }
            }

            event_tx
                .send(NodeEvent::Disconnected {
                    node_id: hello.node_id,
                    reason: "RPC stream closed".to_string(),
                })
                .await
                .ok();
        }
        other => {
            return Err(anyhow!("Unsupported stream channel: {other}"));
        }
    }

    Ok(())
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}
