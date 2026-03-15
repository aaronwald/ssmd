// ssmd-agent/src/cli/commands/lifecycle-consumer.ts
// Daemon that consumes Kalshi market_lifecycle_v2 events from NATS
// and writes them to the market_lifecycle_events table

import { parseArgs } from "https://deno.land/std@0.224.0/cli/parse_args.ts";
import {
  connect,
  type NatsConnection,
  StringCodec,
  AckPolicy,
  DeliverPolicy,
  Events,
} from "npm:nats";
import { getDb, getRawSql, closeDb } from "../../lib/db/mod.ts";
import { marketLifecycleEvents } from "../../lib/db/schema.ts";

const sc = StringCodec();

function log(message: string): void {
  console.log(`${new Date().toISOString()} ${message}`);
}

function logWarn(message: string): void {
  console.warn(`${new Date().toISOString()} WARN ${message}`);
}

function logError(message: string): void {
  console.error(`${new Date().toISOString()} ERROR ${message}`);
}

/** Raw lifecycle message from Kalshi WS (via NATS) */
interface RawLifecycleMessage {
  type: string; // "market_lifecycle_v2"
  sid?: number;
  msg: {
    market_ticker: string;
    event_type: string; // created, activated, deactivated, close_date_updated, closed, determined, settled
    open_ts?: number;
    close_ts?: number;
    determination_ts?: number;
    settled_ts?: number;
    result?: string;
    additional_metadata?: Record<string, unknown>;
  };
}

interface ConsumerConfig {
  natsUrl: string;
  stream: string;
  filter: string;
  consumerName: string;
}

function loadConfig(): ConsumerConfig {
  return {
    natsUrl: Deno.env.get("NATS_URL") ?? "nats://localhost:4222",
    stream: Deno.env.get("NATS_STREAM") ?? "PROD_KALSHI_LIFECYCLE",
    filter: Deno.env.get("NATS_FILTER") ?? "prod.kalshi.json.lifecycle.>",
    consumerName: Deno.env.get("CONSUMER_NAME") ?? "lifecycle-consumer-v1",
  };
}

function epochToDate(epoch: number | undefined): Date | null {
  if (epoch == null) return null;
  return new Date(epoch * 1000);
}

/** Lifecycle event types that indicate a terminal market status */
const TERMINAL_EVENT_TYPES = new Set([
  "determined", "settled", "closed", "finalized", "deactivated",
]);

/** Map lifecycle event_type to market status for DB update */
function eventTypeToStatus(eventType: string): string {
  switch (eventType) {
    case "determined": return "determined";
    case "settled": return "settled";
    case "finalized": return "finalized";
    case "closed": return "closed";
    case "deactivated": return "deactivated";
    default: return eventType;
  }
}

export async function runLifecycleConsumer(args: string[] = Deno.args): Promise<void> {
  const flags = parseArgs(args, {
    boolean: ["help"],
    alias: { h: "help" },
  });

  if (flags.help) {
    console.log(`
SSMD Lifecycle Consumer - Consume Kalshi market lifecycle events from NATS
and write them to market_lifecycle_events table.

Environment variables:
  DATABASE_URL     PostgreSQL connection string (required)
  NATS_URL         NATS server URL (default: nats://localhost:4222)
  NATS_STREAM      JetStream stream name (default: PROD_KALSHI_LIFECYCLE)
  NATS_FILTER      Subject filter (default: prod.kalshi.json.lifecycle.>)
  CONSUMER_NAME    Durable consumer name (default: lifecycle-consumer-v1)
`);
    return;
  }

  log("=== SSMD Lifecycle Consumer ===");

  const config = loadConfig();
  log(`NATS: ${config.natsUrl}`);
  log(`Stream: ${config.stream}`);
  log(`Filter: ${config.filter}`);
  log(`Consumer: ${config.consumerName}`);

  if (!Deno.env.get("DATABASE_URL")) {
    logError("DATABASE_URL environment variable not set");
    Deno.exit(1);
  }

  const db = getDb();
  log("Database connected");

  let nc: NatsConnection;
  try {
    nc = await connect({
      servers: config.natsUrl,
      reconnect: true,
      maxReconnectAttempts: -1,
      reconnectTimeWait: 2000,
      pingInterval: 30000,
      maxPingOut: 3,
    });
    log("NATS connected");
  } catch (e) {
    logError(`Failed to connect to NATS: ${e}`);
    await closeDb();
    Deno.exit(1);
  }

  // Monitor NATS connection status
  (async () => {
    for await (const status of nc.status()) {
      switch (status.type) {
        case Events.Disconnect:
          logWarn("NATS disconnected");
          break;
        case Events.Reconnect:
          log(`NATS reconnected to ${status.data}`);
          break;
        case Events.Error:
          logError(`NATS error: ${status.data}`);
          break;
        case Events.LDM:
          logWarn("NATS entered lame duck mode");
          break;
      }
    }
  })().catch(() => {});

  const jsm = await nc.jetstreamManager();

  try {
    await jsm.consumers.add(config.stream, {
      durable_name: config.consumerName,
      filter_subject: config.filter,
      ack_policy: AckPolicy.Explicit,
      deliver_policy: DeliverPolicy.New,
    });
    log(`Created durable consumer: ${config.consumerName}`);
  } catch (e) {
    if (!String(e).includes("already exists")) {
      logError(`Failed to create consumer: ${e}`);
    }
  }

  const js = nc.jetstream();
  const consumer = await js.consumers.get(config.stream, config.consumerName);
  log("Consumer ready, starting message consumption...");

  let messagesProcessed = 0;
  let eventsWritten = 0;
  let errors = 0;
  let consecutiveErrors = 0;
  const MAX_CONSECUTIVE_ERRORS = 5;
  const startTime = Date.now();

  // Graceful shutdown
  let shuttingDown = false;
  const shutdown = async () => {
    if (shuttingDown) return;
    shuttingDown = true;
    log("Shutting down...");
    const runtime = Math.round((Date.now() - startTime) / 1000);
    log(`Processed: ${messagesProcessed}, written: ${eventsWritten}, errors: ${errors}, runtime: ${runtime}s`);
    await nc.drain();
    await nc.close();
    await closeDb();
    Deno.exit(0);
  };

  Deno.addSignalListener("SIGINT", shutdown);
  Deno.addSignalListener("SIGTERM", shutdown);

  const messages = await consumer.consume();

  for await (const msg of messages) {
    try {
      const raw = JSON.parse(sc.decode(msg.data)) as RawLifecycleMessage;

      if (raw.type !== "market_lifecycle_v2") {
        msg.ack();
        continue;
      }

      const m = raw.msg;
      if (!m?.market_ticker || !m?.event_type) {
        logWarn(`Skipping message with missing fields: ${msg.subject}`);
        msg.ack();
        continue;
      }

      await db.insert(marketLifecycleEvents).values({
        marketTicker: m.market_ticker,
        eventType: m.event_type,
        openTs: epochToDate(m.open_ts),
        closeTs: epochToDate(m.close_ts),
        settledTs: epochToDate(m.settled_ts ?? m.determination_ts),
        metadata: {
          ...(m.additional_metadata ?? {}),
          ...(m.result != null ? { result: m.result } : {}),
        },
      });

      // Update markets table for terminal events → triggers CDC → connector unsubscribe
      if (TERMINAL_EVENT_TYPES.has(m.event_type)) {
        const newStatus = eventTypeToStatus(m.event_type);
        const sql = getRawSql();
        const result = await sql`UPDATE markets SET status = ${newStatus}, updated_at = NOW() WHERE ticker = ${m.market_ticker}`;
        if (result.count > 0) {
          log(`[status→${newStatus}] ${m.market_ticker}`);
        }
      } else if (m.event_type === "close_date_updated" && m.close_ts) {
        const sql = getRawSql();
        await sql`UPDATE markets SET close_time = ${epochToDate(m.close_ts)!}, updated_at = NOW() WHERE ticker = ${m.market_ticker}`;
      }

      eventsWritten++;
      messagesProcessed++;
      consecutiveErrors = 0;

      if (eventsWritten <= 5 || eventsWritten % 100 === 0) {
        log(`[${m.event_type}] ${m.market_ticker} (total: ${eventsWritten})`);
      }

      msg.ack();
    } catch (e) {
      errors++;
      consecutiveErrors++;
      const errStr = String(e);
      logError(`Error processing message: ${e}`);

      // Crash on DB connection failures — let K8s restart
      if (errStr.includes("CONNECT_TIMEOUT") || errStr.includes("Connection terminated") || errStr.includes("connection refused")) {
        logError(`Fatal: database unreachable (${consecutiveErrors} consecutive errors) — crashing for restart`);
        await nc.drain().catch(() => {});
        await closeDb().catch(() => {});
        Deno.exit(1);
      }

      if (consecutiveErrors >= MAX_CONSECUTIVE_ERRORS) {
        logError(`Fatal: ${MAX_CONSECUTIVE_ERRORS} consecutive errors — crashing for restart`);
        await nc.drain().catch(() => {});
        await closeDb().catch(() => {});
        Deno.exit(1);
      }

      msg.nak();
    }
  }
}

if (import.meta.main) {
  await runLifecycleConsumer();
}
