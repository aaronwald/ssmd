# Multi-stage build for ssmd-connector
# Uses cargo-chef to cache dependency compilation across builds.
# Only Cargo.toml/Cargo.lock changes trigger a full rebuild.

# Stage 1: Chef — install cargo-chef
FROM rust:1.94-bookworm AS chef
RUN cargo install cargo-chef
RUN apt-get update && apt-get install -y capnproto && rm -rf /var/lib/apt/lists/*
WORKDIR /build

# Stage 2: Planner — generate recipe.json (dependency lockfile for chef)
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder — cook deps (cached), then build app
FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package ssmd-connector
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --package ssmd-connector

# Stage 4: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /bin/false ssmd

WORKDIR /app

COPY --from=builder /build/target/release/ssmd-connector /app/ssmd-connector

RUN mkdir -p /data && chown ssmd:ssmd /data

USER ssmd

EXPOSE 8080

ENTRYPOINT ["/app/ssmd-connector"]
CMD ["--help"]
