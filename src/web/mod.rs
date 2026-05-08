pub mod command;

#[derive(Clone, Copy, Debug)]
pub(crate) enum WebServerState {
    Stopped,
    Running,
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
            WebServerState::Running => Ok(()),
            WebServerState::Stopped => {
                self.state = WebServerState::Running;
                Ok(())
            }
        }
    }

    pub(crate) async fn stop(&mut self) -> anyhow::Result<()> {
        match self.state {
            WebServerState::Stopped => Ok(()),
            WebServerState::Running => {
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
