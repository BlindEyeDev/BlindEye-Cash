use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub fn shielded_model_summary() -> &'static str {
    "BlindEye supports optional shielded transfers that hide sender, receiver, and amount using a join-split style zk-SNARK model. Transparent transfers remain available for lightweight wallets and audit use cases."
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Transparent,
    Shielded,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldedNote {
    pub commitment: [u8; 32],
    pub nullifier: [u8; 32],
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparentOutput {
    pub address_hash: [u8; 32],
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldedTransfer {
    pub inputs: Vec<ShieldedNote>,
    pub outputs: Vec<ShieldedNote>,
    pub proof: Vec<u8>,
}

pub fn validate_shielded_transfer(transfer: &ShieldedTransfer) -> Result<(), &'static str> {
    if transfer.proof.is_empty() {
        return Err("Shielded transfers must include a proof");
    }
    if transfer.inputs.is_empty() || transfer.outputs.is_empty() {
        return Err("Shielded transfers must have both inputs and outputs");
    }

    let mut seen_nullifiers = HashSet::new();
    let input_value: u64 = transfer.inputs.iter().map(|note| note.value).sum();
    let output_value: u64 = transfer.outputs.iter().map(|note| note.value).sum();

    for note in &transfer.inputs {
        if !seen_nullifiers.insert(note.nullifier) {
            return Err("Duplicate shielded nullifier detected");
        }
    }

    if output_value > input_value {
        return Err("Shielded outputs cannot exceed shielded inputs");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_types_roundtrip() {
        let note = ShieldedNote {
            commitment: [1u8; 32],
            nullifier: [2u8; 32],
            value: 100,
        };
        let json = serde_json::to_string(&note).expect("serialize shielded note");
        let decoded: ShieldedNote = serde_json::from_str(&json).expect("deserialize shielded note");
        assert_eq!(decoded.value, 100);
        assert_eq!(decoded.commitment, [1u8; 32]);
    }

    #[test]
    fn validates_basic_shielded_transfer_rules() {
        let transfer = ShieldedTransfer {
            inputs: vec![ShieldedNote {
                commitment: [1u8; 32],
                nullifier: [2u8; 32],
                value: 50,
            }],
            outputs: vec![ShieldedNote {
                commitment: [3u8; 32],
                nullifier: [4u8; 32],
                value: 40,
            }],
            proof: vec![1, 2, 3],
        };

        assert!(validate_shielded_transfer(&transfer).is_ok());
    }
}
