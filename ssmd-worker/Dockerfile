# Stage 1: Build Deno CLI binary
FROM denoland/deno:2.6.5 AS cli-builder
WORKDIR /cli
COPY ssmd-agent/deno.json ssmd-agent/
COPY ssmd-agent/src/ ssmd-agent/src/
RUN deno compile --target x86_64-unknown-linux-gnu \
    --allow-net --allow-env --allow-read --allow-write --allow-run \
    --output /cli/ssmd ssmd-agent/src/cli/main.ts

# Stage 2: Build Node.js worker
FROM node:22.14.0-bookworm-slim AS worker-builder
WORKDIR /app
COPY ssmd-worker/package*.json ./
RUN npm ci
COPY ssmd-worker/tsconfig.json ./
COPY ssmd-worker/src/ ./src/
RUN npm run build

# Stage 3: Production - glibc required for Temporal SDK native bindings
FROM node:22.14.0-bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && rm -rf /var/lib/apt/lists/*
RUN curl -LO "https://dl.k8s.io/release/v1.31.0/bin/linux/amd64/kubectl" \
    && chmod +x kubectl && mv kubectl /usr/local/bin/
COPY --from=cli-builder /cli/ssmd /usr/local/bin/ssmd
COPY --from=worker-builder /app/dist ./dist
COPY --from=worker-builder /app/node_modules ./node_modules
COPY ssmd-worker/package*.json ./
COPY ssmd-worker/entrypoint.sh ./entrypoint.sh
RUN chmod +x ./entrypoint.sh
ENV NODE_ENV=production
ENV SSMD_COMMAND=/usr/local/bin/ssmd
CMD ["./entrypoint.sh"]
