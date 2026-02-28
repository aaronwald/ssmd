import { z } from "zod";

/**
 * Kalshi Market schema - represents an individual prediction market
 * Prices are in dollars (0.00-1.00 range).
 */
export const MarketSchema = z.object({
  /** Unique market ticker */
  ticker: z.string().min(1),
  /** Parent event ticker */
  event_ticker: z.string().min(1),
  /** Market title/question */
  title: z.string(),
  /** Market status */
  status: z.enum(["active", "closed", "settled"]).default("active"),
  /** When the market closes for trading */
  close_time: z.string().nullable().optional(),
  /** Current yes bid price (dollars, 0.00-1.00) */
  yes_bid: z.number().nullable().optional(),
  /** Current yes ask price (dollars, 0.00-1.00) */
  yes_ask: z.number().nullable().optional(),
  /** Current no bid price (dollars, 0.00-1.00) */
  no_bid: z.number().nullable().optional(),
  /** Current no ask price (dollars, 0.00-1.00) */
  no_ask: z.number().nullable().optional(),
  /** Last traded price (dollars, 0.00-1.00) */
  last_price: z.number().nullable().optional(),
  /** Total volume traded */
  volume: z.number().default(0),
  /** 24-hour volume */
  volume_24h: z.number().default(0),
  /** Open interest */
  open_interest: z.number().default(0),
  /** Minimum expiration value for YES */
  floor_strike: z.number().nullable().optional(),
  /** Maximum expiration value for YES */
  cap_strike: z.number().nullable().optional(),
  /** How market strike is defined (e.g., "greater", "between") */
  strike_type: z.string().nullable().optional(),
  /** Settlement result (yes, no, scalar, empty) */
  result: z.string().nullable().optional(),
  /** Expiration value used for settlement */
  expiration_value: z.string().nullable().optional(),
  /** Shortened title for the yes side */
  yes_sub_title: z.string().nullable().optional(),
  /** Shortened title for the no side */
  no_sub_title: z.string().nullable().optional(),
  /** Whether market can close early */
  can_close_early: z.boolean().nullable().optional(),
  /** Market type: binary or scalar */
  market_type: z.string().nullable().optional(),
  /** When the market opens for trading */
  open_time: z.string().nullable().optional(),
});

/**
 * Market for database insertion
 */
export const MarketInsertSchema = MarketSchema.extend({
  created_at: z.date().optional(),
  updated_at: z.date().optional(),
  deleted_at: z.date().nullable().optional(),
});

export type Market = z.infer<typeof MarketSchema>;
export type MarketInsert = z.infer<typeof MarketInsertSchema>;

/**
 * Kalshi API market response shape.
 *
 * Kalshi is deprecating integer cent fields (yes_bid, yes_ask, etc.) on March 5 2026.
 * The _dollars variants (yes_bid_dollars, yes_ask_dollars, etc.) are the replacement
 * and contain string representations of dollar values (e.g., "0.5500").
 */
export interface KalshiMarket {
  ticker: string;
  event_ticker: string;
  title?: string;
  subtitle?: string;
  status: string;
  close_time?: string;
  // Legacy integer cent fields (deprecated March 5 2026)
  yes_bid?: number;
  yes_ask?: number;
  no_bid?: number;
  no_ask?: number;
  last_price?: number;
  // New dollar string fields (preferred)
  yes_bid_dollars?: string;
  yes_ask_dollars?: string;
  no_bid_dollars?: string;
  no_ask_dollars?: string;
  last_price_dollars?: string;
  volume?: number;
  volume_24h?: number;
  open_interest?: number;
  // Enrichment fields
  floor_strike?: number | null;
  cap_strike?: number | null;
  strike_type?: string;
  result?: string;
  expiration_value?: string;
  yes_sub_title?: string;
  no_sub_title?: string;
  can_close_early?: boolean;
  market_type?: string;
  open_time?: string;
}

/**
 * Parse a Kalshi _dollars string field to a number, or return null.
 * Kalshi sends empty strings for some fields (e.g., price_dollars: ""),
 * so we treat empty/invalid as null.
 */
function parseDollars(s: string | undefined): number | null {
  if (s === undefined || s === "") return null;
  const n = parseFloat(s);
  return Number.isFinite(n) ? n : null;
}

/**
 * Resolve a price field: prefer _dollars string, fall back to cents / 100.
 */
function resolvePrice(dollars: string | undefined, cents: number | undefined): number | null {
  const d = parseDollars(dollars);
  if (d !== null) return d;
  if (cents !== undefined && cents !== null) return cents / 100;
  return null;
}

/**
 * Convert Kalshi API market to our Market type.
 * Prefers _dollars fields (new API), falls back to cents / 100 (legacy).
 */
export function fromKalshiMarket(km: KalshiMarket): Market {
  return {
    ticker: km.ticker,
    event_ticker: km.event_ticker,
    title: km.title ?? km.subtitle ?? km.ticker,
    status: km.status === "active" ? "active" : km.status === "closed" ? "closed" : "settled",
    close_time: km.close_time ?? null,
    yes_bid: resolvePrice(km.yes_bid_dollars, km.yes_bid),
    yes_ask: resolvePrice(km.yes_ask_dollars, km.yes_ask),
    no_bid: resolvePrice(km.no_bid_dollars, km.no_bid),
    no_ask: resolvePrice(km.no_ask_dollars, km.no_ask),
    last_price: resolvePrice(km.last_price_dollars, km.last_price),
    volume: km.volume ?? 0,
    volume_24h: km.volume_24h ?? 0,
    open_interest: km.open_interest ?? 0,
    floor_strike: km.floor_strike ?? null,
    cap_strike: km.cap_strike ?? null,
    strike_type: km.strike_type ?? null,
    result: km.result ?? null,
    expiration_value: km.expiration_value ?? null,
    yes_sub_title: km.yes_sub_title ?? null,
    no_sub_title: km.no_sub_title ?? null,
    can_close_early: km.can_close_early ?? null,
    market_type: km.market_type ?? null,
    open_time: km.open_time ?? null,
  };
}
