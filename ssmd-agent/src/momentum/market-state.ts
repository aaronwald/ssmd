import type { MarketRecord } from "../state/types.ts";

interface PriceSnapshot {
  price: number;
  ts: number;
}

interface VolumeSnapshot {
  volume: number;
  dollarVolume: number;
  ts: number;
}

interface TradeRecord {
  side: string;
  count: number;
  price: number;
  ts: number;
}

export interface TradeFlow {
  yesVolume: number;
  noVolume: number;
  totalTrades: number;
  dominance: number;
  dominantSide: "yes" | "no";
}

export interface VolumeRate {
  contractVolume: number;
  dollarVolume: number;
  perMinuteRate: number;
}

// All timestamps (ts) are Unix seconds. All window parameters are in seconds.
export class MarketState {
  readonly ticker: string;
  lastPrice = 0;
  yesBid = 0;
  yesAsk = 0;
  noBid = 0;
  noAsk = 0;
  lastTs = 0;
  closeTs: number | null = null;

  private priceSnapshots: PriceSnapshot[] = [];
  private volumeSnapshots: VolumeSnapshot[] = [];
  private trades: TradeRecord[] = [];
  private prevVolume = 0;
  private prevDollarVolume = 0;
  private initialized = false;
  private readonly maxRetentionSec = 30 * 60;

  constructor(ticker: string) {
    this.ticker = ticker;
  }

  update(record: MarketRecord): void {
    this.lastTs = record.ts;
    if (record.type === "ticker") {
      this.updateFromTicker(record);
    } else if (record.type === "trade") {
      this.updateFromTrade(record);
    }
    this.trimOldData();
  }

  private updateFromTicker(record: MarketRecord): void {
    const price = (record.price as number) ?? 0;
    const volume = (record.volume as number) ?? 0;
    const dollarVolume = (record.dollar_volume as number) ?? 0;

    if (price > 0) this.lastPrice = price;
    if (record.yes_bid !== undefined) this.yesBid = record.yes_bid;
    if (record.yes_ask !== undefined) this.yesAsk = record.yes_ask;
    if (record.no_bid !== undefined) this.noBid = record.no_bid;
    if (record.no_ask !== undefined) this.noAsk = record.no_ask;

    if (price > 0) {
      this.priceSnapshots.push({ price, ts: record.ts });
    }

    if (!this.initialized) {
      this.prevVolume = volume;
      this.prevDollarVolume = dollarVolume;
      this.initialized = true;
      return;
    }

    const volumeDelta = volume - this.prevVolume;
    const dollarDelta = dollarVolume - this.prevDollarVolume;

    if (volumeDelta > 0 || dollarDelta > 0) {
      this.volumeSnapshots.push({
        volume: Math.max(0, volumeDelta),
        dollarVolume: Math.max(0, dollarDelta),
        ts: record.ts,
      });
    }

    this.prevVolume = volume;
    this.prevDollarVolume = dollarVolume;
  }

  private updateFromTrade(record: MarketRecord): void {
    const side = (record.side as string) ?? "unknown";
    const count = (record.count as number) ?? 0;
    const price = (record.price as number) ?? 0;

    if (price > 0) this.lastPrice = price;

    if (side === "yes" || side === "no") {
      this.trades.push({ side, count, price, ts: record.ts });
    }
  }

  private trimOldData(): void {
    const cutoff = this.lastTs - this.maxRetentionSec;
    this.priceSnapshots = this.priceSnapshots.filter(s => s.ts >= cutoff);
    this.volumeSnapshots = this.volumeSnapshots.filter(s => s.ts >= cutoff);
    this.trades = this.trades.filter(t => t.ts >= cutoff);
  }

  isActivated(thresholdDollars: number, windowSec: number): boolean {
    const rate = this.getVolumeRate(windowSec);
    return rate.dollarVolume >= thresholdDollars;
  }

  getVolumeRate(windowSec: number): VolumeRate {
    const cutoff = this.lastTs - windowSec;
    const inWindow = this.volumeSnapshots.filter(s => s.ts >= cutoff);

    let contractVolume = 0;
    let dollarVolume = 0;
    for (const s of inWindow) {
      contractVolume += s.volume;
      dollarVolume += s.dollarVolume;
    }

    const windowMinutes = windowSec / 60;
    return {
      contractVolume,
      dollarVolume,
      perMinuteRate: windowMinutes > 0 ? dollarVolume / windowMinutes : 0,
    };
  }

  getPriceChange(windowSec: number): number {
    const cutoff = this.lastTs - windowSec;
    const inWindow = this.priceSnapshots.filter(s => s.ts >= cutoff);
    if (inWindow.length < 2) return 0;
    return inWindow[inWindow.length - 1].price - inWindow[0].price;
  }

  getPriceRatePerMinute(windowSec: number): number {
    const cutoff = this.lastTs - windowSec;
    const inWindow = this.priceSnapshots.filter(s => s.ts >= cutoff);
    if (inWindow.length < 2) return 0;

    const first = inWindow[0];
    const last = inWindow[inWindow.length - 1];
    const elapsedMinutes = (last.ts - first.ts) / 60;
    if (elapsedMinutes <= 0) return 0;

    return (last.price - first.price) / elapsedMinutes;
  }

  getTradeFlow(windowSec: number): TradeFlow {
    const cutoff = this.lastTs - windowSec;
    const inWindow = this.trades.filter(t => t.ts >= cutoff);

    let yesVolume = 0;
    let noVolume = 0;
    for (const t of inWindow) {
      if (t.side === "yes") yesVolume += t.count;
      else if (t.side === "no") noVolume += t.count;
    }

    const total = yesVolume + noVolume;
    const dominantSide = yesVolume >= noVolume ? "yes" as const : "no" as const;
    const dominance = total > 0 ? Math.max(yesVolume, noVolume) / total : 0.5;

    return {
      yesVolume,
      noVolume,
      totalTrades: inWindow.length,
      dominance,
      dominantSide,
    };
  }
}
