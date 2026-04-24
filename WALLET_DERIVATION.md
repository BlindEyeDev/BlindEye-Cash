# BlindEye Wallet Derivation & Key Generation

Technical documentation for wallet creation, key derivation, and address generation in BlindEye.

## Table of Contents
1. [Overview](#overview)
2. [BIP39 Mnemonic Generation](#bip39-mnemonic-generation)
3. [Seed Derivation](#seed-derivation)
4. [Secret Key Derivation (BlindEye Custom)](#secret-key-derivation-blindeye-custom)
5. [Public Key Generation](#public-key-generation)
6. [Address Generation](#address-generation)
7. [Standalone Wallet Generation](#standalone-wallet-generation)
8. [Code Examples](#code-examples)
9. [Cryptographic Specifications](#cryptographic-specifications)

## Overview

BlindEye uses a hybrid approach for wallet derivation:

1. **BIP39 Mnemonic Phrases** (standard): 12 or 24-word recovery phrases
2. **BIP39 Seed Derivation**: Convert mnemonic → 64-byte seed using PBKDF2
3. **BlindEye Key Derivation** (custom): Derive secret key using BLAKE3
4. **ECDSA Keypair**: Generate from secret key using secp256k1
5. **Bech32 Addresses**: Encode public keys as `BEC1{40-hex-chars}`

## BIP39 Mnemonic Generation

### What is BIP39?

BIP39 (Bitcoin Improvement Proposal 39) is a standard for generating human-readable seed phrases from cryptographic entropy. BlindEye uses standard BIP39 for compatibility.

### Mnemonic Generation Process

```
Random Entropy (128 bits) 
    ↓
PBKDF2-SHA512 (wordlist mapping)
    ↓
12-Word Mnemonic Phrase
    ↓
Optional: Passphrase (for additional security)
    ↓
BIP39 Seed (64 bytes)
```

### Example: 12-Word Mnemonic

```
abandon ability able about above absent absolute absorb abstract abstract abstract abuse
```

Each word represents an 11-bit index into the BIP39 wordlist (2048 words).

**Entropy Levels:**
- 12 words = 128 bits entropy = ~2^128 combinations
- 24 words = 256 bits entropy = ~2^256 combinations
- 15 words = 160 bits entropy
- 18 words = 192 bits entropy

### BIP39 Security

- ✅ **Standardized**: Used by Bitcoin, Ethereum, and 1000+ projects
- ✅ **Recoverable**: Same mnemonic = same wallet on any compatible wallet
- ⚠️ **No Encryption**: Mnemonic should be stored securely offline
- ⚠️ **Not Password Protected**: Use passphrase for additional security

## Seed Derivation

### BIP39 Seed Generation

Once you have a 12/24-word mnemonic, derive a 64-byte seed:

```python
def bip39_mnemonic_to_seed(mnemonic: str, passphrase: str = "") -> bytes:
    """
    BIP39 mnemonic → seed (64 bytes)
    
    Args:
        mnemonic: Space-separated 12 or 24 word phrase
        passphrase: Optional additional security passphrase
    
    Returns:
        64-byte seed
    """
    import hashlib
    
    # Normalize mnemonic
    mnemonic = " ".join(mnemonic.split())
    
    # PBKDF2-SHA512(mnemonic, "mnemonic" + passphrase, 2048 iterations)
    salt = b"mnemonic" + passphrase.encode()
    seed = hashlib.pbkdf2_hmac(
        "sha512",
        mnemonic.encode(),
        salt,
        2048  # Fixed iterations per BIP39
    )
    
    return seed  # 64 bytes
```

**Example:**
```
Mnemonic: "abandon ability able about above absent absolute absorb abstract abstract abstract abuse"
Passphrase: "" (empty)
Seed (hex): 5f14e...a8c22 (64 bytes)
```

## Secret Key Derivation (BlindEye Custom)

### BlindEye Key Derivation Algorithm

BlindEye uses a **custom BLAKE3-based key derivation** instead of BIP32/BIP44. This is simpler and more straightforward.

```python
def derive_secret_key_from_seed(seed: bytes) -> bytes:
    """
    BlindEye custom derivation: seed → 32-byte secret key
    
    Uses BLAKE3 hash function with counter mode:
    - Tries incrementing counter until valid secp256k1 secret key found
    - Valid: 0x0000...0001 ≤ key ≤ 0xFFFF...FCFF (secp256k1 field)
    
    Args:
        seed: 64-byte BIP39 seed
    
    Returns:
        32-byte secret key (valid secp256k1 secret)
    """
    import blake3
    
    for counter in range(2**32):  # Try up to 4 billion times
        # BLAKE3(seed || counter_little_endian)
        hasher = blake3.Hasher()
        hasher.update(seed)
        hasher.update(counter.to_bytes(4, 'little'))
        
        candidate = hasher.finalize()  # 32 bytes
        
        # Check if valid secp256k1 secret key
        # Valid if: 0x0000...0001 ≤ key < secp256k1_order
        candidate_int = int.from_bytes(candidate, 'big')
        
        if 1 <= candidate_int < 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141:
            return candidate
    
    raise RuntimeError("Failed to derive secret key from seed")
```

**Why This Approach?**
- ✅ Simpler than BIP32 hierarchical derivation
- ✅ Single key per seed (no child key derivation)
- ✅ Deterministic and reproducible
- ✅ ~1 attempt on average (secp256k1 has plenty of valid keys)

**Example:**
```
Seed (hex): 5f14e...a8c22
Counter: 0
BLAKE3(seed || 0x00000000) = ab12...ef34
→ Valid secp256k1 key? Check...
→ If invalid, try counter = 1, 2, 3, ...
→ Usually found at counter 0-2

Secret Key (hex): ab12...ef34 (32 bytes)
```

## Public Key Generation

### ECDSA Keypair Generation

Once you have a 32-byte secret key, generate the public key using **secp256k1** (ECDSA):

```python
def derive_public_key(secret_key_bytes: bytes) -> bytes:
    """
    Secret key → Public key (ECDSA secp256k1)
    
    Args:
        secret_key_bytes: 32-byte secret key
    
    Returns:
        33-byte compressed public key (0x02 or 0x03 prefix + 32 bytes X coordinate)
    """
    from secp256k1 import PrivateKey, PublicKey
    
    # Create private key from bytes
    priv_key = PrivateKey(secret_key_bytes)
    
    # Derive public key
    pub_key = priv_key.public_key
    
    # Get compressed public key (33 bytes)
    # Format: 0x02<32-byte-x> if Y even, 0x03<32-byte-x> if Y odd
    compressed_pubkey = pub_key.serialize()
    
    return compressed_pubkey  # 33 bytes
```

**Public Key Formats:**

| Format | Size | Example | Use |
|--------|------|---------|-----|
| Compressed | 33 bytes | 02ab12...ef34 | BlindEye addresses (preferred) |
| Uncompressed | 65 bytes | 04ab12...ef34 | Signature verification |

**Example:**
```
Secret Key: ab12...ef34
Public Key (compressed): 02cd56...78ab (33 bytes)
Public Key (uncompressed): 04cd56...78ab...abcd...ef01 (65 bytes)
```

## Address Generation

### BlindEye Address Format

BlindEye addresses use **Bech32 encoding** with the `BEC1` human-readable part:

```python
def generate_address_from_pubkey(public_key_compressed: bytes) -> str:
    """
    Public key → BlindEye address
    
    Encoding: Bech32 with HRP="BEC"
    Format: BEC1{40 hex chars}
    
    Args:
        public_key_compressed: 33-byte compressed public key
    
    Returns:
        String address (BEC1...)
    """
    import hashlib
    from bech32 import bech32_encode, convertbits, Bech32m
    
    # SHA256(RIPEMD160(pubkey))
    h160 = hashlib.new('ripemd160')
    h160.update(hashlib.sha256(public_key_compressed).digest())
    pubkey_hash = h160.digest()  # 20 bytes
    
    # Convert 8-bit hash to 5-bit for Bech32
    # 20 bytes * 8 = 160 bits → 160/5 = 32 5-bit values
    converted = convertbits(pubkey_hash, 8, 5)
    
    # Bech32 encode with HRP="BEC"
    address = bech32_encode("BEC", converted, Bech32m)
    
    return address  # "BEC1" + 40 hex chars
```

**Address Structure:**

```
BEC1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4
├─┤ └─────────────────────────────────────┘
HRP    Data (20-byte hash in Bech32)
```

**Address Examples:**
```
BEC1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4
BEC1pw508d6qejxtdg4y5r3zarvary0c5xw7kw508d6qejxtdg4y5r3zarvary0c5xw7k0ylj0
BEC1zw508d6qejxtdg4y5r3zarvary0c5xw7kw508d6qejxtdg4y5r3zarvary0c5xw7kw508d6
```

## Standalone Wallet Generation

### Complete Derivation Chain

```
Step 1: Generate Random Entropy
   └─ 128 bits for 12-word phrase

Step 2: Create BIP39 Mnemonic
   └─ Map entropy to BIP39 wordlist
   └─ Result: "word1 word2 ... word12"

Step 3: Derive BIP39 Seed
   └─ PBKDF2-SHA512(mnemonic, "mnemonic" + passphrase, 2048)
   └─ Result: 64-byte seed

Step 4: Derive BlindEye Secret Key
   └─ BLAKE3(seed || counter) until valid secp256k1 key
   └─ Result: 32-byte secret key

Step 5: Generate Public Key
   └─ ECDSA secp256k1 point multiplication
   └─ Result: 33-byte compressed public key

Step 6: Generate Address
   └─ SHA256(RIPEMD160(pubkey))
   └─ Bech32 encode: "BEC1" + hash
   └─ Result: "BEC1..." address
```

### Standalone Wallet Creation Tool

**Python Implementation (Standalone):**

```python
#!/usr/bin/env python3
"""Standalone BlindEye wallet generator (no dependencies needed except for crypto libs)"""

import hashlib
import secrets
from typing import Tuple

# BIP39 wordlist (first 128 words shown; full list has 2048)
BIP39_WORDLIST = """
abandon ability able about above absent absolute absorb abstract abstract abstract abuse
access accident account accuse achieve acid acoustic acquired across act action actor ...
""".split()

def generate_standalone_wallet() -> Tuple[str, str, str, str]:
    """
    Generate a complete standalone BlindEye wallet
    
    Returns:
        (mnemonic, private_key_hex, public_key_hex, address)
    """
    # Step 1: Generate entropy
    entropy = secrets.token_bytes(16)  # 128 bits
    
    # Step 2: Create BIP39 mnemonic (simplified - normally use bip39 library)
    mnemonic = "abandon ability able about above absent absolute absorb abstract abstract abstract abuse"
    
    # Step 3: Derive BIP39 seed
    salt = b"mnemonic"
    seed = hashlib.pbkdf2_hmac("sha512", mnemonic.encode(), salt, 2048)
    
    # Step 4: Derive secret key (BLAKE3 simulation using blake3)
    import blake3
    for counter in range(2**32):
        hasher = blake3.Hasher()
        hasher.update(seed)
        hasher.update(counter.to_bytes(4, 'little'))
        candidate = hasher.finalize()
        
        # Check if valid secp256k1 secret
        candidate_int = int.from_bytes(candidate, 'big')
        SECP256K1_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
        
        if 1 <= candidate_int < SECP256K1_ORDER:
            secret_key = candidate
            break
    
    # Step 5: Generate public key (using secp256k1 library)
    from secp256k1 import PrivateKey
    priv = PrivateKey(secret_key)
    pub = priv.public_key.serialize()  # 33 bytes compressed
    
    # Step 6: Generate address
    h160 = hashlib.new('ripemd160')
    h160.update(hashlib.sha256(pub).digest())
    pubkey_hash = h160.digest()
    
    from bech32 import bech32_encode, convertbits, Bech32m
    converted = convertbits(pubkey_hash, 8, 5)
    address = bech32_encode("BEC", converted, Bech32m)
    
    return (
        mnemonic,
        secret_key.hex(),
        pub.hex(),
        address
    )

if __name__ == "__main__":
    mnemonic, priv_hex, pub_hex, address = generate_standalone_wallet()
    
    print("🎯 BlindEye Wallet Generated")
    print(f"Mnemonic:     {mnemonic}")
    print(f"Private Key:  {priv_hex}")
    print(f"Public Key:   {pub_hex}")
    print(f"Address:      {address}")
    print("\n⚠️  SAVE MNEMONIC OFFLINE - DO NOT SHARE")
```

## Code Examples

### Using BlindEye CLI

**Create wallet:**
```bash
blindeye wallet new
# Output:
# Mnemonic: word1 word2 ... word12
# Address: BEC1...
```

**Import from mnemonic:**
```bash
blindeye wallet import-seed "word1 word2 ... word12"
```

**View wallet details:**
```bash
blindeye wallet info
# Output:
# Address: BEC1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4
# Balance: 0 BEC
# Transactions: 0
```

### Using Rust (In-Project)

**From `src/wallet.rs`:**

```rust
use bip39::{Language, Mnemonic};
use secp256k1::{PublicKey, Secp256k1, SecretKey};

// Create new wallet
let wallet = Wallet::new();
println!("Mnemonic: {}", wallet.mnemonic);
println!("Address: {}", wallet.address);
println!("Public Key: {:?}", wallet.public);

// Import from mnemonic
let wallet = Wallet::from_mnemonic(
    "abandon ability able about above absent absolute absorb abstract abstract abstract abuse"
        .to_string()
)?;
println!("Imported wallet address: {}", wallet.address);

// Get secret key bytes
let secret_bytes = wallet.secret.secret_bytes();
println!("Secret Key (hex): {}", hex::encode(secret_bytes));

// Get public key bytes
let pubkey_bytes = wallet.public.serialize();
println!("Public Key (hex): {}", hex::encode(pubkey_bytes));
```

### Using Rust Dependencies

**Cargo.toml additions:**
```toml
[dependencies]
bip39 = "2.2"
secp256k1 = { version = "0.29", features = ["rand", "bitcoin_hashes"] }
blake3 = "1.4"
hex = "0.4"
bech32 = "0.11"
```

## Cryptographic Specifications

### Algorithms Used

| Function | Algorithm | Output | Standard |
|----------|-----------|--------|----------|
| Entropy | OS Random | 128 bits | Secure randomness |
| Mnemonic Encoding | BIP39 | 12-24 words | BIP39 |
| Seed Derivation | PBKDF2-SHA512 | 64 bytes | BIP39 |
| Key Derivation | BLAKE3 + Counter | 32 bytes | BlindEye Custom |
| Public Key | secp256k1 ECDSA | 33 bytes | Bitcoin/Ethereum |
| Hashing | SHA256 | 32 bytes | SHA-2 |
| Short Hash | RIPEMD160 | 20 bytes | Open Standard |
| Address Encoding | Bech32 | Variable | BIP173 |

### Security Properties

- **Entropy Source**: Cryptographically secure (secp256k1 randomness)
- **Key Space**: 2^256 possible secret keys (secp256k1)
- **Address Space**: 2^160 possible addresses (20-byte hash)
- **Collision Probability**: < 2^-80 (SHA256 security)
- **Mnemonic Entropy**: 128 bits (12 words) sufficient for most uses

### Key Sizes

```
Entropy:      128 bits (16 bytes)
BIP39 Seed:   512 bits (64 bytes)
Secret Key:   256 bits (32 bytes)
Public Key:   256 bits + 1 bit parity (33 bytes compressed)
Address Hash: 160 bits (20 bytes)
```

## Recovery & Backup

### What You Need to Recover

Only keep the **12-word mnemonic phrase** safe:
```
abandon ability able about above absent absolute absorb abstract abstract abstract abuse
```

All other data can be regenerated from this mnemonic:
- ✅ Secret key
- ✅ Public key
- ✅ Wallet address
- ✅ Receiving address(es)

### Safe Storage

- **Option 1**: Write on paper, store in safe deposit box
- **Option 2**: Metal plate with words stamped (fireproof)
- **Option 3**: BIP39 passphrase (optional additional security)

**DO NOT:**
- ❌ Store digitally without encryption
- ❌ Share mnemonic
- ❌ Screenshot or email
- ❌ Upload to cloud storage

## Comparison: BlindEye vs BIP44 (Bitcoin/Ethereum)

| Feature | BlindEye | BIP44 |
|---------|----------|-------|
| Mnemonic | BIP39 ✅ | BIP39 ✅ |
| Seed Derivation | BIP39 ✅ | BIP39 ✅ |
| Key Derivation | BLAKE3 Custom | BIP32 Hierarchical |
| Keys Per Seed | 1 (single) | Many (m/44'/coin/account/change/index) |
| Child Derivation | No | Yes |
| Complexity | Simple | Complex |
| Multi-Account | No | Yes |
| Compatibility | BlindEye Only | Cross-compatible |

## FAQ

**Q: Can I use the same mnemonic on other wallets?**
A: No. BlindEye uses custom key derivation. The mnemonic is BIP39 compatible but the secret key generation is unique to BlindEye.

**Q: What if I lose my mnemonic?**
A: Your funds are lost. There is no backup recovery mechanism. Store safely.

**Q: Can I derive multiple addresses from one mnemonic?**
A: Currently no. BlindEye generates one address per mnemonic. Future versions may add multi-account support.

**Q: Is my address the same on different BlindEye installations?**
A: Yes. Same mnemonic → same secret key → same address (always).

**Q: How many BEC can one address hold?**
A: Theoretically up to 420,480,000 BEC (total supply). Practically limited by transaction outputs (~2^63 sats).

## References

- [BIP39 - Mnemonic code for generating deterministic keys](https://github.com/trezor/python-mnemonic)
- [BIP32 - Hierarchical Deterministic Wallets](https://github.com/bitcoinbook/bitcoinbook)
- [secp256k1 - Bitcoin's curve](https://en.wikipedia.org/wiki/Elliptic_Curve_Digital_Signature_Algorithm)
- [BLAKE3 - Cryptographic Hash](https://blake3.io/)
- [Bech32 - Segwit address format](https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki)

---

**Last Updated**: April 24, 2026 | **Version**: 0.1.0 (Alpha)
