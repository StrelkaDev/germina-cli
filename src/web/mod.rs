pub mod command;

#[derive(Clone, Copy, Debug)]
pub(crate) enum WebServerState {
    Stopped,
    Starting,
    Running,
    Stopping,
}

pub(crate) struct WebManager {
    bind_addr: std::net::SocketAddr,
    state: WebServerState,
}

impl WebManager {
    pub fn new(bind_addr: std::net::SocketAddr) -> Self {
        Self {
            bind_addr,
            state: WebServerState::Stopped,
        }
    }

    pub(crate) async fn start(&mut self) -> anyhow::Result<()> {
        match self.state {
            WebServerState::Running | WebServerState::Starting => Ok(()),
            WebServerState::Stopped | WebServerState::Stopping => {
                self.state = WebServerState::Starting;
                self.state = WebServerState::Running;
                Ok(())
            }
        }
    }

    pub(crate) async fn stop(&mut self) -> anyhow::Result<()> {
        match self.state {
            WebServerState::Stopped | WebServerState::Stopping => Ok(()),
            WebServerState::Running | WebServerState::Starting => {
                self.state = WebServerState::Stopping;
                self.state = WebServerState::Stopped;
                Ok(())
            }
        }
    }

    pub(crate) fn status(&self) -> WebServerState {
        self.state
    }

    pub(crate) fn bind_addr(&self) -> std::net::SocketAddr {
        self.bind_addr
    }
}
