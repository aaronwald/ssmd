.PHONY: rust-build rust-test rust-clippy rust-clean rust-all
.PHONY: agent-check agent-test agent-run cli-check
.PHONY: all test lint clean

CARGO := . $$HOME/.cargo/env && cargo
RUST_DIR := ssmd-rust

# Rust targets
rust-build:
	cd $(RUST_DIR) && $(CARGO) build --all

rust-test:
	cd $(RUST_DIR) && $(CARGO) test --all

rust-clippy:
	cd $(RUST_DIR) && $(CARGO) clippy --all

rust-clean:
	cd $(RUST_DIR) && $(CARGO) clean

rust-all: rust-clippy rust-test rust-build

# TypeScript CLI/Agent targets
cli-check:
	cd ssmd-agent && deno check src/cli.ts

agent-check:
	cd ssmd-agent && deno check src/main.ts src/cli.ts

agent-test:
	cd ssmd-agent && deno test --allow-read --allow-net --allow-env test/

agent-run:
	cd ssmd-agent && deno task agent

# Combined targets
all: lint test rust-build

test: rust-test agent-test

lint: rust-clippy agent-check

clean: rust-clean
