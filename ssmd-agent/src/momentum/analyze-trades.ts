/**
 * Trade frequency analysis script.
 *
 * Usage:
 *   deno run --allow-read src/momentum/analyze-trades.ts <trades.jsonl> [trades2.jsonl ...]
 *
 * Reads JSONL trade files produced by `ssmd momentum backtest --trades-out`
 * and outputs frequency, timing, and profitability analysis.
 */

interface Trade {
  model: string;
  ticker: string;
  side: string;
  entryPrice: number;
  exitPrice: number;
  contracts: number;
  entryTime: number;
  exitTime: number;
  reason: string;
  pnl: number;
  fees: number;
  entryCost: number;
}

function loadTrades(path: string): Trade[] {
  const text = Deno.readTextFileSync(path);
  return text
    .trim()
    .split("\n")
    .filter((l) => l.length > 0)
    .map((l) => JSON.parse(l) as Trade);
}

function hourOfDay(ts: number): number {
  return new Date(ts * 1000).getUTCHours();
}

function dayOfWeek(ts: number): string {
  return ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][
    new Date(ts * 1000).getUTCDay()
  ];
}

function dateStr(ts: number): string {
  return new Date(ts * 1000).toISOString().slice(0, 10);
}

function holdMinutes(t: Trade): number {
  return (t.exitTime - t.entryTime) / 60;
}

function analyzeFile(path: string): void {
  const label = path.split("/").pop() ?? path;
  const trades = loadTrades(path);

  if (trades.length === 0) {
    console.log(`\n=== ${label}: No trades ===\n`);
    return;
  }

  const wins = trades.filter((t) => t.pnl > 0);
  const losses = trades.filter((t) => t.pnl <= 0);
  const totalPnl = trades.reduce((s, t) => s + t.pnl, 0);

  console.log(`\n${"=".repeat(70)}`);
  console.log(`  ${label}  (${trades.length} trades, ${wins.length}W/${losses.length}L, ${((wins.length / trades.length) * 100).toFixed(0)}%, P&L: $${totalPnl.toFixed(2)})`);
  console.log(`${"=".repeat(70)}`);

  // --- Trades per day ---
  const byDate = new Map<string, Trade[]>();
  for (const t of trades) {
    const d = dateStr(t.entryTime);
    const arr = byDate.get(d) ?? [];
    arr.push(t);
    byDate.set(d, arr);
  }

  // Count days with data (Jan 16-26 = 11 days)
  const firstDate = dateStr(trades[0].entryTime);
  const lastDate = dateStr(trades[trades.length - 1].entryTime);
  const tradingDays = byDate.size;
  const daysInRange = 11;

  console.log(`\n--- Trades Per Day ---`);
  console.log(`Period: ${firstDate} to ${lastDate} (${daysInRange} days, ${tradingDays} with trades)`);
  console.log(`Avg trades/day (overall): ${(trades.length / daysInRange).toFixed(1)}`);
  console.log(`Avg trades/day (trading days): ${(trades.length / tradingDays).toFixed(1)}`);
  console.log(``);

  const sortedDates = [...byDate.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  console.log(`${"Date".padEnd(12)} ${"Day".padEnd(4)} ${"Trades".padStart(6)} ${"W/L".padStart(6)} ${"P&L".padStart(8)}`);
  for (const [date, dt] of sortedDates) {
    const w = dt.filter((t) => t.pnl > 0).length;
    const l = dt.length - w;
    const pnl = dt.reduce((s, t) => s + t.pnl, 0);
    const dow = dayOfWeek(dt[0].entryTime);
    const pnlStr = pnl >= 0 ? `+$${pnl.toFixed(0)}` : `-$${Math.abs(pnl).toFixed(0)}`;
    console.log(`${date.padEnd(12)} ${dow.padEnd(4)} ${String(dt.length).padStart(6)} ${`${w}/${l}`.padStart(6)} ${pnlStr.padStart(8)}`);
  }

  // --- Hour of day distribution ---
  const byHour = new Map<number, { count: number; wins: number; pnl: number }>();
  for (const t of trades) {
    const h = hourOfDay(t.entryTime);
    const stats = byHour.get(h) ?? { count: 0, wins: 0, pnl: 0 };
    stats.count++;
    if (t.pnl > 0) stats.wins++;
    stats.pnl += t.pnl;
    byHour.set(h, stats);
  }

  console.log(`\n--- Hour of Day (UTC) ---`);
  console.log(`${"Hour".padEnd(6)} ${"Trades".padStart(6)} ${"Win%".padStart(6)} ${"P&L".padStart(8)} ${"Bar"}`);
  for (let h = 0; h < 24; h++) {
    const stats = byHour.get(h);
    if (!stats) continue;
    const winPct = ((stats.wins / stats.count) * 100).toFixed(0);
    const pnlStr = stats.pnl >= 0 ? `+$${stats.pnl.toFixed(0)}` : `-$${Math.abs(stats.pnl).toFixed(0)}`;
    const bar = "#".repeat(stats.count);
    console.log(`${String(h).padStart(2)}:00 ${String(stats.count).padStart(6)} ${(winPct + "%").padStart(6)} ${pnlStr.padStart(8)} ${bar}`);
  }

  // --- Hold time analysis ---
  const holdTimes = trades.map(holdMinutes);
  const avgHold = holdTimes.reduce((s, h) => s + h, 0) / holdTimes.length;
  const winHold = wins.length > 0 ? wins.map(holdMinutes).reduce((s, h) => s + h, 0) / wins.length : 0;
  const lossHold = losses.length > 0 ? losses.map(holdMinutes).reduce((s, h) => s + h, 0) / losses.length : 0;

  console.log(`\n--- Hold Time ---`);
  console.log(`Average:  ${avgHold.toFixed(1)} min`);
  console.log(`Winners:  ${winHold.toFixed(1)} min`);
  console.log(`Losers:   ${lossHold.toFixed(1)} min`);

  // Hold time by exit reason
  const byReason = new Map<string, { count: number; holdSum: number; pnlSum: number }>();
  for (const t of trades) {
    const stats = byReason.get(t.reason) ?? { count: 0, holdSum: 0, pnlSum: 0 };
    stats.count++;
    stats.holdSum += holdMinutes(t);
    stats.pnlSum += t.pnl;
    byReason.set(t.reason, stats);
  }

  console.log(`\n${"Reason".padEnd(14)} ${"Count".padStart(6)} ${"Avg Hold".padStart(10)} ${"Avg P&L".padStart(10)}`);
  for (const [reason, stats] of byReason) {
    const avgH = (stats.holdSum / stats.count).toFixed(1);
    const avgP = (stats.pnlSum / stats.count).toFixed(2);
    console.log(`${reason.padEnd(14)} ${String(stats.count).padStart(6)} ${(avgH + "m").padStart(10)} ${("$" + avgP).padStart(10)}`);
  }

  // --- Inter-trade gaps ---
  if (trades.length > 1) {
    const gaps: number[] = [];
    for (let i = 1; i < trades.length; i++) {
      gaps.push((trades[i].entryTime - trades[i - 1].exitTime) / 60);
    }
    const avgGap = gaps.reduce((s, g) => s + g, 0) / gaps.length;
    const minGap = Math.min(...gaps);
    const maxGap = Math.max(...gaps);
    const medianGap = gaps.sort((a, b) => a - b)[Math.floor(gaps.length / 2)];

    console.log(`\n--- Inter-Trade Gaps ---`);
    console.log(`Avg:    ${avgGap.toFixed(0)} min (${(avgGap / 60).toFixed(1)} hrs)`);
    console.log(`Median: ${medianGap.toFixed(0)} min (${(medianGap / 60).toFixed(1)} hrs)`);
    console.log(`Min:    ${minGap.toFixed(1)} min`);
    console.log(`Max:    ${maxGap.toFixed(0)} min (${(maxGap / 60).toFixed(1)} hrs)`);
  }

  // --- Same-ticker re-entries ---
  const tickerCounts = new Map<string, number>();
  for (const t of trades) {
    tickerCounts.set(t.ticker, (tickerCounts.get(t.ticker) ?? 0) + 1);
  }
  const repeats = [...tickerCounts.entries()].filter(([, c]) => c > 1);
  const uniqueTickers = tickerCounts.size;

  console.log(`\n--- Ticker Distribution ---`);
  console.log(`Unique tickers: ${uniqueTickers}`);
  console.log(`Tickers with re-entry: ${repeats.length}`);
  if (repeats.length > 0) {
    repeats.sort((a, b) => b[1] - a[1]);
    for (const [ticker, count] of repeats.slice(0, 5)) {
      const tickerTrades = trades.filter((t) => t.ticker === ticker);
      const pnl = tickerTrades.reduce((s, t) => s + t.pnl, 0);
      const shortTicker = ticker.length > 40 ? ticker.slice(0, 40) + "…" : ticker;
      console.log(`  ${shortTicker.padEnd(42)} ${count}x  P&L: $${pnl.toFixed(0)}`);
    }
  }

  // --- Entry price distribution ---
  const priceBuckets: Record<string, { count: number; wins: number; pnl: number }> = {
    "30-39c": { count: 0, wins: 0, pnl: 0 },
    "40-49c": { count: 0, wins: 0, pnl: 0 },
    "50-59c": { count: 0, wins: 0, pnl: 0 },
    "60-65c": { count: 0, wins: 0, pnl: 0 },
  };
  for (const t of trades) {
    const p = t.entryPrice;
    let bucket: string;
    if (p < 40) bucket = "30-39c";
    else if (p < 50) bucket = "40-49c";
    else if (p < 60) bucket = "50-59c";
    else bucket = "60-65c";
    priceBuckets[bucket].count++;
    if (t.pnl > 0) priceBuckets[bucket].wins++;
    priceBuckets[bucket].pnl += t.pnl;
  }

  console.log(`\n--- Entry Price Buckets ---`);
  console.log(`${"Price".padEnd(10)} ${"Count".padStart(6)} ${"Win%".padStart(6)} ${"P&L".padStart(8)}`);
  for (const [bucket, stats] of Object.entries(priceBuckets)) {
    if (stats.count === 0) continue;
    const winPct = ((stats.wins / stats.count) * 100).toFixed(0);
    const pnlStr = stats.pnl >= 0 ? `+$${stats.pnl.toFixed(0)}` : `-$${Math.abs(stats.pnl).toFixed(0)}`;
    console.log(`${bucket.padEnd(10)} ${String(stats.count).padStart(6)} ${(winPct + "%").padStart(6)} ${pnlStr.padStart(8)}`);
  }
}

// --- Main ---
const files = Deno.args;
if (files.length === 0) {
  console.log("Usage: deno run --allow-read src/momentum/analyze-trades.ts <trades.jsonl> [trades2.jsonl ...]");
  Deno.exit(1);
}

for (const file of files) {
  analyzeFile(file);
}
