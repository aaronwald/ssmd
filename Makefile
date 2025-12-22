.PHONY: build test vet staticcheck govulncheck lint clean install tools

BINARY := ssmd
BUILD_DIR := .

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
