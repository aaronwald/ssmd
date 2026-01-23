// ssmd-agent/src/cli/commands/lifecycle-consumer.ts
// Daemon that consumes lifecycle events from NATS and stores them in PostgreSQL

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
  insertLifecycleEvent,
  getSeries,
  upsertEventFromLifecycle,
  upsertMarketFromLifecycle,
  updateMarketStatus,
  type NewMarketLifecycleEvent,
  type Database,
} from "../../lib/db/mod.ts";

const sc = StringCodec();

/**
 * Raw lifecycle message from NATS (matches connector output)
 */
interface RawLifecycleMessage {
  type: string;
  sid?: number;
  seq?: number;
  msg: {
    market_ticker: string;
    event_type: string;
    open_ts?: number;
    close_ts?: number;
    determination_ts?: number;
    settled_ts?: number;
    result?: string;
    additional_metadata?: Record<string, unknown>;
  };
}

/**
 * Extract series ticker from event_ticker (e.g., "KXBTCD-26JAN2317" -> "KXBTCD")
 */
function extractSeriesTicker(eventTicker: string): string {
  const parts = eventTicker.split("-");
  return parts[0];
}

/**
 * Convert raw NATS message to database record
 */
function toDbRecord(raw: RawLifecycleMessage): NewMarketLifecycleEvent {
  const msg = raw.msg;
  return {
    marketTicker: msg.market_ticker,
    eventType: msg.event_type,
    openTs: msg.open_ts ? new Date(msg.open_ts * 1000) : null,
    closeTs: msg.close_ts ? new Date(msg.close_ts * 1000) : null,
    settledTs: msg.settled_ts || msg.determination_ts
      ? new Date((msg.settled_ts || msg.determination_ts!) * 1000)
      : null,
    metadata: {
      result: msg.result,
      ...msg.additional_metadata,
    },
  };
}

/**
 * Configuration for lifecycle consumer
 */
interface ConsumerConfig {
  natsUrl: string;
  stream: string;
  filter: string;
  consumerName: string;
}

/**
 * Load configuration from environment variables
 */
function loadConfig(): ConsumerConfig {
  return {
    natsUrl: Deno.env.get("NATS_URL") ?? "nats://localhost:4222",
    stream: Deno.env.get("NATS_STREAM") ?? "DEV_KALSHI",
    filter: Deno.env.get("NATS_FILTER") ?? "dev.kalshi.lifecycle.>",
    consumerName: Deno.env.get("CONSUMER_NAME") ?? "lifecycle-consumer",
  };
}

/**
 * Main lifecycle consumer daemon
 */
export async function runLifecycleConsumer(args: string[] = Deno.args): Promise<void> {
  const flags = parseArgs(args, {
    boolean: ["help"],
    alias: { h: "help" },
  });

  if (flags.help) {
    console.log(`
SSMD Lifecycle Consumer - Consume lifecycle events from NATS and store in PostgreSQL

Environment variables:
  DATABASE_URL     PostgreSQL connection string (required)
  NATS_URL         NATS server URL (default: nats://localhost:4222)
  NATS_STREAM      JetStream stream name (default: DEV_KALSHI)
  NATS_FILTER      Subject filter (default: dev.kalshi.lifecycle.>)
  CONSUMER_NAME    Durable consumer name (default: lifecycle-consumer)
`);
    return;
  }

  console.log("=== SSMD Lifecycle Consumer ===");
  console.log();

  const config = loadConfig();
  console.log(`NATS: ${config.natsUrl}`);
  console.log(`Stream: ${config.stream}`);
  console.log(`Filter: ${config.filter}`);
  console.log(`Consumer: ${config.consumerName}`);
  console.log();

  // Verify DATABASE_URL is set
  if (!Deno.env.get("DATABASE_URL")) {
    console.error("DATABASE_URL environment variable not set");
    Deno.exit(1);
  }

  // Initialize database connection
  const db = getDb();
  console.log("Database connected");

  // Connect to NATS
  let nc: NatsConnection;
  try {
    nc = await connect({ servers: config.natsUrl });
    console.log("NATS connected");
  } catch (e) {
    console.error(`Failed to connect to NATS: ${e}`);
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
      deliver_policy: DeliverPolicy.All,
    });
    console.log(`Created durable consumer: ${config.consumerName}`);
  } catch (e) {
    // Consumer might already exist, try to get it
    if (!String(e).includes("already exists")) {
      console.error(`Failed to create consumer: ${e}`);
    }
  }

  const consumer = await js.consumers.get(config.stream, config.consumerName);
  console.log("Consumer ready, starting message consumption...");
  console.log();

  // Stats
  let messagesProcessed = 0;
  let messagesErrored = 0;
  let marketsCreated = 0;
  let marketsUpdated = 0;
  let seriesNotFound = 0;
  const startTime = Date.now();

  // Setup graceful shutdown
  const shutdown = async () => {
    console.log("\nShutting down...");
    const runtime = Math.round((Date.now() - startTime) / 1000);
    console.log(`Processed: ${messagesProcessed} messages`);
    console.log(`Markets created: ${marketsCreated}`);
    console.log(`Markets updated: ${marketsUpdated}`);
    console.log(`Series not found: ${seriesNotFound}`);
    console.log(`Errors: ${messagesErrored}`);
    console.log(`Runtime: ${runtime}s`);
    await nc.drain();
    await nc.close();
    await closeDb();
    Deno.exit(0);
  };

  Deno.addSignalListener("SIGINT", shutdown);
  Deno.addSignalListener("SIGTERM", shutdown);

  // Consume messages
  const messages = await consumer.consume();

  for await (const msg of messages) {
    try {
      const raw = JSON.parse(sc.decode(msg.data)) as RawLifecycleMessage;

      // Only process market_lifecycle_v2 messages
      if (raw.type !== "market_lifecycle_v2") {
        msg.ack();
        continue;
      }

      const lifecycleMsg = raw.msg;
      const eventType = lifecycleMsg.event_type;
      const marketTicker = lifecycleMsg.market_ticker;
      const metadata = lifecycleMsg.additional_metadata as Record<string, unknown> | undefined;

      // Handle 'created' events - create event and market records
      if (eventType === "created" && metadata) {
        const eventTicker = metadata.event_ticker as string | undefined;
        const title = metadata.title as string | undefined;
        const expectedExpirationTs = metadata.expected_expiration_ts as number | undefined;

        if (eventTicker && title) {
          const seriesTicker = extractSeriesTicker(eventTicker);
          const series = await getSeries(seriesTicker);

          if (series) {
            // Upsert event record
            await upsertEventFromLifecycle(
              db,
              eventTicker,
              title,
              series.category,
              seriesTicker
            );

            // Upsert market record
            const closeTime = expectedExpirationTs
              ? new Date(expectedExpirationTs * 1000)
              : null;
            await upsertMarketFromLifecycle(
              db,
              marketTicker,
              eventTicker,
              title,
              closeTime
            );

            marketsCreated++;
          } else {
            console.warn(
              `Series not found for ${seriesTicker} (event: ${eventTicker}), skipping market creation`
            );
            seriesNotFound++;
          }
        }
      }

      // Handle 'settled' and 'determined' events - update market status
      if (eventType === "settled" || eventType === "determined") {
        const updated = await updateMarketStatus(db, marketTicker, "settled");
        if (updated) {
          marketsUpdated++;
        }
      }

      // Always insert lifecycle event (existing behavior)
      const record = toDbRecord(raw);
      await insertLifecycleEvent(db, record);

      messagesProcessed++;

      // Log progress every 100 messages
      if (messagesProcessed % 100 === 0) {
        console.log(
          `Processed ${messagesProcessed} messages (created: ${marketsCreated}, updated: ${marketsUpdated})`
        );
      }

      msg.ack();
    } catch (e) {
      console.error(`Error processing message: ${e}`);
      messagesErrored++;
      // Negative ack to requeue (or just ack to skip bad messages)
      msg.nak();
    }
  }
}

// If run directly, start the consumer
if (import.meta.main) {
  await runLifecycleConsumer();
}
