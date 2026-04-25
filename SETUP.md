# BlindEye Project Setup Guide

Complete guide to building, running, and developing BlindEye.

## Table of Contents
1. [System Requirements](#system-requirements)
2. [Installation](#installation)
3. [Building](#building)
4. [Running](#running)
5. [Development Setup](#development-setup)
6. [Troubleshooting](#troubleshooting)
7. [File Structure](#file-structure)

## System Requirements

### Minimum Requirements
- **OS**: Windows 10/11, macOS 10.15+, Linux (Ubuntu 18.04+)
- **RAM**: 2 GB
- **Disk**: 1 GB (build artifacts ~500MB)
- **Network**: Optional for P2P mining features

### For Development
- **Rust**: 1.70+ ([Install](https://rustup.rs/))
- **Cargo**: Included with Rust
- **Git**: For cloning repository
- **VS Code**: Recommended with Rust Analyzer extension

### For Windows MSIX Packaging
- **Windows SDK 10**: App Package Packaging Tools
- **PowerShell**: 5.1 or newer
- **Visual Studio Build Tools**: Optional (for signing)

## Installation

### 1. Install Rust & Cargo

**Windows/macOS/Linux:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Verify installation:
```bash
rustup --version
cargo --version
```

### 2. Clone Repository

```bash
cd Downloads
git clone https://github.com/blindeye/blindeye-wallet.git
cd BlindEYE
```

Or download as ZIP and extract.

### 3. (Windows Only) Install Windows SDK for MSIX

1. Download [Windows 10 SDK](https://developer.microsoft.com/en-us/windows/downloads/windows-10-sdk/)
2. Run installer
3. Select **App Package Packaging Tools** component
4. Verify: `C:\Program Files (x86)\Windows Kits\10\bin\*\makeappx.exe` exists

## Building

### Standard Build

```bash
# Release build (optimized)
make all
# or
cargo build --release

# Output: target/release/blindeye.exe (Windows) / blindeye (Unix)
```

### Debug Build (Faster compilation, slower execution)

```bash
cargo build
# Output: target/debug/blindeye
```

### Clean Build

```bash
make clean
# or
cargo clean
```

### Build Issues

**"Error: linker `cc` not found"**
- macOS: Install Command Line Tools: `xcode-select --install`
- Linux: `sudo apt-get install build-essential`
- Windows: Install Visual Studio Build Tools

**"Error: failed to parse manifest at"**
- Update Rust: `rustup update`
- Update Cargo: `cargo update`

## Running

### GUI Wallet

```bash
cargo run --release -- --gui
# or
make run
```

**Features:**
- Create/import wallets
- View balance and transactions
- Send BEC
- Mine locally
- Start RPC server
- Import seeds
- Export backups
- QR code receive addresses

### CLI Commands

**Wallet Operations:**
```bash
# Create new wallet (generates seed phrase)
cargo run --release -- wallet new

# Display wallet info (address, balance, heights)
cargo run --release -- wallet info

# Import from seed phrase
cargo run --release -- wallet import-seed "word1 word2 ..."

# Send transaction
cargo run --release -- wallet send BEC1abc... 50.0 0.01

# Backup wallet to file
cargo run --release -- wallet backup ~/backup.json

# Restore wallet from file
cargo run --release -- wallet restore ~/backup.json
```

**Node Operations:**
```bash
# Start local node
cargo run --release -- node start

# Show node status
cargo run --release -- node status

# List connected peers
cargo run --release -- node peers

# Start P2P node (networking)
cargo run --release -- node p2p --listen 127.0.0.1:30303
```

**Mining Operations:**
```bash
# Start mining with 4 worker threads
cargo run --release -- mining start 4

# Stop mining
cargo run --release -- mining stop

# View mining status
cargo run --release -- mining status

# Start RPC server for pool mining
cargo run --release -- mining rpc 0.0.0.0:18443
```

### P2P Networking

**Single Node:**
```bash
cargo run --release -- node p2p
# Listens on 127.0.0.1:30303 (localhost only)
```

**Network Testing (3 nodes):**

Terminal 1 - Seed Node:
```bash
cargo run --release -- node p2p --listen 127.0.0.1:30303
```

Terminal 2 - Peer 1:
```bash
cargo run --release -- node p2p --listen 127.0.0.1:30304 --bootstrap 127.0.0.1:30303
```

Terminal 3 - Peer 2:
```bash
cargo run --release -- node p2p --listen 127.0.0.1:30305 --bootstrap 127.0.0.1:30303
```

**Public Network:**
```bash
# Seed node (accessible from internet)
cargo run --release -- node p2p --listen 0.0.0.0:30303

# Regular node connecting to seed
cargo run --release -- node p2p --listen 0.0.0.0:30303 --bootstrap seed.example.com:30303
```

### Windows MSIX Installer

**Build MSIX package:**
```bash
make msix
# Output: BlindEyeWallet-0.1.0-x64.msix
```

**Install package:**
```powershell
# Option 1: PowerShell
Add-AppxPackage -Path "BlindEyeWallet-0.1.0-x64.msix"

# Option 2: Windows Explorer (double-click .msix file)

# Option 3: Command line
explorer shell:appsFolder
# Then search and launch BlindEyeWallet
```

**Uninstall:**
```powershell
Get-AppxPackage -Name "BlindEyeCrypto.BlindEyeWallet" | Remove-AppxPackage
```

## Development Setup

### VS Code Setup

**Extensions to Install:**
1. Rust Analyzer (rust-lang.rust-analyzer)
2. CodeLLDB (vadimcn.vscode-lldb) - Debugger
3. Better TOML (bungcip.better-toml) - Cargo.toml highlighting

**Settings (.vscode/settings.json):**
```json
{
  "[rust]": {
    "editor.formatOnSave": true,
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  },
  "rust-analyzer.checkOnSave.command": "clippy"
}
```

### Running Tests

```bash
# All tests
cargo test

# With output
cargo test -- --nocapture

# Specific module
cargo test wallet::tests

# Specific test
cargo test test_wallet_new -- --exact
```

### Code Formatting

```bash
# Format code (must do before commits)
make fmt
# or
cargo fmt

# Check formatting without changing
cargo fmt -- --check
```

### Code Quality

```bash
# Lint code (Clippy)
cargo clippy -- -D warnings

# Check without running
cargo check

# Full audit (may take time)
cargo audit
```

### Build Documentation

```bash
make doc
# or
cargo doc --no-deps --open
```

Opens HTML documentation in browser.

### Debugging

**Using VS Code Debugger:**
1. Create `.vscode/launch.json`:
```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug GUI",
      "cargo": {
        "args": [
          "build",
          "--bin=blindeye",
          "--args",
          "--gui"
        ]
      }
    }
  ]
}
```
2. Press F5 to start debugging

**Command Line Debugging:**
```bash
# RUST_LOG enables logging
RUST_LOG=debug cargo run --release

# Backtrace on panic
RUST_BACKTRACE=1 cargo run --release
```

## File Structure Explained

```
BlindEYE/
├── src/
│   ├── main.rs              # CLI & GUI entry point
│   ├── wallet.rs            # Wallet management (BIP39 seeds, key derivation)
│   ├── block.rs             # Block structure & validation
│   ├── blockchain.rs        # Chain state, UTXO set, consensus
│   ├── transaction.rs       # Transaction types & validation
│   ├── mining.rs            # Mining logic & block templates
│   ├── node.rs              # Node state coordination
│   ├── p2p.rs               # Peer-to-peer networking (Tokio)
│   ├── protocol.rs          # Constants: rewards, difficulty, supply
│   ├── network.rs           # Legacy network config (see p2p.rs)
│   ├── rpc.rs               # JSON-RPC server
│   ├── pow.rs               # Proof-of-work (BlindHash)
│   ├── privacy.rs           # Privacy transaction types
│   └── lib.rs               # Module exports
├── target/
│   ├── release/             # Release binaries (after build)
│   └── debug/               # Debug binaries (after cargo build)
├── Cargo.toml               # Dependencies & metadata
├── Cargo.lock               # Dependency lock file
├── Package.appxmanifest     # Windows MSIX manifest
├── build-msix.ps1           # MSIX build script (PowerShell)
├── Makefile                 # Build shortcuts
├── README.md                # Main README (you are here)
├── SETUP.md                 # This file
├── WALLET_DERIVATION.md     # Wallet technical details
├── CURRENT_PROJECT_OVERVIEW.md  # Project status
├── NEXT_STEPS.md            # Development roadmap
├── P2P_AND_MSIX_*.md        # P2P & packaging docs
└── docs/
    └── design.md            # Protocol design
```

### Configuration Files

**Wallet State:** `blindeye-wallet-state.json`
```json
{
  "version": 1,
  "address": "BEC1abc...",
  "mnemonic": "word1 word2 ...",
  "public_key_hex": "02abc..."
}
```

**Node State:** `blindeye-node-state.bin` (binary)
- Persists blockchain, mempool, peer list

## Troubleshooting

### Build Fails with "error: could not compile"

```bash
# Clean and rebuild
cargo clean
cargo build --release

# Update dependencies
cargo update

# Check for incompatible versions
cargo tree
```

### GUI Doesn't Launch

```bash
# Run with debug output
RUST_LOG=debug cargo run --release -- --gui

# Check if eframe/egui is properly installed
cargo build --release 2>&1 | head -20

# Try rebuilding from scratch
cargo clean && cargo build --release
```

### P2P Connection Issues

```bash
# Verify port is open (Windows)
netstat -an | findstr 30303

# Check firewall allows the port
# Settings > Firewall > Allow app > Add blindeye.exe

# Test connection to bootstrap peer
# cargo run --release -- node p2p --listen 127.0.0.1:30304 --bootstrap <peer_addr>

# Enable logging
RUST_LOG=debug cargo run --release -- node p2p
```

### MSIX Build Issues

```bash
# Verify SDK installed
Get-Item "C:\Program Files (x86)\Windows Kits\10\bin\*\makeappx.exe"

# Run as Admin in PowerShell
powershell -Command "Start-Process powershell -ArgumentList '-NoExit -Command \". build-msix.ps1\"' -Verb RunAs"

# Check version in Cargo.toml matches manifest
grep "version" Cargo.toml
grep "Version" Package.appxmanifest
```

### Memory Issues During Build

```bash
# Build with limited parallelism
cargo build --release -j 2

# Or use release profile with optimizations for size
# Add to Cargo.toml:
# [profile.release]
# opt-level = "z"
# lto = true
# codegen-units = 1
```

## Environment Variables

```bash
# Logging (error, warn, info, debug, trace)
RUST_LOG=debug

# Backtrace on panic
RUST_BACKTRACE=1

# Number of threads for compilation
CARGO_BUILD_JOBS=4

# Custom Cargo registry
CARGO_REGISTRIES_CRATES_IO_PROTOCOL=git

# Optimization level (0, 1, 2, 3)
# (Set in Cargo.toml [profile.release])
```

## Performance Optimization Tips

### Build Times
```bash
# Use mold linker (Linux)
# Add to .cargo/config.toml:
# [build]
# rustflags = ["-C", "link-arg=-fuse-ld=mold"]

# Reduce LTO
# [profile.release]
# lto = "thin"
```

### Runtime Performance
```bash
# Profile with perf (Linux)
cargo flamegraph --release

# Profile on Windows (use Windows Performance Toolkit)
```

## Next Steps

After setup, see:
- **[WALLET_DERIVATION.md](WALLET_DERIVATION.md)** - Understand wallet key generation
- **[CURRENT_PROJECT_OVERVIEW.md](CURRENT_PROJECT_OVERVIEW.md)** - Project components
- **[NEXT_STEPS.md](NEXT_STEPS.md)** - Development roadmap
- **[README.md](README.md)** - Main overview

---

**Last Updated**: April 24, 2026 | **Version**: 0.1.0
