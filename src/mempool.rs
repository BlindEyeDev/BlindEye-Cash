use crate::transaction::{OutPoint, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

const CONGESTION_STEP_TRANSACTIONS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MempoolEntry {
    transaction: Transaction,
    fee_paid: u64,
    fee_rate: u64,
    received_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mempool {
    transactions: HashMap<[u8; 32], MempoolEntry>,
    max_size: usize,
    min_fee_rate: u64,
}

impl Mempool {
    pub fn new(max_size: usize, min_fee_rate: u64) -> Self {
        Self {
            transactions: HashMap::new(),
            max_size,
            min_fee_rate,
        }
    }

    pub fn add_transaction(
        &mut self,
        transaction: Transaction,
        fee_paid: u64,
    ) -> Result<(), &'static str> {
        if self.transactions.len() >= self.max_size {
            return Err("Mempool is full");
        }
        let txid = transaction.txid();
        if self.transactions.contains_key(&txid) {
            return Err("Transaction is already in the mempool");
        }

        let size = transaction.estimated_size().max(1);
        let fee_rate = fee_paid.div_ceil(size);
        self.transactions.insert(
            txid,
            MempoolEntry {
                transaction,
                fee_paid,
                fee_rate,
                received_at: current_unix_time(),
            },
        );
        Ok(())
    }

    pub fn standard_fee_rate(&self) -> u64 {
        self.min_fee_rate.saturating_add(
            self.transactions
                .len()
                .div_ceil(CONGESTION_STEP_TRANSACTIONS) as u64,
        )
    }

    pub fn instant_fee_rate(&self) -> u64 {
        let standard = self.standard_fee_rate();
        standard.saturating_add((standard / 4).max(1))
    }

    pub fn minimum_required_fee(&self, transaction: &Transaction) -> u64 {
        transaction
            .estimated_size()
            .max(1)
            .saturating_mul(self.standard_fee_rate())
    }

    pub fn has_conflicting_inputs(&self, transaction: &Transaction) -> bool {
        self.transactions.values().any(|existing| {
            existing.transaction.inputs.iter().any(|existing_input| {
                transaction
                    .inputs
                    .iter()
                    .any(|candidate| candidate.previous_output == existing_input.previous_output)
            })
        })
    }

    pub fn reserved_outpoints(&self) -> HashSet<OutPoint> {
        self.transactions
            .values()
            .flat_map(|transaction| {
                transaction
                    .transaction
                    .inputs
                    .iter()
                    .map(|input| input.previous_output.clone())
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn remove_transaction(&mut self, txid: &[u8; 32]) {
        self.transactions.remove(txid);
    }

    pub fn contains_transaction(&self, txid: &[u8; 32]) -> bool {
        self.transactions.contains_key(txid)
    }

    pub fn get_transactions_for_block(&self, max_size: u64) -> (Vec<Transaction>, u64) {
        let mut entries: Vec<_> = self.transactions.values().cloned().collect();
        let standard_fee_rate = self.standard_fee_rate();
        let instant_fee_rate = self.instant_fee_rate();
        let now = current_unix_time();
        entries.sort_by(|left, right| {
            right
                .fee_rate
                .cmp(&left.fee_rate)
                .then_with(|| left.received_at.cmp(&right.received_at))
                .then_with(|| right.fee_paid.cmp(&left.fee_paid))
                .then_with(|| right.transaction.txid().cmp(&left.transaction.txid()))
        });

        let mut selected = Vec::new();
        let mut total_size = 0u64;
        let mut total_fees = 0u64;
        for entry in entries {
            let eligibility_delay = if entry.fee_rate >= instant_fee_rate {
                crate::protocol::INSTANT_CONFIRMATION_TARGET_SECONDS
            } else if entry.fee_rate >= standard_fee_rate {
                crate::protocol::STANDARD_CONFIRMATION_TARGET_SECONDS
            } else {
                continue;
            };
            if now < entry.received_at.saturating_add(eligibility_delay) {
                continue;
            }

            let tx_size = entry.transaction.estimated_size();
            if total_size.saturating_add(tx_size) > max_size {
                continue;
            }
            total_size = total_size.saturating_add(tx_size);
            total_fees = total_fees.saturating_add(entry.fee_paid);
            selected.push(entry.transaction);
            if selected.len() >= 10 {
                break;
            }
        }

        (selected, total_fees)
    }

    pub fn remove_confirmed_transactions(&mut self, confirmed: &[Transaction]) {
        let mut confirmed_txids = HashSet::new();
        let mut confirmed_inputs = HashSet::new();

        for transaction in confirmed {
            confirmed_txids.insert(transaction.txid());
            for input in &transaction.inputs {
                confirmed_inputs.insert(input.previous_output.clone());
            }
        }

        self.transactions.retain(|txid, entry| {
            !confirmed_txids.contains(txid)
                && !entry
                    .transaction
                    .inputs
                    .iter()
                    .any(|input| confirmed_inputs.contains(&input.previous_output))
        });
    }

    pub fn size(&self) -> usize {
        self.transactions.len()
    }
}

fn current_unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
