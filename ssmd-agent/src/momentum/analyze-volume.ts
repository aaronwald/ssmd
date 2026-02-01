/**
 * Analyze dollar volume distribution across cached backtest data.
 * Run: deno run --allow-read --allow-env --allow-run src/momentum/analyze-volume.ts [dates...]
 * Example: deno run --allow-read --allow-env --allow-run src/momentum/analyze-volume.ts 2026-01-20 2026-01-21 2026-01-22
 */

import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { TextLineStream } from "https://deno.land/std@0.224.0/streams/text_line_stream.ts";
import { parseMomentumRecord } from "./parse.ts";
import { MarketState } from "./market-state.ts";

const CACHE_BASE = join(Deno.env.get("HOME") ?? "/tmp", ".cache", "ssmd-backtest", "ssmd-archive", "kalshi", "sports");

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
  await child.status;
}

interface TickerStats {
  ticker: string;
  maxDollarVolume30m: number;
  maxDollarVolume10m: number;
  maxDollarVolume5m: number;
  totalRecords: number;
  priceRange: [number, number];
  avgPrice: number;
}

async function analyzeDate(date: string): Promise<TickerStats[]> {
  const dir = join(CACHE_BASE, date);
  const files: string[] = [];

  try {
    for await (const entry of Deno.readDir(dir)) {
      if (entry.name.endsWith(".jsonl.gz")) {
        files.push(join(dir, entry.name));
      }
    }
  } catch {
    console.error(`No cached data for ${date}`);
    return [];
  }

  files.sort();

  const states = new Map<string, MarketState>();
  const recordCounts = new Map<string, number>();
  const peakVolume30m = new Map<string, number>();
  const peakVolume10m = new Map<string, number>();
  const peakVolume5m = new Map<string, number>();
  const prices = new Map<string, number[]>();

  for (const file of files) {
    try {
      for await (const line of readLocalJsonlGz(file)) {
        try {
          const raw = JSON.parse(line);
          const record = parseMomentumRecord(raw);
          if (!record || !record.ticker) continue;

          let state = states.get(record.ticker);
          if (!state) {
            state = new MarketState(record.ticker);
            states.set(record.ticker, state);
          }
          state.update(record);

          recordCounts.set(record.ticker, (recordCounts.get(record.ticker) ?? 0) + 1);

          if (state.lastPrice > 0) {
            const arr = prices.get(record.ticker) ?? [];
            arr.push(state.lastPrice);
            prices.set(record.ticker, arr);
          }

          // Track peak dollar volume at different windows
          const vol30m = state.getVolumeRate(30 * 60).dollarVolume;
          const vol10m = state.getVolumeRate(10 * 60).dollarVolume;
          const vol5m = state.getVolumeRate(5 * 60).dollarVolume;

          peakVolume30m.set(record.ticker, Math.max(peakVolume30m.get(record.ticker) ?? 0, vol30m));
          peakVolume10m.set(record.ticker, Math.max(peakVolume10m.get(record.ticker) ?? 0, vol10m));
          peakVolume5m.set(record.ticker, Math.max(peakVolume5m.get(record.ticker) ?? 0, vol5m));
        } catch {
          // skip
        }
      }
    } catch {
      // skip bad files
    }
  }

  const results: TickerStats[] = [];
  for (const [ticker, state] of states) {
    const priceArr = prices.get(ticker) ?? [];
    const min = priceArr.length > 0 ? Math.min(...priceArr) : 0;
    const max = priceArr.length > 0 ? Math.max(...priceArr) : 0;
    const avg = priceArr.length > 0 ? priceArr.reduce((a, b) => a + b, 0) / priceArr.length : 0;

    results.push({
      ticker,
      maxDollarVolume30m: peakVolume30m.get(ticker) ?? 0,
      maxDollarVolume10m: peakVolume10m.get(ticker) ?? 0,
      maxDollarVolume5m: peakVolume5m.get(ticker) ?? 0,
      totalRecords: recordCounts.get(ticker) ?? 0,
      priceRange: [min, max],
      avgPrice: avg,
    });
  }

  return results;
}

// Main
const dates = Deno.args.length > 0 ? Deno.args : ["2026-01-20"];

const allStats: TickerStats[] = [];

for (const date of dates) {
  console.log(`Analyzing ${date}...`);
  const stats = await analyzeDate(date);
  allStats.push(...stats);
}

// Sort by peak 30m dollar volume
allStats.sort((a, b) => b.maxDollarVolume30m - a.maxDollarVolume30m);

// Distribution analysis
const thresholds = [25000, 50000, 75000, 100000, 150000, 200000, 250000, 500000];
console.log(`\n=== Dollar Volume Distribution (peak 30m window) ===`);
console.log(`Total tickers: ${allStats.length}`);
for (const t of thresholds) {
  const count = allStats.filter(s => s.maxDollarVolume30m >= t).length;
  console.log(`  >= $${(t/1000).toFixed(0)}k: ${count} tickers (${(count/allStats.length*100).toFixed(0)}%)`);
}

// Same for 10m window
console.log(`\n=== Dollar Volume Distribution (peak 10m window) ===`);
for (const t of thresholds) {
  const count = allStats.filter(s => s.maxDollarVolume10m >= t).length;
  console.log(`  >= $${(t/1000).toFixed(0)}k: ${count} tickers (${(count/allStats.length*100).toFixed(0)}%)`);
}

// Same for 5m window
console.log(`\n=== Dollar Volume Distribution (peak 5m window) ===`);
for (const t of thresholds) {
  const count = allStats.filter(s => s.maxDollarVolume5m >= t).length;
  console.log(`  >= $${(t/1000).toFixed(0)}k: ${count} tickers (${(count/allStats.length*100).toFixed(0)}%)`);
}

// Top 30 tickers by 30m volume
console.log(`\n=== Top 30 Tickers by Peak 30m Dollar Volume ===`);
console.log(`${"Ticker".padEnd(45)} ${"30m$vol".padStart(10)} ${"10m$vol".padStart(10)} ${"5m$vol".padStart(10)} ${"AvgPrice".padStart(8)} ${"Range".padStart(10)} ${"Records".padStart(8)}`);
for (const s of allStats.slice(0, 30)) {
  const vol30 = `$${(s.maxDollarVolume30m/1000).toFixed(0)}k`;
  const vol10 = `$${(s.maxDollarVolume10m/1000).toFixed(0)}k`;
  const vol5 = `$${(s.maxDollarVolume5m/1000).toFixed(0)}k`;
  const range = `${s.priceRange[0]}-${s.priceRange[1]}c`;
  console.log(`${s.ticker.padEnd(45)} ${vol30.padStart(10)} ${vol10.padStart(10)} ${vol5.padStart(10)} ${s.avgPrice.toFixed(0).padStart(7)}c ${range.padStart(10)} ${String(s.totalRecords).padStart(8)}`);
}

// Price range distribution for activated tickers
const activated250k = allStats.filter(s => s.maxDollarVolume30m >= 250000);
const activated100k = allStats.filter(s => s.maxDollarVolume30m >= 100000);
const activated50k = allStats.filter(s => s.maxDollarVolume30m >= 50000);

console.log(`\n=== Price Distribution of Activated Tickers ===`);
for (const [label, group] of [["$250k", activated250k], ["$100k", activated100k], ["$50k", activated50k]] as [string, TickerStats[]][]) {
  const midRange = group.filter(s => s.avgPrice >= 25 && s.avgPrice <= 75);
  const extreme = group.filter(s => s.avgPrice < 25 || s.avgPrice > 75);
  console.log(`  ${label} threshold: ${group.length} tickers | ${midRange.length} in 25-75c range (${(midRange.length/Math.max(group.length,1)*100).toFixed(0)}%) | ${extreme.length} extreme prices`);
}
