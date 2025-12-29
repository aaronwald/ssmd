// ssmd-agent/src/state/volume_profile.ts
import type { MarketRecord, StateBuilder } from "./types.ts";

export interface VolumeProfileState {
  ticker: string;
  totalVolume: number; // contract volume in window
  dollarVolume: number; // USD volume in window
  ratio: number; // buy / total, 0.5 = balanced (placeholder)
  average: number; // average volume per update
  tradeCount: number; // number of updates in window
  lastUpdate: number;
  windowMs: number; // window size for reference
}

interface VolumeSnapshot {
  volume: number; // cumulative contract volume at this point
  dollars: number; // cumulative dollar volume at this point
  ts: number;
}

/**
 * Tracks volume over a sliding time window using ticker snapshots.
 * Calculates deltas between snapshots to determine volume within the window.
 */
export class VolumeProfileBuilder implements StateBuilder<VolumeProfileState> {
  id = "volumeProfile";
  private snapshots: VolumeSnapshot[] = [];
  private windowMs: number;
  private state: VolumeProfileState = this.initialState();
  private prevVolume = 0;
  private prevDollars = 0;
  private initialized = false; // First snapshot only establishes baseline

  constructor(windowMs: number = 300000) { // default 5 minutes
    this.windowMs = windowMs;
  }

  update(record: MarketRecord): void {
    // Process ticker messages with volume data
    if (record.type !== "ticker") return;

    const volume = (record.volume as number) ?? 0;
    const dollars = (record.dollar_volume as number) ?? 0;
    const ts = record.ts;

    // First snapshot establishes baseline - don't count as new volume
    if (!this.initialized) {
      this.prevVolume = volume;
      this.prevDollars = dollars;
      this.initialized = true;
      this.recalculate(record.ticker, ts);
      return;
    }

    // Calculate delta from previous snapshot
    const volumeDelta = volume - this.prevVolume;
    const dollarsDelta = dollars - this.prevDollars;

    // Only track positive deltas (volume can reset on new day)
    if (volumeDelta > 0 || dollarsDelta > 0) {
      this.snapshots.push({
        volume: Math.max(0, volumeDelta),
        dollars: Math.max(0, dollarsDelta),
        ts,
      });
    }

    this.prevVolume = volume;
    this.prevDollars = dollars;

    // Trim to time window
    const cutoff = ts - this.windowMs;
    this.snapshots = this.snapshots.filter(s => s.ts >= cutoff);

    // Recalculate state
    this.recalculate(record.ticker, ts);
  }

  private recalculate(ticker: string, ts: number): void {
    if (this.snapshots.length === 0) {
      this.state = this.initialState();
      this.state.ticker = ticker;
      this.state.lastUpdate = ts;
      this.state.windowMs = this.windowMs;
      return;
    }

    let totalVolume = 0;
    let dollarVolume = 0;

    for (const s of this.snapshots) {
      totalVolume += s.volume;
      dollarVolume += s.dollars;
    }

    const average = this.snapshots.length > 0
      ? dollarVolume / this.snapshots.length
      : 0;

    this.state = {
      ticker,
      totalVolume,
      dollarVolume,
      ratio: 0.5, // placeholder - no buy/sell info in ticker
      average,
      tradeCount: this.snapshots.length,
      lastUpdate: ts,
      windowMs: this.windowMs,
    };
  }

  getState(): VolumeProfileState {
    return { ...this.state };
  }

  reset(): void {
    this.snapshots = [];
    this.prevVolume = 0;
    this.prevDollars = 0;
    this.initialized = false;
    this.state = this.initialState();
  }

  private initialState(): VolumeProfileState {
    return {
      ticker: "",
      totalVolume: 0,
      dollarVolume: 0,
      ratio: 0.5,
      average: 0,
      tradeCount: 0,
      lastUpdate: 0,
      windowMs: this.windowMs,
    };
  }
}
