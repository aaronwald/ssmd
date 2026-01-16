/**
 * Secmaster sync command - sync Kalshi events and markets to PostgreSQL
 */
import { getDb, closeDb } from "../../lib/db/client.ts";
import { bulkUpsertEvents, softDeleteMissingEvents, upsertEvents } from "../../lib/db/events.ts";
import { bulkUpsertMarkets, softDeleteMissingMarkets } from "../../lib/db/markets.ts";
import { getAllActiveSeries, getSeriesByTags, getSeriesByCategory } from "../../lib/db/series.ts";
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
        const twoDaysAgo = now - 2 * 24 * 60 * 60;
        const fortyEightHoursFromNow = now + 48 * 60 * 60;

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

        // Pass 3: Recently settled markets (status updates)
        console.log(`  Fetching markets settled in last 2 days...`);
        for await (const batch of client.fetchAllMarkets({
          status: "settled",
          minSettledTs: twoDaysAgo,
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

        // Pass 4: Recently closed markets (status updates)
        console.log(`  Fetching markets closed in last 2 days...`);
        for await (const batch of client.fetchAllMarkets({
          status: "closed",
          minCloseTs: twoDaysAgo,
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

  try {
    // Get series from database (filtered by category or tags if specified)
    let seriesList;
    if (options.category) {
      const isGamesOnly = options.category === "Sports";
      console.log(`\n[Series] Fetching series for category: ${options.category}${isGamesOnly ? " (games only)" : ""}`);
      seriesList = await getSeriesByCategory(options.category, isGamesOnly);
    } else if (options.tags && options.tags.length > 0) {
      console.log(`\n[Series] Fetching series for tags: ${options.tags.join(", ")}`);
      seriesList = await getSeriesByTags(options.tags, true); // gamesOnly for sports
    } else {
      console.log(`\n[Series] Fetching all active series from database...`);
      seriesList = await getAllActiveSeries();
    }

    console.log(`[Series] Found ${seriesList.length} series to sync`);

    if (seriesList.length === 0) {
      console.log("[Series] No series found. Run 'ssmd series sync' first.");
      result.totalDurationMs = Date.now() - startTime;
      return result;
    }

    // Sync each series: fetch events with nested markets, upsert events first, then markets
    // Sync open, closed, and settled events
    const statuses: Array<"open" | "closed" | "settled"> = ["open", "closed", "settled"];

    for (const s of seriesList) {
      console.log(`\n[${s.ticker}] Syncing...`);

      for (const status of statuses) {
        for await (const batch of client.fetchEventsBySeries(s.ticker, status)) {
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
    }

    result.totalDurationMs = Date.now() - startTime;

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
 * Show secmaster statistics
 */
async function showStats(): Promise<void> {
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
}

/**
 * Event row from API
 */
interface EventRow {
  event_ticker: string;
  title: string;
  category: string;
  series_ticker: string | null;
  status: string;
  updated_at: string;
}

/**
 * Market row from API
 */
interface MarketRow {
  ticker: string;
  event_ticker: string;
  title: string;
  status: string;
  close_time: string | null;
  last_price: number;
  volume_24h: number;
  updated_at: string;
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

  console.log("\n=== Events ===\n");
  console.log(`Found ${events.length} events\n`);

  for (const e of events) {
    console.log(`${e.event_ticker}`);
    console.log(`  Title: ${e.title}`);
    console.log(`  Category: ${e.category}`);
    console.log(`  Status: ${e.status}`);
    if (e.series_ticker) console.log(`  Series: ${e.series_ticker}`);
    console.log();
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

  console.log("\n=== Markets ===\n");
  console.log(`Found ${markets.length} markets\n`);

  for (const m of markets) {
    console.log(`${m.ticker}`);
    console.log(`  Title: ${m.title}`);
    console.log(`  Event: ${m.event_ticker}`);
    console.log(`  Status: ${m.status}`);
    console.log(`  Last: ${m.last_price}¢  Vol24h: ${m.volume_24h}`);
    if (m.close_time) console.log(`  Closes: ${m.close_time}`);
    console.log();
  }
}

/**
 * Show a single event
 */
async function showEvent(ticker: string): Promise<void> {
  const event = await apiRequest<EventRow & { market_count: number }>(`/v1/events/${encodeURIComponent(ticker)}`);

  console.log("\n=== Event Details ===\n");
  console.log(`Ticker: ${event.event_ticker}`);
  console.log(`Title: ${event.title}`);
  console.log(`Category: ${event.category}`);
  console.log(`Status: ${event.status}`);
  if (event.series_ticker) console.log(`Series: ${event.series_ticker}`);
  console.log(`Markets: ${event.market_count}`);
  console.log(`Updated: ${event.updated_at}`);
}

/**
 * Show a single market
 */
async function showMarket(ticker: string): Promise<void> {
  const m = await apiRequest<MarketRow>(`/v1/markets/${encodeURIComponent(ticker)}`);

  console.log("\n=== Market Details ===\n");
  console.log(`Ticker: ${m.ticker}`);
  console.log(`Title: ${m.title}`);
  console.log(`Event: ${m.event_ticker}`);
  console.log(`Status: ${m.status}`);
  console.log(`Last Price: ${m.last_price}¢`);
  console.log(`Volume 24h: ${m.volume_24h}`);
  if (m.close_time) console.log(`Closes: ${m.close_time}`);
  console.log(`Updated: ${m.updated_at}`);
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
      try {
        await showStats();
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
      console.log("  --active-only    Only sync active/open records (legacy mode)");
      console.log("  --events-only    Only sync events");
      console.log("  --markets-only   Only sync markets");
      console.log("  --no-delete      Skip soft-deleting missing records");
      console.log("  --dry-run        Fetch but don't write to database");
      console.log();
      console.log("Examples:");
      console.log("  ssmd secmaster sync --by-series --category=Sports    # Sports games");
      console.log("  ssmd secmaster sync --by-series --category=Economics # Economics");
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
