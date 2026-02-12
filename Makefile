.PHONY: rust-build rust-test rust-clippy rust-clean rust-all
.PHONY: agent-check agent-test agent-run cli-check
.PHONY: all test lint clean generate-k8s setup
.PHONY: worker-check

CARGO := . $$HOME/.cargo/env && cargo
RUST_DIR := ssmd-rust

# Setup development dependencies (Debian/Ubuntu)
setup:
	@echo "Installing system dependencies..."
	sudo apt-get update && sudo apt-get install -y capnproto pkg-config libssl-dev
	@echo "Checking Rust installation..."
	@if command -v ~/.cargo/bin/rustup >/dev/null 2>&1; then \
		echo "Setting Rust default toolchain..."; \
		~/.cargo/bin/rustup default stable; \
	elif command -v rustup >/dev/null 2>&1; then \
		echo "Setting Rust default toolchain..."; \
		rustup default stable; \
	else \
		echo "Installing Rust..."; \
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
		~/.cargo/bin/rustup default stable; \
	fi
	@echo "Checking Deno installation..."
	@if ! command -v deno >/dev/null 2>&1; then \
		echo "Installing Deno..."; \
		curl -fsSL https://deno.land/install.sh | sh; \
	else \
		echo "Deno already installed: $$(deno --version | head -1)"; \
	fi
	@echo "Setup complete!"

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
	cd ssmd-agent && deno test --allow-read --allow-write --allow-net --allow-env test/

agent-run:
	cd ssmd-agent && deno task agent

# Worker targets
worker-check:
	cd ssmd-worker && npx tsc --noEmit

# Combined targets
all: lint test rust-build

test: rust-test agent-test

lint: rust-clippy agent-check worker-check

clean: rust-clean

# Kubernetes manifest generation
generate-k8s: ## Generate Kubernetes manifests from feeds
	@./scripts/generate-k8s.sh
