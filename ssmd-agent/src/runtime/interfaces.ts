// ssmd-agent/src/runtime/interfaces.ts
import type { MarketRecord } from "../state/types.ts";

/**
 * A signal fire event published when a signal's evaluate() returns true
 */
export interface SignalFire {
  signalId: string;
  ts: number;
  ticker: string;
  payload: unknown;
}

/**
 * Source of market data records.
 * Implementations: NatsRecordSource, FileRecordSource
 */
export interface RecordSource {
  /**
   * Subscribe to market data records.
   * Returns an async iterator that yields records until close() is called.
   */
  subscribe(): AsyncIterable<MarketRecord>;

  /**
   * Gracefully close the source and release resources.
   */
  close(): Promise<void>;
}

/**
 * Sink for signal fire events.
 * Implementations: NatsFireSink, ConsoleFireSink
 */
export interface FireSink {
  /**
   * Publish a signal fire event.
   */
  publish(fire: SignalFire): Promise<void>;

  /**
   * Gracefully close the sink and release resources.
   */
  close(): Promise<void>;
}

/**
 * Configuration for the signal runtime
 */
export interface RuntimeConfig {
  /** Path to signal directory (e.g., "signals/volume-1m-30min") */
  signalPath: string;

  /** Source of market data records */
  source: RecordSource;

  /** Sink for signal fire events */
  sink: FireSink;

  /** Optional state builder configuration */
  stateConfig?: Record<string, Record<string, unknown>>;
}
