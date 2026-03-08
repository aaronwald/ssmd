/**
 * ssmd hols generate - Fetch daily OHLCV candles from Kraken Futures charts API,
 * convert to Parquet via DuckDB, upload to GCS, and email a report.
 *
 * Usage:
 *   ssmd hols generate [--date YYYY-MM-DD] [--dry-run]
 */
import { getDb, closeDb } from "../../lib/db/mod.ts";
import { listActivePerpSymbols } from "../../lib/db/pairs.ts";
import { DuckDBInstance } from "@duckdb/node-api";
import { Storage } from "@google-cloud/storage";
import nodemailer from "nodemailer";

const KRAKEN_CHARTS_BASE = "https://futures.kraken.com/api/charts/v1/trade";
const API_TIMEOUT_MS = 15000;
const CONCURRENCY = 10;
const RATE_LIMIT_MS = 200;
const MAX_RETRIES = 3;
const DEFAULT_LOOKBACK_DAYS = 3;
const GCS_BUCKET = "ssmd-data";

interface OhlcvCandle {
  time: number;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
}

interface ChartsResponse {
  candles: OhlcvCandle[];
  more_candles: boolean;
}

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
  symbol: string;
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
    default:
      console.error(`Unknown hols subcommand: ${subcommand ?? "(none)"}`);
      console.log("Usage: ssmd hols generate [--date YYYY-MM-DD] [--days N] [--dry-run]");
      Deno.exit(1);
  }
}

export async function runHolsGenerate(
  flags: Record<string, unknown>,
): Promise<void> {
  const startTime = Date.now();
  const dryRun = !!flags["dry-run"];

  // Determine date range (default: last 3 days ending yesterday UTC)
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

  console.log(`[hols] Generating OHLCV for ${startDateStr} to ${endDateStr} (${lookbackDays} days) dry-run=${dryRun}`);

  // 1. Query secmaster for active Kraken perpetuals
  const db = getDb();
  let symbols: { symbol: string; base: string }[];
  try {
    symbols = await listActivePerpSymbols(db);
    console.log(`[hols] Found ${symbols.length} active Kraken perpetuals`);
  } finally {
    await closeDb();
  }

  if (symbols.length === 0) {
    console.error("[hols] No active perpetual symbols found. Exiting.");
    Deno.exit(1);
  }

  // 2. Fetch OHLCV for each symbol, streaming rows to NDJSON file
  const ndjsonPath = `/tmp/hols-ohlcv-${endDateStr}.ndjson`;
  const ndjsonFile = await Deno.open(ndjsonPath, { write: true, create: true, truncate: true });
  const encoder = new TextEncoder();
  const writeRow = async (row: NdjsonRow): Promise<void> => {
    await ndjsonFile.write(encoder.encode(JSON.stringify(row) + "\n"));
  };

  console.log(`[hols] Fetching with concurrency=${CONCURRENCY}, rate=${RATE_LIMIT_MS}ms between requests`);
  let results: FetchResult[];
  try {
    results = await fetchAllConcurrent(symbols, startDate, endDate, writeRow);
  } finally {
    ndjsonFile.close();
  }

  const successes = results.filter((r) => !r.error);
  const failures = results.filter((r) => !!r.error);
  const totalRows = successes.reduce((sum, r) => sum + r.rowCount, 0);

  console.log(
    `[hols] Fetch complete: ${successes.length} ok, ${failures.length} failed, ${totalRows} total rows`,
  );

  if (totalRows === 0) {
    console.error("[hols] No data fetched. Exiting.");
    Deno.exit(1);
  }

  console.log(`[hols] Wrote ${totalRows} rows to ${ndjsonPath}`);

  // 3. Download Kraken Spot trade parquet for enrichment (marketorder_volume, tradecount)
  const spotTradesDir = await downloadSpotTrades(startDate, endDate);

  // 4. Convert to Parquet via DuckDB (with Spot trade enrichment if available)
  const parquetPath = `/tmp/hols-ohlcv-${endDateStr}.parquet`;
  await convertToParquet(ndjsonPath, parquetPath, spotTradesDir);
  const parquetStat = await Deno.stat(parquetPath);
  const fileSizeKB = Math.round((parquetStat.size ?? 0) / 1024);
  console.log(`[hols] Parquet written: ${parquetPath} (${fileSizeKB} KB)`);

  // 5. Upload to GCS
  const gcsPath = `hols/crypto/daily/${endDateStr}/ohlcv.parquet`;
  if (dryRun) {
    console.log(`[hols] DRY RUN: would upload to gs://${GCS_BUCKET}/${gcsPath}`);
  } else {
    await uploadToGCS(parquetPath, gcsPath);
    console.log(`[hols] Uploaded to gs://${GCS_BUCKET}/${gcsPath}`);
  }

  // 6. Send email report
  const durationSec = Math.round((Date.now() - startTime) / 1000);
  if (dryRun) {
    console.log("[hols] DRY RUN: skipping email");
  } else {
    await sendReport({
      dateStr: `${startDateStr} to ${endDateStr}`,
      symbolCount: symbols.length,
      successCount: successes.length,
      failCount: failures.length,
      totalRows,
      fileSizeKB,
      durationSec,
      failures: failures.map((f) => ({
        symbol: f.symbol,
        error: f.error!,
      })),
    });
  }

  // 7. Cleanup temp files
  try {
    await Deno.remove(ndjsonPath);
    await Deno.remove(parquetPath);
    if (spotTradesDir) await Deno.remove(spotTradesDir, { recursive: true });
  } catch {
    // Best-effort cleanup
  }

  const elapsed = Math.round((Date.now() - startTime) / 1000);
  console.log(`[hols] Done in ${elapsed}s`);

  // Exit non-zero if >10% failed
  const failPct = (failures.length / symbols.length) * 100;
  if (failPct > 10) {
    console.error(
      `[hols] FAIL: ${failures.length}/${symbols.length} symbols failed (${failPct.toFixed(1)}%)`,
    );
    Deno.exit(1);
  }
}

// --- Ticker mapping ---

/** Known base symbol remaps (Kraken convention → common convention) */
const BASE_REMAP: Record<string, string> = {
  XBT: "BTC",
};

/** Derive hols_ticker (e.g. "BTCUSDT") from secmaster base (e.g. "XBT") */
function toHolsTicker(base: string): string {
  const mapped = BASE_REMAP[base.toUpperCase()] ?? base.toUpperCase();
  return `${mapped}USDT`;
}

// --- Internal helpers ---

async function fetchAllConcurrent(
  symbols: { symbol: string; base: string }[],
  startDate: Date,
  endDate: Date,
  writeRow: (row: NdjsonRow) => Promise<void>,
): Promise<FetchResult[]> {
  const results: FetchResult[] = new Array(symbols.length);
  let completed = 0;
  let queue = 0;

  async function worker(): Promise<void> {
    while (true) {
      const idx = queue++;
      if (idx >= symbols.length) break;

      const { symbol, base } = symbols[idx];
      const result = await fetchWithRetry(symbol, base, startDate, endDate, writeRow);
      results[idx] = result;
      completed++;

      if (result.error) {
        console.log(`[hols] [${completed}/${symbols.length}] ${symbol} FAIL: ${result.error}`);
      } else {
        console.log(`[hols] [${completed}/${symbols.length}] ${symbol} OK: ${result.rowCount} candles`);
      }

      await sleep(RATE_LIMIT_MS);
    }
  }

  const workers = Array.from({ length: CONCURRENCY }, () => worker());
  await Promise.all(workers);
  return results;
}

async function fetchWithRetry(
  symbol: string,
  base: string,
  startDate: Date,
  endDate: Date,
  writeRow: (row: NdjsonRow) => Promise<void>,
): Promise<FetchResult> {
  const startMs = startDate.getTime();
  const endMs = endDate.getTime() + 86400_000;
  let rowCount = 0;
  const seen = new Set<number>();

  // Paginate: API returns max 2000 1m candles per request.
  // Use `to` param (seconds) to page backwards.
  let toSec = Math.floor(endMs / 1000);
  const startSec = Math.floor(startMs / 1000);
  const maxPages = 5;

  for (let page = 0; page < maxPages; page++) {
    const url = page === 0
      ? `${KRAKEN_CHARTS_BASE}/${symbol}/1m`
      : `${KRAKEN_CHARTS_BASE}/${symbol}/1m?to=${toSec}`;

    const candles = await fetchPage(url);
    if (candles === null) {
      return { symbol, base, rowCount: 0, error: `Failed after ${MAX_RETRIES} retries` };
    }
    if (candles.length === 0) break;

    let addedCount = 0;
    for (const c of candles) {
      if (c.time >= startMs && c.time < endMs && !seen.has(c.time)) {
        seen.add(c.time);
        const timeSec = Math.floor(c.time / 1000);
        await writeRow({
          symbol,
          hols_ticker: toHolsTicker(base),
          source: "kraken_futures",
          date: new Date(c.time).toISOString(),
          date_close: new Date(c.time + 60_000).toISOString(),
          unix: timeSec,
          close_unix: timeSec + 60,
          open: parseFloat(c.open),
          high: parseFloat(c.high),
          low: parseFloat(c.low),
          close: parseFloat(c.close),
          volume: parseFloat(c.volume),
          volume_from: null,
          tradecount: null,
          marketorder_volume: null,
          marketorder_volume_from: null,
        });
        rowCount++;
        addedCount++;
      }
    }

    // Check if we've reached far enough back
    const oldestSec = Math.floor(candles[0].time / 1000);
    if (oldestSec <= startSec || addedCount === 0) break;

    // Page back: set `to` to the oldest candle we received
    toSec = oldestSec;
    await sleep(RATE_LIMIT_MS);
  }

  return { symbol, base, rowCount };
}

async function fetchPage(url: string): Promise<OhlcvCandle[] | null> {
  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    if (attempt > 0) {
      await sleep(1000 * Math.pow(2, attempt));
    }
    try {
      const resp = await fetch(url, {
        signal: AbortSignal.timeout(API_TIMEOUT_MS),
        headers: { Accept: "application/json" },
      });
      if (!resp.ok) continue;
      const data = (await resp.json()) as ChartsResponse;
      if (!data.candles || !Array.isArray(data.candles)) continue;
      return data.candles;
    } catch {
      // retry
    }
  }
  return null;
}

async function convertToParquet(
  ndjsonPath: string,
  parquetPath: string,
  spotTradesDir: string | null,
): Promise<void> {
  const instance = await DuckDBInstance.create();
  const conn = await instance.connect();

  if (spotTradesDir) {
    // Enrich OHLCV with Spot trade aggregates via left join.
    // Spot trade parquet columns: symbol, side, price, qty, ord_type, trade_id, timestamp
    // Join key: hols_ticker (e.g. BTCUSDT) matched by stripping '/' from Spot symbol (BTC/USDT → BTCUSDT)
    // and unix timestamp matched by truncating Spot timestamp to minute epoch.
    const spotGlob = `${spotTradesDir}/*.parquet`;
    await conn.run(`
      COPY (
        WITH spot AS (
          SELECT
            REPLACE(symbol, '/', '') as hols_ticker,
            EPOCH(DATE_TRUNC('minute', timestamp))::BIGINT as minute_unix,
            COUNT(*)::BIGINT as tradecount,
            SUM(qty) as volume_from,
            SUM(CASE WHEN ord_type = 'market' THEN qty * price ELSE 0 END) as marketorder_volume,
            SUM(CASE WHEN ord_type = 'market' THEN qty ELSE 0 END) as marketorder_volume_from
          FROM read_parquet('${spotGlob}')
          GROUP BY 1, 2
        )
        SELECT
          o.symbol::VARCHAR as symbol,
          o.hols_ticker::VARCHAR as hols_ticker,
          o.source::VARCHAR as source,
          o.date::TIMESTAMP as date,
          o.date_close::TIMESTAMP as date_close,
          o.unix::BIGINT as unix,
          o.close_unix::BIGINT as close_unix,
          o.open::DOUBLE as open,
          o.high::DOUBLE as high,
          o.low::DOUBLE as low,
          o.close::DOUBLE as close,
          o.volume::DOUBLE as volume,
          s.volume_from::DOUBLE as volume_from,
          s.tradecount::BIGINT as tradecount,
          s.marketorder_volume::DOUBLE as marketorder_volume,
          s.marketorder_volume_from::DOUBLE as marketorder_volume_from
        FROM read_json_auto('${ndjsonPath}') o
        LEFT JOIN spot s ON o.hols_ticker = s.hols_ticker AND o.unix = s.minute_unix
      ) TO '${parquetPath}' (FORMAT PARQUET, COMPRESSION ZSTD)
    `);
  } else {
    // No Spot data available — write with null columns
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
}

async function uploadToGCS(
  localPath: string,
  gcsPath: string,
): Promise<void> {
  const storage = new Storage();
  await storage.bucket(GCS_BUCKET).upload(localPath, {
    destination: gcsPath,
    metadata: {
      contentType: "application/octet-stream",
    },
  });
}

interface ReportData {
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

  const statusColor = data.failCount === 0 ? "#1e7e34" : "#c5221f";
  const statusText = data.failCount === 0 ? "SUCCESS" : `${data.failCount} FAILURES`;

  let failureRows = "";
  if (data.failures.length > 0) {
    failureRows = data.failures
      .map(
        (f) =>
          `<tr><td style="padding:4px 8px;border-bottom:1px solid #eee">${escapeHtml(f.symbol)}</td><td style="padding:4px 8px;border-bottom:1px solid #eee">${escapeHtml(f.error)}</td></tr>`,
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
    <h1>HOLS OHLCV Report — ${data.dateStr}</h1>
    <p style="font-size:14px;font-weight:600;color:${statusColor}">${statusText}</p>
    <table class="summary">
      <tr><td>Date</td><td>${data.dateStr}</td></tr>
      <tr><td>Symbols</td><td>${data.symbolCount}</td></tr>
      <tr><td>Success</td><td>${data.successCount}</td></tr>
      <tr><td>Failed</td><td>${data.failCount}</td></tr>
      <tr><td>Total Rows</td><td>${data.totalRows}</td></tr>
      <tr><td>Parquet Size</td><td>${data.fileSizeKB} KB</td></tr>
      <tr><td>Duration</td><td>${data.durationSec}s</td></tr>
      <tr><td>GCS Path</td><td>gs://${GCS_BUCKET}/hols/crypto/daily/${data.dateStr}/ohlcv.parquet</td></tr>
    </table>
    ${
      data.failures.length > 0
        ? `<h2 style="font-size:14px;margin-top:20px;color:#c5221f">Failures</h2>
           <table style="width:100%;border-collapse:collapse;font-size:13px">
             <tr><th style="text-align:left;padding:6px 8px;border-bottom:2px solid #ddd">Symbol</th><th style="text-align:left;padding:6px 8px;border-bottom:2px solid #ddd">Error</th></tr>
             ${failureRows}
           </table>`
        : ""
    }
    <div class="footer">Generated by ssmd hols generate at ${new Date().toISOString()}</div>
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
    subject: `[SSMD] HOLS OHLCV — ${data.dateStr} — ${statusText}`,
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

/**
 * Download Kraken Spot trade parquet files from GCS for the given date range.
 * Returns the local directory path, or null if no files found.
 * GCS layout: kraken-spot/kraken-spot/spot/YYYY-MM-DD/HH/trade.parquet
 */
async function downloadSpotTrades(
  startDate: Date,
  endDate: Date,
): Promise<string | null> {
  const localDir = "/tmp/spot-trades";
  try {
    await Deno.mkdir(localDir, { recursive: true });
  } catch {
    // already exists
  }

  const storage = new Storage();
  const bucket = storage.bucket(GCS_BUCKET);
  let fileCount = 0;

  // Iterate each date in range
  const current = new Date(startDate);
  const end = new Date(endDate);
  end.setUTCDate(end.getUTCDate() + 1); // inclusive end

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
    console.log("[hols] No Spot trade parquet files found — columns will remain null");
    return null;
  }

  console.log(`[hols] Downloaded ${fileCount} Spot trade parquet files to ${localDir}`);
  return localDir;
}
