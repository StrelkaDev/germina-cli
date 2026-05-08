use anyhow::anyhow;
use clap::Parser;
use std::net::{IpAddr, SocketAddr};

#[derive(Parser, Clone, Debug)]
#[command(name = "germina", about = "Germina CLI")]
struct AppArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,

    #[arg(long = "node-port", default_value_t = 17171)]
    node_port: u16,

    #[arg(long = "web-port", default_value_t = 18080)]
    web_port: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct AppConfig {
    host: IpAddr,
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

        Ok(Self {
            host: args.host,
            node_port: args.node_port,
            web_port: args.web_port,
        })
    }

    pub(crate) fn node_bind_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.node_port)
    }

    pub(crate) fn web_bind_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.web_port)
    }
}
