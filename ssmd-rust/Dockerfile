# Multi-stage build for ssmd-connector
# Stage 1: Build
FROM rust:1.92-bookworm AS builder

# Install Cap'n Proto compiler (required for ssmd-schema)
RUN apt-get update && apt-get install -y capnproto && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release binary
RUN cargo build --release --package ssmd-connector

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false ssmd

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/ssmd-connector /app/ssmd-connector

# Create data directory
RUN mkdir -p /data && chown ssmd:ssmd /data

USER ssmd

# Health check endpoint
EXPOSE 8080

# Default command
ENTRYPOINT ["/app/ssmd-connector"]
CMD ["--help"]
