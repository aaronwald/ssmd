/**
 * Data Quality check for cached backtest archives.
 * Reads local .jsonl.gz files and reports issues that could affect backtest reliability.
 *
 * Run: deno run --allow-read --allow-env --allow-run src/momentum/dq-check.ts [dates...]
 * Example: deno run --allow-read --allow-env --allow-run src/momentum/dq-check.ts 2026-01-16 2026-01-17
 */

import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { TextLineStream } from "https://deno.land/std@0.224.0/streams/text_line_stream.ts";

const CACHE_BASE = join(
  Deno.env.get("HOME") ?? "/tmp",
  ".cache",
  "ssmd-backtest",
  "ssmd-archive",
  "kalshi",
  "sports",
);

// --- Types ---

interface FileDQ {
  file: string;
  totalLines: number;
  parseErrors: number;
  gunzipError: boolean;
  tickerRecords: number;
  tradeRecords: number;
  unknownTypes: number;
  firstTs: number;
  lastTs: number;
  tsOutOfOrder: number;
  issues: string[];
}

interface TickerDQ {
  ticker: string;
  records: number;
  tickerMsgs: number;
  tradeMsgs: number;
  // Field completeness
  missingBidAsk: number;
  missingVolume: number;
  missingPrice: number;
  missingTradeSide: number;
  missingTradeCount: number;
  // Sanity
  bidAboveAsk: number;
  priceOutOfRange: number; // outside 1-99
  volumeDecreases: number;
  tsGaps: number; // gaps > 5 min between consecutive records
  // Price stats
  minPrice: number;
  maxPrice: number;
  firstTs: number;
  lastTs: number;
}

interface DateDQ {
  date: string;
  files: FileDQ[];
  tickers: Map<string, TickerDQ>;
  totalRecords: number;
  totalParseErrors: number;
  gunzipErrors: number;
}

// --- Helpers ---

async function* readLocalJsonlGz(localPath: string): AsyncGenerator<string> {
  const child = new Deno.Command("gunzip", {
    args: ["-c", localPath],
    stdout: "piped",
    stderr: "piped",
  }).spawn();

  const lineStream = child.stdout
    .pipeThrough(new TextDecoderStream())
    .pipeThrough(new TextLineStream());

  for await (const line of lineStream) {
    if (line.length > 0) yield line;
  }

  const status = await child.status;
  if (!status.success) {
    throw new Error("gunzip failed");
  }
}

function ensureTickerDQ(map: Map<string, TickerDQ>, ticker: string): TickerDQ {
  let dq = map.get(ticker);
  if (!dq) {
    dq = {
      ticker,
      records: 0,
      tickerMsgs: 0,
      tradeMsgs: 0,
      missingBidAsk: 0,
      missingVolume: 0,
      missingPrice: 0,
      missingTradeSide: 0,
      missingTradeCount: 0,
      bidAboveAsk: 0,
      priceOutOfRange: 0,
      volumeDecreases: 0,
      tsGaps: 0,
      minPrice: Infinity,
      maxPrice: -Infinity,
      firstTs: Infinity,
      lastTs: 0,
    };
    map.set(ticker, dq);
  }
  return dq;
}

// --- Core DQ logic ---

async function checkFile(filePath: string, tickers: Map<string, TickerDQ>): Promise<FileDQ> {
  const fileName = filePath.split("/").pop() ?? filePath;
  const dq: FileDQ = {
    file: fileName,
    totalLines: 0,
    parseErrors: 0,
    gunzipError: false,
    tickerRecords: 0,
    tradeRecords: 0,
    unknownTypes: 0,
    firstTs: Infinity,
    lastTs: 0,
    tsOutOfOrder: 0,
    issues: [],
  };

  // Track per-ticker state for volume monotonicity
  const prevVolume = new Map<string, number>();
  let prevFileTs = 0;

  try {
    for await (const line of readLocalJsonlGz(filePath)) {
      dq.totalLines++;
      try {
        const raw = JSON.parse(line);
        if (!raw.msg || typeof raw.msg !== "object") {
          dq.parseErrors++;
          continue;
        }

        const type = raw.type as string;
        const msg = raw.msg as Record<string, unknown>;
        const ticker = msg.market_ticker as string;
        const ts = msg.ts as number;

        if (!ticker || typeof ticker !== "string") {
          dq.parseErrors++;
          continue;
        }
        if (!ts || typeof ts !== "number" || ts < 1700000000 || ts > 1900000000) {
          dq.parseErrors++;
          continue;
        }

        // File-level timestamp tracking
        if (ts < dq.firstTs) dq.firstTs = ts;
        if (ts > dq.lastTs) dq.lastTs = ts;
        if (ts < prevFileTs) dq.tsOutOfOrder++;
        prevFileTs = ts;

        const tdq = ensureTickerDQ(tickers, ticker);
        tdq.records++;
        if (ts < tdq.firstTs) tdq.firstTs = ts;
        if (ts > tdq.lastTs) tdq.lastTs = ts;

        if (type === "ticker") {
          dq.tickerRecords++;
          tdq.tickerMsgs++;

          const price = (msg.price ?? msg.yes_price ?? msg.last_price) as number | undefined;
          const yesBid = msg.yes_bid as number | undefined;
          const yesAsk = msg.yes_ask as number | undefined;
          const volume = msg.volume as number | undefined;
          const dollarVolume = msg.dollar_volume as number | undefined;

          // Field completeness
          if (yesBid === undefined || yesAsk === undefined) tdq.missingBidAsk++;
          if (volume === undefined && dollarVolume === undefined) tdq.missingVolume++;
          if (price === undefined || price === 0) tdq.missingPrice++;

          // Bid/ask sanity
          if (yesBid !== undefined && yesAsk !== undefined && yesBid > yesAsk) {
            tdq.bidAboveAsk++;
          }

          // Price range
          if (price !== undefined && price > 0) {
            if (price < 1 || price > 99) tdq.priceOutOfRange++;
            if (price < tdq.minPrice) tdq.minPrice = price;
            if (price > tdq.maxPrice) tdq.maxPrice = price;
          }

          // Volume monotonicity (cumulative volume should not decrease)
          if (volume !== undefined && volume > 0) {
            const prev = prevVolume.get(ticker);
            if (prev !== undefined && volume < prev) {
              tdq.volumeDecreases++;
            }
            prevVolume.set(ticker, volume);
          }
        } else if (type === "trade") {
          dq.tradeRecords++;
          tdq.tradeMsgs++;

          const price = msg.price as number | undefined;
          const count = msg.count as number | undefined;
          const side = (msg.taker_side ?? msg.side) as string | undefined;

          if (price === undefined || price === 0) tdq.missingPrice++;
          if (side === undefined || (side !== "yes" && side !== "no")) tdq.missingTradeSide++;
          if (count === undefined || count <= 0) tdq.missingTradeCount++;

          // Price range on trades
          if (price !== undefined && price > 0) {
            if (price < 1 || price > 99) tdq.priceOutOfRange++;
            if (price < tdq.minPrice) tdq.minPrice = price;
            if (price > tdq.maxPrice) tdq.maxPrice = price;
          }
        } else {
          dq.unknownTypes++;
        }
      } catch {
        dq.parseErrors++;
      }
    }
  } catch {
    dq.gunzipError = true;
  }

  return dq;
}

async function checkDate(date: string): Promise<DateDQ> {
  const dir = join(CACHE_BASE, date);
  const result: DateDQ = {
    date,
    files: [],
    tickers: new Map(),
    totalRecords: 0,
    totalParseErrors: 0,
    gunzipErrors: 0,
  };

  const filePaths: string[] = [];
  try {
    for await (const entry of Deno.readDir(dir)) {
      if (entry.name.endsWith(".jsonl.gz")) {
        filePaths.push(join(dir, entry.name));
      }
    }
  } catch {
    console.error(`  No cached data for ${date}`);
    return result;
  }

  filePaths.sort();

  for (const fp of filePaths) {
    const fdq = await checkFile(fp, result.tickers);
    result.files.push(fdq);
    result.totalRecords += fdq.totalLines;
    result.totalParseErrors += fdq.parseErrors;
    if (fdq.gunzipError) result.gunzipErrors++;
  }

  // Compute per-ticker timestamp gaps (> 5 min between consecutive records)
  // This is done post-hoc since we process files sequentially
  // We already tracked firstTs/lastTs per ticker but gaps require ordered timestamps
  // which we don't have aggregated. Skip for now — file-level gaps are more useful.

  return result;
}

// --- Reporting ---

function printDateReport(dq: DateDQ): void {
  console.log(`\n${"=".repeat(70)}`);
  console.log(`DATE: ${dq.date}`);
  console.log(`${"=".repeat(70)}`);
  console.log(`  Files: ${dq.files.length} | Records: ${dq.totalRecords.toLocaleString()} | Parse errors: ${dq.totalParseErrors} | Gunzip errors: ${dq.gunzipErrors}`);

  // File-level issues
  const badFiles = dq.files.filter((f) => f.gunzipError || f.parseErrors > 0 || f.tsOutOfOrder > 0);
  if (badFiles.length > 0) {
    console.log(`\n  File Issues:`);
    for (const f of badFiles) {
      const issues: string[] = [];
      if (f.gunzipError) issues.push("GUNZIP FAILED");
      if (f.parseErrors > 0) issues.push(`${f.parseErrors} parse errors`);
      if (f.tsOutOfOrder > 0) issues.push(`${f.tsOutOfOrder} ts out-of-order`);
      console.log(`    ${f.file}: ${issues.join(", ")}`);
    }
  }

  // Record type breakdown
  const totalTicker = dq.files.reduce((s, f) => s + f.tickerRecords, 0);
  const totalTrade = dq.files.reduce((s, f) => s + f.tradeRecords, 0);
  const totalUnknown = dq.files.reduce((s, f) => s + f.unknownTypes, 0);
  console.log(`  Record types: ${totalTicker.toLocaleString()} ticker, ${totalTrade.toLocaleString()} trade${totalUnknown > 0 ? `, ${totalUnknown} unknown` : ""}`);

  // Ticker-level summary
  const tickerList = [...dq.tickers.values()];
  console.log(`  Unique tickers: ${tickerList.length}`);

  // Aggregate ticker issues
  let totalMissingBidAsk = 0;
  let totalMissingVolume = 0;
  let totalMissingPrice = 0;
  let totalBidAboveAsk = 0;
  let totalPriceOutOfRange = 0;
  let totalVolumeDecreases = 0;
  let totalMissingSide = 0;
  let totalMissingCount = 0;
  let tickersWithIssues = 0;

  for (const t of tickerList) {
    totalMissingBidAsk += t.missingBidAsk;
    totalMissingVolume += t.missingVolume;
    totalMissingPrice += t.missingPrice;
    totalBidAboveAsk += t.bidAboveAsk;
    totalPriceOutOfRange += t.priceOutOfRange;
    totalVolumeDecreases += t.volumeDecreases;
    totalMissingSide += t.missingTradeSide;
    totalMissingCount += t.missingTradeCount;
    if (
      t.missingBidAsk > 0 || t.missingVolume > 0 || t.bidAboveAsk > 0 ||
      t.priceOutOfRange > 0 || t.volumeDecreases > 0
    ) {
      tickersWithIssues++;
    }
  }

  console.log(`\n  Field Completeness (ticker messages):`);
  console.log(`    Missing bid/ask:     ${totalMissingBidAsk.toLocaleString()}${totalTicker > 0 ? ` (${(totalMissingBidAsk / totalTicker * 100).toFixed(1)}%)` : ""}`);
  console.log(`    Missing volume:      ${totalMissingVolume.toLocaleString()}${totalTicker > 0 ? ` (${(totalMissingVolume / totalTicker * 100).toFixed(1)}%)` : ""}`);
  console.log(`    Missing price:       ${totalMissingPrice.toLocaleString()}${(totalTicker + totalTrade) > 0 ? ` (${(totalMissingPrice / (totalTicker + totalTrade) * 100).toFixed(1)}%)` : ""}`);

  console.log(`  Field Completeness (trade messages):`);
  console.log(`    Missing side:        ${totalMissingSide.toLocaleString()}${totalTrade > 0 ? ` (${(totalMissingSide / totalTrade * 100).toFixed(1)}%)` : ""}`);
  console.log(`    Missing count:       ${totalMissingCount.toLocaleString()}${totalTrade > 0 ? ` (${(totalMissingCount / totalTrade * 100).toFixed(1)}%)` : ""}`);

  console.log(`\n  Data Sanity:`);
  console.log(`    Bid > ask:           ${totalBidAboveAsk.toLocaleString()}`);
  console.log(`    Price out of 1-99:   ${totalPriceOutOfRange.toLocaleString()}`);
  console.log(`    Volume decreases:    ${totalVolumeDecreases.toLocaleString()}`);
  console.log(`    Tickers with issues: ${tickersWithIssues} / ${tickerList.length}`);

  // Time coverage: earliest and latest record across all files
  const goodFiles = dq.files.filter((f) => !f.gunzipError && f.totalLines > 0);
  if (goodFiles.length > 0) {
    const earliest = Math.min(...goodFiles.map((f) => f.firstTs));
    const latest = Math.max(...goodFiles.map((f) => f.lastTs));
    const spanHrs = (latest - earliest) / 3600;
    console.log(`\n  Time Coverage:`);
    console.log(`    Earliest: ${new Date(earliest * 1000).toISOString()}`);
    console.log(`    Latest:   ${new Date(latest * 1000).toISOString()}`);
    console.log(`    Span:     ${spanHrs.toFixed(1)} hours`);

    // Check for time gaps between files (> 30 min)
    const fileWindows = goodFiles
      .filter((f) => f.firstTs < Infinity && f.lastTs > 0)
      .map((f) => ({ file: f.file, first: f.firstTs, last: f.lastTs }))
      .sort((a, b) => a.first - b.first);

    const gaps: { after: string; gapMin: number }[] = [];
    for (let i = 1; i < fileWindows.length; i++) {
      const gapSec = fileWindows[i].first - fileWindows[i - 1].last;
      if (gapSec > 30 * 60) {
        gaps.push({ after: fileWindows[i - 1].file, gapMin: Math.round(gapSec / 60) });
      }
    }
    if (gaps.length > 0) {
      console.log(`    Time gaps (>30min):`);
      for (const g of gaps) {
        console.log(`      After ${g.after}: ${g.gapMin} min gap`);
      }
    } else {
      console.log(`    Time gaps (>30min): none`);
    }
  }

  // Top tickers by record count
  const sorted = tickerList.sort((a, b) => b.records - a.records);
  console.log(`\n  Top 10 tickers by record count:`);
  for (const t of sorted.slice(0, 10)) {
    const span = t.lastTs > 0 && t.firstTs < Infinity
      ? `${((t.lastTs - t.firstTs) / 3600).toFixed(1)}h`
      : "?";
    const price = t.minPrice < Infinity ? `${t.minPrice}-${t.maxPrice}c` : "?";
    const issues: string[] = [];
    if (t.bidAboveAsk > 0) issues.push(`bid>ask:${t.bidAboveAsk}`);
    if (t.volumeDecreases > 0) issues.push(`volDec:${t.volumeDecreases}`);
    if (t.missingVolume > 0 && t.tickerMsgs > 0) {
      const pct = (t.missingVolume / t.tickerMsgs * 100).toFixed(0);
      if (Number(pct) > 10) issues.push(`noVol:${pct}%`);
    }
    console.log(`    ${t.ticker.padEnd(45)} ${String(t.records).padStart(7)} records  ${span.padStart(6)}  ${price.padStart(10)}  ${issues.join(" ")}`);
  }
}

function printOverallSummary(allDates: DateDQ[]): void {
  console.log(`\n${"=".repeat(70)}`);
  console.log(`OVERALL SUMMARY`);
  console.log(`${"=".repeat(70)}`);

  const totalRecords = allDates.reduce((s, d) => s + d.totalRecords, 0);
  const totalFiles = allDates.reduce((s, d) => s + d.files.length, 0);
  const totalGunzip = allDates.reduce((s, d) => s + d.gunzipErrors, 0);
  const totalParse = allDates.reduce((s, d) => s + d.totalParseErrors, 0);

  console.log(`  Dates: ${allDates.length} | Files: ${totalFiles} | Records: ${totalRecords.toLocaleString()}`);
  console.log(`  Gunzip errors: ${totalGunzip} files | Parse errors: ${totalParse}`);

  // Per-date summary table
  console.log(`\n  ${"Date".padEnd(12)} ${"Files".padStart(6)} ${"Records".padStart(10)} ${"GzErr".padStart(6)} ${"Parse".padStart(6)} ${"Tickers".padStart(8)} ${"Tickers".padStart(8)} ${"Bid>Ask".padStart(8)} ${"VolDec".padStart(8)}`);
  for (const d of allDates) {
    const totalTicker = d.files.reduce((s, f) => s + f.tickerRecords, 0);
    const bidAboveAsk = [...d.tickers.values()].reduce((s, t) => s + t.bidAboveAsk, 0);
    const volDec = [...d.tickers.values()].reduce((s, t) => s + t.volumeDecreases, 0);
    console.log(
      `  ${d.date.padEnd(12)} ${String(d.files.length).padStart(6)} ${d.totalRecords.toLocaleString().padStart(10)} ${String(d.gunzipErrors).padStart(6)} ${String(d.totalParseErrors).padStart(6)} ${String(d.tickers.size).padStart(8)} ${String(totalTicker).padStart(8)} ${String(bidAboveAsk).padStart(8)} ${String(volDec).padStart(8)}`,
    );
  }

  // Data quality verdict per date
  console.log(`\n  Data Quality Verdict:`);
  for (const d of allDates) {
    const issues: string[] = [];
    if (d.gunzipErrors > 0) issues.push(`${d.gunzipErrors} corrupt files`);
    if (d.totalParseErrors > 0) issues.push(`${d.totalParseErrors} parse errors`);
    const bidAboveAsk = [...d.tickers.values()].reduce((s, t) => s + t.bidAboveAsk, 0);
    if (bidAboveAsk > 0) issues.push(`${bidAboveAsk} bid>ask`);
    const volDec = [...d.tickers.values()].reduce((s, t) => s + t.volumeDecreases, 0);
    if (volDec > 0) issues.push(`${volDec} vol decreases`);
    const goodFiles = d.files.filter((f) => !f.gunzipError);
    const corruptPct = d.files.length > 0 ? (d.gunzipErrors / d.files.length * 100).toFixed(0) : "0";

    if (d.totalRecords === 0) {
      console.log(`    ${d.date}: ❌ NO DATA`);
    } else if (d.gunzipErrors > d.files.length * 0.1) {
      console.log(`    ${d.date}: ⚠️  ${corruptPct}% files corrupt - ${issues.join(", ")}`);
    } else if (issues.length === 0) {
      console.log(`    ${d.date}: ✅ Clean (${d.totalRecords.toLocaleString()} records, ${d.tickers.size} tickers)`);
    } else {
      console.log(`    ${d.date}: ⚠️  ${issues.join(", ")}`);
    }
  }
}

// --- Main ---

const args = Deno.args;
let dates: string[];

if (args.length > 0) {
  dates = args;
} else {
  // Default: scan all cached dates
  const entries: string[] = [];
  try {
    for await (const entry of Deno.readDir(CACHE_BASE)) {
      if (entry.isDirectory && /^\d{4}-\d{2}-\d{2}$/.test(entry.name)) {
        entries.push(entry.name);
      }
    }
  } catch {
    console.error("No cache directory found");
    Deno.exit(1);
  }
  dates = entries.sort();
}

console.log(`Data Quality Check`);
console.log(`Dates: ${dates.join(", ")}`);
console.log(`Cache: ${CACHE_BASE}`);

const allDates: DateDQ[] = [];

for (const date of dates) {
  console.log(`\nChecking ${date}...`);
  const dq = await checkDate(date);
  allDates.push(dq);
  printDateReport(dq);
}

printOverallSummary(allDates);
