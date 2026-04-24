use crate::blockchain::Blockchain;
use crate::protocol;
use crate::transaction::{address_from_public_key, OutPoint, Transaction, TxInput, TxOutput};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use bip39::{Language, Mnemonic};
use rand::rngs::OsRng;
use rand::RngCore;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use zeroize::Zeroize;

pub const DEFAULT_WALLET_STATE_PATH: &str = "blindeye-wallet-state.json";
pub const DEFAULT_WALLET_BACKUP_PATH: &str = "blindeye-wallet-backup.json";
const INITIAL_FEE_ESTIMATE_BYTES: u64 = 220;

#[derive(Debug, Clone)]
pub struct TransactionPreview {
    pub transaction: Transaction,
    pub recipient_address: String,
    pub amount: u64,
    pub fee: u64,
    pub change: u64,
    pub selected_input_value: u64,
}

impl TransactionPreview {
    pub fn input_count(&self) -> usize {
        self.transaction.inputs.len()
    }

    pub fn total_debit(&self) -> u64 {
        self.amount + self.fee
    }
}

fn encrypt_mnemonic(mnemonic: &str, password: &str) -> Result<(String, String, String), String> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let salt_hex = hex::encode(&salt);

    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| format!("Argon2 hashing failed: {e}"))?;

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("AES key creation failed: {e}"))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let nonce_hex = hex::encode(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, mnemonic.as_bytes())
        .map_err(|e| format!("Encryption failed: {e}"))?;
    let encrypted_hex = hex::encode(&ciphertext);

    key.zeroize();

    Ok((encrypted_hex, salt_hex, nonce_hex))
}

fn decrypt_mnemonic(encrypted_hex: &str, salt_hex: &str, nonce_hex: &str, password: &str) -> Result<String, String> {
    let salt = hex::decode(salt_hex)
        .map_err(|e| format!("Invalid salt hex: {e}"))?;
    if salt.len() != 16 {
        return Err("Invalid salt length".to_string());
    }

    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| format!("Argon2 hashing failed: {e}"))?;

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("AES key creation failed: {e}"))?;

    let nonce_bytes = hex::decode(nonce_hex)
        .map_err(|e| format!("Invalid nonce hex: {e}"))?;
    if nonce_bytes.len() != 12 {
        return Err("Invalid nonce length".to_string());
    }
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = hex::decode(encrypted_hex)
        .map_err(|e| format!("Invalid ciphertext hex: {e}"))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed (wrong password?): {e}"))?;

    let mnemonic = String::from_utf8(plaintext)
        .map_err(|e| format!("Invalid UTF-8 in decrypted mnemonic: {e}"))?;

    key.zeroize();

    Ok(mnemonic)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBackup {
    pub version: u32,
    pub address: String,
    pub public_key_hex: String,
    pub encrypted_mnemonic: String,
    pub salt: String,
    pub nonce: String,
}

#[derive(Debug, Clone)]
pub struct Wallet {
    pub mnemonic: String,
    secret: SecretKey,
    pub public: PublicKey,
    pub address: String,
    pub balance: u64,
    pub transactions: Vec<Transaction>,
}

impl Wallet {
    pub fn new() -> Self {
        let mut entropy = [0u8; 16];
        OsRng.fill_bytes(&mut entropy);
        let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
            .expect("generated entropy must produce a mnemonic");
        Self::from_mnemonic(mnemonic.to_string()).expect("generated mnemonic must be valid")
    }

    pub fn from_mnemonic(mnemonic: String) -> Result<Self, String> {
        let normalized = mnemonic.split_whitespace().collect::<Vec<_>>().join(" ");
        let parsed = Mnemonic::parse_in_normalized(Language::English, &normalized)
            .map_err(|err| format!("Invalid seed phrase: {err}"))?;
        let secret = derive_secret_key(&parsed);
        let secp = Secp256k1::new();
        let public = PublicKey::from_secret_key(&secp, &secret);
        let address = Self::address_from_pubkey(&public);

        Ok(Self {
            mnemonic: parsed.to_string(),
            secret,
            public,
            address,
            balance: 0,
            transactions: Vec::new(),
        })
    }

    pub fn load_or_create_state<P: AsRef<Path>>(path: P, password: &str) -> Result<Self, String> {
        let path_ref = path.as_ref();
        if path_ref.exists() {
            Self::load_from_file(path_ref, password)
        } else {
            let wallet = Self::new();
            wallet.save_to_file(path_ref, password)?;
            Ok(wallet)
        }
    }

    pub fn address_from_pubkey(public: &PublicKey) -> String {
        address_from_public_key(public)
    }

    pub fn validate_address(address: &str) -> bool {
        if !address.starts_with("BEC") {
            return false;
        }

        let payload = &address[3..];
        payload.len() == 40 && hex::decode(payload).is_ok()
    }

    pub fn address_bytes(&self) -> Vec<u8> {
        self.address.as_bytes().to_vec()
    }

    pub fn seed_phrase(&self) -> &str {
        &self.mnemonic
    }

    pub fn private_key_hex(&self) -> String {
        hex::encode(self.secret.secret_bytes())
    }

    pub fn sync_balance(&mut self, blockchain: &Blockchain) {
        self.balance = blockchain.get_balance(&self.address_bytes());
    }

    pub fn balance_bec(&self) -> String {
        protocol::format_bec_amount(self.balance)
    }

    pub fn spendable_balance(
        &self,
        blockchain: &Blockchain,
        reserved_outpoints: &HashSet<OutPoint>,
    ) -> u64 {
        blockchain
            .get_spendable_utxos(&self.address_bytes())
            .into_iter()
            .filter(|(outpoint, _)| !reserved_outpoints.contains(outpoint))
            .map(|(_, utxo)| utxo.output.value)
            .sum()
    }

    pub fn build_transaction(
        &self,
        blockchain: &Blockchain,
        to: &str,
        amount: u64,
        fee: u64,
    ) -> Result<Transaction, String> {
        self.build_transaction_preview(blockchain, &HashSet::new(), to, amount, fee)
            .map(|preview| preview.transaction)
    }

    pub fn estimate_fee_for_rate(
        &self,
        blockchain: &Blockchain,
        reserved_outpoints: &HashSet<OutPoint>,
        to: &str,
        amount: u64,
        fee_rate: u64,
    ) -> Result<u64, String> {
        if fee_rate == 0 {
            return Ok(0);
        }

        let mut fee = fee_rate.saturating_mul(INITIAL_FEE_ESTIMATE_BYTES);
        for _ in 0..4 {
            let preview =
                self.build_transaction_preview(blockchain, reserved_outpoints, to, amount, fee)?;
            let required_fee = preview
                .transaction
                .estimated_size()
                .saturating_mul(fee_rate);
            if required_fee == fee {
                return Ok(fee);
            }
            fee = required_fee;
        }

        Ok(fee)
    }

    pub fn build_transaction_preview(
        &self,
        blockchain: &Blockchain,
        reserved_outpoints: &HashSet<OutPoint>,
        to: &str,
        amount: u64,
        fee: u64,
    ) -> Result<TransactionPreview, String> {
        if !Self::validate_address(to) {
            return Err("Recipient address is not a valid BlindEye address".to_string());
        }
        if amount == 0 {
            return Err("Amount must be greater than zero".to_string());
        }

        let target_amount = amount
            .checked_add(fee)
            .ok_or_else(|| "Amount plus fee exceeds supported range".to_string())?;

        let mut selected_inputs = Vec::new();
        let mut selected_total = 0u64;
        for (outpoint, utxo) in blockchain.get_spendable_utxos(&self.address_bytes()) {
            if reserved_outpoints.contains(&outpoint) {
                continue;
            }
            selected_total = selected_total
                .checked_add(utxo.output.value)
                .ok_or_else(|| "Selected input value overflowed".to_string())?;
            selected_inputs.push((outpoint, utxo));
            if selected_total >= target_amount {
                break;
            }
        }

        if selected_total < target_amount {
            return Err(format!(
                "Insufficient funds: need {} BEC but only {} BEC is spendable",
                protocol::format_bec_amount(target_amount),
                protocol::format_bec_amount(selected_total)
            ));
        }

        let inputs = selected_inputs
            .iter()
            .map(|(outpoint, _)| TxInput {
                previous_output: outpoint.clone(),
                script_sig: self.public.serialize().to_vec(),
                sequence: u32::MAX,
            })
            .collect();

        let mut outputs = vec![TxOutput {
            value: amount,
            script_pubkey: to.as_bytes().to_vec(),
        }];

        let change = selected_total - target_amount;
        if change > 0 {
            outputs.push(TxOutput {
                value: change,
                script_pubkey: self.address_bytes(),
            });
        }

        let transaction = Transaction::new(inputs, outputs);
        blockchain
            .validate_transaction(&transaction)
            .map_err(|err| err.to_string())?;

        Ok(TransactionPreview {
            transaction,
            recipient_address: to.to_string(),
            amount,
            fee,
            change,
            selected_input_value: selected_total,
        })
    }

    pub fn to_backup(&self, password: &str) -> Result<WalletBackup, String> {
        let (encrypted_mnemonic, salt, nonce) = encrypt_mnemonic(&self.mnemonic, password)?;
        Ok(WalletBackup {
            version: 3,
            address: self.address.clone(),
            public_key_hex: hex::encode(self.public.serialize()),
            encrypted_mnemonic,
            salt,
            nonce,
        })
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P, password: &str) -> Result<(), String> {
        let backup = self.to_backup(password)?;
        let json = serde_json::to_string_pretty(&backup)
            .map_err(|err| format!("Failed to serialize wallet state: {err}"))?;
        fs::write(path.as_ref(), json)
            .map_err(|err| format!("Failed to write wallet state: {err}"))
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P, password: &str) -> Result<Self, String> {
        let contents = fs::read_to_string(path.as_ref())
            .map_err(|err| format!("Failed to read wallet state: {err}"))?;
        let backup: WalletBackup = serde_json::from_str(&contents)
            .map_err(|err| format!("Failed to parse wallet state: {err}"))?;

        let mnemonic = decrypt_mnemonic(&backup.encrypted_mnemonic, &backup.salt, &backup.nonce, password)?;
        let wallet = Self::from_mnemonic(mnemonic)?;
        if wallet.address != backup.address {
            return Err("Wallet state address does not match the seed phrase".to_string());
        }
        if hex::encode(wallet.public.serialize()) != backup.public_key_hex {
            return Err("Wallet state public key does not match the seed phrase".to_string());
        }

        Ok(wallet)
    }

    pub fn add_transaction(&mut self, transaction: Transaction) {
        self.transactions.push(transaction);
    }
}

fn derive_secret_key(mnemonic: &Mnemonic) -> SecretKey {
    let seed = mnemonic.to_seed("");
    for counter in 0u32..u32::MAX {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed);
        hasher.update(&counter.to_le_bytes());
        let candidate = hasher.finalize();
        if let Ok(secret) = SecretKey::from_slice(candidate.as_bytes()) {
            return secret;
        }
    }
    panic!("unable to derive a valid secret key from mnemonic");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_blindeye_addresses() {
        let wallet = Wallet::new();
        assert!(Wallet::validate_address(&wallet.address));
        assert!(!Wallet::validate_address("abc"));
        assert!(!Wallet::validate_address("BECnothex"));
    }

    #[test]
    fn builds_transaction_with_change_output() {
        let mut blockchain = Blockchain::new();
        let sender = Wallet::new();
        let recipient = Wallet::new();

        blockchain
            .fund_address_for_testing(&sender.address, 50)
            .unwrap();

        let transaction = sender
            .build_transaction(&blockchain, &recipient.address, 30, 5)
            .unwrap();

        assert_eq!(transaction.inputs.len(), 1);
        assert_eq!(transaction.outputs.len(), 2);
        assert_eq!(transaction.outputs[0].value, 30);
        assert_eq!(transaction.outputs[1].value, 15);
        assert_eq!(transaction.outputs[1].script_pubkey, sender.address_bytes());
    }

    #[test]
    fn excludes_reserved_outpoints_from_preview() {
        let mut blockchain = Blockchain::new();
        let sender = Wallet::new();
        let recipient = Wallet::new();

        blockchain
            .fund_address_for_testing(&sender.address, 40)
            .unwrap();

        let reserved = blockchain
            .get_spendable_utxos(&sender.address_bytes())
            .into_iter()
            .map(|(outpoint, _)| outpoint)
            .collect();

        let result =
            sender.build_transaction_preview(&blockchain, &reserved, &recipient.address, 10, 1);

        assert!(result.is_err());
    }

    #[test]
    fn roundtrips_wallet_backup() {
        let wallet = Wallet::new();
        let backup = wallet.to_backup();
        let json = serde_json::to_string(&backup).expect("serialize backup");
        let decoded: WalletBackup = serde_json::from_str(&json).expect("deserialize backup");

        assert_eq!(decoded.address, wallet.address);
        assert_eq!(decoded.mnemonic, wallet.mnemonic);
    }

    #[test]
    fn restores_wallet_from_seed_phrase() {
        let wallet = Wallet::new();
        let restored = Wallet::from_mnemonic(wallet.mnemonic.clone()).expect("restore mnemonic");
        assert_eq!(wallet.address, restored.address);
    }
}
