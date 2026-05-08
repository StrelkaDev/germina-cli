use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{
    io::AsyncWriteExt,
    sync::Mutex,
};

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

pub(crate) fn unix_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}
