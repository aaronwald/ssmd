import type { MarketRecord } from "../state/types.ts";

interface RawRecord {
  type: string;
  sid?: number;
  msg?: Record<string, unknown>;
}

/**
 * Parse a raw NATS message into a MarketRecord.
 * Enhanced version that extracts side/count from trade messages.
 */
export function parseMomentumRecord(raw: RawRecord): MarketRecord | null {
  if (!raw.msg) return null;

  const msg = raw.msg;
  return {
    type: raw.type,
    ticker: (msg.market_ticker as string) ?? "",
    ts: (msg.ts as number) ?? 0,
    volume: msg.volume as number | undefined,
    dollar_volume: msg.dollar_volume as number | undefined,
    price: (msg.price ?? msg.last_price) as number | undefined,
    yes_bid: msg.yes_bid as number | undefined,
    yes_ask: msg.yes_ask as number | undefined,
    no_bid: msg.no_bid as number | undefined,
    no_ask: msg.no_ask as number | undefined,
    count: msg.count as number | undefined,
    side: msg.side as string | undefined,
  };
}
