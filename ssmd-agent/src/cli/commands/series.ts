/**
 * Series sync command - sync Kalshi series metadata to PostgreSQL
 *
 * Sync flow:
 * 1. Call /search/tags_by_categories to get tags for the category
 * 2. For each tag, call /series?category=X&tags=Y to get filtered series
 * 3. Upsert to PostgreSQL with category and tags
 */
import { getDb, closeDb, upsertSeries, getSeriesStats, type NewSeries } from "../../lib/db/mod.ts";
import { createKalshiClient, type KalshiSeries } from "../../lib/api/kalshi.ts";

// Categories we sync
const CATEGORIES = ["Economics", "Elections", "Entertainment", "Financials", "Politics", "Sports"];

/**
 * Series sync options
 */
export interface SeriesSyncOptions {
  /** Category to sync (e.g., "Economics", "Sports") */
  category?: string;
  /** For Sports, only sync game series (GAME/MATCH in ticker) */
  gamesOnly?: boolean;
  /** Dry run - don't write to database */
  dryRun?: boolean;
}

/**
 * Series sync result
 */
export interface SeriesSyncResult {
  fetched: number;
  inserted: number;
  updated: number;
  filtered: number;
  durationMs: number;
}

/**
 * Check if a series is a game series (for Sports filtering)
 */
function isGameSeries(ticker: string): boolean {
  const upper = ticker.toUpperCase();
  return upper.includes("GAME") || upper.includes("MATCH");
}

/**
 * Run the series sync
 */
export async function runSeriesSync(options: SeriesSyncOptions = {}): Promise<SeriesSyncResult> {
  const startTime = Date.now();
  const client = createKalshiClient();
  const _db = getDb(); // Initialize DB connection

  const result: SeriesSyncResult = {
    fetched: 0,
    inserted: 0,
    updated: 0,
    filtered: 0,
    durationMs: 0,
  };

  try {
    console.log("\n[Series] Syncing series metadata from Kalshi API...");

    // Fetch tags by category from the API
    console.log("[Series] Fetching tags by category...");
    const tagsByCategoriesResponse = await client.fetchTagsByCategories();
    // Response structure: { tags_by_categories: { "Economics": ["Fed", ...], ... } }
    const tagsByCategories: Record<string, string[] | null> =
      (tagsByCategoriesResponse as { tags_by_categories?: Record<string, string[] | null> }).tags_by_categories ||
      (tagsByCategoriesResponse as Record<string, string[] | null>);

    // Determine which categories to sync
    const categoriesToSync = options.category
      ? [options.category]
      : CATEGORIES;

    const allSeries: NewSeries[] = [];
    const seenTickers = new Set<string>();

    for (const category of categoriesToSync) {
      const tags = tagsByCategories[category];

      if (!tags || tags.length === 0) {
        console.log(`[Series] No tags found for ${category}, fetching all...`);
        // Fetch without tag filter, with volume
        const series = await client.fetchAllSeries(category, undefined, true);
        result.fetched += series.length;

        for (const s of series) {
          if (seenTickers.has(s.ticker)) continue;
          seenTickers.add(s.ticker);

          const isGame = isGameSeries(s.ticker);

          // For Sports with gamesOnly, filter out non-game series
          if (options.gamesOnly && category === "Sports" && !isGame) {
            result.filtered++;
            continue;
          }

          allSeries.push({
            ticker: s.ticker,
            title: s.title,
            category: s.category || category,
            tags: s.tags || [],
            isGame,
            active: true,
            volume: s.volume ?? 0,
          });
        }
        continue;
      }

      console.log(`[Series] ${category} has ${tags.length} tags: ${tags.join(", ")}`);

      for (const tag of tags) {
        console.log(`[Series] Fetching ${category}/${tag}...`);
        const series = await client.fetchAllSeries(category, tag, true);
        result.fetched += series.length;

        for (const s of series) {
          // Dedupe by ticker (series can appear in multiple tags)
          if (seenTickers.has(s.ticker)) continue;
          seenTickers.add(s.ticker);

          const isGame = isGameSeries(s.ticker);

          // For Sports with gamesOnly, filter out non-game series
          if (options.gamesOnly && category === "Sports" && !isGame) {
            result.filtered++;
            continue;
          }

          allSeries.push({
            ticker: s.ticker,
            title: s.title,
            category: s.category || category,
            tags: s.tags || [tag],
            isGame,
            active: true,
            volume: s.volume ?? 0,
          });
        }
      }
    }

    console.log(`[Series] Fetched ${result.fetched} series, unique: ${allSeries.length}, filtered: ${result.filtered}`);

    if (!options.dryRun && allSeries.length > 0) {
      console.log(`[Series] Upserting ${allSeries.length} series to database...`);
      const dbResult = await upsertSeries(allSeries);
      result.inserted = dbResult.inserted;
      result.updated = dbResult.updated;
    } else if (options.dryRun) {
      console.log("[Series] Dry run - skipping database writes");
    }

    result.durationMs = Date.now() - startTime;
    return result;
  } finally {
    await closeDb();
  }
}

/**
 * Print sync summary
 */
export function printSyncSummary(result: SeriesSyncResult): void {
  console.log("\n=== Series Sync Summary ===");
  console.log(`Fetched:  ${result.fetched}`);
  console.log(`Filtered: ${result.filtered}`);
  console.log(`Inserted: ${result.inserted}`);
  console.log(`Updated:  ${result.updated}`);
  console.log(`Duration: ${(result.durationMs / 1000).toFixed(2)}s`);
}

/**
 * Show series statistics
 */
export async function showSeriesStats(): Promise<void> {
  const _db = getDb();
  try {
    const stats = await getSeriesStats();

    console.log("\n=== Series Statistics ===");
    console.log("");
    console.log("Category        Total    Games");
    console.log("-".repeat(35));

    let totalAll = 0;
    let totalGames = 0;

    for (const row of stats) {
      const cat = row.category.padEnd(14);
      const total = String(row.total).padStart(6);
      const games = String(row.games).padStart(8);
      console.log(`${cat}  ${total}  ${games}`);
      totalAll += row.total;
      totalGames += row.games;
    }

    console.log("-".repeat(35));
    console.log(`${"Total".padEnd(14)}  ${String(totalAll).padStart(6)}  ${String(totalGames).padStart(8)}`);
  } finally {
    await closeDb();
  }
}

/**
 * Handle series subcommands
 */
export async function handleSeries(
  subcommand: string,
  flags: Record<string, unknown>
): Promise<void> {
  switch (subcommand) {
    case "sync": {
      const options: SeriesSyncOptions = {
        category: flags.category as string | undefined,
        gamesOnly: Boolean(flags["games-only"]),
        dryRun: Boolean(flags["dry-run"]),
      };

      try {
        const result = await runSeriesSync(options);
        printSyncSummary(result);
      } catch (e) {
        console.error(`Series sync failed: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    case "stats": {
      try {
        await showSeriesStats();
      } catch (e) {
        console.error(`Failed to get stats: ${(e as Error).message}`);
        Deno.exit(1);
      }
      break;
    }

    default:
      console.log("Usage: ssmd series <command>");
      console.log();
      console.log("Commands:");
      console.log("  sync         Sync series metadata from Kalshi API");
      console.log("  stats        Show series statistics by category");
      console.log();
      console.log("Options for sync:");
      console.log("  --category=X   Sync specific category (e.g., Economics, Sports)");
      console.log("  --games-only   Only sync game series (Sports)");
      console.log("  --dry-run      Fetch but don't write to database");
      console.log();
      console.log("Examples:");
      console.log("  ssmd series sync                           # Sync all categories");
      console.log("  ssmd series sync --category=Economics      # Sync Economics only");
      console.log("  ssmd series sync --category=Sports --games-only  # Sports games");
      Deno.exit(1);
  }
}
