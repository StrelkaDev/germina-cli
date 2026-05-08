pub mod command;

pub(crate) struct WebManager {
    pub web_addr: std::net::SocketAddr,
}

impl WebManager {
    pub fn new(web_addr: std::net::SocketAddr) -> Self {
        Self { web_addr }
    }
}
