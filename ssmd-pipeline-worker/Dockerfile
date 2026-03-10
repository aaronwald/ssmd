# ssmd-pipeline-worker: Deno-compiled pipeline execution engine

# Build stage
FROM denoland/deno:2.6.5 AS builder

WORKDIR /app

# Copy dependency manifest and cache first
COPY deno.json deno.lock ./
RUN deno cache --lock=deno.lock deno.json

# Copy source
COPY src/ ./src/

# Compile to standalone binary
RUN deno compile --target x86_64-unknown-linux-gnu \
    --allow-net --allow-env --allow-read \
    --output /app/ssmd-pipeline-worker src/main.ts

# Runtime stage - minimal image with CA certs for HTTPS/TLS
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m deno
USER deno

COPY --from=builder /app/ssmd-pipeline-worker /usr/local/bin/ssmd-pipeline-worker

ENTRYPOINT ["/usr/local/bin/ssmd-pipeline-worker"]
