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
  smallint,
  bigint,
  bigserial,
  serial,
  numeric,
  jsonb,
  date,
} from "drizzle-orm/pg-core";

// Fee type enum matching PostgreSQL
export const feeTypeEnum = pgEnum("fee_type", [
  "quadratic",
  "quadratic_with_maker_fees",
  "flat",
]);

// Events table
export const events = pgTable("events", {
  eventTicker: varchar("event_ticker", { length: 128 }).primaryKey(),
  title: text("title").notNull(),
  category: varchar("category", { length: 128 }).notNull().default(""),
  seriesTicker: varchar("series_ticker", { length: 128 }).notNull().default(""),
  strikeDate: timestamp("strike_date", { withTimezone: true }),
  mutuallyExclusive: boolean("mutually_exclusive").notNull().default(false),
  status: varchar("status", { length: 16 }).notNull().default("open"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
});

// Markets table (prices in dollars, 0.00-1.00 range — migration 0026)
export const markets = pgTable("markets", {
  ticker: varchar("ticker", { length: 128 }).primaryKey(),
  eventTicker: varchar("event_ticker", { length: 128 }).notNull()
    .references(() => events.eventTicker),
  title: text("title").notNull(),
  status: varchar("status", { length: 16 }).notNull().default("open"),
  closeTime: timestamp("close_time", { withTimezone: true }),
  yesBid: numeric("yes_bid", { precision: 8, scale: 4 }),
  yesAsk: numeric("yes_ask", { precision: 8, scale: 4 }),
  noBid: numeric("no_bid", { precision: 8, scale: 4 }),
  noAsk: numeric("no_ask", { precision: 8, scale: 4 }),
  lastPrice: numeric("last_price", { precision: 8, scale: 4 }),
  volume: bigint("volume", { mode: "number" }),
  volume24h: bigint("volume_24h", { mode: "number" }),
  openInterest: bigint("open_interest", { mode: "number" }),
  // Enrichment columns (migration 0028)
  floorStrike: numeric("floor_strike"),
  capStrike: numeric("cap_strike"),
  strikeType: varchar("strike_type", { length: 32 }),
  result: varchar("result", { length: 16 }),
  expirationValue: text("expiration_value"),
  yesSubTitle: text("yes_sub_title"),
  noSubTitle: text("no_sub_title"),
  canCloseEarly: boolean("can_close_early"),
  marketType: varchar("market_type", { length: 16 }),
  openTime: timestamp("open_time", { withTimezone: true }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
});

// Series fees table (exclusion constraint lives in SQL migration)
export const seriesFees = pgTable("series_fees", {
  id: serial("id").primaryKey(),
  seriesTicker: varchar("series_ticker", { length: 128 }).notNull(),
  feeType: feeTypeEnum("fee_type").notNull(),
  feeMultiplier: numeric("fee_multiplier", { precision: 6, scale: 4 }).notNull().default("1.0"),
  effectiveFrom: timestamp("effective_from", { withTimezone: true }).notNull(),
  effectiveTo: timestamp("effective_to", { withTimezone: true }),
  sourceId: varchar("source_id", { length: 128 }),
  createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
});

// API keys table for multi-user authentication
export const apiKeys = pgTable("api_keys", {
  id: varchar("id", { length: 36 }).primaryKey(), // UUID as string
  userId: varchar("user_id", { length: 255 }).notNull(),
  userEmail: varchar("user_email", { length: 255 }).notNull(),
  keyPrefix: varchar("key_prefix", { length: 30 }).notNull().unique(),
  keyHash: varchar("key_hash", { length: 64 }).notNull(),
  name: varchar("name", { length: 255 }).notNull(),
  scopes: text("scopes").array().notNull(),
  rateLimitTier: varchar("rate_limit_tier", { length: 20 }).notNull().default("standard"),
  lastUsedAt: timestamp("last_used_at", { withTimezone: true }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  revokedAt: timestamp("revoked_at", { withTimezone: true }),
  expiresAt: timestamp("expires_at", { withTimezone: true }),
  allowedFeeds: text("allowed_feeds").array().notNull(),
  dateRangeStart: date("date_range_start").notNull(),
  dateRangeEnd: date("date_range_end").notNull(),
  billable: boolean("billable").notNull().default(true),
  disabledAt: timestamp("disabled_at", { withTimezone: true }),
});

// Settings table for key-value configuration (e.g., guardrails)
export const settings = pgTable("settings", {
  key: text("key").primaryKey(),
  value: jsonb("value").notNull(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).defaultNow(),
});

// Series table for Kalshi series metadata
export const series = pgTable("series", {
  ticker: varchar("ticker", { length: 128 }).primaryKey(),
  title: text("title").notNull(),
  category: varchar("category", { length: 128 }).notNull(),
  tags: text("tags").array(), // Array of tags from Kalshi API
  isGame: boolean("is_game").notNull().default(false), // For Sports: GAME/MATCH in ticker
  active: boolean("active").notNull().default(true), // Soft disable for filtering
  volume: bigint("volume", { mode: "number" }).notNull().default(0),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
});

// Market lifecycle events from Kalshi market_lifecycle_v2 channel
export const marketLifecycleEvents = pgTable("market_lifecycle_events", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  marketTicker: varchar("market_ticker", { length: 128 }).notNull(),
  eventType: varchar("event_type", { length: 32 }).notNull(), // created, activated, deactivated, close_date_updated, determined, settled
  openTs: timestamp("open_ts", { withTimezone: true }),
  closeTs: timestamp("close_ts", { withTimezone: true }),
  settledTs: timestamp("settled_ts", { withTimezone: true }),
  metadata: jsonb("metadata"),
  receivedAt: timestamp("received_at", { withTimezone: true }).notNull().defaultNow(),
});

// Pairs table for spot + perpetual price tracking
export const pairs = pgTable("pairs", {
  pairId: varchar("pair_id", { length: 128 }).primaryKey(),
  exchange: varchar("exchange", { length: 32 }).notNull(),
  base: varchar("base", { length: 16 }).notNull(),
  quote: varchar("quote", { length: 16 }).notNull(),
  wsName: varchar("ws_name", { length: 32 }).notNull(),
  bid: numeric("bid", { precision: 18, scale: 8 }),
  ask: numeric("ask", { precision: 18, scale: 8 }),
  lastPrice: numeric("last_price", { precision: 18, scale: 8 }),
  volume24h: numeric("volume_24h", { precision: 24, scale: 8 }),
  status: varchar("status", { length: 16 }).default("active"),
  lotDecimals: integer("lot_decimals").default(8),
  pairDecimals: integer("pair_decimals").default(1),
  // New fields (migration 0009)
  marketType: varchar("market_type", { length: 16 }).notNull().default("spot"),
  altname: varchar("altname", { length: 32 }),
  tickSize: numeric("tick_size", { precision: 18, scale: 10 }),
  orderMin: numeric("order_min", { precision: 18, scale: 8 }),
  costMin: numeric("cost_min", { precision: 18, scale: 8 }),
  feeSchedule: jsonb("fee_schedule"),
  // Perpetual-specific
  underlying: varchar("underlying", { length: 32 }),
  contractSize: numeric("contract_size", { precision: 18, scale: 8 }),
  contractType: varchar("contract_type", { length: 32 }),
  markPrice: numeric("mark_price", { precision: 18, scale: 8 }),
  indexPrice: numeric("index_price", { precision: 18, scale: 8 }),
  fundingRate: numeric("funding_rate", { precision: 18, scale: 12 }),
  fundingRatePrediction: numeric("funding_rate_prediction", { precision: 18, scale: 12 }),
  openInterest: numeric("open_interest", { precision: 24, scale: 8 }),
  maxPositionSize: numeric("max_position_size", { precision: 24, scale: 8 }),
  marginLevels: jsonb("margin_levels"),
  tradeable: boolean("tradeable").default(true),
  suspended: boolean("suspended").default(false),
  openingDate: timestamp("opening_date", { withTimezone: true }),
  feeScheduleUid: varchar("fee_schedule_uid", { length: 64 }),
  tags: text("tags").array(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
});

// Pair snapshots for time-series perpetual data (funding rates, mark prices, etc.)
export const pairSnapshots = pgTable("pair_snapshots", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  pairId: varchar("pair_id", { length: 128 }).notNull().references(() => pairs.pairId),
  markPrice: numeric("mark_price", { precision: 18, scale: 8 }),
  indexPrice: numeric("index_price", { precision: 18, scale: 8 }),
  fundingRate: numeric("funding_rate", { precision: 18, scale: 12 }),
  fundingRatePrediction: numeric("funding_rate_prediction", { precision: 18, scale: 12 }),
  openInterest: numeric("open_interest", { precision: 24, scale: 8 }),
  lastPrice: numeric("last_price", { precision: 18, scale: 8 }),
  bid: numeric("bid", { precision: 18, scale: 8 }),
  ask: numeric("ask", { precision: 18, scale: 8 }),
  volume24h: numeric("volume_24h", { precision: 24, scale: 8 }),
  suspended: boolean("suspended").default(false),
  snapshotAt: timestamp("snapshot_at", { withTimezone: true }).notNull().defaultNow(),
});

// Polymarket conditions (prediction markets)
export const polymarketConditions = pgTable("polymarket_conditions", {
  conditionId: varchar("condition_id", { length: 128 }).primaryKey(),
  question: text("question").notNull(),
  slug: varchar("slug", { length: 256 }),
  category: varchar("category", { length: 128 }),
  tags: text("tags").array().notNull().default([]),
  outcomes: text("outcomes").array().notNull().default([]),
  status: varchar("status", { length: 16 }).notNull().default("active"),
  active: boolean("active").notNull().default(true),
  acceptingOrders: boolean("accepting_orders"),
  eventId: varchar("event_id", { length: 32 }),
  negRisk: boolean("neg_risk"),
  description: text("description"),
  orderPriceMinTickSize: numeric("order_price_min_tick_size", { precision: 8, scale: 6 }),
  orderMinSize: numeric("order_min_size", { precision: 12, scale: 2 }),
  endDate: timestamp("end_date", { withTimezone: true }),
  resolutionDate: timestamp("resolution_date", { withTimezone: true }),
  winningOutcome: varchar("winning_outcome", { length: 128 }),
  volume: numeric("volume", { precision: 24, scale: 2 }),
  liquidity: numeric("liquidity", { precision: 24, scale: 2 }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  deletedAt: timestamp("deleted_at", { withTimezone: true }),
});

// Polymarket CLOB tokens (Yes/No per condition)
export const polymarketTokens = pgTable("polymarket_tokens", {
  tokenId: varchar("token_id", { length: 128 }).primaryKey(),
  conditionId: varchar("condition_id", { length: 128 }).notNull().references(() => polymarketConditions.conditionId),
  outcome: varchar("outcome", { length: 128 }).notNull(),
  outcomeIndex: integer("outcome_index").notNull().default(0),
  price: numeric("price", { precision: 8, scale: 4 }),
  bid: numeric("bid", { precision: 8, scale: 4 }),
  ask: numeric("ask", { precision: 8, scale: 4 }),
  volume: numeric("volume", { precision: 24, scale: 2 }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
});

// DQ daily scores table (migration 0014, extended in 0016)
export const dqDailyScores = pgTable("dq_daily_scores", {
  id: serial("id").primaryKey(),
  checkDate: date("check_date").notNull(),
  feed: text("feed").notNull(),
  score: numeric("score", { precision: 5, scale: 2 }).notNull(),
  compositeScore: numeric("composite_score", { precision: 5, scale: 2 }),
  details: jsonb("details").notNull().default({}),
  // Added in migration 0016
  gapCount: integer("gap_count"),
  gapTotalMinutes: numeric("gap_total_minutes", { precision: 10, scale: 2 }),
  coveragePct: numeric("coverage_pct", { precision: 5, scale: 2 }),
  expectedMessages: integer("expected_messages"),
  actualMessages: integer("actual_messages"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
});

// DQ parquet stats table (migration 0016)
export const dqParquetStats = pgTable("dq_parquet_stats", {
  id: serial("id").primaryKey(),
  path: text("path").notNull().unique(),
  feed: text("feed").notNull(),
  msgType: text("msg_type").notNull(),
  date: date("date").notNull(),
  rows: integer("rows").notNull(),
  fileSizeBytes: bigint("file_size_bytes", { mode: "number" }).notNull(),
  compressionRatio: numeric("compression_ratio", { precision: 8, scale: 4 }),
  duplicatesFiltered: integer("duplicates_filtered").notNull().default(0),
  schemaValid: boolean("schema_valid").notNull().default(true),
  nullViolations: jsonb("null_violations").notNull().default({}),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});

// Data access audit log
export const dataAccessLog = pgTable("data_access_log", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  keyPrefix: varchar("key_prefix", { length: 30 }).notNull(),
  userEmail: varchar("user_email", { length: 255 }).notNull(),
  feed: varchar("feed", { length: 64 }).notNull(),
  dateFrom: date("date_from").notNull(),
  dateTo: date("date_to").notNull(),
  msgType: varchar("msg_type", { length: 64 }),
  filesCount: integer("files_count").notNull(),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});

// API request log for billing (only billable keys)
export const apiRequestLog = pgTable("api_request_log", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  keyPrefix: varchar("key_prefix", { length: 30 }).notNull(),
  method: varchar("method", { length: 10 }).notNull(),
  path: varchar("path", { length: 255 }).notNull(),
  statusCode: smallint("status_code").notNull(),
  responseBytes: integer("response_bytes"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});

// API key audit trail
export const apiKeyEvents = pgTable("api_key_events", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  keyPrefix: varchar("key_prefix", { length: 30 }).notNull(),
  eventType: varchar("event_type", { length: 32 }).notNull(),
  actor: varchar("actor", { length: 255 }).notNull(),
  oldValue: jsonb("old_value"),
  newValue: jsonb("new_value"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});

// Durable daily LLM token usage
export const llmUsageDaily = pgTable("llm_usage_daily", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  keyPrefix: varchar("key_prefix", { length: 30 }).notNull(),
  date: date("date").notNull(),
  model: varchar("model", { length: 128 }).notNull(),
  promptTokens: bigint("prompt_tokens", { mode: "number" }).notNull().default(0),
  completionTokens: bigint("completion_tokens", { mode: "number" }).notNull().default(0),
  requests: integer("requests").notNull().default(0),
  costUsd: numeric("cost_usd", { precision: 12, scale: 6 }).notNull().default("0"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
});

// Billing rates: per-endpoint-tier pricing with effective dates
export const billingRates = pgTable("billing_rates", {
  id: serial("id").primaryKey(),
  keyPrefix: varchar("key_prefix", { length: 30 }),
  endpointTier: varchar("endpoint_tier", { length: 64 }).notNull(),
  ratePerRequest: numeric("rate_per_request", { precision: 12, scale: 6 }).notNull().default("0"),
  ratePerMb: numeric("rate_per_mb", { precision: 12, scale: 6 }).notNull().default("0"),
  ratePer1kTokens: numeric("rate_per_1k_tokens", { precision: 12, scale: 6 }).notNull().default("0"),
  effectiveFrom: timestamp("effective_from", { withTimezone: true }).notNull(),
  effectiveTo: timestamp("effective_to", { withTimezone: true }),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});

// Billing ledger: credits and debits per key (all USD)
export const billingLedger = pgTable("billing_ledger", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  keyPrefix: varchar("key_prefix", { length: 30 }).notNull(),
  entryType: varchar("entry_type", { length: 16 }).notNull(),
  amountUsd: numeric("amount_usd", { precision: 12, scale: 6 }).notNull(),
  description: text("description").notNull(),
  referenceMonth: varchar("reference_month", { length: 7 }),
  actor: varchar("actor", { length: 255 }).notNull(),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});

// Inferred types for select/insert
export type Event = typeof events.$inferSelect;
export type NewEvent = typeof events.$inferInsert;
export type Market = typeof markets.$inferSelect;
export type NewMarket = typeof markets.$inferInsert;
export type SeriesFee = typeof seriesFees.$inferSelect;
export type NewSeriesFee = typeof seriesFees.$inferInsert;
export type ApiKey = typeof apiKeys.$inferSelect;
export type NewApiKey = typeof apiKeys.$inferInsert;
export type Setting = typeof settings.$inferSelect;
export type NewSetting = typeof settings.$inferInsert;
export type Series = typeof series.$inferSelect;
export type NewSeries = typeof series.$inferInsert;
export type MarketLifecycleEvent = typeof marketLifecycleEvents.$inferSelect;
export type NewMarketLifecycleEvent = typeof marketLifecycleEvents.$inferInsert;
export type Pair = typeof pairs.$inferSelect;
export type NewPair = typeof pairs.$inferInsert;
export type PairSnapshot = typeof pairSnapshots.$inferSelect;
export type NewPairSnapshot = typeof pairSnapshots.$inferInsert;
export type PolymarketCondition = typeof polymarketConditions.$inferSelect;
export type NewPolymarketCondition = typeof polymarketConditions.$inferInsert;
export type PolymarketToken = typeof polymarketTokens.$inferSelect;
export type NewPolymarketToken = typeof polymarketTokens.$inferInsert;
export type DqDailyScore = typeof dqDailyScores.$inferSelect;
export type NewDqDailyScore = typeof dqDailyScores.$inferInsert;
export type DqParquetStat = typeof dqParquetStats.$inferSelect;
export type NewDqParquetStat = typeof dqParquetStats.$inferInsert;
export type DataAccessLogEntry = typeof dataAccessLog.$inferSelect;
export type NewDataAccessLogEntry = typeof dataAccessLog.$inferInsert;
export type ApiRequestLogEntry = typeof apiRequestLog.$inferSelect;
export type NewApiRequestLogEntry = typeof apiRequestLog.$inferInsert;
export type ApiKeyEvent = typeof apiKeyEvents.$inferSelect;
export type NewApiKeyEvent = typeof apiKeyEvents.$inferInsert;
export type LlmUsageDailyEntry = typeof llmUsageDaily.$inferSelect;
export type NewLlmUsageDailyEntry = typeof llmUsageDaily.$inferInsert;
export type BillingRate = typeof billingRates.$inferSelect;
export type NewBillingRate = typeof billingRates.$inferInsert;
export type BillingLedgerEntry = typeof billingLedger.$inferSelect;
export type NewBillingLedgerEntry = typeof billingLedger.$inferInsert;
