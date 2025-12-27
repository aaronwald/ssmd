// ssmd-agent/src/state/volume_profile.ts
import type { MarketRecord, StateBuilder } from "./types.ts";

export interface VolumeProfileState {
  ticker: string;
  buyVolume: number;
  sellVolume: number;
  totalVolume: number;
  ratio: number; // buy / total, 0.5 = balanced
  average: number; // average trade size
  tradeCount: number;
  lastUpdate: number;
}

interface TradeVolume {
  count: number;
  side: string;
  ts: number;
}

export class VolumeProfileBuilder implements StateBuilder<VolumeProfileState> {
  id = "volumeProfile";
  private trades: TradeVolume[] = [];
  private windowMs: number;
  private state: VolumeProfileState = this.initialState();

  constructor(windowMs: number = 300000) { // default 5 minutes
    this.windowMs = windowMs;
  }

  update(record: MarketRecord): void {
    // Only process trade messages
    if (record.type !== "trade") return;

    const count = record.count ?? 1;
    const side = record.side ?? "unknown";
    const ts = record.ts;

    // Add trade
    this.trades.push({ count, side, ts });

    // Trim to time window
    const cutoff = ts - this.windowMs;
    this.trades = this.trades.filter(t => t.ts >= cutoff);

    // Recalculate derived values
    this.recalculate(record.ticker, ts);
  }

  private recalculate(ticker: string, ts: number): void {
    if (this.trades.length === 0) {
      this.state = this.initialState();
      return;
    }

    let buyVolume = 0;
    let sellVolume = 0;

    for (const t of this.trades) {
      if (t.side === "yes" || t.side === "buy") {
        buyVolume += t.count;
      } else if (t.side === "no" || t.side === "sell") {
        sellVolume += t.count;
      } else {
        // Unknown side - split evenly
        buyVolume += t.count / 2;
        sellVolume += t.count / 2;
      }
    }

    const totalVolume = buyVolume + sellVolume;
    const ratio = totalVolume > 0 ? buyVolume / totalVolume : 0.5;
    const average = this.trades.length > 0
      ? totalVolume / this.trades.length
      : 0;

    this.state = {
      ticker,
      buyVolume,
      sellVolume,
      totalVolume,
      ratio,
      average,
      tradeCount: this.trades.length,
      lastUpdate: ts,
    };
  }

  getState(): VolumeProfileState {
    return { ...this.state };
  }

  reset(): void {
    this.trades = [];
    this.state = this.initialState();
  }

  private initialState(): VolumeProfileState {
    return {
      ticker: "",
      buyVolume: 0,
      sellVolume: 0,
      totalVolume: 0,
      ratio: 0.5,
      average: 0,
      tradeCount: 0,
      lastUpdate: 0,
    };
  }
}
