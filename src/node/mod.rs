use std::collections::HashMap;

pub mod command;

#[derive(clap::ValueEnum, Clone, Debug)]
pub(crate) enum NodeType {
    Client,
    Server,
    Tools,
}

pub(crate) struct Node {
    pub id: u64,
    pub node_type: NodeType,
    pub name: String,
    pub connection: quinn::Connection,
}

pub(crate) struct NodeManager {
    nodes: HashMap<u64, Node>,
}

impl NodeManager {
    pub fn new() -> Self {
        Self {
            nodes: std::collections::HashMap::new(),
        }
    }

    pub fn list(&self) {
        println!("Connected nodes:");
        for node in self.nodes.values() {
            println!(
                "ID: {}, Type: {:?}, Name: {}, Connection: {:?}",
                node.id, node.node_type, node.name, node.connection
            );
        }
    }

    async fn start(&self, node_type: NodeType) {
        todo!()
    }

    async fn set_dev_mode(&self, id: u64, state: bool) {
        todo!()
    }

    async fn info(&self, id: u64) {
        todo!()
    }
}
