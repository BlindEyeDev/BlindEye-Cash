# BlindEye (BEC)

BlindEye is a privacy-focused, GPU-mineable Proof-of-Work cryptocurrency protocol designed for high-speed payments, censorship resistance, and grassroots decentralization.

This repository is an experimental reference implementation. It is suitable for local development and protocol prototyping, but it is not production-ready and should not be used with real economic value.

## Project goals

- Optional private transactions using zero-knowledge primitives
- Near-instant settlement with 1-second block targets
- GPU-friendly, memory-hard mining for consumer hardware
- Fixed supply of exactly 420,480,000 BEC
- Transparent, audit-friendly protocol design
- Local GUI wallet with decimal BEC formatting, first-run seed generation, seed import/export, QR receive view, and integrated mining controls
- Continuous local miner with hash-rate display, tx-aware difficulty retargeting, and basic RPC/pool-style wiring

## Repository structure

- `src/` – reference implementation skeleton
- `docs/` – protocol design and specification
- `Makefile` – build and development helper

## Getting started

```bash
make all
```

## Development commands

- `make all` – build the Rust reference binary
- `make run` – run the CLI
- `make test` – run the unit tests
- `make doc` – build documentation
- `make fmt` – format sources
