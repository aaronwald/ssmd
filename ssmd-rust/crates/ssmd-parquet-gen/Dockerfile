FROM rust:1.85-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    capnproto \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace
COPY . .

# Build release binary
RUN cargo build --release --package ssmd-parquet-gen

# Runtime stage — no gcloud SDK needed (object_store handles GCS natively)
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/ssmd-parquet-gen /usr/local/bin/

ENTRYPOINT ["ssmd-parquet-gen"]
