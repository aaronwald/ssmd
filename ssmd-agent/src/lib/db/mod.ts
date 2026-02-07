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
  marketLifecycleEvents,
  pairs,
  pairSnapshots,
  polymarketConditions,
  polymarketTokens,
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
  type MarketLifecycleEvent,
  type NewMarketLifecycleEvent,
  type Pair,
  type NewPair,
  type PairSnapshot,
  type NewPairSnapshot,
  type PolymarketCondition,
  type NewPolymarketCondition,
  type PolymarketToken,
  type NewPolymarketToken,
} from "./schema.ts";

// Event operations
export {
  upsertEvents,
  bulkUpsertEvents,  // deprecated
  upsertEventFromLifecycle,
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
  upsertMarketFromLifecycle,
  updateMarketStatus,
  softDeleteMissingMarkets,
  listMarkets,
  listMarketsWithSnapshot,
  getMarket,
  getMarketStats,
  getMarketTimeseries,
  getActiveMarketsByCategoryTimeseries,
  type UpsertResult,
  type MarketDayActivity,
  type ActiveByCategoryDay,
  type MarketsWithSnapshot,
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

// Lifecycle operations
export {
  insertLifecycleEvent,
  insertLifecycleEvents,
  getLifecycleEventsByMarket,
  getLifecycleEventsByType,
  getRecentLifecycleEvents,
  getLifecycleStats,
} from "./lifecycle.ts";

// Pair operations
export {
  upsertSpotPairs,
  upsertPerpPairs,
  softDeleteMissingPairs,
  listPairs,
  getPair,
  getPairStats,
  insertPerpSnapshots,
  getPairSnapshots,
  cleanupOldSnapshots,
} from "./pairs.ts";

// Polymarket operations
export {
  upsertConditions,
  upsertTokens,
  softDeleteMissingConditions,
  listConditions,
  getCondition,
  getConditionStats,
} from "./polymarket.ts";
