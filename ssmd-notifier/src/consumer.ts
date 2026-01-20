// ssmd-notifier/src/consumer.ts
import {
  connect,
  StringCodec,
  type NatsConnection,
  type JetStreamClient,
  type Consumer,
  AckPolicy,
  DeliverPolicy,
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

function isSignalFire(obj: unknown): obj is SignalFire {
  if (typeof obj !== "object" || obj === null) return false;
  const fire = obj as Record<string, unknown>;
  return (
    typeof fire.signalId === "string" &&
    typeof fire.ts === "number" &&
    typeof fire.ticker === "string"
  );
}

const RECONNECT_DELAY_MS = 5000;
const IDLE_HEARTBEAT_MS = 30_000;

async function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function runConsumer(config: NotifierConfig): Promise<void> {
  const sender = new NtfySender();

  // Outer reconnection loop
  while (true) {
    let nc: NatsConnection | null = null;

    try {
      console.log(`Connecting to NATS: ${config.natsUrl}`);
      nc = await connect({ servers: config.natsUrl });
      const js: JetStreamClient = nc.jetstream();

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

      // If we get here, the consume loop exited unexpectedly
      console.warn("Consume loop exited unexpectedly, will reconnect...");
    } catch (e) {
      console.error(`Consumer error: ${e}`);
    } finally {
      // Clean up connection before reconnecting
      if (nc) {
        try {
          await nc.close();
        } catch {
          // Ignore close errors
        }
      }
    }

    console.log(`Reconnecting in ${RECONNECT_DELAY_MS}ms...`);
    await delay(RECONNECT_DELAY_MS);
  }
}
