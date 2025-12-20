.PHONY: build test vet staticcheck lint clean install tools

BINARY := ssmd
BUILD_DIR := .

build:
	go build -o $(BUILD_DIR)/$(BINARY) ./cmd/ssmd

test:
	go test ./...

vet:
	go vet ./...

staticcheck:
	@which staticcheck > /dev/null || $(MAKE) tools
	staticcheck ./...

lint: vet staticcheck

clean:
	rm -f $(BUILD_DIR)/$(BINARY)

install:
	go install ./cmd/ssmd

tools:
	go install honnef.co/go/tools/cmd/staticcheck@latest

all: lint test build
