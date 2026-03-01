/**
 * Secmaster sync command - sync Kalshi events and markets to PostgreSQL
 */
import { getDb, closeDb } from "../../lib/db/client.ts";
import { bulkUpsertEvents, softDeleteMissingEvents, upsertEvents } from "../../lib/db/events.ts";
import { bulkUpsertMarkets, softDeleteMissingMarkets } from "../../lib/db/markets.ts";
import { getAllActiveSeries, getSeriesByTags, getSeriesByCategory } from "../../lib/db/series.ts";
import { getSettingValue, upsertSetting } from "../../lib/db/settings.ts";
import { createKalshiClient } from "../../lib/api/kalshi.ts";

const API_TIMEOUT_MS = 10000;


function getApiUrl(): string {
  return Deno.env.get("SSMD_API_URL") ?? "http://localhost:8080";
}

function getApiKey(): string {
  return Deno.env.get("SSMD_DATA_API_KEY") ?? "";
}

async function apiRequest<T>(path: string): Promise<T> {
  const res = await fetch(`${getApiUrl()}${path}`, {
    headers: { "X-API-Key": getApiKey() },
    signal: AbortSignal.timeout(API_TIMEOUT_MS),
  });
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${await res.text()}`);
  }
  return res.json();
}

/**
 * Secmaster sync options
 */
export interface SyncOptions {
  /** Sync only events, skip markets */
  eventsOnly?: boolean;
  /** Sync only markets, skip events */
  marketsOnly?: boolean;
  /** Skip soft-deleting missing records */
  noDelete?: boolean;
  /** Dry run - don't write to database */
  dryRun?: boolean;
  /** Only sync active/open records (faster incremental sync) */
  activeOnly?: boolean;
  /** Use series-based sync (requires series table to be populated) */
  bySeries?: boolean;
  /** Filter to specific category (for series-based sync) */
  category?: string;
  /** Filter to specific tags (for series-based sync) */
  tags?: string[];
  /** Minimum volume threshold for series (filters low-activity series) */
  minVolume?: number;
  /** Minimum close timestamp - only sync markets closing after this (Unix seconds) */
  minCloseTs?: number;
  /** Relative filter: only sync markets closing within N days ago (converted to minCloseTs at runtime) */
  minCloseDaysAgo?: number;
}

/**
 * Sync result statistics
 */
export interface SyncResult {
  events: {
    fetched: number;
    upserted: number;
    deleted: number;
    durationMs: number;
  };
  markets: {
    fetched: number;
    upserted: number;
    skipped: number;
    deleted: number;
    durationMs: number;
  };
  totalDurationMs: number;
}

/**
 * Run the secmaster sync
 */
export async function runSecmasterSync(options: SyncOptions = {}): Promise<SyncResult> {
  const startTime = Date.now();
  const client = createKalshiClient();
  const db = getDb();

  const result: SyncResult = {
    events: { fetched: 0, upserted: 0, deleted: 0, durationMs: 0 },
    markets: { fetched: 0, upserted: 0, skipped: 0, deleted: 0, durationMs: 0 },
    totalDurationMs: 0,
  };

  // Status filter for incremental sync
  const statusFilter = options.activeOnly ? "open" : undefined;
  const syncMode = options.activeOnly ? "incremental (active only)" : "full";

  try {
    // Sync events - upsert each batch as it arrives
    if (!options.marketsOnly) {
      console.log(`\n[Events] Starting ${syncMode} sync...`);
      const eventStart = Date.now();

      const allEventTickers: string[] = [];
      let batchCount = 0;

      for await (const batch of client.fetchAllEvents(statusFilter)) {
        result.events.fetched += batch.length;
        allEventTickers.push(...batch.map((e) => e.event_ticker));

        if (!options.dryRun) {
          await bulkUpsertEvents(db, batch);
          batchCount++;
        }
      }

      result.events.upserted = result.events.fetched;
      console.log(`[Events] Synced ${result.events.fetched} events in ${batchCount} batches`);

      // Skip soft-delete for incremental sync (we only fetched a subset)
      if (!options.dryRun && !options.noDelete && !options.activeOnly) {
        const deleted = await softDeleteMissingEvents(db, allEventTickers);
        result.events.deleted = deleted;
        if (deleted > 0) {
          console.log(`[Events] Soft-deleted ${deleted} missing events`);
        }
      }

      result.events.durationMs = Date.now() - eventStart;
    }

    // Sync markets - upsert each batch as it arrives
    if (!options.eventsOnly) {
      console.log(`\n[Markets] Starting ${syncMode} sync...`);
      const marketStart = Date.now();

      const allMarketTickers: string[] = [];
      let batchCount = 0;

      if (options.activeOnly) {
        // Incremental sync: fetch markets in multiple passes
        const now = Math.floor(Date.now() / 1000);
        const sevenDaysAgo = now - 7 * 24 * 60 * 60;
        const fortyEightHoursFromNow = now + 48 * 60 * 60;

        // Anchor-based lookback for settled/closed: use last successful sync timestamp
        // Falls back to 7 days ago on first run (covers any backlog)
        const SETTLED_ANCHOR_KEY = "secmaster.sync.last_settled_ts";
        const CLOSED_ANCHOR_KEY = "secmaster.sync.last_closed_ts";
        const settledAnchor = await getSettingValue<number>(db, SETTLED_ANCHOR_KEY, sevenDaysAgo);
        const closedAnchor = await getSettingValue<number>(db, CLOSED_ANCHOR_KEY, sevenDaysAgo);

        // Pass 1: Markets closing in next 48 hours (any status)
        // This captures all markets relevant for closeWithinHours connectors
        // including sports games that were created weeks ago
        console.log(`  Fetching markets closing in next 48 hours (any status)...`);
        for await (const batch of client.fetchAllMarkets({
          minCloseTs: now,
          maxCloseTs: fortyEightHoursFromNow,
          mveFilter: "exclude",
        })) {
          result.markets.fetched += batch.length;
          allMarketTickers.push(...batch.map((m) => m.ticker));

          if (!options.dryRun) {
            const batchResult = await bulkUpsertMarkets(db, batch);
            result.markets.upserted += batchResult.total;
            result.markets.skipped += batchResult.skipped;
            batchCount++;
          }
        }

        // Pass 2: Recently created open markets (for connectors without closeWithinHours)
        console.log(`  Fetching open markets created in last 7 days...`);
        for await (const batch of client.fetchAllMarkets({
          status: "open",
          minCreatedTs: sevenDaysAgo,
          mveFilter: "exclude",
        })) {
          result.markets.fetched += batch.length;
          allMarketTickers.push(...batch.map((m) => m.ticker));

          if (!options.dryRun) {
            const batchResult = await bulkUpsertMarkets(db, batch);
            result.markets.upserted += batchResult.total;
            result.markets.skipped += batchResult.skipped;
            batchCount++;
          }
        }

        // Pass 3: Settled markets since last sync (anchor-based)
        console.log(`  Fetching settled markets since ${new Date(settledAnchor * 1000).toISOString()}...`);
        for await (const batch of client.fetchAllMarkets({
          status: "settled",
          minSettledTs: settledAnchor,
          mveFilter: "exclude",
        })) {
          result.markets.fetched += batch.length;
          allMarketTickers.push(...batch.map((m) => m.ticker));

          if (!options.dryRun) {
            const batchResult = await bulkUpsertMarkets(db, batch);
            result.markets.upserted += batchResult.total;
            result.markets.skipped += batchResult.skipped;
            batchCount++;
          }
        }

        // Pass 4: Closed markets since last sync (anchor-based)
        console.log(`  Fetching closed markets since ${new Date(closedAnchor * 1000).toISOString()}...`);
        for await (const batch of client.fetchAllMarkets({
          status: "closed",
          minCloseTs: closedAnchor,
          maxCloseTs: now,
          mveFilter: "exclude",
        })) {
          result.markets.fetched += batch.length;
          allMarketTickers.push(...batch.map((m) => m.ticker));

          if (!options.dryRun) {
            const batchResult = await bulkUpsertMarkets(db, batch);
            result.markets.upserted += batchResult.total;
            result.markets.skipped += batchResult.skipped;
            batchCount++;
          }
        }

        // Update anchors on successful sync (not dry-run)
        if (!options.dryRun) {
          await upsertSetting(db, SETTLED_ANCHOR_KEY, now);
          await upsertSetting(db, CLOSED_ANCHOR_KEY, now);
        }
      } else {
        // Full sync: fetch all markets
        for await (const batch of client.fetchAllMarkets()) {
          result.markets.fetched += batch.length;
          allMarketTickers.push(...batch.map((m) => m.ticker));

          if (!options.dryRun) {
            const batchResult = await bulkUpsertMarkets(db, batch);
            result.markets.upserted += batchResult.total;
            result.markets.skipped += batchResult.skipped;
            batchCount++;
          }
        }
      }

      console.log(
        `[Markets] Synced ${result.markets.upserted} markets in ${batchCount} batches` +
          (result.markets.skipped > 0 ? ` (${result.markets.skipped} skipped)` : "")
      );

      // Skip soft-delete for incremental sync (we only fetched a subset)
      if (!options.dryRun && !options.noDelete && !options.activeOnly) {
        const deleted = await softDeleteMissingMarkets(db, allMarketTickers);
        result.markets.deleted = deleted;
        if (deleted > 0) {
          console.log(`[Markets] Soft-deleted ${deleted} missing markets`);
        }
      }

      result.markets.durationMs = Date.now() - marketStart;
    }

    result.totalDurationMs = Date.now() - startTime;
    return result;
  } finally {
    await closeDb();
  }
}

/**
 * Run series-based secmaster sync (faster, targeted approach)
 * Uses fetchEventsBySeries with nested markets for efficiency.
 *
 * Error handling:
 * - Per-series try/catch: continues on single failure
 * - Fail-fast: aborts after 3 consecutive failures (API likely down)
 * - Progress markers: PROGRESS:series:N/total:ticker for Temporal heartbeats
 */
export async function runSeriesBasedSync(options: SyncOptions = {}): Promise<SyncResult> {
  const startTime = Date.now();
  const client = createKalshiClient();
  const db = getDb();

  const result: SyncResult = {
    events: { fetched: 0, upserted: 0, deleted: 0, durationMs: 0 },
    markets: { fetched: 0, upserted: 0, skipped: 0, deleted: 0, durationMs: 0 },
    totalDurationMs: 0,
  };

  // Track errors for fail-fast behavior
  let consecutiveErrors = 0;
  const errors: Array<{ ticker: string; error: string }> = [];
  const FAIL_FAST_THRESHOLD = 3;

  // Convert minCloseDaysAgo to minCloseTs if provided
  if (options.minCloseDaysAgo && !options.minCloseTs) {
    const now = Math.floor(Date.now() / 1000);
    options.minCloseTs = now - options.minCloseDaysAgo * 24 * 60 * 60;
    console.log(`[Filter] minCloseDaysAgo=${options.minCloseDaysAgo} → minCloseTs=${options.minCloseTs} (${new Date(options.minCloseTs * 1000).toISOString()})`);
  }

  try {
    // Get series from database (filtered by category, tags, and/or minVolume)
    let seriesList: Array<{ ticker: string }>;
    const volFilter = options.minVolume ? `, minVolume=${options.minVolume}` : "";

    if (options.category) {
      const isGamesOnly = options.category === "Sports";
      console.log(`\n[Series] Fetching series for category: ${options.category}${isGamesOnly ? " (games only)" : ""}${volFilter}`);
      seriesList = await getSeriesByCategory(options.category, isGamesOnly, options.minVolume);
    } else if (options.tags && options.tags.length > 0) {
      console.log(`\n[Series] Fetching series for tags: ${options.tags.join(", ")}${volFilter}`);
      seriesList = await getSeriesByTags(options.tags, false);
    } else {
      console.log(`\n[Series] Fetching all active series from database...${volFilter}`);
      seriesList = await getAllActiveSeries(options.minVolume);
    }

    console.log(`[Series] Found ${seriesList.length} series to sync`);

    if (seriesList.length === 0) {
      console.log("[Series] No series found. Run 'ssmd series sync' first.");
      console.log(`PROGRESS:complete:events=0,markets=0,errors=0`);
      result.totalDurationMs = Date.now() - startTime;
      return result;
    }

    // Sync each series: fetch events with nested markets, upsert events first, then markets
    // Sync open, closed, and settled events
    const statuses: Array<"open" | "closed" | "settled"> = ["open", "closed", "settled"];
    const total = seriesList.length;

    for (let i = 0; i < seriesList.length; i++) {
      const s = seriesList[i];

      // Emit progress marker for Temporal heartbeat
      console.log(`PROGRESS:series:${i + 1}/${total}:${s.ticker}`);

      try {
        for (const status of statuses) {
          // min_close_ts is incompatible with status=open (silently returns 0 events).
          // Apply it to closed and settled to avoid fetching entire history.
          const filters = (status !== "open" && options.minCloseTs)
            ? { minCloseTs: options.minCloseTs }
            : undefined;
          for await (const batch of client.fetchEventsBySeries(s.ticker, status, filters)) {
            result.events.fetched += batch.events.length;
            result.markets.fetched += batch.markets.length;

            if (!options.dryRun) {
              // Upsert events first (FK constraint)
              if (batch.events.length > 0) {
                result.events.upserted += await upsertEvents(db, batch.events);
              }
              // Then upsert markets
              if (batch.markets.length > 0) {
                const marketResult = await bulkUpsertMarkets(db, batch.markets);
                result.markets.upserted += marketResult.total;
              }
            }
          }
        }
        // Success - reset consecutive error counter
        consecutiveErrors = 0;
      } catch (err) {
        consecutiveErrors++;
        const errorMsg = String(err).slice(0, 200);
        errors.push({ ticker: s.ticker, error: errorMsg });
        console.log(`PROGRESS:error:${s.ticker}:${errorMsg}`);

        // Fail-fast: abort after N consecutive failures
        if (consecutiveErrors >= FAIL_FAST_THRESHOLD) {
          console.log(`PROGRESS:fatal:${FAIL_FAST_THRESHOLD} consecutive failures - aborting`);
          throw new Error(
            `Aborting: ${FAIL_FAST_THRESHOLD} consecutive failures. Last error on ${s.ticker}: ${errorMsg}`
          );
        }
      }
    }

    result.totalDurationMs = Date.now() - startTime;

    // Emit completion marker
    console.log(`PROGRESS:complete:events=${result.events.upserted},markets=${result.markets.upserted},errors=${errors.length}`);

    if (errors.length > 0) {
      console.log(`\n[Warning] ${errors.length} series failed (non-consecutive):`);
      for (const e of errors.slice(0, 10)) {
        console.log(`  - ${e.ticker}: ${e.error.slice(0, 80)}`);
      }
      if (errors.length > 10) {
        console.log(`  ... and ${errors.length - 10} more`);
      }
    }

    console.log(
      `\n[Done] Synced ${result.events.upserted} events, ${result.markets.upserted} markets`
    );

    return result;
  } finally {
    await closeDb();
  }
}

/**
 * Print sync summary
 */
export function printSyncSummary(result: SyncResult): void {
  console.log("\n=== Secmaster Sync Summary ===");

  if (result.events.fetched > 0) {
    console.log(`Events:`);
    console.log(`  Fetched:  ${result.events.fetched}`);
    console.log(`  Upserted: ${result.events.upserted}`);
    console.log(`  Deleted:  ${result.events.deleted}`);
    console.log(`  Duration: ${(result.events.durationMs / 1000).toFixed(2)}s`);
  }

  if (result.markets.fetched > 0) {
    console.log(`Markets:`);
    console.log(`  Fetched:  ${result.markets.fetched}`);
    console.log(`  Upserted: ${result.markets.upserted}`);
    console.log(`  Skipped:  ${result.markets.skipped}`);
    console.log(`  Deleted:  ${result.markets.deleted}`);
    console.log(`  Duration: ${(result.markets.durationMs / 1000).toFixed(2)}s`);
  }

  console.log(`\nTotal time: ${(result.totalDurationMs / 1000).toFixed(2)}s`);
}

/**
 * Stats response from API
 */
interface SecmasterStats {
  events: {
    total: number;
    by_status: Record<string, number>;
    by_category: Record<string, number>;
  };
  markets: {
    total: number;
    by_status: Record<string, number>;
  };
}

/**
 * Active by category timeseries response
 */
interface ActiveByCategoryResponse {
  timeseries: Array<{
    date: string;
    categories: Record<string, number>;
    total: number;
  }>;
}

/**
 * Show secmaster statistics
 */
async function showStats(days?: number): Promise<void> {
  const stats = await apiRequest<SecmasterStats>("/v1/secmaster/stats");

  console.log("\n=== Secmaster Statistics ===\n");

  console.log("Events:");
  console.log(`  Total: ${stats.events.total}`);
  if (Object.keys(stats.events.by_status).length > 0) {
    console.log("  By status:");
    for (const [status, count] of Object.entries(stats.events.by_status)) {
      console.log(`    ${status}: ${count}`);
    }
  }
  if (Object.keys(stats.events.by_category).length > 0) {
    console.log("  Top categories:");
    for (const [category, count] of Object.entries(stats.events.by_category)) {
      console.log(`    ${category}: ${count}`);
    }
  }

  console.log("\nMarkets:");
  console.log(`  Total: ${stats.markets.total}`);
  if (Object.keys(stats.markets.by_status).length > 0) {
    console.log("  By status:");
    for (const [status, count] of Object.entries(stats.markets.by_status)) {
      console.log(`    ${status}: ${count}`);
    }
  }

  // Show active markets by category over time
  if (days && days > 0) {
    await showActiveByCategory(days);
  }
}

/**
 * Show active markets by category over time as a table
 */
async function showActiveByCategory(days: number): Promise<void> {
  const data = await apiRequest<ActiveByCategoryResponse>(
    `/v1/secmaster/markets/active-by-category?days=${days}`
  );

  if (data.timeseries.length === 0) {
    console.log("\nNo active market history data available.");
    return;
  }

  // Collect all unique categories across all days
  const allCategories = new Set<string>();
  for (const day of data.timeseries) {
    for (const cat of Object.keys(day.categories)) {
      allCategories.add(cat);
    }
  }

  // Sort categories by total count in most recent day (descending)
  const lastDay = data.timeseries[data.timeseries.length - 1];
  const sortedCategories = Array.from(allCategories).sort((a, b) => {
    return (lastDay.categories[b] || 0) - (lastDay.categories[a] || 0);
  });

  // Calculate column widths
  const dateWidth = 10; // YYYY-MM-DD
  const categoryWidths: Record<string, number> = {};
  for (const cat of sortedCategories) {
    // Width is max of category name or largest number
    let maxVal = 0;
    for (const day of data.timeseries) {
      maxVal = Math.max(maxVal, day.categories[cat] || 0);
    }
    categoryWidths[cat] = Math.max(cat.length, String(maxVal).length);
  }
  const totalWidth = Math.max(5, String(lastDay.total).length);

  // Print header
  console.log(`\n=== Active Markets by Category (Last ${days} Days) ===\n`);

  let header = "Date".padEnd(dateWidth);
  for (const cat of sortedCategories) {
    header += " | " + cat.padStart(categoryWidths[cat]);
  }
  header += " | " + "Total".padStart(totalWidth);
  console.log(header);

  // Print separator
  let separator = "-".repeat(dateWidth);
  for (const cat of sortedCategories) {
    separator += "-+-" + "-".repeat(categoryWidths[cat]);
  }
  separator += "-+-" + "-".repeat(totalWidth);
  console.log(separator);

  // Print rows
  for (const day of data.timeseries) {
    let row = day.date.padEnd(dateWidth);
    for (const cat of sortedCategories) {
      const val = day.categories[cat] || 0;
      row += " | " + String(val).padStart(categoryWidths[cat]);
    }
    row += " | " + String(day.total).padStart(totalWidth);
    console.log(row);
  }
}

/**
 * Event row from API (camelCase to match API response)
 */
interface EventRow {
  eventTicker: string;
  title: string;
  category: string;
  seriesTicker: string | null;
  status: string;
  updatedAt: string;
  marketCount: number;
}

/**
 * Market row from API (camelCase to match API response)
 */
interface MarketRow {
  ticker: string;
  eventTicker: string;
  title: string;
  status: string;
  closeTime: string | null;
  lastPrice: number;
  volume24h: number;
  updatedAt: string;
}

/**
 * List events
 */
async function listEvents(flags: Record<string, unknown>): Promise<void> {
  const params = new URLSearchParams();
  if (flags.category) params.set("category", String(flags.category));
  if (flags.status) params.set("status", String(flags.status));
  if (flags.series) params.set("series", String(flags.series));
  if (flags.limit) params.set("limit", String(flags.limit));

  const url = `/v1/events${params.toString() ? "?" + params : ""}`;
  const { events } = await apiRequest<{ events: EventRow[] }>(url);

  console.log(`\nFound ${events.length} events\n`);

  if (events.length === 0) return;

  // Calculate column widths
  const tickerWidth = Math.max(12, ...events.map(e => e.eventTicker.length));
  const titleWidth = Math.min(45, Math.max(10, ...events.map(e => e.title.length)));
  const catWidth = Math.max(8, ...events.map(e => e.category.length));

  // Header
  console.log(
    "TICKER".padEnd(tickerWidth) + "  " +
    "TITLE".padEnd(titleWidth) + "  " +
    "CATEGORY".padEnd(catWidth) + "  " +
    "MKTS".padStart(4) + "  " +
    "STATUS"
  );
  console.log("-".repeat(tickerWidth + titleWidth + catWidth + 30));

  // Rows
  for (const e of events) {
    const title = e.title.length > titleWidth ? e.title.slice(0, titleWidth - 3) + "..." : e.title;
    const mkts = e.marketCount !== undefined ? String(e.marketCount) : "-";
    console.log(
      e.eventTicker.padEnd(tickerWidth) + "  " +
      title.padEnd(titleWidth) + "  " +
      e.category.padEnd(catWidth) + "  " +
      mkts.padStart(4) + "  " +
      e.status
    );
  }
}

/**
 * List markets
 */
async function listMarkets(flags: Record<string, unknown>): Promise<void> {
  const params = new URLSearchParams();
  if (flags.category) params.set("category", String(flags.category));
  if (flags.status) params.set("status", String(flags.status));
  if (flags.series) params.set("series", String(flags.series));
  if (flags.event) params.set("event", String(flags.event));
  if (flags.limit) params.set("limit", String(flags.limit));

  const url = `/v1/markets${params.toString() ? "?" + params : ""}`;
  const { markets } = await apiRequest<{ markets: MarketRow[] }>(url);

  console.log(`\nFound ${markets.length} markets\n`);

  if (markets.length === 0) return;

  // Calculate column widths
  const tickerWidth = Math.max(12, ...markets.map(m => m.ticker.length));
  const titleWidth = Math.min(40, Math.max(10, ...markets.map(m => m.title.length)));

  // Header
  console.log(
    "TICKER".padEnd(tickerWidth) + "  " +
    "TITLE".padEnd(titleWidth) + "  " +
    "LAST".padStart(6) + "  " +
    "VOL24H".padStart(10) + "  " +
    "STATUS"
  );
  console.log("-".repeat(tickerWidth + titleWidth + 40));

  // Rows
  for (const m of markets) {
    const title = m.title.length > titleWidth ? m.title.slice(0, titleWidth - 3) + "..." : m.title;
    const lastPrice = m.lastPrice !== null ? `$${Number(m.lastPrice).toFixed(2)}` : "-";
    const vol = m.volume24h !== null ? m.volume24h.toLocaleString() : "-";
    console.log(
      m.ticker.padEnd(tickerWidth) + "  " +
      title.padEnd(titleWidth) + "  " +
      lastPrice.padStart(6) + "  " +
      vol.padStart(10) + "  " +
      m.status
    );
  }
}

/**
 * Show a single event
 */
async function showEvent(ticker: string): Promise<void> {
  const event = await apiRequest<EventRow & { marketCount: number }>(`/v1/events/${encodeURIComponent(ticker)}`);

  console.log("\n=== Event Details ===\n");
  console.log(`Ticker:   ${event.eventTicker}`);
  console.log(`Title:    ${event.title}`);
  console.log(`Category: ${event.category}`);
  console.log(`Status:   ${event.status}`);
  if (event.seriesTicker) console.log(`Series:   ${event.seriesTicker}`);
  console.log(`Markets:  ${event.marketCount}`);
  console.log(`Updated:  ${event.updatedAt}`);
}

/**
 * Show a single market
 */
async function showMarket(ticker: string): Promise<void> {
  const m = await apiRequest<MarketRow>(`/v1/markets/${encodeURIComponent(ticker)}`);

  console.log("\n=== Market Details ===\n");
  console.log(`Ticker:     ${m.ticker}`);
  console.log(`Title:      ${m.title}`);
  console.log(`Event:      ${m.eventTicker}`);
  console.log(`Status:     ${m.status}`);
  console.log(`Last Price: $${Number(m.lastPrice).toFixed(2)}`);
  console.log(`Volume 24h: ${m.volume24h?.toLocaleString() ?? "-"}`);
  if (m.closeTime) console.log(`Closes:     ${m.closeTime}`);
  console.log(`Updated:    ${m.updatedAt}`);
}

/**
 * Handle secmaster subcommands
 */
export async function handleSecmaster(
  subcommand: string,
  flags: Record<string, unknown>
): Promise<void> {
  switch (subcommand) {
    case "sync": {
      // Parse --tag flags (can be multiple)
      const tagFlags = flags.tag;
      const tags: string[] = [];
      if (typeof tagFlags === "string") {
        tags.push(tagFlags);
      } else if (Array.isArray(tagFlags)) {
        tags.push(...tagFlags.map(String));
      }

      const options: SyncOptions = {
        eventsOnly: Boolean(flags["events-only"]),
        marketsOnly: Boolean(flags["markets-only"]),
        noDelete: Boolean(flags["no-delete"]),
        dryRun: Boolean(flags["dry-run"]),
        activeOnly: Boolean(flags["active-only"]),
        bySeries: Boolean(flags["by-series"]),
        category: flags.category ? String(flags.category) : undefined,
        tags: tags.length > 0 ? tags : undefined,
        minVolume: flags["min-volume"] ? Number(flags["min-volume"]) : undefined,
        minCloseDaysAgo: flags["min-close-days-ago"] ? Number(flags["min-close-days-ago"]) : undefined,
      };

      if (options.eventsOnly && options.marketsOnly) {
        console.error("Cannot specify both --events-only and --markets-only");
        Deno.exit(1);
      }

      try {
        // Use series-based sync if --by-series flag is set
        if (options.bySeries) {
          const result = await runSeriesBasedSync(options);
          printSyncSummary(result);
        } else {
          const result = await runSecmasterSync(options);
          printSyncSummary(result);
        }
      } catch (e) {
        console.error(`Sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    case "stats": {
      const days = flags.days ? Number(flags.days) : undefined;
      try {
        await showStats(days);
      } catch (e) {
        console.error(`Failed to get stats: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    case "events": {
      const args = flags._ as string[];
      const ticker = args[2]; // flags._[0]=secmaster, _[1]=events, _[2]=ticker
      try {
        if (ticker) {
          await showEvent(ticker);
        } else {
          await listEvents(flags);
        }
      } catch (e) {
        console.error(`Failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    case "markets": {
      const args = flags._ as string[];
      const ticker = args[2]; // flags._[0]=secmaster, _[1]=markets, _[2]=ticker
      try {
        if (ticker) {
          await showMarket(ticker);
        } else {
          await listMarkets(flags);
        }
      } catch (e) {
        console.error(`Failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd secmaster <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync events and markets from Kalshi API");
      console.log("  stats        Show event and market statistics");
      console.log("  events       List events (or show one: events <ticker>)");
      console.log("  markets      List markets (or show one: markets <ticker>)");
      console.log();
      console.log("Options for sync:");
      console.log("  --by-series      Use series-based sync (fast, targeted)");
      console.log("  --category=X     Filter by category (with --by-series)");
      console.log("  --tag=X          Filter to specific tags (with --by-series)");
      console.log("  --min-volume=N   Only sync series with volume >= N");
      console.log("  --min-close-days-ago=N  Only sync markets closing within N days");
      console.log("  --active-only    Only sync active/open records (legacy mode)");
      console.log("  --events-only    Only sync events");
      console.log("  --markets-only   Only sync markets");
      console.log("  --no-delete      Skip soft-deleting missing records");
      console.log("  --dry-run        Fetch but don't write to database");
      console.log();
      console.log("Options for stats:");
      console.log("  --days=N         Show active markets by category over N days");
      console.log();
      console.log("Examples:");
      console.log("  ssmd secmaster sync --by-series --category=Sports    # Sports games");
      console.log("  ssmd secmaster sync --by-series --category=Economics # Economics");
      console.log("  ssmd secmaster stats --days=7                        # Show 7-day history");
      console.log();
      console.log("Options for events/markets:");
      console.log("  --category       Filter by category");
      console.log("  --status         Filter by status");
      console.log("  --series         Filter by series ticker");
      console.log("  --event          Filter markets by event ticker");
      console.log("  --limit          Limit results (default: 100)");
      Deno.exit(1);
  }
}
