// ssmd-agent/src/state/price_history.ts
import type { MarketRecord, StateBuilder } from "./types.ts";

export interface PriceHistoryState {
  ticker: string;
  last: number;
  high: number;
  low: number;
  vwap: number;
  returns: number;
  volatility: number;
  tradeCount: number;
  lastUpdate: number;
}

interface Trade {
  price: number;
  count: number;
  ts: number;
}

export class PriceHistoryBuilder implements StateBuilder<PriceHistoryState> {
  id = "priceHistory";
  private trades: Trade[] = [];
  private windowSize: number;
  private state: PriceHistoryState = this.initialState();

  constructor(windowSize: number = 100) {
    this.windowSize = windowSize;
  }

  update(record: MarketRecord): void {
    // Only process trade messages
    if (record.type !== "trade") return;

    const price = record.price ?? 0;
    const count = record.count ?? 1;

    // Add trade to window
    this.trades.push({ price, count, ts: record.ts });

    // Trim to window size
    if (this.trades.length > this.windowSize) {
      this.trades.shift();
    }

    // Recalculate derived values
    this.recalculate(record.ticker, record.ts);
  }

  private recalculate(ticker: string, ts: number): void {
    if (this.trades.length === 0) return;

    const prices = this.trades.map(t => t.price);
    const last = prices[prices.length - 1];
    const first = prices[0];
    const high = Math.max(...prices);
    const low = Math.min(...prices);

    // VWAP = sum(price * count) / sum(count)
    let sumPriceCount = 0;
    let sumCount = 0;
    for (const t of this.trades) {
      sumPriceCount += t.price * t.count;
      sumCount += t.count;
    }
    const vwap = sumCount > 0 ? sumPriceCount / sumCount : last;

    // Returns = (last - first) / first
    const returns = first > 0 ? (last - first) / first : 0;

    // Volatility = standard deviation of prices
    const mean = prices.reduce((a, b) => a + b, 0) / prices.length;
    const variance = prices.reduce((sum, p) => sum + Math.pow(p - mean, 2), 0) / prices.length;
    const volatility = Math.sqrt(variance);

    this.state = {
      ticker,
      last,
      high,
      low,
      vwap,
      returns,
      volatility,
      tradeCount: this.trades.length,
      lastUpdate: ts,
    };
  }

  getState(): PriceHistoryState {
    return { ...this.state };
  }

  reset(): void {
    this.trades = [];
    this.state = this.initialState();
  }

  private initialState(): PriceHistoryState {
    return {
      ticker: "",
      last: 0,
      high: 0,
      low: 0,
      vwap: 0,
      returns: 0,
      volatility: 0,
      tradeCount: 0,
      lastUpdate: 0,
    };
  }
}
