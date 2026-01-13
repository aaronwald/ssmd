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

export async function runConsumer(config: NotifierConfig): Promise<void> {
  console.log(`Connecting to NATS: ${config.natsUrl}`);
  const nc: NatsConnection = await connect({ servers: config.natsUrl });
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
    });
    consumer = await js.consumers.get(config.stream, config.consumer);
  }

  const sender = new NtfySender();
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

  // Wait for shutdown signal
  await nc.closed();
}
