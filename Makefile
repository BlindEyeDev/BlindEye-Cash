CARGO ?= cargo

.PHONY: all run test doc fmt clean msix p2p

all:
	$(CARGO) build --release

run:
	$(CARGO) run --release --

test:
	$(CARGO) test

doc:
	$(CARGO) doc --no-deps

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean

p2p:
	$(CARGO) run --release -- node p2p

msix:
	powershell -ExecutionPolicy Bypass -File build-msix.ps1

