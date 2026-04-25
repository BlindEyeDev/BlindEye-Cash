use crate::block::{Block, BlockHeader};
use crate::pow::BlindHash;
use crate::protocol;
use crate::transaction::{address_from_script_sig, OutPoint, Transaction, TxOutput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UTXO {
    pub output: TxOutput,
    pub block_height: u64,
    pub is_coinbase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletTransactionDirection {
    Incoming,
    Outgoing,
    MiningReward,
    SelfTransfer,
}

#[derive(Debug, Clone)]
pub struct WalletTransactionRecord {
    pub transaction: Transaction,
    pub txid: [u8; 32],
    pub height: u64,
    pub timestamp: u64,
    pub direction: WalletTransactionDirection,
    pub amount: u64,
    pub fee: u64,
    pub counterparty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blockchain {
    pub blocks: HashMap<[u8; 32], Block>,
    pub headers: HashMap<[u8; 32], BlockHeader>,
    pub utxo_set: HashMap<OutPoint, UTXO>,
    pub bootstrap_utxos: HashMap<OutPoint, UTXO>,
    pub best_chain: Vec<[u8; 32]>,
    pub genesis_hash: [u8; 32],
    pub current_height: u64,
}

impl Blockchain {
    pub fn new() -> Self {
        let mut blockchain = Self {
            blocks: HashMap::new(),
            headers: HashMap::new(),
            utxo_set: HashMap::new(),
            bootstrap_utxos: HashMap::new(),
            best_chain: Vec::new(),
            genesis_hash: [0; 32],
            current_height: 0,
        };

        let genesis_block = Self::create_genesis_block();
        let genesis_hash = genesis_block.hash();
        blockchain.genesis_hash = genesis_hash;
        blockchain.best_chain.push(genesis_hash);
        blockchain
            .blocks
            .insert(genesis_hash, genesis_block.clone());
        blockchain
            .headers
            .insert(genesis_hash, genesis_block.header.clone());
        blockchain.utxo_set = blockchain
            .build_utxo_state_for_chain(&[genesis_hash])
            .expect("genesis chain must be valid");

        blockchain
    }

    fn create_genesis_block() -> Block {
        let genesis_tx = Transaction::new(
            vec![],
            vec![TxOutput {
                value: 0,
                script_pubkey: b"genesis".to_vec(),
            }],
        );

        // Create genesis block with deterministic timestamp and fixed bits
        let mut block = Block::with_bits([0; 32], vec![genesis_tx], 0, protocol::DEFAULT_BITS);
        // Override timestamp to be deterministic across all nodes
        block.header.timestamp = protocol::GENESIS_TIMESTAMP;
        block
    }

    pub fn add_block(&mut self, block: Block) -> Result<(), &'static str> {
        let block_hash = block.hash();
        if self.blocks.contains_key(&block_hash) {
            return Err("Block is already known");
        }
        if block.header.height > 0 && !self.blocks.contains_key(&block.header.previous_block_hash) {
            return Err("Previous block not found");
        }

        let previous_block = self.get_block(&block.header.previous_block_hash);
        block.validate(previous_block)?;

        self.blocks.insert(block_hash, block.clone());
        self.headers.insert(block_hash, block.header.clone());

        let candidate_chain = self
            .path_to_genesis(block_hash)
            .ok_or("Unable to build candidate chain path")?;
        let candidate_utxo = self.build_utxo_state_for_chain(&candidate_chain)?;

        if candidate_chain.len() > self.best_chain.len() {
            self.best_chain = candidate_chain;
            self.current_height = block.header.height;
            self.utxo_set = candidate_utxo;
        }

        Ok(())
    }

    fn path_to_genesis(&self, tip_hash: [u8; 32]) -> Option<Vec<[u8; 32]>> {
        let mut path = Vec::new();
        let mut current = tip_hash;

        loop {
            let block = self.blocks.get(&current)?;
            path.push(current);
            if current == self.genesis_hash {
                break;
            }
            current = block.header.previous_block_hash;
        }

        path.reverse();
        Some(path)
    }

    fn build_utxo_state_for_chain(
        &self,
        chain: &[[u8; 32]],
    ) -> Result<HashMap<OutPoint, UTXO>, &'static str> {
        let mut utxo_set = self.bootstrap_utxos.clone();

        for hash in chain {
            let block = self
                .blocks
                .get(hash)
                .ok_or("Block missing from candidate chain")?;
            self.apply_block_to_utxo_set(block, &mut utxo_set)?;
        }

        Ok(utxo_set)
    }

    fn apply_block_to_utxo_set(
        &self,
        block: &Block,
        utxo_set: &mut HashMap<OutPoint, UTXO>,
    ) -> Result<(), &'static str> {
        if block.transactions.is_empty() {
            return Err("Block must contain at least one transaction");
        }

        let mut fees = 0u64;
        for (index, tx) in block.transactions.iter().enumerate() {
            let is_coinbase = tx.inputs.is_empty();

            if index == 0 {
                if !is_coinbase {
                    return Err("First transaction in block must be coinbase");
                }
                if block.header.height != 0 || tx.outputs.iter().any(|output| output.value != 0) {
                    tx.validate()?;
                }
            } else {
                if is_coinbase {
                    return Err("Only the first transaction in a block may be coinbase");
                }
                fees = fees
                    .checked_add(self.validate_transaction_against_utxo_set(tx, utxo_set)?)
                    .ok_or("Transaction fee overflow")?;
            }

            self.apply_transaction(tx, utxo_set, block.header.height);
        }

        self.validate_block_reward(block, fees)?;
        Ok(())
    }

    fn apply_transaction(
        &self,
        tx: &Transaction,
        utxo_set: &mut HashMap<OutPoint, UTXO>,
        height: u64,
    ) {
        if !tx.inputs.is_empty() {
            for input in &tx.inputs {
                utxo_set.remove(&input.previous_output);
            }
        }

        let txid = tx.txid();
        for (vout, output) in tx.outputs.iter().enumerate() {
            let outpoint = OutPoint {
                txid,
                vout: vout as u32,
            };
            utxo_set.insert(
                outpoint,
                UTXO {
                    output: output.clone(),
                    block_height: height,
                    is_coinbase: tx.inputs.is_empty(),
                },
            );
        }
    }

    fn validate_block_reward(&self, block: &Block, fees: u64) -> Result<(), &'static str> {
        let schedule = protocol::EmissionSchedule::default();
        let allowed_reward = schedule
            .block_reward(block.header.height)
            .checked_add(fees)
            .ok_or("Coinbase reward overflow")?;

        let coinbase_amount: u64 = block.transactions[0]
            .outputs
            .iter()
            .map(|output| output.value)
            .sum();

        if coinbase_amount > allowed_reward {
            return Err("Coinbase reward exceeds allowed subsidy and fees");
        }

        Ok(())
    }

    pub fn next_work_bits(&self, tx_count: usize) -> u32 {
        if self.best_chain.len() < 2 {
            return self.apply_tx_pressure(protocol::DEFAULT_BITS, tx_count);
        }

        let window = self
            .best_chain
            .len()
            .min(protocol::DIFFICULTY_RETARGET_WINDOW);
        let recent_hashes = &self.best_chain[self.best_chain.len() - window..];
        let first = self.headers.get(&recent_hashes[0]);
        let last = self.headers.get(recent_hashes.last().unwrap());

        let Some(first_header) = first else {
            return self.apply_tx_pressure(protocol::DEFAULT_BITS, tx_count);
        };
        let Some(last_header) = last else {
            return self.apply_tx_pressure(protocol::DEFAULT_BITS, tx_count);
        };

        let expected_span =
            protocol::DEFAULT_BLOCK_TIME_SECONDS * (window.saturating_sub(1) as u64);
        let actual_span = last_header
            .timestamp
            .saturating_sub(first_header.timestamp)
            .max(1);

        let base_target = BlindHash::target_from_bits(last_header.bits);
        let retarget_ratio = (actual_span as f64 / expected_span.max(1) as f64).clamp(0.5, 2.0);
        let tx_ratio = (1.0 / (1.0 + (tx_count as f64 * 0.03))).clamp(0.55, 1.0);
        let adjusted_target =
            ((base_target as f64) * retarget_ratio * tx_ratio).clamp(1.0, u128::MAX as f64) as u128;

        BlindHash::bits_from_target(adjusted_target)
    }

    fn apply_tx_pressure(&self, bits: u32, tx_count: usize) -> u32 {
        let target = BlindHash::target_from_bits(bits);
        let tx_ratio = (1.0 / (1.0 + (tx_count as f64 * 0.03))).clamp(0.55, 1.0);
        BlindHash::bits_from_target(((target as f64) * tx_ratio).max(1.0) as u128)
    }

    pub fn get_block(&self, hash: &[u8; 32]) -> Option<&Block> {
        self.blocks.get(hash)
    }

    pub fn get_header(&self, hash: &[u8; 32]) -> Option<&BlockHeader> {
        self.headers.get(hash)
    }

    pub fn get_utxo(&self, outpoint: &OutPoint) -> Option<&UTXO> {
        self.utxo_set.get(outpoint)
    }

    pub fn get_best_block_hash(&self) -> [u8; 32] {
        *self.best_chain.last().unwrap_or(&self.genesis_hash)
    }

    pub fn get_balance(&self, script_pubkey: &[u8]) -> u64 {
        self.utxo_set
            .values()
            .filter(|utxo| utxo.output.script_pubkey == script_pubkey)
            .map(|utxo| utxo.output.value)
            .sum()
    }

    pub fn calculate_transaction_fee(&self, tx: &Transaction) -> Result<u64, &'static str> {
        self.validate_transaction_against_utxo_set(tx, &self.utxo_set)
    }

    pub fn contains_transaction(&self, txid: &[u8; 32]) -> bool {
        self.blocks.values().any(|block| {
            block
                .transactions
                .iter()
                .any(|transaction| &transaction.txid() == txid)
        })
    }

    pub fn get_spendable_utxos(&self, script_pubkey: &[u8]) -> Vec<(OutPoint, UTXO)> {
        let mut utxos: Vec<_> = self
            .utxo_set
            .iter()
            .filter(|(_, utxo)| utxo.output.script_pubkey == script_pubkey)
            .map(|(outpoint, utxo)| (outpoint.clone(), utxo.clone()))
            .collect();
        utxos.sort_by_key(|(outpoint, utxo)| (utxo.block_height, outpoint.txid, outpoint.vout));
        utxos
    }

    pub fn wallet_transaction_history(
        &self,
        script_pubkey: &[u8],
    ) -> Vec<WalletTransactionRecord> {
        let mut known_outputs = HashMap::<[u8; 32], Vec<TxOutput>>::new();
        let mut history = Vec::new();

        for block_hash in &self.best_chain {
            let Some(block) = self.blocks.get(block_hash) else {
                continue;
            };

            for transaction in &block.transactions {
                let txid = transaction.txid();
                let incoming_amount: u64 = transaction
                    .outputs
                    .iter()
                    .filter(|output| output.script_pubkey == script_pubkey)
                    .map(|output| output.value)
                    .sum();

                let owned_input_amount: u64 = transaction
                    .inputs
                    .iter()
                    .filter_map(|input| {
                        known_outputs
                            .get(&input.previous_output.txid)
                            .and_then(|outputs| outputs.get(input.previous_output.vout as usize))
                    })
                    .filter(|output| output.script_pubkey == script_pubkey)
                    .map(|output| output.value)
                    .sum();

                if incoming_amount == 0 && owned_input_amount == 0 {
                    known_outputs.insert(txid, transaction.outputs.clone());
                    continue;
                }

                let first_external_output = transaction
                    .outputs
                    .iter()
                    .find(|output| output.script_pubkey != script_pubkey)
                    .map(|output| display_script_pubkey(&output.script_pubkey));
                let first_input_address = transaction
                    .inputs
                    .iter()
                    .find_map(|input| address_from_script_sig(&input.script_sig))
                    .map(|bytes| display_script_pubkey(&bytes));
                let external_send_amount: u64 = transaction
                    .outputs
                    .iter()
                    .filter(|output| output.script_pubkey != script_pubkey)
                    .map(|output| output.value)
                    .sum();
                let fee = owned_input_amount.saturating_sub(transaction.total_output_value());

                let (direction, amount, counterparty) = if transaction.inputs.is_empty() {
                    (
                        WalletTransactionDirection::MiningReward,
                        incoming_amount,
                        None,
                    )
                } else if owned_input_amount > 0 {
                    if external_send_amount == 0 {
                        (
                            WalletTransactionDirection::SelfTransfer,
                            incoming_amount,
                            Some("Self".to_string()),
                        )
                    } else {
                        (
                            WalletTransactionDirection::Outgoing,
                            external_send_amount,
                            first_external_output,
                        )
                    }
                } else {
                    (
                        WalletTransactionDirection::Incoming,
                        incoming_amount,
                        first_input_address,
                    )
                };

                history.push(WalletTransactionRecord {
                    transaction: transaction.clone(),
                    txid,
                    height: block.header.height,
                    timestamp: block.header.timestamp,
                    direction,
                    amount,
                    fee,
                    counterparty,
                });

                known_outputs.insert(txid, transaction.outputs.clone());
            }
        }

        history.reverse();
        history
    }

    pub fn fund_address_for_testing(
        &mut self,
        address: &str,
        amount: u64,
    ) -> Result<(), &'static str> {
        if amount == 0 {
            return Err("Funding amount must be greater than zero");
        }

        let funding_tx = Transaction::new(
            vec![],
            vec![TxOutput {
                value: amount,
                script_pubkey: address.as_bytes().to_vec(),
            }],
        );
        let outpoint = OutPoint {
            txid: funding_tx.txid(),
            vout: 0,
        };

        let demo_utxo = UTXO {
            output: funding_tx.outputs[0].clone(),
            block_height: self.current_height,
            is_coinbase: true,
        };
        self.bootstrap_utxos
            .insert(outpoint.clone(), demo_utxo.clone());
        self.utxo_set.insert(outpoint, demo_utxo);

        Ok(())
    }

    pub fn validate_transaction(&self, tx: &Transaction) -> Result<(), &'static str> {
        if tx.inputs.is_empty() {
            return Err("Non-coinbase transactions must have at least one input");
        }

        self.validate_transaction_against_utxo_set(tx, &self.utxo_set)
            .map(|_| ())
    }

    fn validate_transaction_against_utxo_set(
        &self,
        tx: &Transaction,
        utxo_set: &HashMap<OutPoint, UTXO>,
    ) -> Result<u64, &'static str> {
        tx.validate()?;

        let mut total_input_value = 0u64;
        for input in &tx.inputs {
            let utxo = utxo_set
                .get(&input.previous_output)
                .ok_or("Input UTXO not found or already spent")?;

            let claimed_owner = address_from_script_sig(&input.script_sig)
                .ok_or("Transparent input must provide a valid public key in script_sig")?;
            if claimed_owner != utxo.output.script_pubkey {
                return Err("Input does not authorize spending the referenced output");
            }

            total_input_value = total_input_value
                .checked_add(utxo.output.value)
                .ok_or("Transaction input value overflow")?;
        }

        let total_output_value = tx.total_output_value();
        if total_output_value > total_input_value {
            return Err("Transaction outputs exceed inputs");
        }

        Ok(total_input_value - total_output_value)
    }
}

fn display_script_pubkey(script_pubkey: &[u8]) -> String {
    String::from_utf8(script_pubkey.to_vec())
        .unwrap_or_else(|_| format!("hex:{}", hex::encode(script_pubkey)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mining::create_coinbase_transaction;
    use crate::wallet::Wallet;

    #[test]
    fn rejects_transaction_with_wrong_spending_key() {
        let mut blockchain = Blockchain::new();
        let owner = Wallet::new();
        let attacker = Wallet::new();
        let recipient = Wallet::new();

        blockchain
            .fund_address_for_testing(&owner.address, 50)
            .expect("fund test wallet");

        let utxo = blockchain
            .get_spendable_utxos(&owner.address_bytes())
            .into_iter()
            .next()
            .expect("owner utxo must exist")
            .0;

        let forged_tx = Transaction::new(
            vec![crate::transaction::TxInput {
                previous_output: utxo,
                script_sig: attacker.public.serialize().to_vec(),
                sequence: u32::MAX,
            }],
            vec![TxOutput {
                value: 25,
                script_pubkey: recipient.address_bytes(),
            }],
        );

        assert!(blockchain.validate_transaction(&forged_tx).is_err());
    }

    #[test]
    fn adopts_longer_valid_branch() {
        let mut blockchain = Blockchain::new();
        let miner = Wallet::new();
        let alternate_miner = Wallet::new();

        let genesis = blockchain.get_best_block_hash();

        let block_one = Block::with_bits(
            genesis,
            vec![create_coinbase_transaction(1, 100, &miner.address_bytes())],
            1,
            protocol::DEFAULT_BITS,
        );
        blockchain
            .add_block(block_one.clone())
            .expect("accept block one");

        let fork_a = Block::with_bits(
            genesis,
            vec![create_coinbase_transaction(
                1,
                100,
                &alternate_miner.address_bytes(),
            )],
            1,
            protocol::DEFAULT_BITS,
        );
        blockchain
            .add_block(fork_a.clone())
            .expect("accept side block");

        let fork_b = Block::with_bits(
            fork_a.hash(),
            vec![create_coinbase_transaction(2, 50, &miner.address_bytes())],
            2,
            protocol::DEFAULT_BITS,
        );
        blockchain
            .add_block(fork_b.clone())
            .expect("accept longer fork");

        assert_eq!(blockchain.get_best_block_hash(), fork_b.hash());
        assert_eq!(blockchain.current_height, 2);
    }

    #[test]
    fn rejects_duplicate_block_submission() {
        let mut blockchain = Blockchain::new();
        let miner = Wallet::new();
        let genesis = blockchain.get_best_block_hash();

        let block = Block::with_bits(
            genesis,
            vec![create_coinbase_transaction(1, 100, &miner.address_bytes())],
            1,
            protocol::DEFAULT_BITS,
        );

        blockchain.add_block(block.clone()).expect("accept block");
        assert_eq!(blockchain.add_block(block), Err("Block is already known"));
    }
}
