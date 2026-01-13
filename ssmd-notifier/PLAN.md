# ssmd-notifier Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Deno service that subscribes to NATS signal fires and routes notifications to ntfy.

**Architecture:** NATS consumer pulls from SIGNALS stream, router evaluates match rules per destination, ntfy sender does HTTP POST. Health/metrics on :9090.

**Tech Stack:** Deno 2.x, npm:nats, native fetch, jsr:@std/assert for tests

---

## Task 1: Project Setup

**Files:**
- Create: `ssmd-notifier/deno.json`
- Create: `ssmd-notifier/mod.ts`
- Create: `ssmd-notifier/src/types.ts`

**Step 1: Create deno.json**

```json
{
  "name": "ssmd-notifier",
  "version": "0.1.0",
  "tasks": {
    "start": "deno run --allow-net --allow-read --allow-env mod.ts",
    "test": "deno test --allow-net --allow-read --allow-env",
    "check": "deno check mod.ts"
  },
  "imports": {
    "nats": "npm:nats@2.28.2",
    "@std/assert": "jsr:@std/assert@1"
  }
}
```

**Step 2: Create types.ts**

```typescript
// ssmd-notifier/src/types.ts

/** Signal fire event from NATS */
export interface SignalFire {
  signalId: string;
  ts: number;
  ticker: string;
  payload: unknown;
}

/** Match rule for routing */
export interface MatchRule {
  field: string;
  operator: "eq" | "contains";
  value: string;
}

/** ntfy-specific configuration */
export interface NtfyConfig {
  server?: string;
  topic: string;
  priority?: "min" | "low" | "default" | "high" | "urgent";
}

/** Notification destination */
export interface Destination {
  name: string;
  type: "ntfy";
  config: NtfyConfig;
  match?: MatchRule;
}

/** Notifier configuration */
export interface NotifierConfig {
  natsUrl: string;
  subjects: string[];
  destinations: Destination[];
}
```

**Step 3: Create minimal mod.ts**

```typescript
// ssmd-notifier/mod.ts

console.log("ssmd-notifier starting...");

// Placeholder - will be replaced in Task 6
Deno.exit(0);
```

**Step 4: Verify setup**

Run: `deno task check`
Expected: No errors

**Step 5: Commit**

```bash
git add ssmd-notifier/
git commit -m "feat(notifier): initial project setup with types"
```

---

## Task 2: Router - Match Rule Logic

**Files:**
- Create: `ssmd-notifier/src/router.ts`
- Create: `ssmd-notifier/test/router.test.ts`

**Step 1: Write failing tests**

```typescript
// ssmd-notifier/test/router.test.ts
import { assertEquals } from "@std/assert";
import { matches, shouldRoute } from "../src/router.ts";
import type { SignalFire, MatchRule, Destination } from "../src/types.ts";

const fire: SignalFire = {
  signalId: "volume-spike",
  ts: 1704067200,
  ticker: "GOOGL-250117-W185",
  payload: { dollarVolume: 15234 },
};

Deno.test("matches - eq operator matches exact value", () => {
  const rule: MatchRule = { field: "signalId", operator: "eq", value: "volume-spike" };
  assertEquals(matches(fire, rule), true);
});

Deno.test("matches - eq operator rejects different value", () => {
  const rule: MatchRule = { field: "signalId", operator: "eq", value: "other-signal" };
  assertEquals(matches(fire, rule), false);
});

Deno.test("matches - contains operator matches substring", () => {
  const rule: MatchRule = { field: "signalId", operator: "contains", value: "volume" };
  assertEquals(matches(fire, rule), true);
});

Deno.test("matches - contains operator rejects non-substring", () => {
  const rule: MatchRule = { field: "signalId", operator: "contains", value: "momentum" };
  assertEquals(matches(fire, rule), false);
});

Deno.test("matches - ticker field works", () => {
  const rule: MatchRule = { field: "ticker", operator: "contains", value: "GOOGL" };
  assertEquals(matches(fire, rule), true);
});

Deno.test("matches - unknown field returns false", () => {
  const rule: MatchRule = { field: "unknown", operator: "eq", value: "test" };
  assertEquals(matches(fire, rule), false);
});

Deno.test("shouldRoute - no match rule routes all fires", () => {
  const dest: Destination = {
    name: "all",
    type: "ntfy",
    config: { topic: "test" },
  };
  assertEquals(shouldRoute(fire, dest), true);
});

Deno.test("shouldRoute - matching rule routes fire", () => {
  const dest: Destination = {
    name: "volume-only",
    type: "ntfy",
    config: { topic: "test" },
    match: { field: "signalId", operator: "contains", value: "volume" },
  };
  assertEquals(shouldRoute(fire, dest), true);
});

Deno.test("shouldRoute - non-matching rule blocks fire", () => {
  const dest: Destination = {
    name: "momentum-only",
    type: "ntfy",
    config: { topic: "test" },
    match: { field: "signalId", operator: "eq", value: "momentum" },
  };
  assertEquals(shouldRoute(fire, dest), false);
});
```

**Step 2: Run tests to verify they fail**

Run: `deno task test`
Expected: FAIL - module not found

**Step 3: Implement router**

```typescript
// ssmd-notifier/src/router.ts
import type { SignalFire, MatchRule, Destination } from "./types.ts";

/**
 * Check if a signal fire matches a single rule.
 */
export function matches(fire: SignalFire, rule: MatchRule): boolean {
  const fieldValue = fire[rule.field as keyof SignalFire];
  if (fieldValue === undefined) return false;

  const strValue = String(fieldValue);
  switch (rule.operator) {
    case "eq":
      return strValue === rule.value;
    case "contains":
      return strValue.includes(rule.value);
    default:
      return false;
  }
}

/**
 * Check if a fire should be routed to a destination.
 * No match rule = route all fires.
 */
export function shouldRoute(fire: SignalFire, dest: Destination): boolean {
  if (!dest.match) return true;
  return matches(fire, dest.match);
}
```

**Step 4: Run tests to verify they pass**

Run: `deno task test`
Expected: 9 tests pass

**Step 5: Commit**

```bash
git add ssmd-notifier/src/router.ts ssmd-notifier/test/router.test.ts
git commit -m "feat(notifier): add router with match rule logic"
```

---

## Task 3: ntfy Sender

**Files:**
- Create: `ssmd-notifier/src/senders/mod.ts`
- Create: `ssmd-notifier/src/senders/ntfy.ts`
- Create: `ssmd-notifier/test/ntfy.test.ts`

**Step 1: Write sender interface**

```typescript
// ssmd-notifier/src/senders/mod.ts
import type { SignalFire, Destination } from "../types.ts";

/** Sender interface for notification destinations */
export interface Sender {
  send(fire: SignalFire, dest: Destination): Promise<void>;
}

export { NtfySender } from "./ntfy.ts";
```

**Step 2: Write failing tests**

```typescript
// ssmd-notifier/test/ntfy.test.ts
import { assertEquals, assertStringIncludes } from "@std/assert";
import { NtfySender } from "../src/senders/ntfy.ts";
import type { SignalFire, Destination } from "../src/types.ts";

const fire: SignalFire = {
  signalId: "volume-spike",
  ts: 1704067200,
  ticker: "GOOGL-250117-W185",
  payload: { dollarVolume: 15234 },
};

Deno.test("NtfySender - formats title correctly", () => {
  const sender = new NtfySender();
  const title = sender.formatTitle(fire);
  assertStringIncludes(title, "volume-spike");
  assertStringIncludes(title, "GOOGL-250117-W185");
});

Deno.test("NtfySender - formats body as JSON", () => {
  const sender = new NtfySender();
  const body = sender.formatBody(fire);
  assertStringIncludes(body, "dollarVolume");
  assertStringIncludes(body, "15234");
});

Deno.test("NtfySender - builds correct URL", () => {
  const sender = new NtfySender();
  const dest: Destination = {
    name: "test",
    type: "ntfy",
    config: { server: "https://ntfy.example.com", topic: "alerts" },
  };
  const url = sender.buildUrl(dest);
  assertEquals(url, "https://ntfy.example.com/alerts");
});

Deno.test("NtfySender - uses default server", () => {
  const sender = new NtfySender();
  const dest: Destination = {
    name: "test",
    type: "ntfy",
    config: { topic: "alerts" },
  };
  const url = sender.buildUrl(dest);
  assertEquals(url, "https://ntfy.sh/alerts");
});
```

**Step 3: Run tests to verify they fail**

Run: `deno task test test/ntfy.test.ts`
Expected: FAIL - module not found

**Step 4: Implement ntfy sender**

```typescript
// ssmd-notifier/src/senders/ntfy.ts
import type { SignalFire, Destination } from "../types.ts";
import type { Sender } from "./mod.ts";

const DEFAULT_SERVER = "https://ntfy.sh";

export class NtfySender implements Sender {
  formatTitle(fire: SignalFire): string {
    return `🔔 ${fire.signalId}: ${fire.ticker}`;
  }

  formatBody(fire: SignalFire): string {
    return JSON.stringify(fire.payload);
  }

  buildUrl(dest: Destination): string {
    const server = dest.config.server ?? DEFAULT_SERVER;
    return `${server}/${dest.config.topic}`;
  }

  async send(fire: SignalFire, dest: Destination): Promise<void> {
    const url = this.buildUrl(dest);
    const headers: Record<string, string> = {
      "Title": this.formatTitle(fire),
    };

    if (dest.config.priority) {
      headers["Priority"] = dest.config.priority;
    }

    const response = await fetch(url, {
      method: "POST",
      headers,
      body: this.formatBody(fire),
    });

    if (!response.ok) {
      throw new Error(`ntfy request failed: ${response.status} ${response.statusText}`);
    }
  }
}
```

**Step 5: Run tests to verify they pass**

Run: `deno task test`
Expected: 13 tests pass

**Step 6: Commit**

```bash
git add ssmd-notifier/src/senders/ ssmd-notifier/test/ntfy.test.ts
git commit -m "feat(notifier): add ntfy sender with HTTP POST"
```

---

## Task 4: Config Loading

**Files:**
- Create: `ssmd-notifier/src/config.ts`
- Create: `ssmd-notifier/test/config.test.ts`

**Step 1: Write failing tests**

```typescript
// ssmd-notifier/test/config.test.ts
import { assertEquals, assertRejects } from "@std/assert";
import { loadConfig, parseDestinations } from "../src/config.ts";
import type { Destination } from "../src/types.ts";

Deno.test("parseDestinations - parses valid JSON", () => {
  const json = JSON.stringify([
    { name: "test", type: "ntfy", config: { topic: "alerts" } },
  ]);
  const dests = parseDestinations(json);
  assertEquals(dests.length, 1);
  assertEquals(dests[0].name, "test");
});

Deno.test("parseDestinations - throws on invalid JSON", () => {
  assertRejects(
    async () => parseDestinations("not json"),
    Error,
    "parse"
  );
});

Deno.test("parseDestinations - throws on non-array", () => {
  assertRejects(
    async () => parseDestinations("{}"),
    Error,
    "array"
  );
});
```

**Step 2: Run tests to verify they fail**

Run: `deno task test test/config.test.ts`
Expected: FAIL - module not found

**Step 3: Implement config loading**

```typescript
// ssmd-notifier/src/config.ts
import type { NotifierConfig, Destination } from "./types.ts";

/**
 * Parse destinations JSON string.
 */
export function parseDestinations(json: string): Destination[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    throw new Error("Failed to parse destinations JSON");
  }

  if (!Array.isArray(parsed)) {
    throw new Error("Destinations must be an array");
  }

  return parsed as Destination[];
}

/**
 * Load configuration from environment variables.
 */
export function loadConfig(): NotifierConfig {
  const natsUrl = Deno.env.get("NATS_URL");
  if (!natsUrl) {
    throw new Error("NATS_URL environment variable is required");
  }

  const subjectsStr = Deno.env.get("SUBJECTS");
  if (!subjectsStr) {
    throw new Error("SUBJECTS environment variable is required");
  }
  const subjects = subjectsStr.split(",").map((s) => s.trim());

  const configPath = Deno.env.get("DESTINATIONS_CONFIG");
  if (!configPath) {
    throw new Error("DESTINATIONS_CONFIG environment variable is required");
  }

  const destJson = Deno.readTextFileSync(configPath);
  const destinations = parseDestinations(destJson);

  return { natsUrl, subjects, destinations };
}
```

**Step 4: Run tests to verify they pass**

Run: `deno task test`
Expected: 16 tests pass

**Step 5: Commit**

```bash
git add ssmd-notifier/src/config.ts ssmd-notifier/test/config.test.ts
git commit -m "feat(notifier): add config loading from env and file"
```

---

## Task 5: Health Server

**Files:**
- Create: `ssmd-notifier/src/server.ts`

**Step 1: Implement health server**

```typescript
// ssmd-notifier/src/server.ts

export interface Metrics {
  firesReceived: number;
  notificationsSent: number;
  notificationsFailed: number;
}

const metrics: Metrics = {
  firesReceived: 0,
  notificationsSent: 0,
  notificationsFailed: 0,
};

export function getMetrics(): Metrics {
  return { ...metrics };
}

export function incrementFiresReceived(): void {
  metrics.firesReceived++;
}

export function incrementNotificationsSent(): void {
  metrics.notificationsSent++;
}

export function incrementNotificationsFailed(): void {
  metrics.notificationsFailed++;
}

/**
 * Start HTTP server for health checks and metrics.
 */
export function startServer(port: number = 9090): void {
  Deno.serve({ port }, (req) => {
    const url = new URL(req.url);

    switch (url.pathname) {
      case "/health":
        return new Response("ok", { status: 200 });

      case "/ready":
        return new Response("ok", { status: 200 });

      case "/metrics":
        const m = getMetrics();
        const body = [
          `# HELP ssmd_notifier_fires_received Total signal fires received`,
          `# TYPE ssmd_notifier_fires_received counter`,
          `ssmd_notifier_fires_received ${m.firesReceived}`,
          `# HELP ssmd_notifier_notifications_sent Total notifications sent`,
          `# TYPE ssmd_notifier_notifications_sent counter`,
          `ssmd_notifier_notifications_sent ${m.notificationsSent}`,
          `# HELP ssmd_notifier_notifications_failed Total notifications failed`,
          `# TYPE ssmd_notifier_notifications_failed counter`,
          `ssmd_notifier_notifications_failed ${m.notificationsFailed}`,
        ].join("\n");
        return new Response(body, {
          status: 200,
          headers: { "Content-Type": "text/plain" },
        });

      default:
        return new Response("Not Found", { status: 404 });
    }
  });

  console.log(`Health server listening on :${port}`);
}
```

**Step 2: Verify syntax**

Run: `deno check ssmd-notifier/src/server.ts`
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-notifier/src/server.ts
git commit -m "feat(notifier): add health/metrics HTTP server"
```

---

## Task 6: NATS Consumer

**Files:**
- Create: `ssmd-notifier/src/consumer.ts`

**Step 1: Implement NATS consumer**

```typescript
// ssmd-notifier/src/consumer.ts
import { connect, StringCodec, type NatsConnection, type JetStreamClient } from "nats";
import type { SignalFire, NotifierConfig, Destination } from "./types.ts";
import { shouldRoute } from "./router.ts";
import { NtfySender } from "./senders/mod.ts";
import {
  incrementFiresReceived,
  incrementNotificationsSent,
  incrementNotificationsFailed,
} from "./server.ts";

const sc = StringCodec();

function isSignalFire(obj: unknown): obj is SignalFire {
  if (typeof obj !== "object" || obj === null) return false;
  const fire = obj as Record<string, unknown>;
  return (
    typeof fire.signalId === "string" &&
    typeof fire.ts === "number" &&
    typeof fire.ticker === "string"
  );
}

export async function runConsumer(config: NotifierConfig): Promise<void> {
  console.log(`Connecting to NATS: ${config.natsUrl}`);
  const nc: NatsConnection = await connect({ servers: config.natsUrl });
  const js: JetStreamClient = nc.jetstream();

  console.log(`Subscribing to: ${config.subjects.join(", ")}`);

  const sender = new NtfySender();

  // Subscribe to each subject
  for (const subject of config.subjects) {
    const sub = nc.subscribe(subject);
    console.log(`Subscribed to ${subject}`);

    (async () => {
      for await (const msg of sub) {
        try {
          const data = JSON.parse(sc.decode(msg.data));

          if (!isSignalFire(data)) {
            console.warn("Received non-SignalFire message, skipping");
            continue;
          }

          incrementFiresReceived();
          console.log(`Fire: ${data.signalId} ${data.ticker}`);

          // Route to matching destinations
          for (const dest of config.destinations) {
            if (shouldRoute(data, dest)) {
              try {
                await sender.send(data, dest);
                incrementNotificationsSent();
                console.log(`  -> ${dest.name} (${dest.type})`);
              } catch (e) {
                incrementNotificationsFailed();
                console.error(`  -> ${dest.name} FAILED: ${e}`);
              }
            }
          }
        } catch (e) {
          console.error(`Failed to process message: ${e}`);
        }
      }
    })();
  }

  // Wait for shutdown signal
  await nc.closed();
}
```

**Step 2: Verify syntax**

Run: `deno check ssmd-notifier/src/consumer.ts`
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-notifier/src/consumer.ts
git commit -m "feat(notifier): add NATS consumer with routing"
```

---

## Task 7: Main Entry Point

**Files:**
- Modify: `ssmd-notifier/mod.ts`

**Step 1: Update mod.ts**

```typescript
// ssmd-notifier/mod.ts
import { loadConfig } from "./src/config.ts";
import { runConsumer } from "./src/consumer.ts";
import { startServer } from "./src/server.ts";

console.log("=== SSMD Notifier ===");
console.log();

try {
  const config = loadConfig();

  console.log(`NATS: ${config.natsUrl}`);
  console.log(`Subjects: ${config.subjects.join(", ")}`);
  console.log(`Destinations: ${config.destinations.length}`);
  for (const dest of config.destinations) {
    console.log(`  - ${dest.name} (${dest.type})`);
  }
  console.log();

  // Start health server
  startServer(9090);

  // Run consumer (blocks until shutdown)
  await runConsumer(config);
} catch (e) {
  console.error(`Fatal error: ${e}`);
  Deno.exit(1);
}
```

**Step 2: Verify syntax**

Run: `deno task check`
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-notifier/mod.ts
git commit -m "feat(notifier): wire up main entry point"
```

---

## Task 8: Dockerfile

**Files:**
- Create: `ssmd-notifier/Dockerfile`

**Step 1: Create Dockerfile**

```dockerfile
# ssmd-notifier container
FROM denoland/deno:2.1.4

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
ENV SUBJECTS=signals.>
ENV DESTINATIONS_CONFIG=/config/destinations.json

CMD ["run", "--allow-net", "--allow-env", "--allow-read", "mod.ts"]
```

**Step 2: Commit**

```bash
git add ssmd-notifier/Dockerfile
git commit -m "feat(notifier): add Dockerfile"
```

---

## Task 9: GitHub Actions Workflow

**Files:**
- Create: `.github/workflows/build-notifier.yaml`

**Step 1: Create workflow**

```yaml
name: Build ssmd-notifier

on:
  push:
    tags:
      - 'notifier-v*'
  workflow_dispatch:
    inputs:
      tag:
        description: 'Image tag (default: sha-<commit>)'
        required: false
        type: string

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository_owner }}/ssmd-notifier

jobs:
  build:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract metadata
        id: meta
        uses: docker/metadata-action@v5
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            type=match,pattern=notifier-v(.*),group=1
            type=raw,value=${{ inputs.tag }},enable=${{ inputs.tag != '' }}
            type=sha,prefix=sha-,enable=${{ github.event_name == 'workflow_dispatch' && inputs.tag == '' }}

      - name: Build and push
        uses: docker/build-push-action@v6
        with:
          context: ./ssmd-notifier
          file: ./ssmd-notifier/Dockerfile
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          platforms: linux/amd64,linux/arm64
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

**Step 2: Commit**

```bash
git add .github/workflows/build-notifier.yaml
git commit -m "ci: add build-notifier workflow"
```

---

## Task 10: README

**Files:**
- Create: `ssmd-notifier/README.md`

**Step 1: Create README**

```markdown
# ssmd-notifier

Routes NATS signal fires to notification destinations.

## Quick Start

```bash
# Set environment
export NATS_URL=nats://localhost:4222
export SUBJECTS=signals.>
export DESTINATIONS_CONFIG=./destinations.json

# Run
deno task start
```

## Configuration

**Environment Variables:**

| Var | Required | Description |
|-----|----------|-------------|
| `NATS_URL` | Yes | NATS server URL |
| `SUBJECTS` | Yes | Comma-separated subjects |
| `DESTINATIONS_CONFIG` | Yes | Path to destinations.json |

**destinations.json:**

```json
[
  {
    "name": "all-alerts",
    "type": "ntfy",
    "config": {
      "server": "https://ntfy.sh",
      "topic": "my-alerts",
      "priority": "default"
    }
  },
  {
    "name": "volume-only",
    "type": "ntfy",
    "config": { "topic": "volume-alerts", "priority": "high" },
    "match": { "field": "signalId", "operator": "contains", "value": "volume" }
  }
]
```

## Endpoints

- `GET /health` - Liveness probe
- `GET /ready` - Readiness probe
- `GET /metrics` - Prometheus metrics

## Development

```bash
deno task test    # Run tests
deno task check   # Type check
```

## Deployment

Managed by Notifier CRD. See ssmd-operators.
```

**Step 2: Commit**

```bash
git add ssmd-notifier/README.md
git commit -m "docs: add ssmd-notifier README"
```

---

## Task 11: Integration Test (Local)

**Files:**
- Create: `ssmd-notifier/test/fixtures/destinations.json`

**Step 1: Create test fixtures**

```json
[
  {
    "name": "test-ntfy",
    "type": "ntfy",
    "config": {
      "server": "https://ntfy.sh",
      "topic": "ssmd-test-do-not-use"
    }
  }
]
```

**Step 2: Run local test**

```bash
cd ssmd-notifier
export NATS_URL=nats://localhost:4222
export SUBJECTS=signals.>
export DESTINATIONS_CONFIG=./test/fixtures/destinations.json
deno task start
# In another terminal: publish a test message to NATS
```

**Step 3: Commit**

```bash
git add ssmd-notifier/test/fixtures/
git commit -m "test: add integration test fixtures"
```

---

## Task 12: Final PR Preparation

**Step 1: Run all tests**

Run: `cd ssmd-notifier && deno task test`
Expected: All tests pass

**Step 2: Type check**

Run: `deno task check`
Expected: No errors

**Step 3: Squash/rebase if needed**

Review commit history, clean up if desired.

**Step 4: Push feature branch**

```bash
git push -u origin feature/ssmd-notifier
```

**Step 5: Create PR or merge**

Ready for review or direct merge to main.

---

## Summary

| Task | Description | Tests |
|------|-------------|-------|
| 1 | Project setup | - |
| 2 | Router logic | 9 |
| 3 | ntfy sender | 4 |
| 4 | Config loading | 3 |
| 5 | Health server | - |
| 6 | NATS consumer | - |
| 7 | Main entry | - |
| 8 | Dockerfile | - |
| 9 | GitHub Actions | - |
| 10 | README | - |
| 11 | Integration test | - |
| 12 | PR preparation | - |

**Total: 12 tasks, 16 unit tests**
