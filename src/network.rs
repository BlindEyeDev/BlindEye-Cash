use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: String,
    pub max_peers: usize,
    pub bootstrap_nodes: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:30303".to_string(),
            max_peers: 32,
            bootstrap_nodes: vec![],
        }
    }
}

impl NetworkConfig {
    /// Create a production-ready network config that listens on all interfaces
    #[allow(dead_code)]
    pub fn production() -> Self {
        Self {
            listen_addr: "0.0.0.0:30303".to_string(),
            max_peers: 32,
            bootstrap_nodes: vec![
                // TODO: Add production seed nodes
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    pub best_height: u64,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerManager {
    pub network: NetworkConfig,
    pub peers: Vec<Peer>,
}

impl PeerManager {
    pub fn new(network: NetworkConfig) -> Self {
        Self {
            network,
            peers: Vec::new(),
        }
    }

    pub fn get_connected_peers(&self) -> Vec<&Peer> {
        self.peers.iter().collect()
    }

    #[allow(dead_code)]
    pub fn add_or_update_peer(&mut self, peer: Peer) {
        if let Some(existing) = self
            .peers
            .iter_mut()
            .find(|existing| existing.id == peer.id)
        {
            *existing = peer;
        } else if self.peers.len() < self.network.max_peers {
            self.peers.push(peer);
        }
    }

    #[allow(dead_code)]
    pub fn remove_peer(&mut self, id: &str) {
        self.peers.retain(|peer| peer.id != id);
    }
}
