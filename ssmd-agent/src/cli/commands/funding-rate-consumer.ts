// ssmd-agent/src/cli/commands/funding-rate-consumer.ts
// Daemon that consumes Kraken Futures ticker messages from NATS
// and writes funding rate snapshots to pair_snapshots table

import { parseArgs } from "https://deno.land/std@0.224.0/cli/parse_args.ts";
import {
  connect,
  type NatsConnection,
  StringCodec,
  AckPolicy,
  DeliverPolicy,
} from "npm:nats";
import {
  getDb,
  closeDb,
  insertPerpSnapshots,
  cleanupOldSnapshots,
  type Database,
} from "../../lib/db/mod.ts";
import type { NewPair } from "../../lib/db/mod.ts";
import { MetricsRegistry } from "../../server/metrics.ts";

const sc = StringCodec();

/**
 * Timestamped console output
 */
function log(message: string): void {
  const ts = new Date().toISOString();
  console.log(`${ts} ${message}`);
}

function logWarn(message: string): void {
  const ts = new Date().toISOString();
  console.warn(`${ts} WARN ${message}`);
}

function logError(message: string): void {
  const ts = new Date().toISOString();
  console.error(`${ts} ERROR ${message}`);
}

/**
 * Raw ticker message from Kraken Futures WebSocket (via NATS)
 *
 * The connector publishes raw WS JSON. Kraken Futures uses mixed
 * camelCase/snake_case: markPrice, funding_rate, openInterest, etc.
 */
interface KrakenFuturesTickerMessage {
  feed: string;
  product_id: string;
  bid?: number;
  ask?: number;
  last?: number;
  markPrice?: number;
  mark_price?: number;
  index?: number;
  indexPrice?: number;
  index_price?: number;
  funding_rate?: number;
  fundingRate?: number;
  funding_rate_prediction?: number;
  fundingRatePrediction?: number;
  openInterest?: number;
  open_interest?: number;
  vol24h?: number;
  volume?: number;
  suspended?: boolean;
}

/**
 * Buffered ticker data for a single product
 */
interface TickerBuffer {
  productId: string;
  pairId: string;
  markPrice: number | null;
  indexPrice: number | null;
  fundingRate: number | null;
  fundingRatePrediction: number | null;
  openInterest: number | null;
  lastPrice: number | null;
  bid: number | null;
  ask: number | null;
  volume24h: number | null;
  suspended: boolean;
  lastUpdate: number;
}

/**
 * Configuration for funding rate consumer
 */
interface ConsumerConfig {
  natsUrl: string;
  stream: string;
  filter: string;
  consumerName: string;
  flushIntervalMs: number;
  cleanupIntervalMs: number;
  retentionDays: number;
  metricsPort: number;
}

/**
 * Load configuration from environment variables
 */
function loadConfig(): ConsumerConfig {
  return {
    natsUrl: Deno.env.get("NATS_URL") ?? "nats://localhost:4222",
    stream: Deno.env.get("NATS_STREAM") ?? "PROD_KRAKEN_FUTURES",
    filter: Deno.env.get("NATS_FILTER") ?? "prod.kraken-futures.json.ticker.>",
    consumerName: Deno.env.get("CONSUMER_NAME") ?? "funding-rate-consumer",
    flushIntervalMs: parseInt(Deno.env.get("FLUSH_INTERVAL_MS") ?? "300000", 10), // 5 min
    cleanupIntervalMs: parseInt(Deno.env.get("CLEANUP_INTERVAL_MS") ?? "3600000", 10), // 1 hr
    retentionDays: parseInt(Deno.env.get("RETENTION_DAYS") ?? "7", 10),
    metricsPort: parseInt(Deno.env.get("METRICS_PORT") ?? "9090", 10),
  };
}

// Prometheus metrics for this consumer
const metrics = new MetricsRegistry();
const messagesProcessedMetric = metrics.counter(
  "ssmd_funding_rate_messages_total",
  "Total ticker messages processed"
);
const flushesCompletedMetric = metrics.counter(
  "ssmd_funding_rate_flushes_total",
  "Total flush cycles completed"
);
const snapshotsWrittenMetric = metrics.counter(
  "ssmd_funding_rate_snapshots_total",
  "Total snapshots written to DB"
);
const productsTrackedMetric = metrics.gauge(
  "ssmd_funding_rate_products_tracked",
  "Number of products currently tracked"
);
const lastFlushTimestampMetric = metrics.gauge(
  "ssmd_funding_rate_last_flush_timestamp",
  "Unix timestamp of last successful flush"
);
const bufferSizeMetric = metrics.gauge(
  "ssmd_funding_rate_buffer_size",
  "Current buffered ticker entries"
);
const consumerConnectedMetric = metrics.gauge(
  "ssmd_funding_rate_connected",
  "NATS consumer connected (1=yes, 0=no)"
);

/**
 * Extract product_id from NATS subject
 * Subject format: prod.kraken-futures.json.ticker.PF_XBTUSD
 */
function extractProductId(subject: string): string | null {
  const parts = subject.split(".");
  // Last part is the product_id
  return parts.length >= 5 ? parts[parts.length - 1] : null;
}

/**
 * Parse a ticker message into a TickerBuffer
 */
function parseTickerMessage(msg: KrakenFuturesTickerMessage, productId: string): TickerBuffer {
  return {
    productId,
    pairId: `kraken:${productId}`,
    markPrice: msg.markPrice ?? msg.mark_price ?? null,
    indexPrice: msg.index ?? msg.indexPrice ?? msg.index_price ?? null,
    fundingRate: msg.funding_rate ?? msg.fundingRate ?? null,
    fundingRatePrediction: msg.funding_rate_prediction ?? msg.fundingRatePrediction ?? null,
    openInterest: msg.openInterest ?? msg.open_interest ?? null,
    lastPrice: msg.last ?? null,
    bid: msg.bid ?? null,
    ask: msg.ask ?? null,
    volume24h: msg.vol24h ?? msg.volume ?? null,
    suspended: msg.suspended ?? false,
    lastUpdate: Date.now(),
  };
}

/**
 * Convert buffered ticker data to NewPair for insertPerpSnapshots
 */
function bufferToNewPair(buf: TickerBuffer): NewPair {
  return {
    pairId: buf.pairId,
    exchange: "kraken",
    base: buf.productId.replace(/^PF_/, "").replace(/USD$/, ""),
    quote: "USD",
    wsName: buf.productId,
    marketType: "perpetual",
    markPrice: buf.markPrice != null ? String(buf.markPrice) : null,
    indexPrice: buf.indexPrice != null ? String(buf.indexPrice) : null,
    fundingRate: buf.fundingRate != null ? String(buf.fundingRate) : null,
    fundingRatePrediction: buf.fundingRatePrediction != null ? String(buf.fundingRatePrediction) : null,
    openInterest: buf.openInterest != null ? String(buf.openInterest) : null,
    lastPrice: buf.lastPrice != null ? String(buf.lastPrice) : null,
    bid: buf.bid != null ? String(buf.bid) : null,
    ask: buf.ask != null ? String(buf.ask) : null,
    volume24h: buf.volume24h != null ? String(buf.volume24h) : null,
    suspended: buf.suspended,
  };
}

/**
 * Main funding rate consumer daemon
 */
export async function runFundingRateConsumer(args: string[] = Deno.args): Promise<void> {
  const flags = parseArgs(args, {
    boolean: ["help"],
    alias: { h: "help" },
  });

  if (flags.help) {
    console.log(`
SSMD Funding Rate Consumer - Consume Kraken Futures ticker data from NATS
and write funding rate snapshots to pair_snapshots table.

Environment variables:
  DATABASE_URL       PostgreSQL connection string (required)
  NATS_URL           NATS server URL (default: nats://localhost:4222)
  NATS_STREAM        JetStream stream name (default: PROD_KRAKEN_FUTURES)
  NATS_FILTER        Subject filter (default: prod.kraken-futures.json.ticker.>)
  CONSUMER_NAME      Durable consumer name (default: funding-rate-consumer)
  FLUSH_INTERVAL_MS  Snapshot flush interval in ms (default: 300000 = 5 min)
  CLEANUP_INTERVAL_MS  Old snapshot cleanup interval in ms (default: 3600000 = 1 hr)
  RETENTION_DAYS     Days of snapshot retention (default: 7)
  METRICS_PORT       HTTP metrics/health port (default: 9090)
`);
    return;
  }

  log("=== SSMD Funding Rate Consumer ===");

  const config = loadConfig();
  log(`NATS: ${config.natsUrl}`);
  log(`Stream: ${config.stream}`);
  log(`Filter: ${config.filter}`);
  log(`Consumer: ${config.consumerName}`);
  log(`Flush interval: ${config.flushIntervalMs}ms`);
  log(`Retention: ${config.retentionDays} days`);

  // Verify DATABASE_URL is set
  if (!Deno.env.get("DATABASE_URL")) {
    logError("DATABASE_URL environment variable not set");
    Deno.exit(1);
  }

  // Initialize database connection
  const db = getDb();
  log("Database connected");

  // Connect to NATS
  let nc: NatsConnection;
  try {
    nc = await connect({ servers: config.natsUrl });
    log("NATS connected");
    consumerConnectedMetric.set({}, 1);
  } catch (e) {
    logError(`Failed to connect to NATS: ${e}`);
    await closeDb();
    Deno.exit(1);
  }

  const js = nc.jetstream();
  const jsm = await nc.jetstreamManager();

  // Create or get durable consumer
  try {
    await jsm.consumers.add(config.stream, {
      durable_name: config.consumerName,
      filter_subject: config.filter,
      ack_policy: AckPolicy.Explicit,
      deliver_policy: DeliverPolicy.New, // Only new messages, not historical
    });
    log(`Created durable consumer: ${config.consumerName}`);
  } catch (e) {
    if (!String(e).includes("already exists")) {
      logError(`Failed to create consumer: ${e}`);
    }
  }

  const consumer = await js.consumers.get(config.stream, config.consumerName);
  log("Consumer ready, starting message consumption...");

  // Buffer: latest ticker data per product
  const tickerBuffers = new Map<string, TickerBuffer>();

  // Stats
  let messagesProcessed = 0;
  let snapshotsFlushed = 0;
  let flushCount = 0;
  let cleanupCount = 0;
  const startTime = Date.now();

  /**
   * Flush buffered ticker data to pair_snapshots
   */
  async function flushSnapshots(): Promise<void> {
    if (tickerBuffers.size === 0) return;

    const pairs: NewPair[] = [];
    for (const buf of tickerBuffers.values()) {
      pairs.push(bufferToNewPair(buf));
    }

    try {
      const count = await insertPerpSnapshots(db, pairs);
      snapshotsFlushed += count;
      flushCount++;
      log(`[flush #${flushCount}] Wrote ${count} snapshots (total: ${snapshotsFlushed})`);
      flushesCompletedMetric.inc();
      snapshotsWrittenMetric.inc({}, count);
      lastFlushTimestampMetric.set({}, Date.now() / 1000);
      productsTrackedMetric.set({}, tickerBuffers.size);
      bufferSizeMetric.set({}, tickerBuffers.size);
    } catch (e) {
      logError(`Failed to flush snapshots: ${e}`);
    }
  }

  // Setup periodic flush timer
  const flushTimer = setInterval(async () => {
    await flushSnapshots();
  }, config.flushIntervalMs);

  // Setup periodic cleanup timer
  const cleanupTimer = setInterval(async () => {
    try {
      const cleaned = await cleanupOldSnapshots(config.retentionDays);
      if (cleaned > 0) {
        cleanupCount += cleaned;
        log(`[cleanup] Removed ${cleaned} old snapshots (total: ${cleanupCount})`);
      }
    } catch (e) {
      logError(`Cleanup failed: ${e}`);
    }
  }, config.cleanupIntervalMs);

  // Setup graceful shutdown
  let shuttingDown = false;
  const shutdown = async () => {
    if (shuttingDown) return;
    shuttingDown = true;

    log("Shutting down...");
    clearInterval(flushTimer);
    clearInterval(cleanupTimer);
    consumerConnectedMetric.set({}, 0);
    await metricsServer.shutdown();

    // Final flush
    await flushSnapshots();

    const runtime = Math.round((Date.now() - startTime) / 1000);
    log(`Messages processed: ${messagesProcessed}`);
    log(`Snapshots flushed: ${snapshotsFlushed}`);
    log(`Flush cycles: ${flushCount}`);
    log(`Products tracked: ${tickerBuffers.size}`);
    log(`Runtime: ${runtime}s`);

    await nc.drain();
    await nc.close();
    await closeDb();
    Deno.exit(0);
  };

  Deno.addSignalListener("SIGINT", shutdown);
  Deno.addSignalListener("SIGTERM", shutdown);

  // Start metrics/health HTTP server
  const metricsServer = Deno.serve(
    { port: config.metricsPort, hostname: "0.0.0.0" },
    (req) => {
      const url = new URL(req.url);
      if (url.pathname === "/health") {
        return new Response(JSON.stringify({ status: "ok" }), {
          headers: { "content-type": "application/json" },
        });
      }
      if (url.pathname === "/ready") {
        const ready = !shuttingDown;
        return new Response(
          JSON.stringify({ status: ready ? "ok" : "not_ready" }),
          {
            status: ready ? 200 : 503,
            headers: { "content-type": "application/json" },
          }
        );
      }
      if (url.pathname === "/metrics") {
        return new Response(metrics.format(), {
          headers: { "content-type": "text/plain; charset=utf-8" },
        });
      }
      return new Response("Not Found", { status: 404 });
    }
  );
  log(`Metrics server listening on :${config.metricsPort}`);

  // Consume messages
  const messages = await consumer.consume();

  for await (const msg of messages) {
    try {
      const raw = JSON.parse(sc.decode(msg.data)) as KrakenFuturesTickerMessage;

      // Only process ticker messages
      if (raw.feed !== "ticker") {
        msg.ack();
        continue;
      }

      // Extract product_id from message or subject
      const productId = raw.product_id || extractProductId(msg.subject);
      if (!productId) {
        msg.ack();
        continue;
      }

      // Update buffer with latest data
      const buffer = parseTickerMessage(raw, productId);
      tickerBuffers.set(productId, buffer);

      messagesProcessed++;
      messagesProcessedMetric.inc();

      // Log progress every 1000 messages
      if (messagesProcessed % 1000 === 0) {
        const products = Array.from(tickerBuffers.keys()).join(", ");
        log(`Processed ${messagesProcessed} messages, tracking: ${products}`);
      }

      msg.ack();
    } catch (e) {
      logError(`Error processing message: ${e}`);
      msg.nak();
    }
  }
}

// If run directly, start the consumer
if (import.meta.main) {
  await runFundingRateConsumer();
}
