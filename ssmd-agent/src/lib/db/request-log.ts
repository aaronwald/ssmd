/**
 * Batch request logger for billing.
 * Buffers API request entries in memory and bulk-inserts to PostgreSQL
 * on a timer (5s) or when buffer reaches threshold (100 rows).
 */
import { apiRequestLog, type NewApiRequestLogEntry } from "./schema.ts";
import type { Database } from "./client.ts";

const FLUSH_INTERVAL_MS = 5_000;
const FLUSH_THRESHOLD = 100;
const MAX_BUFFER_SIZE = 1_000;

export interface RequestLogEntry {
  keyPrefix: string;
  method: string;
  path: string;
  statusCode: number;
  responseBytes: number | null;
}

export class RequestLogBuffer {
  private buffer: RequestLogEntry[] = [];
  private db: Database;
  private timer: ReturnType<typeof setInterval> | null = null;
  private flushFailures = 0;

  constructor(db: Database) {
    this.db = db;
  }

  /**
   * Start the periodic flush timer.
   */
  start(): void {
    if (this.timer) return;
    this.timer = setInterval(() => {
      this.flush().catch((err) => {
        console.error("Request log flush failed:", err);
      });
    }, FLUSH_INTERVAL_MS);
  }

  /**
   * Stop the periodic flush timer.
   */
  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  /**
   * Add an entry to the buffer. Triggers flush if threshold reached.
   */
  push(entry: RequestLogEntry): void {
    this.buffer.push(entry);

    // Drop oldest entries if buffer exceeds max
    if (this.buffer.length > MAX_BUFFER_SIZE) {
      const dropped = this.buffer.length - MAX_BUFFER_SIZE;
      this.buffer = this.buffer.slice(dropped);
      console.warn(`Request log buffer overflow: dropped ${dropped} oldest entries`);
    }

    if (this.buffer.length >= FLUSH_THRESHOLD) {
      this.flush().catch((err) => {
        console.error("Request log flush failed:", err);
      });
    }
  }

  /**
   * Flush buffer to PostgreSQL. Called periodically and on shutdown.
   */
  async flush(): Promise<void> {
    if (this.buffer.length === 0) return;

    const entries = this.buffer.splice(0);

    try {
      const rows: NewApiRequestLogEntry[] = entries.map((e) => ({
        keyPrefix: e.keyPrefix,
        method: e.method,
        path: e.path,
        statusCode: e.statusCode,
        responseBytes: e.responseBytes,
      }));

      await this.db.insert(apiRequestLog).values(rows);
      this.flushFailures = 0;
    } catch (err) {
      // Put entries back at the front of the buffer for retry
      this.buffer.unshift(...entries);
      this.flushFailures++;

      // Drop oldest if buffer exceeds max after re-adding
      if (this.buffer.length > MAX_BUFFER_SIZE) {
        const dropped = this.buffer.length - MAX_BUFFER_SIZE;
        this.buffer = this.buffer.slice(dropped);
        console.warn(`Request log buffer overflow after flush failure: dropped ${dropped} entries`);
      }

      throw err;
    }
  }

  /**
   * Graceful shutdown: flush remaining entries.
   */
  async shutdown(): Promise<void> {
    this.stop();
    await this.flush();
  }

  /**
   * Get current buffer size (for monitoring).
   */
  get size(): number {
    return this.buffer.length;
  }

  /**
   * Get flush failure count (for monitoring).
   */
  get failures(): number {
    return this.flushFailures;
  }
}
