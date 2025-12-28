// Datasets handler
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";

export interface Dataset {
  feed: string;
  date: string;
  records: number;
  tickers: number;
  size_mb: number;
}

export interface DatasetManifest {
  feed: string;
  date: string;
  total_records: number;
  tickers: string[];
  total_bytes: number;
  has_gaps: boolean;
}

/**
 * List datasets from a data directory
 */
export async function listDatasets(
  dataDir: string,
  feedFilter?: string,
  fromDate?: string,
  toDate?: string
): Promise<Dataset[]> {
  const datasets: Dataset[] = [];

  try {
    // List feeds (subdirectories)
    for await (const feedEntry of Deno.readDir(dataDir)) {
      if (!feedEntry.isDirectory) continue;
      if (feedFilter && feedEntry.name !== feedFilter) continue;

      const feedPath = join(dataDir, feedEntry.name);

      // List dates (subdirectories)
      for await (const dateEntry of Deno.readDir(feedPath)) {
        if (!dateEntry.isDirectory) continue;

        const date = dateEntry.name;

        // Apply date filter
        if (fromDate && date < fromDate) continue;
        if (toDate && date > toDate) continue;

        const datePath = join(feedPath, date);

        // Try to load manifest
        const manifest = await loadManifest(datePath);
        if (manifest) {
          datasets.push({
            feed: feedEntry.name,
            date,
            records: manifest.total_records,
            tickers: manifest.tickers.length,
            size_mb: manifest.total_bytes / 1024 / 1024,
          });
        } else {
          // No manifest, estimate from files
          const info = await estimateDataset(datePath, feedEntry.name, date);
          if (info) datasets.push(info);
        }
      }
    }
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) throw e;
  }

  // Sort by date descending
  datasets.sort((a, b) => {
    if (a.feed !== b.feed) return a.feed.localeCompare(b.feed);
    return b.date.localeCompare(a.date);
  });

  return datasets;
}

async function loadManifest(datePath: string): Promise<DatasetManifest | null> {
  try {
    const manifestPath = join(datePath, "manifest.json");
    const content = await Deno.readTextFile(manifestPath);
    return JSON.parse(content);
  } catch {
    return null;
  }
}

async function estimateDataset(
  datePath: string,
  feed: string,
  date: string
): Promise<Dataset | null> {
  let totalBytes = 0;
  let fileCount = 0;

  try {
    for await (const file of Deno.readDir(datePath)) {
      if (file.isFile && file.name.endsWith(".jsonl.gz")) {
        const stat = await Deno.stat(join(datePath, file.name));
        totalBytes += stat.size;
        fileCount++;
      }
    }
  } catch {
    return null;
  }

  if (fileCount === 0) return null;

  return {
    feed,
    date,
    records: 0, // Unknown without reading files
    tickers: fileCount,
    size_mb: totalBytes / 1024 / 1024,
  };
}

/**
 * Create HTTP handler for datasets endpoint
 */
export function createDatasetsHandler(dataDir: string) {
  return async (req: Request): Promise<Response> => {
    const url = new URL(req.url);
    const feedFilter = url.searchParams.get("feed") ?? undefined;
    const fromDate = url.searchParams.get("from") ?? undefined;
    const toDate = url.searchParams.get("to") ?? undefined;

    const datasets = await listDatasets(dataDir, feedFilter, fromDate, toDate);

    return new Response(JSON.stringify({ datasets }), {
      headers: { "Content-Type": "application/json" },
    });
  };
}
