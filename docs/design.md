# BlindEye Protocol Design

## Overview

BlindEye (BEC) is a privacy-focused Proof-of-Work cryptocurrency designed for:

- optional shielded transactions,
- near-instant settlement,
- grassroots GPU mining,
- high throughput with parallel validation,
- a permanently fixed supply of 420,480,000 BEC.

## Consensus Architecture

- Pure Proof-of-Work, permissionless, no Proof-of-Stake, no delegated validators.
- Single-chain Nakamoto-style consensus with highest cumulative work.
- 1-second target block time for fast retail payment experience.
- Finality heuristics for user-facing wallets to reduce reorg risk.

## Mining Design

- `BlindHash`: a memory-hard algorithm tuned for consumer GPUs.
- Strong ASIC resistance through large scratchpad use and random memory access.
- Encourages distributed mining across many small GPU miners.
- No premine, no founder allocation, fair launch.

## Emission Model

- Fixed maximum supply: exactly 420,480,000 BEC.
- Initial block subsidy: 100 BEC.
- Subsidy halves every 2,102,400 blocks, which is about 24.3 days at a 1-second target block time.
- Final subsidy schedule is calibrated to guarantee the cap.
- After max supply, miners earn only transaction fees.

## Privacy System

- Optional shielded transfers hide sender, receiver, and amount.
- Transparent transfers remain available for lightweight wallets.
- Shielded model uses note commitments, nullifiers, and zk-SNARK proofs.
- Metadata leakage is minimized at the network layer.

## Network Architecture

- Fast peer-to-peer relay using compact blocks and transaction pre-announcement.
- Parallel validation where safe to avoid sequential processing bottlenecks.
- Pruning support for consumer-grade full nodes.
- Lightweight clients and SPV-style verification.

## Security and Governance

- Designed for censorship resistance and resilience in hostile environments.
- No centralized foundation controlling protocol changes.
- Protocol evolution requires broad community coordination.

## MVP Philosophy

- Focus on private digital payments first.
- Keep version 1 minimal and auditable.
- Avoid unnecessary complexity while preserving performance and privacy.
