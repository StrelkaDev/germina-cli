#[derive(Clone, Debug)]
pub(crate) enum NodeEvent {
    Connected {
        node_id: u64,
        connection: quinn::Connection,
    },
    RpcStreamReady {
        node_id: u64,
        sender: std::sync::Arc<tokio::sync::Mutex<quinn::SendStream>>,
    },
    RpcIncoming {
        node_id: u64,
        message: crate::node::session::protocol::RpcMessage,
    },
    Log {
        node_id: u64,
        record: crate::node::session::protocol::LogRecord,
    },
    Disconnected {
        node_id: u64,
        reason: String,
    },
}
