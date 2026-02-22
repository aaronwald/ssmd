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
  dqDailyScores,
  dqParquetStats,
  dataAccessLog,
  apiRequestLog,
  apiKeyEvents,
  llmUsageDaily,
  billingRates,
  billingLedger,
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
  type DqDailyScore,
  type NewDqDailyScore,
  type DqParquetStat,
  type NewDqParquetStat,
  type DataAccessLogEntry,
  type NewDataAccessLogEntry,
  type ApiRequestLogEntry,
  type NewApiRequestLogEntry,
  type ApiKeyEvent,
  type NewApiKeyEvent,
  type LlmUsageDailyEntry,
  type NewLlmUsageDailyEntry,
  type BillingRate,
  type NewBillingRate,
  type BillingLedgerEntry,
  type NewBillingLedgerEntry,
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
  seedMissingFees,
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
  updateApiKeyScopes,
  updateLastUsed,
  disableApiKey,
  enableApiKey,
  logKeyEvent,
} from "./apikeys.ts";

// Request log operations
export { RequestLogBuffer, type RequestLogEntry } from "./request-log.ts";

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
  listTokensByCategories,
  getCondition,
  getConditionStats,
} from "./polymarket.ts";

// Health check operations
export {
  listDailyScores,
  getSlaMetrics,
  getGapReports,
} from "./health.ts";

// Access log operations
export {
  logDataAccess,
  listRecentAccess,
  listAccessByUser,
} from "./accesslog.ts";

// Cross-feed market lookup
export {
  lookupMarketsByIds,
  VALID_FEEDS,
  type LookupResult,
} from "./lookup.ts";
