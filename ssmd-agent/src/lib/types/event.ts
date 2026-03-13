import { z } from "zod";

/**
 * Kalshi Event schema - represents a prediction market event
 */
export const EventSchema = z.object({
  /** Unique event ticker (e.g., "KXBTC-24DEC31") */
  event_ticker: z.string().min(1),
  /** Event title */
  title: z.string(),
  /** Category (e.g., "Crypto", "Politics") */
  category: z.string(),
  /** Parent series ticker if part of a series */
  series_ticker: z.string().nullable().optional(),
  /** Strike date for resolution */
  strike_date: z.string().nullable().optional(),
  /** Whether markets in this event are mutually exclusive */
  mutually_exclusive: z.boolean().default(false),
  /** Event status */
  status: z.enum(["active", "closed", "settled"]).default("active"),
});

/**
 * Event for database insertion
 */
export const EventInsertSchema = EventSchema.extend({
  /** When the event was created in our system */
  created_at: z.date().optional(),
  /** When the event was last updated */
  updated_at: z.date().optional(),
  /** Soft delete timestamp */
  deleted_at: z.date().nullable().optional(),
});

export type Event = z.infer<typeof EventSchema>;
export type EventInsert = z.infer<typeof EventInsertSchema>;

/**
 * Kalshi API event response shape
 */
export interface KalshiEvent {
  event_ticker: string;
  title: string;
  category: string;
  series_ticker: string | null;
  strike_date: string | null;
  mutually_exclusive: boolean;
  /** Event status from API (e.g., "open", "closed", "settled") */
  status?: string;
  // Additional fields from API that we don't store
  sub_title?: string;
  event_type?: string;
  // Nested markets when with_nested_markets=true
  markets?: import("./market.ts").KalshiMarket[];
}

/**
 * Map Kalshi API event status to our status enum.
 * Kalshi events have 3 statuses (open, closed, settled) used as query filters.
 * The API response does NOT include a status field — status is undefined when
 * parsed from the response, meaning the event was returned by Kalshi and is alive.
 */
function mapEventStatus(status: string | undefined): "active" | "closed" | "settled" {
  switch (status) {
    case "open":
    case "active":
    case undefined:
      return "active";
    case "closed":
      return "closed";
    case "settled":
    case "determined":
    case "finalized":
      return "settled";
    default:
      console.warn(`Unknown Kalshi event status: ${status}, defaulting to active`);
      return "active";
  }
}

/**
 * Convert Kalshi API event to our Event type
 */
export function fromKalshiEvent(ke: KalshiEvent): Event {
  return {
    event_ticker: ke.event_ticker,
    title: ke.title,
    category: ke.category,
    series_ticker: ke.series_ticker ?? null,
    strike_date: ke.strike_date ?? null,
    mutually_exclusive: ke.mutually_exclusive ?? false,
    status: mapEventStatus(ke.status),
  };
}
