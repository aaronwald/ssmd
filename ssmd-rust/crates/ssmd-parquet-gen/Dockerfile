# Multi-stage build for ssmd-parquet-gen
# Uses cargo-chef to cache dependency compilation across builds.

FROM rust:1.94-bookworm AS chef
RUN cargo install cargo-chef
RUN apt-get update && apt-get install -y capnproto pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package ssmd-parquet-gen
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --package ssmd-parquet-gen

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/ssmd-parquet-gen /usr/local/bin/
ENTRYPOINT ["ssmd-parquet-gen"]
