import type { MarketState } from "../market-state.ts";
import type { EntrySignal, MomentumModel } from "./types.ts";

export interface PriceAccelerationConfig {
  accelerationMultiplier: number;
  shortWindowMinutes: number;
  longWindowMinutes: number;
  minShortRateCentsPerMin: number;
  minLongMoveCents: number;
}

export class PriceAcceleration implements MomentumModel {
  readonly name = "price-acceleration";
  private config: PriceAccelerationConfig;

  constructor(config: PriceAccelerationConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): EntrySignal | null {
    const shortSec = this.config.shortWindowMinutes * 60;
    const longSec = this.config.longWindowMinutes * 60;

    const shortRate = state.getPriceRatePerMinute(shortSec);
    const longRate = state.getPriceRatePerMinute(longSec);
    const longMove = state.getPriceChange(longSec);

    if (shortRate === 0 || longRate === 0) return null;
    if ((shortRate > 0) !== (longRate > 0)) return null;

    if (Math.abs(longMove) < this.config.minLongMoveCents) return null;
    if (Math.abs(shortRate) < this.config.minShortRateCentsPerMin) return null;

    const ratio = Math.abs(shortRate) / Math.abs(longRate);
    if (ratio < this.config.accelerationMultiplier) return null;

    const side = shortRate > 0 ? "yes" as const : "no" as const;
    const price = side === "yes" ? state.yesAsk : state.noBid;

    if (price <= 0) return null;

    return {
      model: this.name,
      ticker: state.ticker,
      side,
      price,
      reason: `Price accelerating ${ratio.toFixed(1)}x (${shortRate.toFixed(1)}c/min vs ${longRate.toFixed(1)}c/min avg)`,
    };
  }
}
