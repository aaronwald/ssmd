# Multi-stage build for ssmd-cache
# Uses cargo-chef to cache dependency compilation across builds.

FROM rust:1.94-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package ssmd-cache
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --package ssmd-cache

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/ssmd-cache /usr/local/bin/
ENTRYPOINT ["ssmd-cache"]
