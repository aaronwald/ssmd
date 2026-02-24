FROM rust:1.85-slim-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release -p ssmd-snap

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ssmd-snap /usr/local/bin/
ENTRYPOINT ["ssmd-snap"]
