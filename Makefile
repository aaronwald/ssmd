.PHONY: build test vet staticcheck govulncheck lint clean install tools
.PHONY: rust-build rust-test rust-clippy rust-clean rust-all
.PHONY: all-build all-test all-lint

BINARY := ssmd
BUILD_DIR := .
CARGO := . $$HOME/.cargo/env && cargo
RUST_DIR := ssmd-rust

build:
	go build -o $(BUILD_DIR)/$(BINARY) ./cmd/ssmd

test:
	go test ./...

vet:
	go vet ./...

staticcheck:
	@which staticcheck > /dev/null 2>&1 || $(MAKE) tools
	$(shell go env GOPATH)/bin/staticcheck ./...

govulncheck:
	@which govulncheck > /dev/null 2>&1 || go install golang.org/x/vuln/cmd/govulncheck@latest
	$(shell go env GOPATH)/bin/govulncheck ./...

lint: vet staticcheck

security: govulncheck

clean:
	rm -f $(BUILD_DIR)/$(BINARY)

install:
	go install ./cmd/ssmd

tools:
	go install honnef.co/go/tools/cmd/staticcheck@latest
	go install golang.org/x/vuln/cmd/govulncheck@latest

all: lint security test build

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

# ssmd-data targets
.PHONY: data-build data-test data-run

data-build:
	go build -o bin/ssmd-data ./cmd/ssmd-data

data-test:
	go test ./internal/api/... -v

data-run: data-build
	SSMD_DATA_PATH=./testdata SSMD_API_KEY=dev ./bin/ssmd-data

# ssmd-agent targets
.PHONY: agent-check agent-test agent-run

agent-check:
	cd ssmd-agent && deno check src/main.ts src/cli.ts

agent-test:
	cd ssmd-agent && deno test --allow-read --allow-net --allow-env test/

agent-run:
	cd ssmd-agent && deno task agent

# Combined targets
all-build: build rust-build data-build

all-test: test rust-test

all-lint: lint rust-clippy
