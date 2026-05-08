use crate::node::{
    NodeType,
    session::{LogRecord, RpcMessage, StreamHello, client_config, write_json_line},
};
use anyhow::{Context, anyhow};
use serde_json::json;
use std::{
    io::{BufRead, BufReader},
    net::SocketAddr,
    process::{Child, Command, Stdio},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncBufReadExt, BufReader as AsyncBufReader};

pub(crate) struct SpawnedNodeProcess {
    pub child: Child,
}

#[derive(Clone, Debug)]
pub(crate) struct NodeRuntimeArgs {
    pub orchestrator_addr: SocketAddr,
    pub node_id: u64,
    pub node_type: NodeType,
    pub dev: bool,
}

pub(crate) fn try_parse_runtime_args_from_env() -> anyhow::Result<Option<NodeRuntimeArgs>> {
    let args: Vec<String> = std::env::args().collect();
    let mut cli_addr = None;
    let mut node_id = None;
    let mut node_type = None;
    let mut dev = false;

    let mut idx = 1usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--cli" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow!("Missing value for --cli"))?;
                cli_addr = Some(SocketAddr::from_str(value).context("Invalid --cli address")?);
            }
            "--node-id" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow!("Missing value for --node-id"))?;
                node_id = Some(value.parse::<u64>().context("Invalid --node-id")?);
            }
            "--node-type" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow!("Missing value for --node-type"))?;
                node_type = Some(parse_node_type(value)?);
            }
            "--dev" => {
                dev = true;
            }
            _ => {}
        }
        idx += 1;
    }

    if cli_addr.is_none() {
        return Ok(None);
    }

    Ok(Some(NodeRuntimeArgs {
        orchestrator_addr: cli_addr.ok_or_else(|| anyhow!("--cli is required"))?,
        node_id: node_id.ok_or_else(|| anyhow!("--node-id is required in node mode"))?,
        node_type: node_type.ok_or_else(|| anyhow!("--node-type is required in node mode"))?,
        dev,
    }))
}

pub(crate) fn spawn_node_process(
    orchestrator_addr: SocketAddr,
    node_id: u64,
    node_type: NodeType,
    dev: bool,
) -> anyhow::Result<SpawnedNodeProcess> {
    let current_exe = std::env::current_exe().context("Failed to resolve current executable")?;

    let mut command = Command::new(current_exe);
    command
        .arg("--cli")
        .arg(orchestrator_addr.to_string())
        .arg("--node-id")
        .arg(node_id.to_string())
        .arg("--node-type")
        .arg(node_type.as_cli_value())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if dev {
        command.arg("--dev");
    }

    let mut child = command.spawn().context("Failed to spawn node child process")?;
    attach_stdio_forwarders(node_id, node_type, &mut child);

    Ok(SpawnedNodeProcess { child })
}

fn attach_stdio_forwarders(node_id: u64, node_type: NodeType, child: &mut Child) {
    if let Some(stdout) = child.stdout.take() {
        let prefix = format!("[node-{node_id} {:?} stdout]", node_type);
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                println!("{prefix} {line}");
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let prefix = format!("[node-{node_id} {:?} stderr]", node_type);
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("{prefix} {line}");
            }
        });
    }
}

pub(crate) async fn run_node_runtime(args: NodeRuntimeArgs) -> anyhow::Result<()> {
    println!(
        "Node runtime started: id={}, type={:?}, dev={}, orchestrator={}",
        args.node_id, args.node_type, args.dev, args.orchestrator_addr
    );

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .context("Failed to create QUIC client endpoint")?;
    endpoint.set_default_client_config(client_config()?);

    let connection = endpoint
        .connect(args.orchestrator_addr, "localhost")
        .context("Failed to connect to orchestrator")?
        .await
        .context("QUIC connect handshake failed")?;

    let (mut log_send, mut _log_recv) = connection.open_bi().await?;
    write_json_line(
        &mut log_send,
        &StreamHello {
            node_id: args.node_id,
            channel: "log".to_string(),
        },
    )
    .await?;

    let startup_log = LogRecord {
        timestamp: unix_ts(),
        level: "INFO".to_string(),
        source: "node-runtime".to_string(),
        message: format!(
            "node {} ({:?}) connected; dev={}",
            args.node_id, args.node_type, args.dev
        ),
    };
    write_json_line(&mut log_send, &startup_log).await?;

    let (mut rpc_send, rpc_recv) = connection.open_bi().await?;
    write_json_line(
        &mut rpc_send,
        &StreamHello {
            node_id: args.node_id,
            channel: "rpc".to_string(),
        },
    )
    .await?;

    let mut dev_mode = args.dev;
    let mut reader = AsyncBufReader::new(rpc_recv);

    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            break;
        }

        let parsed = serde_json::from_str::<RpcMessage>(line.trim());
        let message = match parsed {
            Ok(msg) => msg,
            Err(err) => {
                let log = LogRecord {
                    timestamp: unix_ts(),
                    level: "WARN".to_string(),
                    source: "rpc".to_string(),
                    message: format!("Invalid RPC payload: {err}"),
                };
                let _ = write_json_line(&mut log_send, &log).await;
                continue;
            }
        };

        if let Some(method) = message.method.as_deref() {
            let request_id = message.request_id;
            let response = match method {
                "set_dev" => {
                    let new_state = message
                        .params
                        .as_ref()
                        .and_then(|v| v.get("state"))
                        .and_then(|v| v.as_bool())
                        .ok_or_else(|| anyhow!("Missing params.state bool"));

                    match new_state {
                        Ok(state) => {
                            dev_mode = state;
                            let log = LogRecord {
                                timestamp: unix_ts(),
                                level: "INFO".to_string(),
                                source: "rpc".to_string(),
                                message: format!("dev mode switched to {dev_mode}"),
                            };
                            let _ = write_json_line(&mut log_send, &log).await;
                            RpcMessage {
                                request_id,
                                method: None,
                                params: None,
                                result: Some(json!({ "ok": true, "dev": dev_mode })),
                                error: None,
                            }
                        }
                        Err(err) => RpcMessage {
                            request_id,
                            method: None,
                            params: None,
                            result: None,
                            error: Some(err.to_string()),
                        },
                    }
                }
                "info" => RpcMessage {
                    request_id,
                    method: None,
                    params: None,
                    result: Some(json!({
                        "node_id": args.node_id,
                        "node_type": format!("{:?}", args.node_type),
                        "dev": dev_mode,
                        "status": "running"
                    })),
                    error: None,
                },
                "ping" => RpcMessage {
                    request_id,
                    method: None,
                    params: None,
                    result: Some(json!({ "pong": true })),
                    error: None,
                },
                other => RpcMessage {
                    request_id,
                    method: None,
                    params: None,
                    result: None,
                    error: Some(format!("Unsupported method: {other}")),
                },
            };

            write_json_line(&mut rpc_send, &response).await?;
        }
    }

    Ok(())
}

fn parse_node_type(value: &str) -> anyhow::Result<NodeType> {
    match value.to_ascii_lowercase().as_str() {
        "client" => Ok(NodeType::Client),
        "server" => Ok(NodeType::Server),
        "tools" => Ok(NodeType::Tools),
        _ => Err(anyhow!("Invalid node type: {value}")),
    }
}

fn unix_ts() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_secs().to_string()
}
