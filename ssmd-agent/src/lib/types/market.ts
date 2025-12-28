import { z } from "zod";

/**
 * Kalshi Market schema - represents an individual prediction market
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
  /** Current yes bid price (0-100) */
  yes_bid: z.number().nullable().optional(),
  /** Current yes ask price (0-100) */
  yes_ask: z.number().nullable().optional(),
  /** Current no bid price (0-100) */
  no_bid: z.number().nullable().optional(),
  /** Current no ask price (0-100) */
  no_ask: z.number().nullable().optional(),
  /** Last traded price */
  last_price: z.number().nullable().optional(),
  /** Total volume traded */
  volume: z.number().default(0),
  /** 24-hour volume */
  volume_24h: z.number().default(0),
  /** Open interest */
  open_interest: z.number().default(0),
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
 * Kalshi API market response shape
 */
export interface KalshiMarket {
  ticker: string;
  event_ticker: string;
  title?: string;
  subtitle?: string;
  status: string;
  close_time?: string;
  yes_bid?: number;
  yes_ask?: number;
  no_bid?: number;
  no_ask?: number;
  last_price?: number;
  volume?: number;
  volume_24h?: number;
  open_interest?: number;
  // Additional fields we don't store
  result?: string;
  can_close_early?: boolean;
}

/**
 * Convert Kalshi API market to our Market type
 */
export function fromKalshiMarket(km: KalshiMarket): Market {
  return {
    ticker: km.ticker,
    event_ticker: km.event_ticker,
    title: km.title ?? km.subtitle ?? km.ticker,
    status: km.status === "active" ? "active" : km.status === "closed" ? "closed" : "settled",
    close_time: km.close_time,
    yes_bid: km.yes_bid,
    yes_ask: km.yes_ask,
    no_bid: km.no_bid,
    no_ask: km.no_ask,
    last_price: km.last_price,
    volume: km.volume ?? 0,
    volume_24h: km.volume_24h ?? 0,
    open_interest: km.open_interest ?? 0,
  };
}
