use anyhow::Context;
use std::net::SocketAddr;
use tokio::sync::mpsc;

pub(crate) struct ListenerHandle {
    _endpoint: quinn::Endpoint,
    _task: tokio::task::JoinHandle<()>,
}

pub(crate) async fn start_listener(
    bind_addr: SocketAddr,
    event_tx: mpsc::Sender<crate::node::session::events::TransportEvent>,
) -> anyhow::Result<ListenerHandle> {
    let endpoint = quinn::Endpoint::server(crate::node::session::config::server_config()?, bind_addr)
        .context("Failed to start QUIC listener")?;

    let acceptor = endpoint.clone();
    let task = tokio::spawn(async move {
        while let Some(incoming) = acceptor.accept().await {
            let tx = event_tx.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        if let Err(err) = handle_connection(connection, tx).await {
                            let _ = crate::ui::print_line(format!("Connection handling error: {err}"));
                        }
                    }
                    Err(err) => {
                        let _ = crate::ui::print_line(format!("Incoming connection failed: {err}"));
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
    event_tx: mpsc::Sender<crate::node::session::events::TransportEvent>,
) -> anyhow::Result<()> {
    loop {
        let next = connection.accept_bi().await;
        let (send, recv) = match next {
            Ok(streams) => streams,
            Err(err) => {
                let _ = crate::ui::print_line(format!("Connection stream accept error: {err}"));
                break;
            }
        };

        let tx = event_tx.clone();
        let conn = connection.clone();
        tokio::spawn(async move {
            if let Err(err) = crate::node::session::handlers::handle_stream(conn, send, recv, tx).await
            {
                let _ = crate::ui::print_line(format!("Stream handling error: {err}"));
            }
        });
    }

    Ok(())
}
