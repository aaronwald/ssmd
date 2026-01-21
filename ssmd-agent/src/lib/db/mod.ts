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
  settings,
  series,
  feeTypeEnum,
  type Event,
  type NewEvent,
  type Market,
  type NewMarket,
  type SeriesFee,
  type NewSeriesFee,
  type ApiKey,
  type NewApiKey,
  type Setting,
  type NewSetting,
  type Series,
  type NewSeries,
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
  getMarketTimeseries,
  getActiveMarketsByCategoryTimeseries,
  type UpsertResult,
  type MarketDayActivity,
  type ActiveByCategoryDay,
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

// Settings operations
export {
  getSetting,
  getAllSettings,
  upsertSetting,
  getSettingValue,
} from "./settings.ts";

// Series operations
export {
  upsertSeries,
  getSeriesByTags,
  getSeriesByCategory,
  getAllActiveSeries,
  getSeriesStats,
  getSeries,
  listSeries,
} from "./series.ts";
