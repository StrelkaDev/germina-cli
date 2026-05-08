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

    let (core, tx) = core::Core::new();

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        cli::run_loop(tx).await?;
        Ok(())
    });
}
