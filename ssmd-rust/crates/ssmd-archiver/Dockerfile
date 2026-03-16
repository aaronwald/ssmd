# Multi-stage build for ssmd-archiver
# Uses cargo-chef to cache dependency compilation across builds.

# Stage 1: Chef
FROM rust:1.94-bookworm AS chef
RUN cargo install cargo-chef
RUN apt-get update && apt-get install -y capnproto pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /build

# Stage 2: Planner
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder — cook deps (cached), then build app
FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package ssmd-archiver
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --package ssmd-archiver

# Stage 4: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    python3 \
    && rm -rf /var/lib/apt/lists/*

# Install Google Cloud SDK
RUN curl -sSL https://sdk.cloud.google.com | bash -s -- --disable-prompts
ENV PATH="/root/google-cloud-sdk/bin:$PATH"

COPY --from=builder /build/target/release/ssmd-archiver /usr/local/bin/

ENTRYPOINT ["ssmd-archiver"]
