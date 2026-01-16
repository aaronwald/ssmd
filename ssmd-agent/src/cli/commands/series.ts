/**
 * Series sync command - sync Kalshi series metadata to PostgreSQL
 */
import { getDb, closeDb, upsertSeries, getSeriesStats, type NewSeries } from "../../lib/db/mod.ts";
import { createKalshiClient, type KalshiSeries } from "../../lib/api/kalshi.ts";

// Categories we sync
const CATEGORIES = ["Economics", "Elections", "Entertainment", "Financials", "Politics", "Sports"];

// Sports tags to sync (when filtering by tag)
const SPORTS_TAGS = ["Basketball", "Football", "Soccer", "Hockey", "Baseball", "Tennis", "Golf", "Esports"];

/**
 * Series sync options
 */
export interface SeriesSyncOptions {
  /** Filter to specific tags (repeatable) */
  tags?: string[];
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

    // If tags are specified, only sync those tags
    // Otherwise, sync all categories
    const tagsToSync = options.tags?.length ? options.tags : null;

    const allSeries: NewSeries[] = [];

    if (tagsToSync) {
      // Sync specific tags
      console.log(`[Series] Syncing tags: ${tagsToSync.join(", ")}`);

      for (const tag of tagsToSync) {
        // Determine category from tag (Sports tags vs other category tags)
        const isSportsTag = SPORTS_TAGS.includes(tag);
        const category = isSportsTag ? "Sports" : tag;

        console.log(`[Series] Fetching series for ${isSportsTag ? "Sports/" : ""}${tag}...`);

        const series = await client.fetchAllSeries(category, isSportsTag ? tag : undefined);
        result.fetched += series.length;

        for (const s of series) {
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
          });
        }
      }
    } else {
      // Sync all categories
      for (const category of CATEGORIES) {
        console.log(`[Series] Fetching series for ${category}...`);

        if (category === "Sports" && options.gamesOnly) {
          // For Sports with gamesOnly, fetch by each sport tag
          for (const tag of SPORTS_TAGS) {
            const series = await client.fetchAllSeries(category, tag);
            result.fetched += series.length;

            for (const s of series) {
              const isGame = isGameSeries(s.ticker);

              if (!isGame) {
                result.filtered++;
                continue;
              }

              allSeries.push({
                ticker: s.ticker,
                title: s.title,
                category: s.category || category,
                tags: s.tags || [tag],
                isGame: true,
                active: true,
              });
            }
          }
        } else {
          // Fetch all series for this category
          const series = await client.fetchAllSeries(category);
          result.fetched += series.length;

          for (const s of series) {
            const isGame = isGameSeries(s.ticker);

            allSeries.push({
              ticker: s.ticker,
              title: s.title,
              category: s.category || category,
              tags: s.tags || [],
              isGame,
              active: true,
            });
          }
        }
      }
    }

    console.log(`[Series] Fetched ${result.fetched} series, filtered ${result.filtered}`);

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
      // Parse --tag flags (can be multiple)
      const tagFlags = flags.tag;
      const tags: string[] = [];
      if (typeof tagFlags === "string") {
        tags.push(tagFlags);
      } else if (Array.isArray(tagFlags)) {
        tags.push(...tagFlags.map(String));
      }

      const options: SeriesSyncOptions = {
        tags: tags.length > 0 ? tags : undefined,
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
      console.log("  --tag=X      Filter to specific tags (repeatable)");
      console.log("  --games-only Only sync game series (Sports)");
      console.log("  --dry-run    Fetch but don't write to database");
      console.log();
      console.log("Examples:");
      console.log("  ssmd series sync                           # Sync all series");
      console.log("  ssmd series sync --tag=Basketball          # Sync Basketball only");
      console.log("  ssmd series sync --tag=Basketball --games-only  # Basketball games");
      Deno.exit(1);
  }
}
