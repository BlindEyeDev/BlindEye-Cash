use crate::block::Block;
use crate::node::Node;
use crate::transaction::Transaction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::{OwnedReadHalf, OwnedWriteHalf}, TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

#[derive(Debug, Error)]
pub enum P2PError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Invalid message")]
    InvalidMessage,
    #[error("Peer disconnected")]
    PeerDisconnected,
    #[error("Network error: {0}")]
    NetworkError(String),
}

pub type P2PResult<T> = Result<T, P2PError>;

/// Peer message types for protocol communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerMessage {
    /// Handshake with version and chain height
    Handshake {
        version: u32,
        best_height: u64,
        timestamp: u64,
    },
    /// Request peer information
    GetPeers,
    /// Respond with peer addresses
    Peers(Vec<String>),
    /// Get blocks by hash
    GetBlocks(Vec<[u8; 32]>),
    /// Block propagation
    Block(Block),
    /// Transaction propagation
    Transaction(Transaction),
    /// Request block headers for sync
    GetHeaders {
        from_height: u64,
        count: u32,
    },
    /// Heartbeat/keepalive
    Ping { nonce: u64 },
    /// Heartbeat response
    Pong { nonce: u64 },
}

/// Connected peer information
#[derive(Debug, Clone)]
pub struct ConnectedPeer {
    pub address: SocketAddr,
    pub best_height: u64,
    pub last_seen: SystemTime,
    pub version: u32,
}

/// P2P network manager
pub struct P2PManager {
    listen_addr: SocketAddr,
    peers: Arc<RwLock<HashMap<SocketAddr, ConnectedPeer>>>,
    node: Arc<Node>,
    max_peers: usize,
}

impl P2PManager {
    pub fn new(listen_addr: SocketAddr, node: Arc<Node>, max_peers: usize) -> Self {
        Self {
            listen_addr,
            peers: Arc::new(RwLock::new(HashMap::new())),
            node,
            max_peers,
        }
    }

    /// Start the P2P network listener
    pub async fn start(self: Arc<Self>) -> P2PResult<()> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        eprintln!("[P2P] Listening on {}", self.listen_addr);

        let peers = self.peers.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let mut peers_map = peers.write().await;
                peers_map.retain(|_, peer| {
                    peer
                        .last_seen
                        .elapsed()
                        .unwrap_or(Duration::from_secs(300))
                        < Duration::from_secs(300)
                });
            }
        });

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let manager = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = manager.handle_peer(stream, addr).await {
                            eprintln!("[P2P] Error handling peer {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("[P2P] Accept error: {}", e);
                }
            }
        }
    }

    /// Connect to a bootstrap peer
    pub async fn connect_peer(self: Arc<Self>, addr: SocketAddr) -> P2PResult<()> {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                eprintln!("[P2P] Connected to bootstrap peer {}", addr);
                self.handle_peer_connection(stream, addr).await
            }
            Err(e) => {
                eprintln!("[P2P] Failed to connect to {}: {}", addr, e);
                Err(P2PError::NetworkError(format!("Connection failed: {}", e)))
            }
        }
    }

    /// Handle incoming peer connection
    async fn handle_peer(
        &self,
        stream: TcpStream,
        addr: SocketAddr,
    ) -> P2PResult<()> {
        let (reader, writer) = stream.into_split();
        let reader = Arc::new(tokio::sync::Mutex::new(reader));
        let writer = Arc::new(tokio::sync::Mutex::new(writer));

        let handshake = self.read_message(reader.clone()).await?;
        if let PeerMessage::Handshake {
            version,
            best_height,
            ..
        } = handshake
        {
            let peer = ConnectedPeer {
                address: addr,
                best_height,
                last_seen: SystemTime::now(),
                version,
            };
            self.peers.write().await.insert(addr, peer);
            eprintln!("[P2P] Peer handshake from {} (height: {})", addr, best_height);
        }

        // Send our handshake
        let our_height = self.node.get_best_height();
        let response = PeerMessage::Handshake {
            version: 1,
            best_height: our_height,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        self.write_message(writer.clone(), &response).await?;


        loop {
            match self.read_message(reader.clone()).await {
                Ok(msg) => {
                    self.handle_message(addr, &msg, writer.clone()).await?;
                }
                Err(P2PError::PeerDisconnected) => {
                    eprintln!("[P2P] Peer {} disconnected", addr);
                    self.peers.write().await.remove(&addr);
                    break;
                }
                Err(e) => {
                    eprintln!("[P2P] Message error from {}: {}", addr, e);
                    self.peers.write().await.remove(&addr);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle peer connection (outgoing)
    async fn handle_peer_connection(
        &self,
        stream: TcpStream,
        addr: SocketAddr,
    ) -> P2PResult<()> {
        let (reader, writer) = stream.into_split();
        let reader = Arc::new(tokio::sync::Mutex::new(reader));
        let writer = Arc::new(tokio::sync::Mutex::new(writer));

        let our_height = self.node.get_best_height();
        let handshake = PeerMessage::Handshake {
            version: 1,
            best_height: our_height,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        self.write_message(writer.clone(), &handshake).await?;

        // Receive handshake
        if let PeerMessage::Handshake {
            version,
            best_height,
            ..
        } = self.read_message(reader.clone()).await?
        {
            let peer = ConnectedPeer {
                address: addr,
                best_height,
                last_seen: SystemTime::now(),
                version,
            };
            self.peers.write().await.insert(addr, peer);
        }


        loop {
            match self.read_message(reader.clone()).await {
                Ok(msg) => {
                    self.handle_message(addr, &msg, writer.clone()).await?;
                }
                Err(P2PError::PeerDisconnected) => {
                    eprintln!("[P2P] Peer {} disconnected", addr);
                    break;
                }
                Err(e) => {
                    eprintln!("[P2P] Message error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle incoming P2P message
    async fn handle_message(
        &self,
        addr: SocketAddr,
        msg: &PeerMessage,
        writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    ) -> P2PResult<()> {
        match msg {
            PeerMessage::Block(block) => {
                if let Err(e) = self.node.add_block(block.clone()) {
                    eprintln!("[P2P] Invalid block from {}: {}", addr, e);
                } else {
                    eprintln!("[P2P] Block received and added from {}", addr);
                }
            }
            PeerMessage::Transaction(tx) => {
                if let Err(e) = self.node.submit_transaction(tx.clone()) {
                    eprintln!("[P2P] Invalid transaction from {}: {}", addr, e);
                } else {
                    eprintln!("[P2P] Transaction received from {}", addr);
                }
            }
            PeerMessage::GetPeers => {
                let peers: Vec<String> = self
                    .peers
                    .read()
                    .await
                    .values()
                    .map(|p| p.address.to_string())
                    .collect();
                self.write_message(writer, &PeerMessage::Peers(peers))
                    .await?;
            }
            PeerMessage::Ping { nonce } => {
                self.write_message(writer, &PeerMessage::Pong { nonce: *nonce })
                    .await?;
            }
            PeerMessage::Pong { .. } => {
                if let Some(peer) = self.peers.write().await.get_mut(&addr) {
                    peer.last_seen = SystemTime::now();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Read a P2P message from stream
    async fn read_message(
        &self,
        reader: Arc<tokio::sync::Mutex<OwnedReadHalf>>,
    ) -> P2PResult<PeerMessage> {
        let mut reader = reader.lock().await;
        let mut len_bytes = [0u8; 4];
        reader
            .read_exact(&mut len_bytes)
            .await
            .map_err(|_| P2PError::PeerDisconnected)?;

        let len = u32::from_be_bytes(len_bytes) as usize;
        if len > 10_000_000 {
            return Err(P2PError::InvalidMessage);
        }

        let mut buf = vec![0u8; len];
        reader
            .read_exact(&mut buf)
            .await
            .map_err(|_| P2PError::PeerDisconnected)?;

        bincode::deserialize(&buf).map_err(P2PError::from)
    }

    /// Write a P2P message to stream
    async fn write_message(
        &self,
        writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
        msg: &PeerMessage,
    ) -> P2PResult<()> {
        let serialized = bincode::serialize(msg)?;
        let len = serialized.len() as u32;

        let mut writer = writer.lock().await;
        writer.write_all(&len.to_be_bytes()).await?;
        writer.write_all(&serialized).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Broadcast a block to all peers
    pub async fn broadcast_block(&self, block: &Block) {
        let msg = PeerMessage::Block(block.clone());
        let peers: Vec<_> = self.peers.read().await.keys().copied().collect();

        for peer_addr in peers {
            if let Err(e) = self.broadcast_message(peer_addr, &msg).await {
                eprintln!("[P2P] Failed to broadcast to {}: {}", peer_addr, e);
            }
        }
    }

    /// Broadcast a transaction to all peers
    pub async fn broadcast_transaction(&self, tx: &Transaction) {
        let msg = PeerMessage::Transaction(tx.clone());
        let peers: Vec<_> = self.peers.read().await.keys().copied().collect();

        for peer_addr in peers {
            if let Err(e) = self.broadcast_message(peer_addr, &msg).await {
                eprintln!("[P2P] Failed to broadcast to {}: {}", peer_addr, e);
            }
        }
    }

    /// Send message to specific peer
    async fn broadcast_message(&self, addr: SocketAddr, msg: &PeerMessage) -> P2PResult<()> {
        let stream = TcpStream::connect(addr).await?;
        let (reader, writer) = stream.into_split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer));
        self.write_message(writer, msg).await
    }

    /// Get connected peers count
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Get list of connected peers
    pub async fn get_peers(&self) -> Vec<ConnectedPeer> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Synchronize blocks from peers
    pub async fn synchronize_blocks(&self) -> P2PResult<()> {
        let peers = self.peers.read().await.values().cloned().collect::<Vec<_>>();
        let our_height = self.node.get_best_height();

        for peer in peers {
            if peer.best_height > our_height {
                eprintln!(
                    "[P2P] Peer {} has height {}, we have {}. Requesting blocks...",
                    peer.address, peer.best_height, our_height
                );

                match TcpStream::connect(peer.address).await {
                    Ok(stream) => {
                        let (reader, writer) = stream.into_split();
                        let reader = Arc::new(tokio::sync::Mutex::new(reader));
                        let writer = Arc::new(tokio::sync::Mutex::new(writer));

                        let request = PeerMessage::GetHeaders {
                            from_height: our_height + 1,
                            count: 100,
                        };
                        let _ = self.write_message(writer.clone(), &request).await;

                        for _ in 0..100 {
                            if let Ok(msg) = self.read_message(reader.clone()).await {
                                if let PeerMessage::Block(block) = msg {
                                    let _ = self.node.add_block(block);
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[P2P] Failed to connect to {} for block sync: {}", peer.address, e);
                    }
                }
            }
        }
        Ok(())
    }
}

impl Clone for P2PManager {
    fn clone(&self) -> Self {
        Self {
            listen_addr: self.listen_addr,
            peers: self.peers.clone(),
            node: self.node.clone(),
            max_peers: self.max_peers,
        }
    }
}
