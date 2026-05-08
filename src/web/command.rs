#[derive(clap::Subcommand, Clone, Debug)]
pub(crate) enum WebCommand {
    /// Start the web server
    Start,
    /// Stop the web server
    Stop,
    /// Show web server status
    Status,
}

impl WebCommand {
    pub(crate) async fn execute(&self, manager: &mut crate::web::WebManager) -> anyhow::Result<()> {
        match self {
            WebCommand::Start => {
                println!("Starting web server at {}...", manager.web_addr);
                // Implement start logic here
            }
            WebCommand::Stop => {
                println!("Stopping web server...");
                // Implement stop logic here
            }
            WebCommand::Status => {
                println!("Checking web server status...");
                // Implement status logic here
            }
        }
        Ok(())
    }
}
