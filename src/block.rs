use crate::pow::BlindHash;
use crate::protocol;
use crate::transaction::Transaction;
use blake3;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub previous_block_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub timestamp: u64,
    pub bits: u32,
    pub nonce: u64,
    pub height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn new(previous_block_hash: [u8; 32], transactions: Vec<Transaction>, height: u64) -> Self {
        Self::with_bits(
            previous_block_hash,
            transactions,
            height,
            protocol::DEFAULT_BITS,
        )
    }

    pub fn with_bits(
        previous_block_hash: [u8; 32],
        transactions: Vec<Transaction>,
        height: u64,
        bits: u32,
    ) -> Self {
        let merkle_root = Self::calculate_merkle_root(&transactions);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let header = BlockHeader {
            previous_block_hash,
            merkle_root,
            timestamp,
            bits,
            nonce: 0,
            height,
        };

        Self {
            header,
            transactions,
        }
    }

    pub fn hash(&self) -> [u8; 32] {
        BlindHash::hash(&self.header.serialize())
    }

    fn calculate_merkle_root(transactions: &[Transaction]) -> [u8; 32] {
        if transactions.is_empty() {
            return [0; 32];
        }

        let mut hashes: Vec<[u8; 32]> = transactions.iter().map(|tx| tx.txid()).collect();

        while hashes.len() > 1 {
            let mut next = Vec::new();
            for chunk in hashes.chunks(2) {
                let mut hasher = blake3::Hasher::new();
                hasher.update(&chunk[0]);
                if chunk.len() == 2 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                next.push(hasher.finalize().into());
            }
            hashes = next;
        }

        hashes[0]
    }

    pub fn validate(&self, previous_block: Option<&Block>) -> Result<(), &'static str> {
        if self.transactions.is_empty() {
            return Err("Block must contain at least one transaction");
        }

        if let Some(prev) = previous_block {
            if self.header.height != prev.header.height + 1 {
                return Err("Block height does not follow previous block");
            }
            if self.header.previous_block_hash != prev.hash() {
                return Err("Previous block hash mismatch");
            }
            if self.header.timestamp < prev.header.timestamp {
                return Err("Block timestamp moved backwards");
            }
        } else if self.header.height != 0 {
            return Err("Genesis block must have height 0");
        }

        let computed_merkle = Self::calculate_merkle_root(&self.transactions);
        if self.header.merkle_root != computed_merkle {
            return Err("Invalid merkle root");
        }

        let target = BlindHash::target_from_bits(self.header.bits);
        let hash_value = u128::from_le_bytes(self.hash()[0..16].try_into().unwrap());
        if hash_value > target {
            return Err("Proof-of-work target not met");
        }

        Ok(())
    }
}

impl BlockHeader {
    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(&self.previous_block_hash);
        bytes.extend(&self.merkle_root);
        bytes.extend(&self.timestamp.to_le_bytes());
        bytes.extend(&self.bits.to_le_bytes());
        bytes.extend(&self.nonce.to_le_bytes());
        bytes.extend(&self.height.to_le_bytes());
        bytes
    }
}
