/**
 * Drizzle ORM schema definitions
 * Generated from existing PostgreSQL tables, then cleaned up
 */
import {
  pgTable,
  pgEnum,
  varchar,
  text,
  boolean,
  timestamp,
  integer,
  bigint,
  serial,
  numeric,
} from "drizzle-orm/pg-core";

// Fee type enum matching PostgreSQL
export const feeTypeEnum = pgEnum("fee_type", [
  "quadratic",
  "quadratic_with_maker_fees",
  "flat",
]);

// Events table
export const events = pgTable("events", {
  eventTicker: varchar("event_ticker", { length: 64 }).primaryKey(),
  title: text("title").notNull(),
  category: varchar("category", { length: 64 }).notNull().default(""),
  seriesTicker: varchar("series_ticker", { length: 64 }).notNull().default(""),
  strikeDate: timestamp("strike_date", { withTimezone: true }),
  mutuallyExclusive: boolean("mutually_exclusive").notNull().default(false),
  status: varchar("status", { length: 16 }).notNull().default("open"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
});

// Markets table
export const markets = pgTable("markets", {
  ticker: varchar("ticker", { length: 64 }).primaryKey(),
  eventTicker: varchar("event_ticker", { length: 64 }).notNull()
    .references(() => events.eventTicker),
  title: text("title").notNull(),
  status: varchar("status", { length: 16 }).notNull().default("open"),
  closeTime: timestamp("close_time", { withTimezone: true }),
  yesBid: integer("yes_bid"),
  yesAsk: integer("yes_ask"),
  noBid: integer("no_bid"),
  noAsk: integer("no_ask"),
  lastPrice: integer("last_price"),
  volume: bigint("volume", { mode: "number" }),
  volume24h: bigint("volume_24h", { mode: "number" }),
  openInterest: bigint("open_interest", { mode: "number" }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
});

// Series fees table (exclusion constraint lives in SQL migration)
export const seriesFees = pgTable("series_fees", {
  id: serial("id").primaryKey(),
  seriesTicker: varchar("series_ticker", { length: 64 }).notNull(),
  feeType: feeTypeEnum("fee_type").notNull(),
  feeMultiplier: numeric("fee_multiplier", { precision: 6, scale: 4 }).notNull().default("1.0"),
  effectiveFrom: timestamp("effective_from", { withTimezone: true }).notNull(),
  effectiveTo: timestamp("effective_to", { withTimezone: true }),
  sourceId: varchar("source_id", { length: 128 }),
  createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
});

// Inferred types for select/insert
export type Event = typeof events.$inferSelect;
export type NewEvent = typeof events.$inferInsert;
export type Market = typeof markets.$inferSelect;
export type NewMarket = typeof markets.$inferInsert;
export type SeriesFee = typeof seriesFees.$inferSelect;
export type NewSeriesFee = typeof seriesFees.$inferInsert;
