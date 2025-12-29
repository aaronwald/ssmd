/**
 * Database module exports
 */
export { getDb, closeDb, withTiming } from "./client.ts";
export {
  bulkUpsertEvents,
  getExistingEventTickers,
  softDeleteMissingEvents,
  listEvents,
  getEvent,
  getEventStats,
  type EventRow,
} from "./events.ts";
export {
  bulkUpsertMarkets,
  softDeleteMissingMarkets,
  listMarkets,
  getMarket,
  getMarketStats,
  type MarketBulkResult,
  type MarketRow,
} from "./markets.ts";
export {
  upsertFeeChanges,
  getCurrentFee,
  getFeeAsOf,
  listCurrentFees,
  getFeeStats,
  type FeeSyncResult,
} from "./fees.ts";
