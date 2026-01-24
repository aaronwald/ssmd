// ssmd-agent/src/runtime/nats.ts
import {
  connect,
  type NatsConnection,
  type JetStreamClient,
  type Consumer,
  StringCodec,
  Events,
} from "npm:nats";
import type { MarketRecord } from "../state/types.ts";
import type { RecordSource, FireSink, SignalFire } from "./interfaces.ts";

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
 * Monitor NATS connection status and log events
 */
async function monitorConnection(nc: NatsConnection, name: string): Promise<void> {
  for await (const status of nc.status()) {
    switch (status.type) {
      case Events.Disconnect:
        console.warn(`[${name}] NATS disconnected`);
        break;
      case Events.Reconnect:
        console.log(`[${name}] NATS reconnected to ${status.data}`);
        break;
      case Events.Error:
        console.error(`[${name}] NATS error: ${status.data}`);
        break;
      case Events.LDM:
        console.warn(`[${name}] NATS entered lame duck mode`);
        break;
    }
  }
}

/**
 * Raw record format from NATS (matches archiver output)
 */
interface RawRecord {
  type: string;
  sid?: number;
  msg?: Record<string, unknown>;
}

/**
 * Parse a raw NATS message into a MarketRecord
 */
function parseRecord(raw: RawRecord): MarketRecord | null {
  if (!raw.msg) return null;

  const msg = raw.msg;
  return {
    type: raw.type,
    ticker: (msg.market_ticker as string) ?? "",
    ts: (msg.ts as number) ?? 0,
    volume: msg.volume as number | undefined,
    dollar_volume: msg.dollar_volume as number | undefined,
    price: msg.price as number | undefined,
    yes_bid: msg.yes_bid as number | undefined,
    yes_ask: msg.yes_ask as number | undefined,
  };
}

/**
 * NATS JetStream record source.
 * Subscribes to a stream and yields market records.
 */
export class NatsRecordSource implements RecordSource {
  private nc: NatsConnection | null = null;
  private js: JetStreamClient | null = null;
  private consumer: Consumer | null = null;
  private closed = false;

  constructor(
    private servers: string,
    private stream: string,
    private filterSubject?: string,
    private consumerName?: string,
    private startSeq?: number,
  ) {}

  async *subscribe(): AsyncIterable<MarketRecord> {
    this.nc = await connect({
      servers: this.servers,
      ...DEFAULT_NATS_OPTIONS,
    });
    this.js = this.nc.jetstream();

    // Start connection monitor (fire and forget)
    monitorConnection(this.nc, `RecordSource:${this.stream}`).catch(() => {});

    console.log(`Connected to NATS: ${this.servers}`);
    console.log(`Stream: ${this.stream}, Filter: ${this.filterSubject ?? "all"}`);

    // Get or create consumer
    const jsm = await this.nc.jetstreamManager();

    if (this.consumerName) {
      // Use existing durable consumer
      this.consumer = await this.js.consumers.get(this.stream, this.consumerName);
    } else {
      // Create ephemeral ordered consumer
      this.consumer = await this.js.consumers.get(this.stream);
    }

    const messages = await this.consumer.consume();

    for await (const msg of messages) {
      if (this.closed) break;

      try {
        const raw = JSON.parse(sc.decode(msg.data)) as RawRecord;
        const record = parseRecord(raw);
        if (record && record.ticker) {
          yield record;
        }
      } catch (e) {
        console.error(`Failed to parse message: ${e}`);
      }

      msg.ack();
    }
  }

  async close(): Promise<void> {
    this.closed = true;
    if (this.nc) {
      await this.nc.drain();
      await this.nc.close();
    }
  }
}

/**
 * NATS JetStream fire sink.
 * Publishes signal fires to a JetStream stream for durability.
 */
export class NatsFireSink implements FireSink {
  private nc: NatsConnection | null = null;
  private js: JetStreamClient | null = null;

  constructor(private servers: string) {}

  async publish(fire: SignalFire): Promise<void> {
    if (!this.nc) {
      this.nc = await connect({
        servers: this.servers,
        ...DEFAULT_NATS_OPTIONS,
      });
      this.js = this.nc.jetstream();
      // Start connection monitor (fire and forget)
      monitorConnection(this.nc, "FireSink").catch(() => {});
      console.log(`Fire sink connected to NATS JetStream: ${this.servers}`);
    }

    const subject = `signals.${fire.signalId}.fires`;
    const data = JSON.stringify(fire);
    await this.js!.publish(subject, sc.encode(data));
  }

  async close(): Promise<void> {
    if (this.nc) {
      await this.nc.drain();
      await this.nc.close();
    }
  }
}

/**
 * Console fire sink for local testing.
 * Logs fires to stdout.
 */
export class ConsoleFireSink implements FireSink {
  async publish(fire: SignalFire): Promise<void> {
    const time = new Date(fire.ts * 1000).toISOString();
    console.log(`FIRE ${time} ${fire.ticker}`);
    console.log(`  ${JSON.stringify(fire.payload)}`);
  }

  async close(): Promise<void> {
    // Nothing to close
  }
}

/**
 * Logging fire sink wrapper.
 * Logs fires to console AND delegates to another sink.
 */
export class LoggingFireSink implements FireSink {
  constructor(private inner: FireSink) {}

  async publish(fire: SignalFire): Promise<void> {
    const time = new Date(fire.ts * 1000).toISOString();
    console.log(`FIRE [${fire.signalId}] ${time} ${fire.ticker}`);
    console.log(`  ${JSON.stringify(fire.payload)}`);
    await this.inner.publish(fire);
  }

  async close(): Promise<void> {
    await this.inner.close();
  }
}
