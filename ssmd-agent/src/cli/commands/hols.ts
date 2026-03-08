/**
 * ssmd hols — OHLCV generation from Kraken Spot data.
 *
 * Two independent jobs (never combined):
 *   hols generate   — Fetch OHLCV from Kraken Spot REST OHLC API
 *   hols aggregate   — Generate OHLCV from archived real-time WS trade data
 */
import { getDb, closeDb } from "../../lib/db/mod.ts";
import { listActiveSpotPairs } from "../../lib/db/pairs.ts";
import { DuckDBInstance } from "@duckdb/node-api";
import { Storage } from "@google-cloud/storage";
import nodemailer from "nodemailer";

// --- Kraken Spot REST OHLC ---
const KRAKEN_SPOT_OHLC_URL = "https://api.kraken.com/0/public/OHLC";
const API_TIMEOUT_MS = 15000;
// Kraken public REST: counter=15, replenish 1/sec, cost 1/call.
// Single worker with 1.1s spacing for safe sustained throughput.
const CONCURRENCY = 1;
const RATE_LIMIT_MS = 1100;
const MAX_RETRIES = 3;
const DEFAULT_LOOKBACK_DAYS = 3;
const CANDLES_PER_REQUEST = 720; // Kraken max per OHLC response
const GCS_BUCKET = "ssmd-data";

interface NdjsonRow {
  symbol: string;
  hols_ticker: string;
  source: string;
  date: string;
  date_close: string;
  unix: number;
  close_unix: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  volume_from: number | null;
  tradecount: number | null;
  marketorder_volume: number | null;
  marketorder_volume_from: number | null;
}

interface FetchResult {
  pair: string;
  base: string;
  rowCount: number;
  error?: string;
}

// --- Public API ---

export async function handleHols(
  subcommand: string,
  flags: Record<string, unknown>,
): Promise<void> {
  switch (subcommand) {
    case "generate":
      await runHolsGenerate(flags);
      break;
    case "aggregate":
      await runHolsAggregate(flags);
      break;
    default:
      console.error(`Unknown hols subcommand: ${subcommand ?? "(none)"}`);
      console.log("Usage:");
      console.log("  ssmd hols generate   [--date YYYY-MM-DD] [--days N] [--dry-run]  # Kraken Spot REST OHLC");
      console.log("  ssmd hols aggregate  [--date YYYY-MM-DD] [--days N] [--dry-run]  # Aggregated WS trade data");
      Deno.exit(1);
  }
}

// ============================================================
// Job 1: Kraken Spot REST OHLC
// ============================================================

export async function runHolsGenerate(
  flags: Record<string, unknown>,
): Promise<void> {
  const startTime = Date.now();
  const dryRun = !!flags["dry-run"];

  const { startDate, endDate, startDateStr, endDateStr, lookbackDays } = parseDateRange(flags);

  console.log(`[hols:generate] Kraken Spot REST OHLC for ${startDateStr} to ${endDateStr} (${lookbackDays} days) dry-run=${dryRun}`);

  // 1. Query secmaster for active Kraken spot USDT pairs
  const db = getDb();
  let spotPairs: { krakenPair: string; wsName: string; base: string; quote: string }[];
  try {
    spotPairs = await listActiveSpotPairs(db);
    console.log(`[hols:generate] Found ${spotPairs.length} active Kraken spot USDT pairs`);
  } finally {
    await closeDb();
  }

  if (spotPairs.length === 0) {
    console.error("[hols:generate] No active spot USDT pairs found. Exiting.");
    Deno.exit(1);
  }

  // 2. Fetch OHLCV for each pair, streaming rows to NDJSON
  const ndjsonPath = `/tmp/hols-spot-${endDateStr}.ndjson`;
  const ndjsonFile = await Deno.open(ndjsonPath, { write: true, create: true, truncate: true });
  const encoder = new TextEncoder();
  const writeRow = async (row: NdjsonRow): Promise<void> => {
    await ndjsonFile.write(encoder.encode(JSON.stringify(row) + "\n"));
  };

  console.log(`[hols:generate] Fetching with concurrency=${CONCURRENCY}, rate=${RATE_LIMIT_MS}ms`);
  let results: FetchResult[];
  try {
    results = await fetchAllSpotOhlc(spotPairs, startDate, endDate, writeRow);
  } finally {
    ndjsonFile.close();
  }

  const successes = results.filter((r) => !r.error);
  const failures = results.filter((r) => !!r.error);
  const totalRows = successes.reduce((sum, r) => sum + r.rowCount, 0);

  console.log(`[hols:generate] Fetch complete: ${successes.length} ok, ${failures.length} failed, ${totalRows} total rows`);

  if (totalRows === 0) {
    console.error("[hols:generate] No data fetched. Exiting.");
    Deno.exit(1);
  }

  // 3. Convert to Parquet via DuckDB
  const parquetPath = `/tmp/hols-spot-${endDateStr}.parquet`;
  await convertNdjsonToParquet(ndjsonPath, parquetPath);
  const parquetStat = await Deno.stat(parquetPath);
  const fileSizeKB = Math.round((parquetStat.size ?? 0) / 1024);
  console.log(`[hols:generate] Parquet written: ${parquetPath} (${fileSizeKB} KB)`);

  // 4. Upload to GCS
  const gcsPath = `hols/crypto/daily/${endDateStr}/ohlcv.parquet`;
  if (dryRun) {
    console.log(`[hols:generate] DRY RUN: would upload to gs://${GCS_BUCKET}/${gcsPath}`);
  } else {
    await uploadToGCS(parquetPath, gcsPath);
    console.log(`[hols:generate] Uploaded to gs://${GCS_BUCKET}/${gcsPath}`);
  }

  // 5. Send email report
  const durationSec = Math.round((Date.now() - startTime) / 1000);
  if (!dryRun) {
    await sendReport({
      job: "generate",
      dateStr: `${startDateStr} to ${endDateStr}`,
      symbolCount: spotPairs.length,
      successCount: successes.length,
      failCount: failures.length,
      totalRows,
      fileSizeKB,
      durationSec,
      failures: failures.map((f) => ({ symbol: f.pair, error: f.error! })),
    });
  }

  // 6. Cleanup
  try { await Deno.remove(ndjsonPath); } catch { /* best-effort */ }
  try { await Deno.remove(parquetPath); } catch { /* best-effort */ }

  console.log(`[hols:generate] Done in ${Math.round((Date.now() - startTime) / 1000)}s`);

  const failPct = (failures.length / spotPairs.length) * 100;
  if (failPct > 10) {
    console.error(`[hols:generate] FAIL: ${failures.length}/${spotPairs.length} pairs failed (${failPct.toFixed(1)}%)`);
    Deno.exit(1);
  }
}

// --- Spot OHLC fetch ---

interface KrakenOhlcResponse {
  error: string[];
  result: Record<string, unknown>;
}

async function fetchAllSpotOhlc(
  pairs: { krakenPair: string; wsName: string; base: string; quote: string }[],
  startDate: Date,
  endDate: Date,
  writeRow: (row: NdjsonRow) => Promise<void>,
): Promise<FetchResult[]> {
  const results: FetchResult[] = new Array(pairs.length);
  let completed = 0;
  let queue = 0;

  async function worker(): Promise<void> {
    while (true) {
      const idx = queue++;
      if (idx >= pairs.length) break;

      const pair = pairs[idx];
      const result = await fetchSpotOhlcWithPagination(pair, startDate, endDate, writeRow);
      results[idx] = result;
      completed++;

      if (result.error) {
        console.log(`[hols:generate] [${completed}/${pairs.length}] ${pair.krakenPair} FAIL: ${result.error}`);
      } else {
        console.log(`[hols:generate] [${completed}/${pairs.length}] ${pair.krakenPair} OK: ${result.rowCount} candles`);
      }

      await sleep(RATE_LIMIT_MS);
    }
  }

  const workers = Array.from({ length: CONCURRENCY }, () => worker());
  await Promise.all(workers);
  return results;
}

async function fetchSpotOhlcWithPagination(
  pair: { krakenPair: string; wsName: string; base: string; quote: string },
  startDate: Date,
  endDate: Date,
  writeRow: (row: NdjsonRow) => Promise<void>,
): Promise<FetchResult> {
  const startSec = Math.floor(startDate.getTime() / 1000);
  const endSec = Math.floor(endDate.getTime() / 1000) + 86400;
  let rowCount = 0;
  const seen = new Set<number>();

  // Kraken Spot OHLC: `since` param pages forward, returns max 720 candles
  let sinceSec = startSec;
  const maxPages = Math.ceil((endSec - startSec) / (CANDLES_PER_REQUEST * 60)) + 1;
  const holsTicker = `${pair.base}${pair.quote}`;

  for (let page = 0; page < maxPages; page++) {
    const candles = await fetchSpotOhlcPage(pair.krakenPair, sinceSec);
    if (candles === null) {
      return { pair: pair.krakenPair, base: pair.base, rowCount: 0, error: `Failed after ${MAX_RETRIES} retries` };
    }
    if (candles.length === 0) break;

    let lastTime = sinceSec;
    for (const c of candles) {
      // Kraken OHLC array: [time, open, high, low, close, vwap, volume, count]
      const timeSec = c[0] as number;
      if (timeSec >= startSec && timeSec < endSec && !seen.has(timeSec)) {
        seen.add(timeSec);
        await writeRow({
          symbol: pair.wsName,
          hols_ticker: holsTicker,
          source: "kraken_spot",
          date: new Date(timeSec * 1000).toISOString(),
          date_close: new Date((timeSec + 60) * 1000).toISOString(),
          unix: timeSec,
          close_unix: timeSec + 60,
          open: parseFloat(c[1] as string),
          high: parseFloat(c[2] as string),
          low: parseFloat(c[3] as string),
          close: parseFloat(c[4] as string),
          volume: parseFloat(c[6] as string),
          volume_from: null,
          tradecount: c[7] as number,
          marketorder_volume: null,
          marketorder_volume_from: null,
        });
        rowCount++;
      }
      if (timeSec > lastTime) lastTime = timeSec;
    }

    // If we've passed endSec or got fewer than max candles, done
    if (lastTime >= endSec || candles.length < CANDLES_PER_REQUEST) break;

    sinceSec = lastTime;
    await sleep(RATE_LIMIT_MS);
  }

  return { pair: pair.krakenPair, base: pair.base, rowCount };
}

async function fetchSpotOhlcPage(
  pair: string,
  sinceSec: number,
): Promise<unknown[][] | null> {
  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    if (attempt > 0) await sleep(1000 * Math.pow(2, attempt));
    try {
      const url = `${KRAKEN_SPOT_OHLC_URL}?pair=${pair}&interval=1&since=${sinceSec}`;
      const resp = await fetch(url, {
        signal: AbortSignal.timeout(API_TIMEOUT_MS),
        headers: { Accept: "application/json" },
      });
      if (!resp.ok) continue;
      const data = (await resp.json()) as KrakenOhlcResponse;
      if (data.error && data.error.length > 0) continue;

      // Result keys vary (e.g., "XXBTZUSD" for pair "XBTUSDT").
      // Take the first non-"last" key.
      for (const key of Object.keys(data.result)) {
        if (key === "last") continue;
        const candles = data.result[key];
        if (Array.isArray(candles)) return candles as unknown[][];
      }
      return [];
    } catch {
      // retry
    }
  }
  return null;
}

// ============================================================
// Job 2: Aggregate from archived WS trade data
// ============================================================

export async function runHolsAggregate(
  flags: Record<string, unknown>,
): Promise<void> {
  const startTime = Date.now();
  const dryRun = !!flags["dry-run"];

  const { startDate, endDate, startDateStr, endDateStr, lookbackDays } = parseDateRange(flags);

  console.log(`[hols:aggregate] WS trade aggregation for ${startDateStr} to ${endDateStr} (${lookbackDays} days) dry-run=${dryRun}`);

  // 1. Download Spot trade parquet files from GCS
  const spotTradesDir = await downloadSpotTrades(startDate, endDate);
  if (!spotTradesDir) {
    console.error("[hols:aggregate] No Spot trade parquet files found in GCS. Exiting.");
    Deno.exit(1);
  }

  // 2. Aggregate trades into OHLCV bars via DuckDB
  const parquetPath = `/tmp/hols-aggregate-${endDateStr}.parquet`;
  const { rowCount, pairCount } = await aggregateTradesToOhlcv(spotTradesDir, parquetPath);
  const parquetStat = await Deno.stat(parquetPath);
  const fileSizeKB = Math.round((parquetStat.size ?? 0) / 1024);
  console.log(`[hols:aggregate] Parquet written: ${parquetPath} (${fileSizeKB} KB, ${rowCount} rows, ${pairCount} pairs)`);

  if (rowCount === 0) {
    console.error("[hols:aggregate] No rows produced. Exiting.");
    Deno.exit(1);
  }

  // 3. Upload to GCS (separate path from REST-sourced data)
  const gcsPath = `hols/crypto/daily/${endDateStr}/ohlcv-trades.parquet`;
  if (dryRun) {
    console.log(`[hols:aggregate] DRY RUN: would upload to gs://${GCS_BUCKET}/${gcsPath}`);
  } else {
    await uploadToGCS(parquetPath, gcsPath);
    console.log(`[hols:aggregate] Uploaded to gs://${GCS_BUCKET}/${gcsPath}`);
  }

  // 4. Send email report
  const durationSec = Math.round((Date.now() - startTime) / 1000);
  if (!dryRun) {
    await sendReport({
      job: "aggregate",
      dateStr: `${startDateStr} to ${endDateStr}`,
      symbolCount: pairCount,
      successCount: pairCount,
      failCount: 0,
      totalRows: rowCount,
      fileSizeKB,
      durationSec,
      failures: [],
    });
  }

  // 5. Cleanup
  try { await Deno.remove(parquetPath); } catch { /* best-effort */ }
  try { await Deno.remove(spotTradesDir, { recursive: true }); } catch { /* best-effort */ }

  console.log(`[hols:aggregate] Done in ${Math.round((Date.now() - startTime) / 1000)}s`);
}

async function aggregateTradesToOhlcv(
  spotTradesDir: string,
  parquetPath: string,
): Promise<{ rowCount: number; pairCount: number }> {
  const instance = await DuckDBInstance.create();
  const conn = await instance.connect();

  const spotGlob = `${spotTradesDir}/*.parquet`;

  // Aggregate trades into 1-minute OHLCV bars.
  // Spot trade parquet columns: symbol, side, price, qty, ord_type, trade_id, timestamp
  // open = first trade price in the minute, close = last trade price
  await conn.run(`
    COPY (
      SELECT
        symbol::VARCHAR as symbol,
        REPLACE(symbol, '/', '')::VARCHAR as hols_ticker,
        'kraken_spot_trades'::VARCHAR as source,
        DATE_TRUNC('minute', timestamp)::TIMESTAMP as date,
        (DATE_TRUNC('minute', timestamp) + INTERVAL '1 minute')::TIMESTAMP as date_close,
        EPOCH(DATE_TRUNC('minute', timestamp))::BIGINT as unix,
        EPOCH(DATE_TRUNC('minute', timestamp) + INTERVAL '1 minute')::BIGINT as close_unix,
        arg_min(price, timestamp)::DOUBLE as open,
        MAX(price)::DOUBLE as high,
        MIN(price)::DOUBLE as low,
        arg_max(price, timestamp)::DOUBLE as close,
        SUM(qty * price)::DOUBLE as volume,
        SUM(qty)::DOUBLE as volume_from,
        COUNT(*)::BIGINT as tradecount,
        SUM(CASE WHEN ord_type = 'market' THEN qty * price ELSE 0 END)::DOUBLE as marketorder_volume,
        SUM(CASE WHEN ord_type = 'market' THEN qty ELSE 0 END)::DOUBLE as marketorder_volume_from
      FROM read_parquet('${spotGlob}')
      GROUP BY symbol, DATE_TRUNC('minute', timestamp)
      ORDER BY symbol, date
    ) TO '${parquetPath}' (FORMAT PARQUET, COMPRESSION ZSTD)
  `);

  // Get row count and pair count
  const result = await conn.run(`
    SELECT COUNT(*) as cnt, COUNT(DISTINCT symbol) as pairs
    FROM read_parquet('${parquetPath}')
  `);
  const reader = result.getRows();
  const rows = reader.toArray();
  const rowCount = rows.length > 0 ? Number(rows[0][0]) : 0;
  const pairCount = rows.length > 0 ? Number(rows[0][1]) : 0;

  return { rowCount, pairCount };
}

// ============================================================
// Shared helpers
// ============================================================

function parseDateRange(flags: Record<string, unknown>): {
  startDate: Date;
  endDate: Date;
  startDateStr: string;
  endDateStr: string;
  lookbackDays: number;
} {
  const lookbackDays = flags.days ? parseInt(flags.days as string, 10) : DEFAULT_LOOKBACK_DAYS;
  let endDateStr: string;
  if (flags.date && typeof flags.date === "string") {
    endDateStr = flags.date;
  } else {
    const yesterday = new Date();
    yesterday.setUTCDate(yesterday.getUTCDate() - 1);
    endDateStr = yesterday.toISOString().slice(0, 10);
  }
  const endDate = new Date(endDateStr + "T00:00:00Z");
  const startDate = new Date(endDate);
  startDate.setUTCDate(startDate.getUTCDate() - lookbackDays + 1);
  const startDateStr = startDate.toISOString().slice(0, 10);

  return { startDate, endDate, startDateStr, endDateStr, lookbackDays };
}

async function convertNdjsonToParquet(
  ndjsonPath: string,
  parquetPath: string,
): Promise<void> {
  const instance = await DuckDBInstance.create();
  const conn = await instance.connect();

  await conn.run(`
    COPY (
      SELECT
        symbol::VARCHAR as symbol,
        hols_ticker::VARCHAR as hols_ticker,
        source::VARCHAR as source,
        date::TIMESTAMP as date,
        date_close::TIMESTAMP as date_close,
        unix::BIGINT as unix,
        close_unix::BIGINT as close_unix,
        open::DOUBLE as open,
        high::DOUBLE as high,
        low::DOUBLE as low,
        close::DOUBLE as close,
        volume::DOUBLE as volume,
        volume_from::DOUBLE as volume_from,
        tradecount::BIGINT as tradecount,
        marketorder_volume::DOUBLE as marketorder_volume,
        marketorder_volume_from::DOUBLE as marketorder_volume_from
      FROM read_json_auto('${ndjsonPath}')
    ) TO '${parquetPath}' (FORMAT PARQUET, COMPRESSION ZSTD)
  `);
}

async function uploadToGCS(localPath: string, gcsPath: string): Promise<void> {
  const storage = new Storage();
  await storage.bucket(GCS_BUCKET).upload(localPath, {
    destination: gcsPath,
    metadata: { contentType: "application/octet-stream" },
  });
}

/**
 * Download Kraken Spot trade parquet files from GCS for the given date range.
 * GCS layout: kraken-spot/kraken-spot/spot/YYYY-MM-DD/HH/trade.parquet
 */
async function downloadSpotTrades(
  startDate: Date,
  endDate: Date,
): Promise<string | null> {
  const localDir = "/tmp/spot-trades";
  try { await Deno.mkdir(localDir, { recursive: true }); } catch { /* exists */ }

  const storage = new Storage();
  const bucket = storage.bucket(GCS_BUCKET);
  let fileCount = 0;

  const current = new Date(startDate);
  const end = new Date(endDate);
  end.setUTCDate(end.getUTCDate() + 1);

  while (current < end) {
    const dateStr = current.toISOString().slice(0, 10);
    const prefix = `kraken-spot/kraken-spot/spot/${dateStr}/`;

    try {
      const [files] = await bucket.getFiles({ prefix, matchGlob: "*/trade.parquet" });
      for (const file of files) {
        const localPath = `${localDir}/${dateStr}-${file.name.split("/").slice(-2).join("-")}`;
        await file.download({ destination: localPath });
        fileCount++;
      }
    } catch (e) {
      console.log(`[hols] Spot trade download for ${dateStr}: ${(e as Error).message}`);
    }

    current.setUTCDate(current.getUTCDate() + 1);
  }

  if (fileCount === 0) {
    console.log("[hols] No Spot trade parquet files found in GCS");
    return null;
  }

  console.log(`[hols] Downloaded ${fileCount} Spot trade parquet files to ${localDir}`);
  return localDir;
}

interface ReportData {
  job: string;
  dateStr: string;
  symbolCount: number;
  successCount: number;
  failCount: number;
  totalRows: number;
  fileSizeKB: number;
  durationSec: number;
  failures: { symbol: string; error: string }[];
}

async function sendReport(data: ReportData): Promise<void> {
  const host = Deno.env.get("SMTP_HOST") ?? "smtp.gmail.com";
  const port = Number(Deno.env.get("SMTP_PORT") ?? "587");
  const user = Deno.env.get("SMTP_USER");
  const pass = Deno.env.get("SMTP_PASS");
  const to = Deno.env.get("SMTP_TO");

  if (!user || !pass || !to) {
    console.log("[hols] SMTP not configured, skipping email report");
    return;
  }

  const jobLabel = data.job === "generate" ? "Spot REST OHLC" : "WS Trade Aggregate";
  const statusColor = data.failCount === 0 ? "#1e7e34" : "#c5221f";
  const statusText = data.failCount === 0 ? "SUCCESS" : `${data.failCount} FAILURES`;

  let failureRows = "";
  if (data.failures.length > 0) {
    failureRows = data.failures
      .map((f) =>
        `<tr><td style="padding:4px 8px;border-bottom:1px solid #eee">${escapeHtml(f.symbol)}</td><td style="padding:4px 8px;border-bottom:1px solid #eee">${escapeHtml(f.error)}</td></tr>`
      )
      .join("");
  }

  const html = `<!DOCTYPE html>
<html>
<head>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }
    .container { max-width: 600px; margin: 0 auto; background: #fff; border-radius: 8px; padding: 24px; }
    h1 { font-size: 18px; color: #1a1a1a; border-bottom: 2px solid #e0e0e0; padding-bottom: 8px; }
    table.summary td { padding: 4px 12px 4px 0; }
    table.summary td:first-child { font-weight: 600; color: #555; }
    .footer { margin-top: 20px; font-size: 11px; color: #999; border-top: 1px solid #eee; padding-top: 10px; }
  </style>
</head>
<body>
  <div class="container">
    <h1>HOLS ${jobLabel} — ${data.dateStr}</h1>
    <p style="font-size:14px;font-weight:600;color:${statusColor}">${statusText}</p>
    <table class="summary">
      <tr><td>Date</td><td>${data.dateStr}</td></tr>
      <tr><td>Pairs</td><td>${data.symbolCount}</td></tr>
      <tr><td>Success</td><td>${data.successCount}</td></tr>
      <tr><td>Failed</td><td>${data.failCount}</td></tr>
      <tr><td>Total Rows</td><td>${data.totalRows}</td></tr>
      <tr><td>Parquet Size</td><td>${data.fileSizeKB} KB</td></tr>
      <tr><td>Duration</td><td>${data.durationSec}s</td></tr>
    </table>
    ${data.failures.length > 0
      ? `<h2 style="font-size:14px;margin-top:20px;color:#c5221f">Failures</h2>
         <table style="width:100%;border-collapse:collapse;font-size:13px">
           <tr><th style="text-align:left;padding:6px 8px;border-bottom:2px solid #ddd">Pair</th><th style="text-align:left;padding:6px 8px;border-bottom:2px solid #ddd">Error</th></tr>
           ${failureRows}
         </table>`
      : ""}
    <div class="footer">Generated by ssmd hols ${data.job} at ${new Date().toISOString()}</div>
  </div>
</body>
</html>`;

  const transporter = nodemailer.createTransport({
    host,
    port,
    secure: false,
    auth: { user, pass },
  });

  await transporter.sendMail({
    from: user,
    to,
    subject: `[SSMD] HOLS ${jobLabel} — ${data.dateStr} — ${statusText}`,
    html,
  });

  console.log(`[hols] Report email sent to ${to}`);
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
