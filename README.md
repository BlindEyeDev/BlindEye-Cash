# BlindEye (BEC) - Privacy-Preserving GPU-Mineable Cryptocurrency

BlindEye is a reference implementation for a privacy-focused, GPU-mineable Proof-of-Work cryptocurrency with fast settlement times and grassroots decentralization.

## Key Features

Optional zero-knowledge transaction privacy. 1-second block targets with near-instant settlement. Consumer-grade hardware optimization for mining. Fixed supply of exactly 420,480,000 BEC. Transparent, audit-friendly protocol design. Full-featured GUI wallet with seed management and QR receive codes. Async peer-to-peer network for block and transaction propagation. Windows MSIX installer support.

## Experimental & In Development

This project is actively under development and experimental. The long-term philosophy prioritizes anti-ASIC mining and grassroots accessibility, with an intended direction toward GPU-friendly mining resistant to centralization from specialized hardware.

**Current Implementation Status:**
- Mining currently uses CPU-based multi-threaded workers. GPU acceleration is planned but not yet implemented.
- Privacy features have foundational structures in place, but zero-knowledge proof verification is incomplete and not yet hardened. Shielded transactions remain a work in progress.
- Consensus rules, mining algorithms, and privacy implementations are subject to change as development and research continue.

**Important Disclaimers:**
- This implementation does not yet guarantee ASIC resistance or deliver complete privacy. The vision is to eventually support complete privacy upon user request, but this remains under development.
- Privacy features and anti-ASIC mechanisms are evolving. Current implementations may not be fully effective or cryptographically hardened for production use.
- The project is a work in progress. Use only for testing and experimentation, not with real economic value.

## Quick Start

### Prerequisites

Rust 1.70 or later (install from https://rustup.rs/). Cargo (included with Rust). Windows SDK with App Package Packaging Tools (optional, for MSIX).

### Build and Run

GUI wallet and miner:
```bash
cargo run --release -- --gui
```

CLI node:
```bash
cargo run --release -- node start
```

P2P node (seed):
```bash
cargo run --release -- node p2p --listen 0.0.0.0:30303
```

P2P node (peer):
```bash
cargo run --release -- node p2p --listen 0.0.0.0:30304 --bootstrap seed.example.com:30303
```

Build all:
```bash
make all
```

See [SETUP.md](SETUP.md) for detailed setup instructions.

## Wallet Usage

Create new wallet:
```bash
cargo run --release -- wallet new
```

Import from seed phrase:
```bash
cargo run --release -- wallet import-seed "word1 word2 word3 ..."
```

View wallet info:
```bash
cargo run --release -- wallet info
```

Send transaction:
```bash
cargo run --release -- wallet send BEC1abc... 50.5 0.001
```

Backup wallet:
```bash
cargo run --release -- wallet backup ~/blindeye-backup.json
```

See [WALLET_DERIVATION.md](WALLET_DERIVATION.md) for technical details on BIP39 mnemonics, key derivation, and address format.

## Mining

Local mining via GUI: Open GUI with `cargo run --release -- --gui`, navigate to Mining tab, set worker threads, and start.

CLI mining:
```bash
cargo run --release -- mining start 4
```

Mining pool with RPC:
```bash
cargo run --release -- mining rpc 0.0.0.0:18443
```

Public RPC discovery and publishing:
- Host [website/rpc-registry.php](website/rpc-registry.php) on your website.
- Put that URL into the GUI Mining tab `Registry URL` field.
- Use `Refresh Public RPCs` to discover open endpoints.
- Use `Publish My RPC` to publish your own remote RPC there.
- See [docs/PUBLIC_RPC_REGISTRY.md](docs/PUBLIC_RPC_REGISTRY.md) for setup details.

## P2P Networking

Start a P2P node:
```bash
cargo run --release -- node p2p
```

Default listen address is 127.0.0.1:30303. For multi-node testing, run multiple instances with different ports and bootstrap to the first node.

Messages propagated include block discovery and relay, transaction pool distribution, peer discovery via GetPeers, and heartbeat via Ping/Pong.

## Windows Installer (MSIX)

Build MSIX package:
```bash
make msix
```

Install MSIX:
```powershell
Add-AppxPackage -Path "BlindEyeWallet-0.1.0-x64.msix"
```

See [P2P_AND_MSIX_QUICK_START.md](P2P_AND_MSIX_QUICK_START.md) for advanced setup.

## Protocol Parameters

Block Target: 1 second. Block Reward: 100 BEC. Halving Interval: 2,102,400 blocks. Max Supply: 420,480,000 BEC. Consensus Threshold: 67% peer supermajority. PoW Algorithm: BlindHash (blake3-based). Address Format: BEC1{40 hex chars}. Signature Algorithm: ECDSA (secp256k1).

## Privacy (In Development)

BlindEye supports optional shielded (private) transactions using zero-knowledge proofs. See src/privacy.rs for current implementation status.

## Project Structure

```
BlindEYE/
├── src/
│   ├── main.rs           # CLI/GUI entry point
│   ├── wallet.rs         # Wallet key/address generation (BIP39)
│   ├── p2p.rs            # Async P2P networking (Tokio)
│   ├── block.rs          # Block structure & validation
│   ├── blockchain.rs     # Chain state & UTXO management
│   ├── transaction.rs    # UTXO transaction model
│   ├── mining.rs         # Block mining & template creation
│   ├── node.rs           # Node consensus logic
│   ├── protocol.rs       # Emission schedule & parameters
│   ├── privacy.rs        # Shielded transaction types
│   ├── rpc.rs            # JSON-RPC server interface
│   └── pow.rs            # Proof-of-work logic (BlindHash)
├── Cargo.toml            # Dependencies
├── Package.appxmanifest  # Windows MSIX manifest
├── build-msix.ps1        # MSIX build script
├── Makefile              # Build helpers
└── docs/                 # Design documentation
```

## Testing

Run all tests:
```bash
cargo test
```

Run with output:
```bash
cargo test -- --nocapture
```

Run specific test module:
```bash
cargo test wallet::tests
```

## Documentation

[SETUP.md](SETUP.md) - Detailed setup and configuration guide. [WALLET_DERIVATION.md](WALLET_DERIVATION.md) - Wallet derivation path, key generation, address format. [CURRENT_PROJECT_OVERVIEW.md](CURRENT_PROJECT_OVERVIEW.md) - Project status and components. [NEXT_STEPS.md](NEXT_STEPS.md) - Roadmap and upcoming work. [P2P_AND_MSIX_QUICK_START.md](P2P_AND_MSIX_QUICK_START.md) - P2P networking and MSIX packaging guide. [P2P_AND_MSIX_IMPLEMENTATION.md](P2P_AND_MSIX_IMPLEMENTATION.md) - Technical details of P2P and MSIX.

## Disclaimer

This is an experimental reference implementation. It is not production-ready and should not be used with real economic value. Known limitations include non-hardened transaction signing, missing block synchronization from peers, CPU-based mining, unverified privacy proofs, and unencrypted wallet storage.

## Development Roadmap

Current focus: P2P networking layer, Windows MSIX packaging, block synchronization protocol, GUI P2P integration. Next phase: Transaction signing and verification, persistent peer list, DNS seed support, privacy proof verification, GPU mining implementation.

## Support

For issues, questions, or contributions, review [CURRENT_PROJECT_OVERVIEW.md](CURRENT_PROJECT_OVERVIEW.md) for component details, check [NEXT_STEPS.md](NEXT_STEPS.md) for known limitations, and see [WALLET_DERIVATION.md](WALLET_DERIVATION.md) for wallet implementation specifics.

## 📄 License

MIT OR Apache-2.0

Last Updated: April 24, 2026 | Status: Alpha (Networking Phase)
