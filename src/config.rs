use anyhow::anyhow;
use clap::Parser;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser, Clone, Debug)]
#[command(name = "germina", about = "Germina CLI")]
struct AppArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Runtime root directory (defaults to current executable directory)"
    )]
    path: Option<PathBuf>,

    #[arg(long)]
    host: Option<IpAddr>,

    #[arg(long = "node-port", default_value_t = 17171)]
    node_port: u16,

    #[arg(long = "web-port", default_value_t = 18080)]
    web_port: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct AppConfig {
    path: Option<PathBuf>,
    host: IpAddr,
    host_is_explicit: bool,
    node_port: u16,
    web_port: u16,
}

impl AppConfig {
    pub(crate) fn parse() -> anyhow::Result<Self> {
        let args = AppArgs::parse();
        Self::from_args(args)
    }

    fn from_args(args: AppArgs) -> anyhow::Result<Self> {
        if args.node_port == 0 {
            return Err(anyhow!("node-port must be greater than zero"));
        }
        if args.web_port == 0 {
            return Err(anyhow!("web-port must be greater than zero"));
        }
        if args.node_port == args.web_port {
            return Err(anyhow!("node-port and web-port must be different"));
        }

        let host_is_explicit = args.host.is_some();
        let host = args.host.unwrap_or_else(resolve_default_host);

        Ok(Self {
            path: args.path,
            host,
            host_is_explicit,
            node_port: args.node_port,
            web_port: args.web_port,
        })
    }

    pub(crate) fn root_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn node_bind_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.node_port)
    }

    pub(crate) fn web_bind_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.web_port)
    }

    pub(crate) fn cli_endpoint(&self) -> String {
        format!("{}:{}", self.host, self.node_port)
    }

    pub(crate) fn host_startup_summary(&self) -> String {
        let source = if self.host_is_explicit {
            "from --host"
        } else {
            "auto-detected"
        };

        format!("CLI host: {} ({source})", self.host)
    }
}

fn resolve_default_host() -> IpAddr {
    detect_active_network_ip()
        .or_else(detect_public_ip_from_services)
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn detect_active_network_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect("8.8.8.8:80").ok()?;

    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() {
        return None;
    }

    Some(ip)
}

fn detect_public_ip_from_services() -> Option<IpAddr> {
    const SERVICES: [(&str, &str); 3] = [
        ("api.ipify.org", "/"),
        ("ifconfig.me", "/ip"),
        ("ipv4.icanhazip.com", "/"),
    ];

    SERVICES
        .iter()
        .find_map(|(host, path)| query_public_ip_over_http(host, path))
}

fn query_public_ip_over_http(host: &str, path: &str) -> Option<IpAddr> {
    let address = format!("{host}:80");
    let mut addrs = address.to_socket_addrs().ok()?;
    let first = addrs.next()?;

    let timeout = Duration::from_secs(2);
    let mut stream = std::net::TcpStream::connect_timeout(&first, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: germina-cli\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;

    let body = response.split("\r\n\r\n").nth(1)?.trim();
    body.parse::<IpAddr>().ok()
}
