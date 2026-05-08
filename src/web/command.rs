#[derive(clap::Subcommand, Clone, Debug)]
#[command(about = "Web server commands")]
pub(crate) enum WebCommand {
    /// Start the web server
    Start,
    /// Stop the web server
    Stop,
    /// Show current web server status
    Status,
}

impl WebCommand {
    pub(crate) async fn execute(&self, manager: &mut crate::web::WebManager) -> anyhow::Result<()> {
        match self {
            WebCommand::Start => {
                manager.start().await?;
                let _ = crate::ui::print_line(format!(
                    "Web server start requested at {} (state: {:?})",
                    manager.bind_addr(),
                    manager.status()
                ));
            }
            WebCommand::Stop => {
                manager.stop().await?;
                let _ = crate::ui::print_line(format!(
                    "Web server stop requested at {} (state: {:?})",
                    manager.bind_addr(),
                    manager.status()
                ));
            }
            WebCommand::Status => {
                let _ = crate::ui::print_line(format!(
                    "Web server status at {}: {:?}",
                    manager.bind_addr(),
                    manager.status()
                ));
            }
        }
        Ok(())
    }
}
