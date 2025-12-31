/**
 * Database module exports
 */
export { getDb, getRawSql, closeDb, type Database } from "./client.ts";

// Schema and types
export {
  events,
  markets,
  seriesFees,
  apiKeys,
  feeTypeEnum,
  type Event,
  type NewEvent,
  type Market,
  type NewMarket,
  type SeriesFee,
  type NewSeriesFee,
  type ApiKey,
  type NewApiKey,
} from "./schema.ts";

// Event operations
export {
  upsertEvents,
  bulkUpsertEvents,  // deprecated
  getExistingEventTickers,
  softDeleteMissingEvents,
  listEvents,
  getEvent,
  getEventStats,
} from "./events.ts";

// Market operations
export {
  upsertMarkets,
  bulkUpsertMarkets,  // deprecated
  softDeleteMissingMarkets,
  listMarkets,
  getMarket,
  getMarketStats,
  type UpsertResult,
} from "./markets.ts";

// Fee operations
export {
  upsertFeeChanges,
  getCurrentFee,
  getFeeAsOf,
  listCurrentFees,
  getFeeStats,
} from "./fees.ts";

// API key operations
export {
  getApiKeyByPrefix,
  createApiKey,
  listApiKeysByUser,
  listAllApiKeys,
  revokeApiKey,
  updateLastUsed,
} from "./apikeys.ts";
