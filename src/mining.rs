use crate::block::{Block, BlockHeader};
use crate::pow::BlindHash;
use crate::transaction::{Transaction, TxOutput};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct MiningSettings {
    pub worker_count: usize,
    pub mine_empty_blocks: bool,
}

impl Default for MiningSettings {
    fn default() -> Self {
        Self {
            worker_count: thread::available_parallelism()
                .map(|parallelism| parallelism.get())
                .unwrap_or(1)
                .clamp(1, 32),
            mine_empty_blocks: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MiningSnapshot {
    pub active: bool,
    pub worker_count: usize,
    pub hash_rate: f64,
    pub total_hashes: u64,
    pub last_block_hash: Option<[u8; 32]>,
    pub log_lines: Vec<String>,
}

#[derive(Debug)]
pub struct Miner {
    pub running: Arc<AtomicBool>,
    total_hashes: Arc<AtomicU64>,
    started_at: Arc<Mutex<Option<Instant>>>,
    last_block_hash: Arc<Mutex<Option<[u8; 32]>>>,
    logs: Arc<Mutex<VecDeque<String>>>,
    worker_count: Arc<Mutex<usize>>,
    mine_empty_blocks: Arc<AtomicBool>,
    controller: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Miner {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            total_hashes: Arc::new(AtomicU64::new(0)),
            started_at: Arc::new(Mutex::new(None)),
            last_block_hash: Arc::new(Mutex::new(None)),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(256))),
            worker_count: Arc::new(Mutex::new(MiningSettings::default().worker_count)),
            mine_empty_blocks: Arc::new(AtomicBool::new(true)),
            controller: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start_continuous<MakeTemplate, SubmitBlock>(
        &self,
        settings: MiningSettings,
        make_template: MakeTemplate,
        submit_block: SubmitBlock,
    ) -> Result<(), String>
    where
        MakeTemplate: Fn() -> Result<BlockTemplate, String> + Send + Sync + 'static,
        SubmitBlock: Fn(Block) -> Result<(), String> + Send + Sync + 'static,
    {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("Mining is already running".to_string());
        }

        self.total_hashes.store(0, Ordering::SeqCst);
        *self.started_at.lock().unwrap() = Some(Instant::now());
        *self.last_block_hash.lock().unwrap() = None;
        *self.worker_count.lock().unwrap() = settings.worker_count.max(1);
        self.mine_empty_blocks
            .store(settings.mine_empty_blocks, Ordering::SeqCst);
        self.push_log(format!(
            "Mining started with {} worker(s)",
            settings.worker_count.max(1)
        ));

        let running = self.running.clone();
        let total_hashes = self.total_hashes.clone();
        let last_block_hash = self.last_block_hash.clone();
        let logs = self.logs.clone();
        let worker_count = settings.worker_count.max(1);
        let mine_empty_blocks = settings.mine_empty_blocks;
        let make_template = Arc::new(make_template);
        let submit_block = Arc::new(submit_block);

        let handle = thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                let template = match make_template() {
                    Ok(template) => template,
                    Err(err) => {
                        push_log_line(&logs, format!("Template error: {err}"));
                        thread::sleep(std::time::Duration::from_millis(250));
                        continue;
                    }
                };

                if !mine_empty_blocks && template.block.transactions.len() <= 1 {
                    push_log_line(&logs, "Waiting for transactions before mining".to_string());
                    thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }

                match mine_template_parallel(
                    template,
                    worker_count,
                    running.clone(),
                    total_hashes.clone(),
                ) {
                    Ok(block) => {
                        let block_hash = block.hash();
                        if let Err(err) = submit_block(block.clone()) {
                            push_log_line(&logs, format!("Submit block failed: {err}"));
                            thread::sleep(std::time::Duration::from_millis(100));
                            continue;
                        }
                        *last_block_hash.lock().unwrap() = Some(block_hash);
                        push_log_line(
                            &logs,
                            format!(
                                "Accepted block {} at height {}",
                                hex::encode(block_hash),
                                block.header.height
                            ),
                        );
                    }
                    Err(MiningError::Stopped) => break,
                    Err(MiningError::WorkerFailed(err)) => {
                        push_log_line(&logs, format!("Mining worker failed: {err}"));
                        thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }

            push_log_line(&logs, "Mining stopped".to_string());
        });

        *self.controller.lock().unwrap() = Some(handle);
        Ok(())
    }

    pub fn stop_mining(&self) {
        self.running.store(false, Ordering::SeqCst);
        *self.started_at.lock().unwrap() = None;
        if let Some(handle) = self.controller.lock().unwrap().take() {
            let _ = handle.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> MiningSnapshot {
        let total_hashes = self.total_hashes.load(Ordering::SeqCst);
        let hash_rate = if self.is_running() {
            self.started_at
                .lock()
                .unwrap()
                .map(|started_at| {
                    let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
                    total_hashes as f64 / elapsed
                })
                .unwrap_or(0.0)
        } else {
            0.0
        };

        MiningSnapshot {
            active: self.is_running(),
            worker_count: *self.worker_count.lock().unwrap(),
            hash_rate,
            total_hashes,
            last_block_hash: *self.last_block_hash.lock().unwrap(),
            log_lines: self.logs.lock().unwrap().iter().cloned().collect(),
        }
    }

    pub fn push_log(&self, line: String) {
        push_log_line(&self.logs, line);
    }
}

fn push_log_line(logs: &Arc<Mutex<VecDeque<String>>>, line: String) {
    let mut logs = logs.lock().unwrap();
    while logs.len() >= 200 {
        logs.pop_front();
    }
    logs.push_back(line);
}

pub(crate) fn mine_template_parallel(
    template: BlockTemplate,
    worker_count: usize,
    running: Arc<AtomicBool>,
    total_hashes: Arc<AtomicU64>,
) -> Result<Block, MiningError> {
    let (sender, receiver) = mpsc::channel::<Result<Block, String>>();
    let found = Arc::new(AtomicBool::new(false));
    let template = Arc::new(template);
    let mut handles = Vec::new();

    for worker_id in 0..worker_count {
        let sender = sender.clone();
        let found = found.clone();
        let running = running.clone();
        let total_hashes = total_hashes.clone();
        let template = template.clone();

        handles.push(thread::spawn(move || {
            let mut header = template.header.clone();
            let target = BlindHash::target_from_bits(header.bits);
            let mut nonce = worker_id as u64;
            let step = worker_count as u64;

            loop {
                if !running.load(Ordering::SeqCst) || found.load(Ordering::SeqCst) {
                    return;
                }

                header.nonce = nonce;
                let hash = BlindHash::hash(&header.serialize());
                total_hashes.fetch_add(1, Ordering::SeqCst);
                let hash_value = u128::from_le_bytes(hash[0..16].try_into().unwrap());

                if hash_value <= target {
                    found.store(true, Ordering::SeqCst);
                    let mut block = template.block.clone();
                    block.header = header;
                    let _ = sender.send(Ok(block));
                    return;
                }

                nonce = nonce.wrapping_add(step);
            }
        }));
    }
    drop(sender);

    let result = loop {
        if !running.load(Ordering::SeqCst) {
            break Err(MiningError::Stopped);
        }
        match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Ok(block)) => break Ok(block),
            Ok(Err(err)) => break Err(MiningError::WorkerFailed(err)),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break Err(MiningError::Stopped),
        }
    };

    found.store(true, Ordering::SeqCst);
    for handle in handles {
        let _ = handle.join();
    }

    result
}

#[derive(Debug, Clone)]
pub struct BlockTemplate {
    pub block: Block,
    pub header: BlockHeader,
}

impl BlockTemplate {
    pub fn new(
        previous_block_hash: [u8; 32],
        mut transactions: Vec<Transaction>,
        height: u64,
        block_reward: u64,
        miner_address: &[u8],
        bits: u32,
    ) -> Self {
        let coinbase = create_coinbase_transaction(height, block_reward, miner_address);
        transactions.insert(0, coinbase);

        let block = Block::with_bits(previous_block_hash, transactions, height, bits);
        let header = block.header.clone();
        Self { block, header }
    }
}

pub fn create_coinbase_transaction(height: u64, reward: u64, miner_address: &[u8]) -> Transaction {
    let _ = height;

    Transaction::new(
        vec![],
        vec![TxOutput {
            value: reward,
            script_pubkey: miner_address.to_vec(),
        }],
    )
}

#[derive(Debug)]
pub enum MiningError {
    Stopped,
    WorkerFailed(String),
}
