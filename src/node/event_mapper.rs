pub(crate) fn map_transport_event(event: crate::node::session::events::TransportEvent) -> crate::node::events::NodeEvent {
    match event {
        crate::node::session::events::TransportEvent::Connected {
            node_id,
            connection,
        } => crate::node::events::NodeEvent::Connected {
            node_id,
            connection,
        },
        crate::node::session::events::TransportEvent::RpcStreamReady { node_id, sender } => {
            crate::node::events::NodeEvent::RpcStreamReady { node_id, sender }
        }
        crate::node::session::events::TransportEvent::RpcIncoming { node_id, message } => {
            crate::node::events::NodeEvent::RpcIncoming { node_id, message }
        }
        crate::node::session::events::TransportEvent::Log { node_id, record } => {
            crate::node::events::NodeEvent::Log { node_id, record }
        }
        crate::node::session::events::TransportEvent::Disconnected { node_id, reason } => {
            crate::node::events::NodeEvent::Disconnected { node_id, reason }
        }
    }
}
