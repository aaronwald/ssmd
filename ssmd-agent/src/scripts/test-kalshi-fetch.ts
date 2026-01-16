/**
 * Test script to measure Kalshi API fetch performance
 * Run with: deno run --allow-net --allow-env src/scripts/test-kalshi-fetch.ts
 */

const KALSHI_API_KEY = Deno.env.get("KALSHI_API_KEY");
if (!KALSHI_API_KEY) {
  console.error("KALSHI_API_KEY not set");
  Deno.exit(1);
}

const BASE_URL = "https://api.elections.kalshi.com/trade-api/v2";
const headers = {
  Authorization: `Bearer ${KALSHI_API_KEY}`,
  "Content-Type": "application/json",
};

interface Market {
  ticker: string;
  close_time: string;
  status: string;
}

interface MarketResponse {
  cursor: string;
  markets: Market[];
}

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

async function fetchMarkets(params: string): Promise<{ markets: Market[]; pages: number; durationMs: number }> {
  const start = Date.now();
  const markets: Market[] = [];
  let cursor: string | undefined;
  let pages = 0;

  do {
    const url = cursor
      ? `${BASE_URL}/markets?cursor=${cursor}${params}`
      : `${BASE_URL}/markets?${params.substring(1)}`;

    const res = await fetch(url, { headers });
    if (res.status === 429) {
      console.log("  Rate limited, waiting 10s...");
      await sleep(10000);
      continue;
    }
    if (!res.ok) {
      throw new Error(`API error: ${res.status} ${await res.text()}`);
    }

    const data: MarketResponse = await res.json();
    markets.push(...data.markets);
    cursor = data.cursor || undefined;
    pages++;

    console.log(`  Page ${pages}: ${data.markets.length} markets (total: ${markets.length})`);

    await sleep(100);
  } while (cursor);

  return { markets, pages, durationMs: Date.now() - start };
}

interface Series {
  ticker: string;
  title: string;
  category: string;
}

interface SeriesResponse {
  cursor: string;
  series: Series[];
}

async function fetchSeries(): Promise<{ series: Series[]; pages: number; durationMs: number }> {
  const start = Date.now();
  const series: Series[] = [];
  let cursor: string | undefined;
  let pages = 0;

  do {
    const url = cursor
      ? `${BASE_URL}/series?cursor=${cursor}`
      : `${BASE_URL}/series`;

    const res = await fetch(url, { headers });
    if (res.status === 429) {
      console.log("  Rate limited, waiting 10s...");
      await sleep(10000);
      continue;
    }
    if (!res.ok) {
      throw new Error(`API error: ${res.status} ${await res.text()}`);
    }

    const data: SeriesResponse = await res.json();
    series.push(...data.series);
    cursor = data.cursor || undefined;
    pages++;

    console.log(`  Page ${pages}: ${data.series.length} series (total: ${series.length})`);

    await sleep(100);
  } while (cursor);

  return { series, pages, durationMs: Date.now() - start };
}

async function fetchTagsByCategories(): Promise<unknown> {
  const url = `${BASE_URL}/search/tags_by_categories`;
  const res = await fetch(url, { headers });
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${await res.text()}`);
  }
  return res.json();
}

async function fetchFiltersBySport(): Promise<unknown> {
  const url = `${BASE_URL}/search/filters_by_sport`;
  const res = await fetch(url, { headers });
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${await res.text()}`);
  }
  return res.json();
}

async function fetchSeriesByCategory(category: string, extraParams = ""): Promise<{ series: Series[]; pages: number; durationMs: number }> {
  const start = Date.now();
  const series: Series[] = [];
  let cursor: string | undefined;
  let pages = 0;

  do {
    const url = cursor
      ? `${BASE_URL}/series?cursor=${cursor}&category=${encodeURIComponent(category)}${extraParams}`
      : `${BASE_URL}/series?category=${encodeURIComponent(category)}${extraParams}`;

    const res = await fetch(url, { headers });
    if (res.status === 429) {
      console.log("  Rate limited, waiting 10s...");
      await sleep(10000);
      continue;
    }
    if (!res.ok) {
      throw new Error(`API error: ${res.status} ${await res.text()}`);
    }

    const data: SeriesResponse = await res.json();
    series.push(...data.series);
    cursor = data.cursor || undefined;
    pages++;

    console.log(`  Page ${pages}: ${data.series.length} series (total: ${series.length})`);
    await sleep(100);
  } while (cursor);

  return { series, pages, durationMs: Date.now() - start };
}

async function fetchSeriesWithTags(category: string, tags: string): Promise<{ series: Series[]; durationMs: number }> {
  const start = Date.now();
  const url = `${BASE_URL}/series?category=${encodeURIComponent(category)}&tags=${encodeURIComponent(tags)}`;
  const res = await fetch(url, { headers });
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${await res.text()}`);
  }
  const data: SeriesResponse = await res.json();
  return { series: data.series, durationMs: Date.now() - start };
}

async function fetchMarketsBySeries(seriesTicker: string, status = "open"): Promise<{ markets: Market[]; pages: number; durationMs: number }> {
  const start = Date.now();
  const markets: Market[] = [];
  let cursor: string | undefined;
  let pages = 0;

  do {
    const url = cursor
      ? `${BASE_URL}/markets?cursor=${cursor}&series_ticker=${encodeURIComponent(seriesTicker)}&status=${status}`
      : `${BASE_URL}/markets?series_ticker=${encodeURIComponent(seriesTicker)}&status=${status}`;

    const res = await fetch(url, { headers });
    if (res.status === 429) {
      console.log("  Rate limited, waiting 10s...");
      await sleep(10000);
      continue;
    }
    if (!res.ok) {
      throw new Error(`API error: ${res.status} ${await res.text()}`);
    }

    const data: MarketResponse = await res.json();
    markets.push(...data.markets);
    cursor = data.cursor || undefined;
    pages++;

    console.log(`  Page ${pages}: ${data.markets.length} markets (total: ${markets.length})`);
    await sleep(100);
  } while (cursor);

  return { markets, pages, durationMs: Date.now() - start };
}

async function fetchMarketsBySeriesNoFilter(seriesTicker: string): Promise<{ markets: Market[]; pages: number; durationMs: number }> {
  const start = Date.now();
  const markets: Market[] = [];
  let cursor: string | undefined;
  let pages = 0;

  do {
    const url = cursor
      ? `${BASE_URL}/markets?cursor=${cursor}&series_ticker=${encodeURIComponent(seriesTicker)}`
      : `${BASE_URL}/markets?series_ticker=${encodeURIComponent(seriesTicker)}`;

    const res = await fetch(url, { headers });
    if (res.status === 429) {
      console.log("  Rate limited, waiting 10s...");
      await sleep(10000);
      continue;
    }
    if (!res.ok) {
      throw new Error(`API error: ${res.status} ${await res.text()}`);
    }

    const data: MarketResponse = await res.json();
    markets.push(...data.markets);
    cursor = data.cursor || undefined;
    pages++;

    console.log(`  Page ${pages}: ${data.markets.length} markets (total: ${markets.length})`);
    await sleep(100);
  } while (cursor);

  return { markets, pages, durationMs: Date.now() - start };
}

async function fetchSettledMarketsBySeries(seriesTicker: string, minSettledTs: number): Promise<{ markets: Market[]; pages: number; durationMs: number }> {
  const start = Date.now();
  const markets: Market[] = [];
  let cursor: string | undefined;
  let pages = 0;

  do {
    const url = cursor
      ? `${BASE_URL}/markets?cursor=${cursor}&series_ticker=${encodeURIComponent(seriesTicker)}&status=settled&min_settled_ts=${minSettledTs}`
      : `${BASE_URL}/markets?series_ticker=${encodeURIComponent(seriesTicker)}&status=settled&min_settled_ts=${minSettledTs}`;

    const res = await fetch(url, { headers });
    if (res.status === 429) {
      console.log("  Rate limited, waiting 10s...");
      await sleep(10000);
      continue;
    }
    if (!res.ok) {
      throw new Error(`API error: ${res.status} ${await res.text()}`);
    }

    const data: MarketResponse = await res.json();
    markets.push(...data.markets);
    cursor = data.cursor || undefined;
    pages++;

    console.log(`  Page ${pages}: ${data.markets.length} markets (total: ${markets.length})`);
    await sleep(100);
  } while (cursor);

  return { markets, pages, durationMs: Date.now() - start };
}

async function fetchClosedMarketsBySeries(seriesTicker: string, minCloseTs: number, maxCloseTs: number): Promise<{ markets: Market[]; pages: number; durationMs: number }> {
  const start = Date.now();
  const markets: Market[] = [];
  let cursor: string | undefined;
  let pages = 0;

  do {
    const url = cursor
      ? `${BASE_URL}/markets?cursor=${cursor}&series_ticker=${encodeURIComponent(seriesTicker)}&status=closed&min_close_ts=${minCloseTs}&max_close_ts=${maxCloseTs}`
      : `${BASE_URL}/markets?series_ticker=${encodeURIComponent(seriesTicker)}&status=closed&min_close_ts=${minCloseTs}&max_close_ts=${maxCloseTs}`;

    const res = await fetch(url, { headers });
    if (res.status === 429) {
      console.log("  Rate limited, waiting 10s...");
      await sleep(10000);
      continue;
    }
    if (!res.ok) {
      throw new Error(`API error: ${res.status} ${await res.text()}`);
    }

    const data: MarketResponse = await res.json();
    markets.push(...data.markets);
    cursor = data.cursor || undefined;
    pages++;

    console.log(`  Page ${pages}: ${data.markets.length} markets (total: ${markets.length})`);
    await sleep(100);
  } while (cursor);

  return { markets, pages, durationMs: Date.now() - start };
}

async function main() {
  const gameSeries = [
    "KXNBAGAME",    // NBA
    "KXNFLGAME",    // NFL
    "KXNHLGAME",    // NHL
    "KXNCAAMBGAME", // College Basketball
    "KXEPLGAME",    // EPL
  ];

  const now = Math.floor(Date.now() / 1000);
  const oneDayAgo = now - 24 * 60 * 60;

  console.log("=== Query 1: Open markets by series ===\n");

  for (const series of gameSeries) {
    console.log(`${series}:`);
    const result = await fetchMarketsBySeries(series, "open");
    console.log(`  Open: ${result.markets.length} markets (${result.durationMs}ms)`);
    await sleep(100);
  }

  console.log("\n=== Query 2: Closed in last 24h by series ===\n");

  for (const series of gameSeries) {
    console.log(`${series}:`);
    const result = await fetchClosedMarketsBySeries(series, oneDayAgo, now);
    console.log(`  Closed (24h): ${result.markets.length} markets (${result.durationMs}ms)`);
    await sleep(100);
  }

  console.log("\n=== Query 3: Settled in last 24h by series ===\n");

  for (const series of gameSeries) {
    console.log(`${series}:`);
    const result = await fetchSettledMarketsBySeries(series, oneDayAgo);
    console.log(`  Settled (24h): ${result.markets.length} markets (${result.durationMs}ms)`);
    await sleep(100);
  }
}

main().catch(console.error);
