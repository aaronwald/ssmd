// ssmd-notifier/src/consumer.ts
import {
  connect,
  StringCodec,
  type NatsConnection,
  type JetStreamClient,
  type Consumer,
  AckPolicy,
  DeliverPolicy,
  Events,
} from "nats";
import type { SignalFire, NotifierConfig } from "./types.ts";
import { shouldRoute } from "./router.ts";
import { NtfySender } from "./senders/mod.ts";
import {
  incrementFiresReceived,
  incrementNotificationsSent,
  incrementNotificationsFailed,
} from "./server.ts";

const sc = StringCodec();

/**
 * Default NATS connection options with reconnection enabled
 */
const DEFAULT_NATS_OPTIONS = {
  reconnect: true,
  maxReconnectAttempts: -1, // unlimited
  reconnectTimeWait: 2000, // 2 seconds between attempts
  pingInterval: 30000, // 30 second ping
  maxPingOut: 3, // 3 missed pings before disconnect
};

/**
 * Monitor NATS connection status and log events.
 * Exits the process on fatal errors to allow K8s restart.
 */
async function monitorConnection(nc: NatsConnection): Promise<void> {
  for await (const status of nc.status()) {
    switch (status.type) {
      case Events.Disconnect:
        console.warn(`[NATS] Disconnected`);
        break;
      case Events.Reconnect:
        console.log(`[NATS] Reconnected to ${status.data}`);
        break;
      case Events.Error:
        console.error(`[NATS] Error: ${status.data}`);
        break;
      case Events.LDM:
        console.warn(`[NATS] Entered lame duck mode`);
        break;
    }
  }
  // If we exit the status loop, connection is truly dead
  console.error("[NATS] Connection status monitor exited - connection dead");
}

function isSignalFire(obj: unknown): obj is SignalFire {
  if (typeof obj !== "object" || obj === null) return false;
  const fire = obj as Record<string, unknown>;
  return (
    typeof fire.signalId === "string" &&
    typeof fire.ts === "number" &&
    typeof fire.ticker === "string"
  );
}

const IDLE_HEARTBEAT_MS = 30_000;

export async function runConsumer(config: NotifierConfig): Promise<void> {
  const sender = new NtfySender();

  console.log(`Connecting to NATS: ${config.natsUrl}`);
  const nc = await connect({
    servers: config.natsUrl,
    ...DEFAULT_NATS_OPTIONS,
  });
  const js: JetStreamClient = nc.jetstream();

  // Start connection monitor (fire and forget)
  monitorConnection(nc).catch(() => {});

  console.log(`Stream: ${config.stream}`);
  console.log(`Consumer: ${config.consumer}`);
  if (config.filterSubject) {
    console.log(`Filter: ${config.filterSubject}`);
  }

  // Get or create the durable consumer
  const jsm = await nc.jetstreamManager();

  // Try to get existing consumer, or create it
  let consumer: Consumer;
  try {
    consumer = await js.consumers.get(config.stream, config.consumer);
    console.log(`Using existing consumer: ${config.consumer}`);
  } catch {
    // Consumer doesn't exist, create it
    console.log(`Creating consumer: ${config.consumer}`);
    await jsm.consumers.add(config.stream, {
      durable_name: config.consumer,
      ack_policy: AckPolicy.Explicit,
      deliver_policy: DeliverPolicy.New,
      filter_subject: config.filterSubject,
      idle_heartbeat: IDLE_HEARTBEAT_MS * 1_000_000, // nanoseconds
    });
    consumer = await js.consumers.get(config.stream, config.consumer);
  }

  const messages = await consumer.consume();
  console.log(`Consuming messages...`);

  for await (const msg of messages) {
    try {
      const data = JSON.parse(sc.decode(msg.data));

      if (!isSignalFire(data)) {
        console.warn("Received non-SignalFire message, skipping");
        msg.ack();
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

      // Ack after processing
      msg.ack();
    } catch (e) {
      console.error(`Failed to process message: ${e}`);
      // Still ack to avoid redelivery loop on bad messages
      msg.ack();
    }
  }

  // If we get here, the consume loop exited - connection is dead
  // Exit to let K8s restart us with a fresh connection
  console.error("Consume loop exited - NATS connection lost, exiting for restart");
  Deno.exit(1);
}
