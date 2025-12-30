/**
 * Database module exports
 */
export { getDb, getRawSql, closeDb, type Database } from "./client.ts";

// Schema and types
export {
  events,
  markets,
  seriesFees,
  feeTypeEnum,
  type Event,
  type NewEvent,
  type Market,
  type NewMarket,
  type SeriesFee,
  type NewSeriesFee,
} from "./schema.ts";

// Event operations
export {
  bulkUpsertEvents,
  getExistingEventTickers,
  softDeleteMissingEvents,
  listEvents,
  getEvent,
  getEventStats,
  type BulkResult,
} from "./events.ts";

// Market operations
export {
  bulkUpsertMarkets,
  softDeleteMissingMarkets,
  listMarkets,
  getMarket,
  getMarketStats,
} from "./markets.ts";

// Fee operations
export {
  upsertFeeChanges,
  getCurrentFee,
  getFeeAsOf,
  listCurrentFees,
  getFeeStats,
} from "./fees.ts";
