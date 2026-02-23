# ssmd-agent container
FROM denoland/deno:2.6.10

# Create non-root user
RUN useradd -r -s /bin/false ssmd

WORKDIR /app

# Copy source
COPY deno.json .
COPY src ./src

# Cache dependencies (creates /deno-dir cache)
RUN deno cache src/main.ts

# Fix ownership for non-root user
RUN chown -R ssmd:ssmd /app /deno-dir

# Switch to non-root user
USER ssmd

# Health check endpoint
EXPOSE 8080

CMD ["run", "--allow-net", "--allow-env", "src/main.ts"]
