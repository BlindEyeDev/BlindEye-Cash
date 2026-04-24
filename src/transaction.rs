use blake3;
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub version: u32,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub lock_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInput {
    pub previous_output: OutPoint,
    pub script_sig: Vec<u8>,
    pub sequence: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxOutput {
    pub value: u64,
    pub script_pubkey: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct OutPoint {
    pub txid: [u8; 32],
    pub vout: u32,
}

impl Transaction {
    pub fn new(inputs: Vec<TxInput>, outputs: Vec<TxOutput>) -> Self {
        Self {
            version: 1,
            inputs,
            outputs,
            lock_time: 0,
        }
    }

    pub fn txid(&self) -> [u8; 32] {
        blake3::hash(&self.serialize()).into()
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(&self.version.to_le_bytes());
        bytes.extend(&(self.inputs.len() as u32).to_le_bytes());
        for input in &self.inputs {
            bytes.extend(&input.serialize());
        }
        bytes.extend(&(self.outputs.len() as u32).to_le_bytes());
        for output in &self.outputs {
            bytes.extend(&output.serialize());
        }
        bytes.extend(&self.lock_time.to_le_bytes());
        bytes
    }

    pub fn estimated_size(&self) -> u64 {
        self.serialize().len() as u64
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.outputs.is_empty() {
            return Err("Transaction must have at least one output");
        }

        let mut seen_inputs = HashSet::new();
        for input in &self.inputs {
            if !seen_inputs.insert(input.previous_output.clone()) {
                return Err("Duplicate transaction inputs");
            }
        }

        for output in &self.outputs {
            if output.value == 0 {
                return Err("Transaction output value cannot be zero");
            }
        }

        Ok(())
    }

    pub fn total_output_value(&self) -> u64 {
        self.outputs.iter().map(|output| output.value).sum()
    }
}

pub fn address_from_public_key(public_key: &PublicKey) -> String {
    let hash = blake3::hash(&public_key.serialize());
    format!("BEC{}", hex::encode(&hash.as_bytes()[..20]))
}

pub fn address_from_script_sig(script_sig: &[u8]) -> Option<Vec<u8>> {
    let public_key = PublicKey::from_slice(script_sig).ok()?;
    Some(address_from_public_key(&public_key).into_bytes())
}

impl TxInput {
    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(&self.previous_output.txid);
        bytes.extend(&self.previous_output.vout.to_le_bytes());
        bytes.extend(&(self.script_sig.len() as u32).to_le_bytes());
        bytes.extend(&self.script_sig);
        bytes.extend(&self.sequence.to_le_bytes());
        bytes
    }
}

impl TxOutput {
    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(&self.value.to_le_bytes());
        bytes.extend(&(self.script_pubkey.len() as u32).to_le_bytes());
        bytes.extend(&self.script_pubkey);
        bytes
    }
}
