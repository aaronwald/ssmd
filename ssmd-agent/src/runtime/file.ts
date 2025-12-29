// ssmd-agent/src/runtime/file.ts
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { gunzip } from "https://deno.land/x/compress@v0.4.6/gzip/mod.ts";
import type { MarketRecord } from "../state/types.ts";
import type { RecordSource } from "./interfaces.ts";

/**
 * Raw record format from JSONL files
 */
interface RawRecord {
  type: string;
  sid?: number;
  msg?: Record<string, unknown>;
}

/**
 * Parse a raw JSONL record into a MarketRecord
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
 * Read and decompress a .jsonl.gz file
 */
async function readGzipJsonl(path: string): Promise<string[]> {
  const compressed = await Deno.readFile(path);
  const decompressed = gunzip(compressed);
  const text = new TextDecoder().decode(decompressed);
  return text.trim().split("\n").filter(Boolean);
}

/**
 * File-based record source.
 * Reads JSONL.gz files from a data directory.
 */
export class FileRecordSource implements RecordSource {
  private closed = false;

  constructor(
    private dataDir: string,
    private feed: string,
    private dates: string[],
  ) {}

  async *subscribe(): AsyncIterable<MarketRecord> {
    for (const date of this.dates) {
      if (this.closed) break;

      const dateDir = join(this.dataDir, this.feed, date);

      // List files for this date
      let files: string[];
      try {
        const entries = [];
        for await (const entry of Deno.readDir(dateDir)) {
          if (entry.isFile && entry.name.endsWith(".jsonl.gz")) {
            entries.push(entry.name);
          }
        }
        files = entries.sort(); // Process in chronological order
      } catch (e) {
        console.error(`Failed to read ${dateDir}: ${e}`);
        continue;
      }

      console.log(`Processing ${date}: ${files.length} files`);

      for (const file of files) {
        if (this.closed) break;

        const filePath = join(dateDir, file);

        try {
          const lines = await readGzipJsonl(filePath);

          for (const line of lines) {
            if (this.closed) break;

            try {
              const raw = JSON.parse(line) as RawRecord;
              const record = parseRecord(raw);
              if (record && record.ticker) {
                yield record;
              }
            } catch {
              // Skip unparseable lines
            }
          }
        } catch (e) {
          console.error(`Failed to process ${file}: ${e}`);
        }
      }
    }
  }

  async close(): Promise<void> {
    this.closed = true;
  }
}

/**
 * Get today's date in YYYY-MM-DD format
 */
export function getTodayDate(): string {
  return new Date().toISOString().split("T")[0];
}

/**
 * Expand a date range into an array of YYYY-MM-DD strings
 */
export function expandDateRange(from: string, to: string): string[] {
  const dates: string[] = [];
  const start = new Date(from);
  const end = new Date(to);

  const current = new Date(start);
  while (current <= end) {
    dates.push(current.toISOString().split("T")[0]);
    current.setDate(current.getDate() + 1);
  }

  return dates;
}
