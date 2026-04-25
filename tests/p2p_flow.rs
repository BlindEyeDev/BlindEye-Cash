use blindeye::node::Node;
use blindeye::p2p::P2PManager;
use blindeye::wallet::Wallet;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

fn free_local_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("read local addr");
    drop(listener);
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bootstrap_connection_updates_live_and_node_peer_views() {
    let node_a = Arc::new(Node::new(None));
    let node_b = Arc::new(Node::new(None));
    let addr_a = free_local_addr();
    let addr_b = free_local_addr();
    let manager_a = Arc::new(P2PManager::new(addr_a, node_a.clone(), 16));
    let manager_b = Arc::new(P2PManager::new(addr_b, node_b.clone(), 16));

    let handle_a = tokio::spawn(manager_a.clone().start());
    let handle_b = tokio::spawn(manager_b.clone().start());

    tokio::time::sleep(Duration::from_millis(150)).await;
    manager_b
        .clone()
        .connect_peer(addr_a)
        .await
        .expect("connect bootstrap peer");
    tokio::time::sleep(Duration::from_millis(350)).await;

    assert!(manager_a.peer_count().await >= 1);
    assert!(manager_b.peer_count().await >= 1);
    assert!(node_a.get_status().connected_peers >= 1);
    assert!(node_b.get_status().connected_peers >= 1);

    handle_a.abort();
    handle_b.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn synchronizes_blocks_from_connected_peer() {
    let node_a = Arc::new(Node::new(None));
    let node_b = Arc::new(Node::new(None));
    let miner = Wallet::new();
    node_a
        .mine_one_block(&miner.address_bytes())
        .expect("mine source block");

    let addr_a = free_local_addr();
    let addr_b = free_local_addr();
    let manager_a = Arc::new(P2PManager::new(addr_a, node_a.clone(), 16));
    let manager_b = Arc::new(P2PManager::new(addr_b, node_b.clone(), 16));

    let handle_a = tokio::spawn(manager_a.clone().start());
    let handle_b = tokio::spawn(manager_b.clone().start());

    tokio::time::sleep(Duration::from_millis(150)).await;
    manager_b
        .clone()
        .connect_peer(addr_a)
        .await
        .expect("connect for sync");
    tokio::time::sleep(Duration::from_millis(350)).await;

    let imported_blocks = manager_b
        .synchronize_blocks()
        .await
        .expect("synchronize blocks");

    assert!(imported_blocks >= 1);
    assert_eq!(node_b.get_best_height(), node_a.get_best_height());

    handle_a.abort();
    handle_b.abort();
}
