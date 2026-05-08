mod assets;
mod cli;
mod core;
mod node;
mod util;
mod version;
mod web;

#[derive(clap::Parser, Debug)]
#[command(version, about = "Germina orchestrator")]
struct Args {
    /// Host address to bind listeners on
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port for the QUIC node listener
    #[arg(long, default_value_t = 17171)]
    node_port: u16,

    /// Port for the web server listener
    #[arg(long, default_value_t = 17172)]
    web_port: u16,
}

fn main() -> anyhow::Result<()> {
    let args = <Args as clap::Parser>::parse();

    let node_addr: std::net::SocketAddr = format!("{}:{}", args.host, args.node_port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid node address: {e}"))?;
    let web_addr: std::net::SocketAddr = format!("{}:{}", args.host, args.web_port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid web address: {e}"))?;

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install AWS-LC-RS crypto provider");

    println!("Germina CLI {}", version::VERSION);

    let (mut core, tx) = core::Core::new(node_addr, web_addr);

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let core_task = tokio::spawn(async move { core.run(ready_tx).await });
        let cli_result = cli::run_loop(tx, ready_rx).await;

        let core_result = core_task
            .await
            .map_err(|e| anyhow::anyhow!("Core task join failed: {e}"))?;

        cli_result?;
        core_result?;
        Ok(())
    })
}
