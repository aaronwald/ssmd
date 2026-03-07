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
  volume_from: null;
  tradecount: null;
  marketorder_volume: null;
  marketorder_volume_from: null;
}

interface FetchResult {
  symbol: string;
  base: string;
  rows: NdjsonRow[];
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

  // 2. Fetch OHLCV for each symbol (concurrent with rate limiting)
  console.log(`[hols] Fetching with concurrency=${CONCURRENCY}, rate=${RATE_LIMIT_MS}ms between requests`);
  const results = await fetchAllConcurrent(symbols, startDate, endDate);

  const successes = results.filter((r) => !r.error);
  const failures = results.filter((r) => !!r.error);
  const allRows = successes.flatMap((r) => r.rows);

  console.log(
    `[hols] Fetch complete: ${successes.length} ok, ${failures.length} failed, ${allRows.length} total rows`,
  );

  if (allRows.length === 0) {
    console.error("[hols] No data fetched. Exiting.");
    Deno.exit(1);
  }

  // 3. Write NDJSON
  const ndjsonPath = `/tmp/hols-ohlcv-${endDateStr}.ndjson`;
  const ndjsonContent = allRows.map((r) => JSON.stringify(r)).join("\n") + "\n";
  await Deno.writeTextFile(ndjsonPath, ndjsonContent);
  console.log(`[hols] Wrote ${allRows.length} rows to ${ndjsonPath}`);

  // 4. Convert to Parquet via DuckDB
  const parquetPath = `/tmp/hols-ohlcv-${endDateStr}.parquet`;
  await convertToParquet(ndjsonPath, parquetPath);
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
      totalRows: allRows.length,
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

// --- Internal helpers ---

async function fetchAllConcurrent(
  symbols: { symbol: string; base: string }[],
  startDate: Date,
  endDate: Date,
): Promise<FetchResult[]> {
  const results: FetchResult[] = new Array(symbols.length);
  let completed = 0;
  let queue = 0;

  async function worker(): Promise<void> {
    while (true) {
      const idx = queue++;
      if (idx >= symbols.length) break;

      const { symbol, base } = symbols[idx];
      const result = await fetchWithRetry(symbol, base, startDate, endDate);
      results[idx] = result;
      completed++;

      if (result.error) {
        console.log(`[hols] [${completed}/${symbols.length}] ${symbol} FAIL: ${result.error}`);
      } else {
        console.log(`[hols] [${completed}/${symbols.length}] ${symbol} OK: ${result.rows.length} candles`);
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
): Promise<FetchResult> {
  const url = `${KRAKEN_CHARTS_BASE}/${symbol}/1d`;
  let lastError: string | undefined;

  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    if (attempt > 0) {
      const backoffMs = 1000 * Math.pow(2, attempt);
      await sleep(backoffMs);
    }

    try {
      const resp = await fetch(url, {
        signal: AbortSignal.timeout(API_TIMEOUT_MS),
        headers: { Accept: "application/json" },
      });

      if (!resp.ok) {
        lastError = `HTTP ${resp.status} ${resp.statusText}`;
        continue;
      }

      const data = (await resp.json()) as ChartsResponse;
      if (!data.candles || !Array.isArray(data.candles)) {
        lastError = "Invalid response: missing candles array";
        continue;
      }

      // Filter candles for the date range
      // Kraken charts API returns timestamps in milliseconds
      const startMs = startDate.getTime();
      const endMs = endDate.getTime() + 86400_000;

      const filtered = data.candles.filter(
        (c) => c.time >= startMs && c.time < endMs,
      );

      const rows: NdjsonRow[] = filtered.map((c) => {
        const timeSec = Math.floor(c.time / 1000);
        const openDate = new Date(c.time);
        const closeDate = new Date(c.time + 86400_000);
        return {
          symbol,
          source: "kraken_futures",
          date: openDate.toISOString(),
          date_close: closeDate.toISOString(),
          unix: timeSec,
          close_unix: timeSec + 86400,
          open: parseFloat(c.open),
          high: parseFloat(c.high),
          low: parseFloat(c.low),
          close: parseFloat(c.close),
          volume: parseFloat(c.volume),
          volume_from: null,
          tradecount: null,
          marketorder_volume: null,
          marketorder_volume_from: null,
        };
      });

      return { symbol, base, rows };
    } catch (e) {
      lastError = e instanceof Error ? e.message : String(e);
    }
  }

  return { symbol, base, rows: [], error: lastError };
}

async function convertToParquet(
  ndjsonPath: string,
  parquetPath: string,
): Promise<void> {
  const instance = await DuckDBInstance.create();
  const conn = await instance.connect();

  await conn.run(`
    COPY (
      SELECT
        symbol::VARCHAR as symbol,
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
        NULL::DOUBLE as volume_from,
        NULL::BIGINT as tradecount,
        NULL::DOUBLE as marketorder_volume,
        NULL::DOUBLE as marketorder_volume_from
      FROM read_json_auto('${ndjsonPath}')
    ) TO '${parquetPath}' (FORMAT PARQUET, COMPRESSION ZSTD)
  `);
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
