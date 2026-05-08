mod assets;
mod cli;
mod core;
mod node;
mod util;
mod version;
mod web;

fn main() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install AWS-LC-RS crypto provider");

    println!("Germina CLI {}", version::VERSION);

    let (mut core, tx) = core::Core::new();

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let core_task = tokio::spawn(async move { core.run().await });
        let cli_result = cli::run_loop(tx).await;

        let core_result = core_task
            .await
            .map_err(|e| anyhow::anyhow!("Core task join failed: {e}"))?;

        cli_result?;
        core_result?;
        Ok(())
    })
}
