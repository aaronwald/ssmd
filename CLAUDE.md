# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ssmd - Simple/Streaming Market Data system

## Build Commands

```bash
# Build everything (Go + Rust)
make all-build

# Go only
make build

# Rust only
make rust-build
```

## Test Commands

```bash
# Test everything (Go + Rust)
make all-test

# Go only
make test

# Rust only
make rust-test
```

## Lint Commands

```bash
# Lint everything (Go + Rust)
make all-lint

# Go only
make lint

# Rust only
make rust-clippy
```

## Full Validation

```bash
# Run lint + test + build for both Go and Rust
make all
```

## Prerequisites

```bash
# Install Cap'n Proto compiler (required for ssmd-schema crate)
sudo apt-get install -y capnproto

# Install Rust (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

## Architecture

<!-- Document key architectural decisions as the project develops -->

## Instructions

1. All code must go through pr code review.
1. Use idiomatic go. See .github/instructions/go.instructions.md
