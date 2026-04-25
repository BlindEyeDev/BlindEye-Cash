CARGO ?= cargo
PSHELL ?= powershell.exe

.PHONY: all build check run gui node p2p mining test doc fmt clean msix help

all:
	$(CARGO) build --release

build:
	$(CARGO) build --release

check:
	$(CARGO) check

run:
	$(CARGO) run --release -- $(ARGS)

gui:
	$(CARGO) run --release -- --gui

node:
	$(CARGO) run --release -- node start

p2p:
	$(CARGO) run --release -- node p2p

mining:
	$(CARGO) run --release -- mining start 1

test:
	$(CARGO) test

doc:
	$(CARGO) doc --no-deps

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean

msix:
	$(PSHELL) -ExecutionPolicy Bypass -File build-msix.ps1

help:
	@printf "%s\n" \
		"make all      - build release binary" \
		"make build    - build release binary" \
		"make check    - run cargo check" \
		"make run      - run binary with custom args via ARGS=\"...\"" \
		"make gui      - launch GUI wallet" \
		"make node     - start CLI node" \
		"make p2p      - start P2P node" \
		"make mining   - start CLI mining with 1 worker" \
		"make test     - run cargo test" \
		"make fmt      - run cargo fmt" \
		"make doc      - build docs" \
		"make clean    - clean build artifacts" \
		"make msix     - build Windows MSIX package"
