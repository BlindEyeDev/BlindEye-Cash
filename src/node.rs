use crate::block::Block;
use crate::blockchain::Blockchain;
use crate::mempool::Mempool;
use crate::mining::{BlockTemplate, Miner, MiningSettings, MiningSnapshot};
use crate::network::{NetworkConfig, PeerManager};
use crate::protocol::{format_bec_amount, ConsensusParameters, EmissionSchedule};
use crate::rpc::RpcServer;
use crate::transaction::Transaction;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_NODE_STATE_PATH: &str = "blindeye-node-state.bin";

#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub best_height: u64,
    pub mempool_size: usize,
    pub connected_peers: usize,
    pub consensus_threshold: usize,
    pub mining_active: bool,
    pub hash_rate: f64,
    pub total_hashes: u64,
    pub rpc_active: bool,
    pub rpc_bind_addr: String,
    pub rpc_advertised_url: String,
    pub rpc_allow_remote: bool,
    pub standard_fee_rate: u64,
    pub instant_fee_rate: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeDiskState {
    blockchain: Blockchain,
    mempool: Mempool,
    peer_manager: PeerManager,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub blockchain: Arc<Mutex<Blockchain>>,
    pub mempool: Arc<Mutex<Mempool>>,
    pub miner: Arc<Miner>,
    pub rpc_server: Arc<RpcServer>,
    pub peer_manager: Arc<Mutex<PeerManager>>,
    pub consensus_params: ConsensusParameters,
    pub storage_path: Arc<PathBuf>,
}

impl Node {
    #[allow(dead_code)]
    pub fn new(network_config: Option<NetworkConfig>) -> Self {
        Self::in_memory(network_config)
    }

    pub fn in_memory(network_config: Option<NetworkConfig>) -> Self {
        let blockchain = Blockchain::new();
        let consensus_params = ConsensusParameters::default();
        let network = network_config.unwrap_or_default();

        Self {
            blockchain: Arc::new(Mutex::new(blockchain)),
            mempool: Arc::new(Mutex::new(Mempool::new(10000, 1))),
            miner: Arc::new(Miner::new()),
            rpc_server: Arc::new(RpcServer::new()),
            peer_manager: Arc::new(Mutex::new(PeerManager::new(network))),
            consensus_params,
            storage_path: Arc::new(PathBuf::from(DEFAULT_NODE_STATE_PATH)),
        }
    }

    pub fn load_or_create<P: AsRef<Path>>(
        storage_path: P,
        network_config: Option<NetworkConfig>,
    ) -> Result<Self, String> {
        let storage_path = storage_path.as_ref().to_path_buf();
        let consensus_params = ConsensusParameters::default();
        let network = network_config.unwrap_or_default();

        let (blockchain, mempool, peer_manager) = if storage_path.exists() {
            let contents = fs::read(&storage_path)
                .map_err(|err| format!("Failed to read node state: {err}"))?;
            let disk_state: NodeDiskState = bincode::deserialize(&contents)
                .map_err(|err| format!("Failed to parse node state: {err}"))?;
            (
                disk_state.blockchain,
                disk_state.mempool,
                disk_state.peer_manager,
            )
        } else {
            (
                Blockchain::new(),
                Mempool::new(10000, 1),
                PeerManager::new(network.clone()),
            )
        };

        let node = Self {
            blockchain: Arc::new(Mutex::new(blockchain)),
            mempool: Arc::new(Mutex::new(mempool)),
            miner: Arc::new(Miner::new()),
            rpc_server: Arc::new(RpcServer::new()),
            peer_manager: Arc::new(Mutex::new(peer_manager)),
            consensus_params,
            storage_path: Arc::new(storage_path),
        };

        node.save_state()?;
        Ok(node)
    }

    pub fn save_state(&self) -> Result<(), String> {
        let disk_state = NodeDiskState {
            blockchain: self.blockchain.lock().unwrap().clone(),
            mempool: self.mempool.lock().unwrap().clone(),
            peer_manager: self.peer_manager.lock().unwrap().clone(),
        };
        let serialized = bincode::serialize(&disk_state)
            .map_err(|err| format!("Failed to serialize node state: {err}"))?;
        fs::write(self.storage_path.as_ref(), serialized)
            .map_err(|err| format!("Failed to write node state: {err}"))
    }

    pub fn submit_transaction(&self, transaction: Transaction) -> Result<(), String> {
        let txid = transaction.txid();
        let fee_paid = {
            let blockchain = self.blockchain.lock().unwrap();
            if blockchain.contains_transaction(&txid) {
                return Err("Transaction is already confirmed on the chain".to_string());
            }
            blockchain
                .calculate_transaction_fee(&transaction)
                .map_err(|e| e.to_string())?
        };

        let mut mempool = self.mempool.lock().unwrap();
        if mempool.contains_transaction(&txid) {
            return Err("Transaction is already in the mempool".to_string());
        }
        let required_fee = mempool.minimum_required_fee(&transaction);
        if fee_paid < required_fee {
            return Err(format!(
                "Transaction fee is too low: requires at least {} BEC, but only {} BEC were provided",
                format_bec_amount(required_fee),
                format_bec_amount(fee_paid)
            ));
        }
        if mempool.has_conflicting_inputs(&transaction) {
            return Err("Transaction conflicts with an existing mempool entry".to_string());
        }
        mempool
            .add_transaction(transaction, fee_paid)
            .map_err(|e| e.to_string())?;
        drop(mempool);
        self.save_state()
    }

    pub fn get_best_height(&self) -> u64 {
        let blockchain = self.blockchain.lock().unwrap();
        blockchain.current_height
    }

    pub fn get_block(&self, hash: &[u8; 32]) -> Option<Block> {
        let blockchain = self.blockchain.lock().unwrap();
        blockchain.get_block(hash).cloned()
    }

    pub fn create_block_template(&self, miner_address: &[u8]) -> Result<BlockTemplate, String> {
        let blockchain = self.blockchain.lock().unwrap();
        let mempool = self.mempool.lock().unwrap();

        let best_hash = blockchain
            .best_chain
            .last()
            .copied()
            .ok_or("No best chain")?;
        let previous_timestamp = blockchain
            .blocks
            .get(&best_hash)
            .map(|block| block.header.timestamp)
            .ok_or("Best block missing")?;

        let (transactions, total_fees) = mempool.get_transactions_for_block(1_000_000);
        let tx_count = transactions.len();
        let height = blockchain.current_height + 1;
        let emission_schedule = EmissionSchedule::default();
        let block_reward = emission_schedule
            .block_reward(height)
            .saturating_add(total_fees);
        let bits = blockchain.next_work_bits(tx_count);
        let template_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .max(previous_timestamp);
        let mut template = BlockTemplate::new(
            best_hash,
            transactions,
            height,
            block_reward,
            miner_address,
            bits,
        );
        template.header.timestamp = template_timestamp;
        template.block.header.timestamp = template_timestamp;

        Ok(template)
    }

    pub fn start_continuous_mining(
        &self,
        miner_address: &[u8],
        settings: MiningSettings,
    ) -> Result<(), String> {
        let address = miner_address.to_vec();
        let node = self.clone();
        let node_for_submit = self.clone();
        self.miner.start_continuous(
            settings,
            move || node.create_block_template(&address),
            move |block| node_for_submit.submit_block(block),
        )
    }

    #[allow(dead_code)]
    pub fn mine_one_block(&self, miner_address: &[u8]) -> Result<Block, String> {
        let template = self.create_block_template(miner_address)?;
        let running = Arc::new(AtomicBool::new(true));
        let session_id = Arc::new(AtomicU64::new(1));
        let block = crate::mining::mine_template_parallel(
            template,
            1,
            running,
            session_id,
            1,
            Arc::new(AtomicU64::new(0)),
        )
        .map_err(|err| format!("Mining failed: {err:?}"))?;
        self.submit_block(block.clone())?;
        Ok(block)
    }

    pub fn stop_continuous_mining(&self) {
        self.miner.stop_mining();
    }

    pub fn mining_snapshot(&self) -> MiningSnapshot {
        self.miner.snapshot()
    }

    pub fn submit_block(&self, block: Block) -> Result<(), String> {
        let submitted_hash = block.hash();
        let mut blockchain = self.blockchain.lock().unwrap();
        blockchain.add_block(block).map_err(|e| e.to_string())?;

        let mut mempool = self.mempool.lock().unwrap();
        if blockchain.best_chain.last().copied() == Some(submitted_hash) {
            let confirmed_transactions = blockchain
                .blocks
                .get(&submitted_hash)
                .map(|block| block.transactions.clone())
                .unwrap_or_default();
            mempool.remove_confirmed_transactions(&confirmed_transactions);
        }
        drop(mempool);
        drop(blockchain);
        self.save_state()
    }

    pub fn add_block(&self, block: Block) -> Result<(), String> {
        self.submit_block(block)
    }

    pub fn blocks_from_height(&self, from_height: u64, count: u32) -> Vec<Block> {
        let blockchain = self.blockchain.lock().unwrap();
        blockchain
            .best_chain
            .iter()
            .filter_map(|hash| blockchain.blocks.get(hash))
            .filter(|block| block.header.height >= from_height)
            .take(count as usize)
            .cloned()
            .collect()
    }

    pub fn get_status(&self) -> NodeStatus {
        let connected_peers = self
            .peer_manager
            .lock()
            .unwrap()
            .get_connected_peers()
            .len();
        let best_height = self.get_best_height();
        let mempool_size = self.mempool.lock().unwrap().size();
        let (standard_fee_rate, instant_fee_rate) = {
            let mempool = self.mempool.lock().unwrap();
            (mempool.standard_fee_rate(), mempool.instant_fee_rate())
        };
        let mining = self.miner.snapshot();
        let rpc = self.rpc_server.snapshot();

        NodeStatus {
            best_height,
            mempool_size,
            connected_peers,
            consensus_threshold: self
                .consensus_params
                .required_supermajority(connected_peers + 1),
            mining_active: mining.active,
            hash_rate: mining.hash_rate,
            total_hashes: mining.total_hashes,
            rpc_active: rpc.active,
            rpc_bind_addr: rpc.bind_addr,
            rpc_advertised_url: rpc.advertised_url,
            rpc_allow_remote: rpc.allow_remote,
            standard_fee_rate,
            instant_fee_rate,
        }
    }

    #[allow(dead_code)]
    pub fn fund_address_for_testing(&self, address: &str, amount: u64) -> Result<(), String> {
        let mut blockchain = self.blockchain.lock().unwrap();
        blockchain
            .fund_address_for_testing(address, amount)
            .map_err(|e| e.to_string())?;
        drop(blockchain);
        self.save_state()
    }

    pub fn reset_to_genesis(&self) -> Result<(), String> {
        self.stop_continuous_mining();
        self.rpc_server.stop();
        *self.blockchain.lock().unwrap() = Blockchain::new();
        *self.mempool.lock().unwrap() = Mempool::new(10000, 1);
        self.peer_manager.lock().unwrap().peers.clear();
        self.miner
            .push_log("Local blockchain reset to genesis block".to_string());
        self.save_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mining::create_coinbase_transaction;
    use crate::protocol;

    #[test]
    fn block_template_timestamp_never_precedes_tip() {
        let node = Node::in_memory(None);
        let miner_address = b"miner-address".to_vec();

        {
            let mut blockchain = node.blockchain.lock().unwrap();
            let genesis_hash = blockchain.get_best_block_hash();
            let genesis_timestamp = blockchain
                .blocks
                .get(&genesis_hash)
                .expect("genesis block must exist")
                .header
                .timestamp;

            let mut block = Block::with_bits(
                genesis_hash,
                vec![create_coinbase_transaction(1, 100, &miner_address)],
                1,
                protocol::DEFAULT_BITS,
            );
            block.header.timestamp = genesis_timestamp + 10;
            blockchain.add_block(block).expect("tip block should be accepted");
        }

        let template = node
            .create_block_template(&miner_address)
            .expect("template should be created");
        let tip_timestamp = {
            let blockchain = node.blockchain.lock().unwrap();
            let tip_hash = blockchain.get_best_block_hash();
            blockchain
                .blocks
                .get(&tip_hash)
                .expect("tip block must exist")
                .header
                .timestamp
        };

        assert!(template.header.timestamp >= tip_timestamp);
        assert_eq!(template.block.header.timestamp, template.header.timestamp);
    }
}
