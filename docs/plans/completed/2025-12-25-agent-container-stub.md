# Agent Container Stub Design

**Date:** 2025-12-25
**Status:** Approved

## Overview

Create a minimal Deno-based container for the ssmd agent pipeline. This stub proves the CI/CD pipeline works and provides a foundation for LangGraph integration.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Runtime | Deno | Native TypeScript, matches agent-pipeline design |
| Directory | `ssmd-agent/` | Top-level, consistent with `ssmd-rust/` |
| Scope | Health check only | Minimal proof of concept, add features incrementally |
| Image name | `ssmd-agent` | Generic, matches `ssmd-connector` pattern |

## Directory Structure

```
ssmd-agent/
├── Dockerfile          # Multi-stage: deno base → slim runtime
├── deno.json           # Deno config (imports, tasks)
├── src/
│   └── main.ts         # Entry point with health check server
└── README.md           # Usage notes
```

## Dockerfile

```dockerfile
# ssmd-agent container
FROM denoland/deno:2.1.4

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
```

## Stub Code

### src/main.ts

```typescript
const PORT = parseInt(Deno.env.get("PORT") ?? "8080");

async function handler(req: Request): Promise<Response> {
  const url = new URL(req.url);

  if (url.pathname === "/health" || url.pathname === "/healthz") {
    return new Response(JSON.stringify({ status: "ok" }), {
      headers: { "content-type": "application/json" },
    });
  }

  return new Response("Not Found", { status: 404 });
}

console.log(`ssmd-agent listening on :${PORT}`);
Deno.serve({ port: PORT }, handler);
```

### deno.json

```json
{
  "tasks": {
    "start": "deno run --allow-net --allow-env src/main.ts",
    "dev": "deno run --watch --allow-net --allow-env src/main.ts",
    "check": "deno check src/main.ts"
  },
  "compilerOptions": {
    "strict": true
  }
}
```

## CI Changes

### ci.yaml - Add agent job

```yaml
agent:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: denoland/setup-deno@v2
      with:
        deno-version: v2.x
    - name: Deno check
      run: deno check ssmd-agent/src/main.ts
    - name: Build (no push)
      uses: docker/build-push-action@v6
      with:
        context: ./ssmd-agent
        file: ./ssmd-agent/Dockerfile
        push: false
        cache-from: type=gha
        cache-to: type=gha,mode=max
```

### build-agent.yaml - New workflow

Mirrors `build-connector.yaml`:
- Triggers on `v*` tags
- Pushes to `ghcr.io/aaronwald/ssmd-agent`
- Same semver tagging pattern

## Result

- Container image: `ghcr.io/aaronwald/ssmd-agent:<version>`
- CI validates Deno compiles and Docker builds
- Health endpoint at `/health` or `/healthz`

## Future Additions (not in scope)

- NATS client connection
- LangGraph.js dependency
- Signal/state loader
- Cap'n Proto schema bindings
