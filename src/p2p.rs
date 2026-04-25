use crate::block::Block;
use crate::network::Peer;
use crate::node::Node;
use crate::transaction::Transaction;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
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
        listen_addr: String,
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
    pub connection_addr: SocketAddr,
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

    fn local_handshake(&self) -> PeerMessage {
        PeerMessage::Handshake {
            version: 1,
            best_height: self.node.get_best_height(),
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            listen_addr: self.listen_addr.to_string(),
        }
    }

    fn canonicalize_advertised_addr(
        peer_addr: SocketAddr,
        listen_addr: &str,
    ) -> SocketAddr {
        match listen_addr.parse::<SocketAddr>() {
            Ok(mut advertised_addr) => {
                if advertised_addr.ip().is_unspecified() {
                    advertised_addr.set_ip(peer_addr.ip());
                }
                advertised_addr
            }
            Err(_) => peer_addr,
        }
    }

    fn sync_node_peer_manager(
        &self,
        peers: &HashMap<SocketAddr, ConnectedPeer>,
    ) {
        let mut deduped = HashMap::<String, Peer>::new();
        for peer in peers.values() {
            let key = peer.address.to_string();
            deduped
                .entry(key.clone())
                .and_modify(|existing| {
                    existing.best_height = existing.best_height.max(peer.best_height);
                })
                .or_insert_with(|| Peer {
                    id: key.clone(),
                    best_height: peer.best_height,
                    address: key,
                });
        }

        let mut peer_entries: Vec<_> = deduped.into_values().collect();
        peer_entries.sort_by(|left, right| left.address.cmp(&right.address));
        self.node.peer_manager.lock().unwrap().peers = peer_entries;
    }

    async fn upsert_peer(
        &self,
        connection_addr: SocketAddr,
        advertised_addr: SocketAddr,
        best_height: u64,
        version: u32,
    ) {
        let mut peers = self.peers.write().await;
        peers.insert(
            connection_addr,
            ConnectedPeer {
                connection_addr,
                address: advertised_addr,
                best_height,
                last_seen: SystemTime::now(),
                version,
            },
        );
        self.sync_node_peer_manager(&peers);
    }

    async fn remove_peer(&self, connection_addr: &SocketAddr) {
        let mut peers = self.peers.write().await;
        peers.remove(connection_addr);
        self.sync_node_peer_manager(&peers);
    }

    async fn note_peer_activity(
        &self,
        connection_addr: &SocketAddr,
        best_height: Option<u64>,
    ) {
        let mut peers = self.peers.write().await;
        let mut should_sync = false;
        if let Some(peer) = peers.get_mut(connection_addr) {
            peer.last_seen = SystemTime::now();
            if let Some(best_height) = best_height {
                peer.best_height = peer.best_height.max(best_height);
                should_sync = true;
            }
        }
        if should_sync {
            self.sync_node_peer_manager(&peers);
        }
    }

    fn spawn_keepalive(&self, writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>) {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(60));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let nonce = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;
                if manager
                    .write_message(writer.clone(), &PeerMessage::Ping { nonce })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    async fn connected_peer_addresses(
        &self,
        excluded_addr: Option<SocketAddr>,
    ) -> Vec<SocketAddr> {
        self.peers
            .read()
            .await
            .values()
            .map(|peer| peer.address)
            .filter(|peer_addr| Some(*peer_addr) != excluded_addr)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }

    async fn advertised_addr_for_connection(
        &self,
        connection_addr: &SocketAddr,
    ) -> Option<SocketAddr> {
        self.peers
            .read()
            .await
            .get(connection_addr)
            .map(|peer| peer.address)
    }

    /// Start the P2P network listener
    pub async fn start(self: Arc<Self>) -> P2PResult<()> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        eprintln!("[P2P] Listening on {}", self.listen_addr);

        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let mut peers_map = manager.peers.write().await;
                peers_map.retain(|_, peer| {
                    peer
                        .last_seen
                        .elapsed()
                        .unwrap_or(Duration::from_secs(300))
                        < Duration::from_secs(300)
                });
                manager.sync_node_peer_manager(&peers_map);
            }
        });

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    if self.peers.read().await.len() >= self.max_peers {
                        eprintln!("[P2P] Max peers reached, rejecting {}", addr);
                        continue;
                    }
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
    pub async fn connect_peer(&self, addr: SocketAddr) -> P2PResult<()> {
        if addr == self.listen_addr
            || self
                .peers
                .read()
                .await
                .values()
                .any(|peer| peer.address == addr)
        {
            return Ok(());
        }

        match TcpStream::connect(addr).await {
            Ok(stream) => {
                eprintln!("[P2P] Connected to bootstrap peer {}", addr);
                let manager = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = manager.handle_peer_connection(stream, addr).await {
                        eprintln!("[P2P] Outbound peer {} failed: {}", addr, e);
                    }
                });
                Ok(())
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

        let peer_connection_addr = addr;
        let handshake = self.read_message(reader.clone()).await?;
        let peer_advertised_addr = if let PeerMessage::Handshake {
            version,
            best_height,
            listen_addr,
            ..
        } = handshake
        {
            let advertised_addr =
                Self::canonicalize_advertised_addr(peer_connection_addr, &listen_addr);
            self.upsert_peer(peer_connection_addr, advertised_addr, best_height, version)
                .await;
            eprintln!(
                "[P2P] Peer handshake from {} (height: {}, advertised: {})",
                peer_connection_addr, best_height, advertised_addr
            );
            advertised_addr
        } else {
            return Err(P2PError::InvalidMessage);
        };

        // Send our handshake
        self.write_message(writer.clone(), &self.local_handshake()).await?;
        self.spawn_keepalive(writer.clone());
        self.write_message(writer.clone(), &PeerMessage::GetPeers).await?;

        loop {
            match self.read_message(reader.clone()).await {
                Ok(msg) => {
                    self.handle_message(peer_connection_addr, msg, writer.clone())
                        .await?;
                }
                Err(P2PError::PeerDisconnected) => {
                    eprintln!(
                        "[P2P] Peer {} disconnected",
                        peer_advertised_addr
                    );
                    self.remove_peer(&peer_connection_addr).await;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[P2P] Message error from {}: {}",
                        peer_advertised_addr, e
                    );
                    self.remove_peer(&peer_connection_addr).await;
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

        let peer_connection_addr = addr;
        self.write_message(writer.clone(), &self.local_handshake()).await?;

        // Receive handshake
        let peer_advertised_addr = if let PeerMessage::Handshake {
            version,
            best_height,
            listen_addr,
            ..
        } = self.read_message(reader.clone()).await?
        {
            let advertised_addr =
                Self::canonicalize_advertised_addr(peer_connection_addr, &listen_addr);
            self.upsert_peer(peer_connection_addr, advertised_addr, best_height, version)
                .await;
            advertised_addr
        } else {
            return Err(P2PError::InvalidMessage);
        };
        self.spawn_keepalive(writer.clone());
        self.write_message(writer.clone(), &PeerMessage::GetPeers).await?;

        loop {
            match self.read_message(reader.clone()).await {
                Ok(msg) => {
                    self.handle_message(peer_connection_addr, msg, writer.clone())
                        .await?;
                }
                Err(P2PError::PeerDisconnected) => {
                    eprintln!("[P2P] Peer {} disconnected", peer_advertised_addr);
                    self.remove_peer(&peer_connection_addr).await;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[P2P] Message error from {}: {}",
                        peer_advertised_addr, e
                    );
                    self.remove_peer(&peer_connection_addr).await;
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle incoming P2P message
    fn handle_message(
        &self,
        connection_addr: SocketAddr,
        msg: PeerMessage,
        writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    ) -> Pin<Box<dyn Future<Output = P2PResult<()>> + Send + '_>> {
        Box::pin(async move {
        let best_height = match &msg {
            PeerMessage::Block(block) => Some(block.header.height),
            PeerMessage::Handshake { best_height, .. } => Some(*best_height),
            _ => None,
        };
        self.note_peer_activity(&connection_addr, best_height).await;

        match msg {
            PeerMessage::Block(block) => {
                if let Err(e) = self.node.add_block(block.clone()) {
                    eprintln!("[P2P] Invalid block from {}: {}", connection_addr, e);
                } else {
                    let origin_addr = self
                        .advertised_addr_for_connection(&connection_addr)
                        .await;
                    let relay_msg = PeerMessage::Block(block);
                    eprintln!(
                        "[P2P] Block received and added from {}",
                        connection_addr
                    );
                    for peer_addr in self.connected_peer_addresses(origin_addr).await {
                        if let Err(e) = self.broadcast_message(peer_addr, &relay_msg).await {
                            eprintln!(
                                "[P2P] Failed to relay block to {}: {}",
                                peer_addr, e
                            );
                        }
                    }
                }
            }
            PeerMessage::Transaction(tx) => {
                if let Err(e) = self.node.submit_transaction(tx.clone()) {
                    eprintln!("[P2P] Invalid transaction from {}: {}", connection_addr, e);
                } else {
                    let origin_addr = self
                        .advertised_addr_for_connection(&connection_addr)
                        .await;
                    let relay_msg = PeerMessage::Transaction(tx);
                    eprintln!("[P2P] Transaction received from {}", connection_addr);
                    for peer_addr in self.connected_peer_addresses(origin_addr).await {
                        if let Err(e) = self.broadcast_message(peer_addr, &relay_msg).await {
                            eprintln!(
                                "[P2P] Failed to relay transaction to {}: {}",
                                peer_addr, e
                            );
                        }
                    }
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
            PeerMessage::Peers(addresses) => {
                for address in addresses {
                    if let Ok(address) = address.parse::<SocketAddr>() {
                        if address == self.listen_addr {
                            continue;
                        }
                        let manager = self.clone();
                        tokio::spawn(async move {
                            let _ = manager.connect_peer(address).await;
                        });
                    }
                }
            }
            PeerMessage::GetHeaders { from_height, count } => {
                for block in self.node.blocks_from_height(from_height, count) {
                    self.write_message(writer.clone(), &PeerMessage::Block(block))
                        .await?;
                }
                self.write_message(writer, &PeerMessage::Ping { nonce: 0 })
                    .await?;
            }
            PeerMessage::Ping { nonce } => {
                self.write_message(writer, &PeerMessage::Pong { nonce })
                    .await?;
            }
            PeerMessage::Pong { .. } => {}
            _ => {}
        }
        Ok(())
        })
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
        let peers = self.connected_peer_addresses(None).await;

        for peer_addr in peers {
            if let Err(e) = self.broadcast_message(peer_addr, &msg).await {
                eprintln!("[P2P] Failed to broadcast to {}: {}", peer_addr, e);
            }
        }
    }

    /// Broadcast a transaction to all peers
    pub async fn broadcast_transaction(&self, tx: &Transaction) {
        let msg = PeerMessage::Transaction(tx.clone());
        let peers = self.connected_peer_addresses(None).await;

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
        let reader = Arc::new(tokio::sync::Mutex::new(reader));
        let writer = Arc::new(tokio::sync::Mutex::new(writer));
        self.write_message(writer.clone(), &self.local_handshake()).await?;
        match self.read_message(reader).await? {
            PeerMessage::Handshake { .. } => {}
            _ => return Err(P2PError::InvalidMessage),
        }
        self.write_message(writer, msg).await
    }

    /// Get connected peers count
    pub async fn peer_count(&self) -> usize {
        self.peers
            .read()
            .await
            .values()
            .map(|peer| peer.address)
            .collect::<HashSet<_>>()
            .len()
    }

    pub fn peer_count_now(&self) -> usize {
        self.peers
            .try_read()
            .map(|peers| peers.values().map(|peer| peer.address).collect::<HashSet<_>>().len())
            .unwrap_or(0)
    }

    pub fn best_known_height_now(&self) -> u64 {
        self.peers
            .try_read()
            .map(|peers| peers.values().map(|peer| peer.best_height).max().unwrap_or(0))
            .unwrap_or(0)
    }

    pub fn peers_now(&self) -> Vec<ConnectedPeer> {
        let mut peers = self
            .peers
            .try_read()
            .map(|peers| peers.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        peers.sort_by(|left, right| left.address.cmp(&right.address));
        peers
    }

    /// Get list of connected peers
    pub async fn get_peers(&self) -> Vec<ConnectedPeer> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Synchronize blocks from peers
    pub async fn synchronize_blocks(&self) -> P2PResult<usize> {
        let peers = self
            .peers
            .read()
            .await
            .values()
            .fold(HashMap::<SocketAddr, u64>::new(), |mut deduped, peer| {
                deduped
                    .entry(peer.address)
                    .and_modify(|height| *height = (*height).max(peer.best_height))
                    .or_insert(peer.best_height);
                deduped
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut local_height = self.node.get_best_height();
        let mut imported_blocks = 0usize;

        for (peer_addr, peer_best_height) in peers {
            if peer_best_height > local_height {
                eprintln!(
                    "[P2P] Peer {} has height {}, we have {}. Requesting blocks...",
                    peer_addr, peer_best_height, local_height
                );

                match TcpStream::connect(peer_addr).await {
                    Ok(stream) => {
                        let (reader, writer) = stream.into_split();
                        let reader = Arc::new(tokio::sync::Mutex::new(reader));
                        let writer = Arc::new(tokio::sync::Mutex::new(writer));
                        self.write_message(writer.clone(), &self.local_handshake()).await?;
                        match self.read_message(reader.clone()).await? {
                            PeerMessage::Handshake { .. } => {}
                            _ => return Err(P2PError::InvalidMessage),
                        }

                        let request_count =
                            peer_best_height.saturating_sub(local_height).min(100) as u32;

                        let request = PeerMessage::GetHeaders {
                            from_height: local_height + 1,
                            count: request_count,
                        };
                        self.write_message(writer.clone(), &request).await?;

                        for _ in 0..request_count {
                            if let Ok(msg) = self.read_message(reader.clone()).await {
                                if let PeerMessage::Block(block) = msg {
                                    let previous_height = self.node.get_best_height();
                                    if self.node.add_block(block).is_ok() {
                                        let new_height = self.node.get_best_height();
                                        if new_height > previous_height {
                                            imported_blocks += (new_height - previous_height) as usize;
                                            local_height = new_height;
                                        }
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[P2P] Failed to connect to {} for block sync: {}", peer_addr, e);
                    }
                }
            }
        }
        Ok(imported_blocks)
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
