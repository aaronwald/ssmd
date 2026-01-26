# ssmd-notifier container
FROM denoland/deno:2.6.6

# Create non-root user
RUN useradd -r -s /bin/false ssmd

WORKDIR /app

# Copy source
COPY deno.json .
COPY mod.ts .
COPY src ./src

# Cache dependencies
RUN deno cache mod.ts

# Fix ownership for non-root user
RUN chown -R ssmd:ssmd /app /deno-dir

# Switch to non-root user
USER ssmd

# Health check port
EXPOSE 9090

# Default env vars (must be overridden)
ENV NATS_URL=nats://nats.nats.svc.cluster.local:4222
ENV STREAM=SIGNALS
ENV CONSUMER=notifier
ENV DESTINATIONS_CONFIG=/config/destinations.json

CMD ["run", "--allow-net", "--allow-env", "--allow-read", "mod.ts"]
