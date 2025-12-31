FROM rust:1.83-slim-bookworm as builder

WORKDIR /app
COPY . .
RUN cargo build --release -p ssmd-cdc

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ssmd-cdc /usr/local/bin/
ENTRYPOINT ["ssmd-cdc"]
