/**
 * Database module exports
 */
export { getDb, closeDb, withTiming } from "./client.ts";
export { bulkUpsertEvents, getExistingEventTickers, softDeleteMissingEvents } from "./events.ts";
export { bulkUpsertMarkets, softDeleteMissingMarkets, type MarketBulkResult } from "./markets.ts";
export {
  upsertFeeChanges,
  getCurrentFee,
  getFeeAsOf,
  listCurrentFees,
  getFeeStats,
  type FeeSyncResult,
} from "./fees.ts";
