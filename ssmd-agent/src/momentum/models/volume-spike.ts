import type { MarketState } from "../market-state.ts";
import type { EntrySignal, MomentumModel } from "./types.ts";

export interface VolumeSpikeMomentumConfig {
  spikeMultiplier: number;
  spikeWindowMinutes: number;
  baselineWindowMinutes: number;
  minPriceMoveCents: number;
}

export class VolumeSpikeMomentum implements MomentumModel {
  readonly name = "volume-spike";
  private config: VolumeSpikeMomentumConfig;

  constructor(config: VolumeSpikeMomentumConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): EntrySignal | null {
    const spikeSec = this.config.spikeWindowMinutes * 60;
    const baselineSec = this.config.baselineWindowMinutes * 60;

    const spikeRate = state.getVolumeRate(spikeSec);
    const fullRate = state.getVolumeRate(baselineSec);

    // Compute baseline rate excluding the spike window to avoid self-comparison.
    // Without this, sparse data (e.g., 1 minute of data in a 10-minute window)
    // produces artificially low baseline rates and false spike detections.
    const remainingMinutes = this.config.baselineWindowMinutes - this.config.spikeWindowMinutes;
    if (remainingMinutes <= 0) return null;

    const baselineDollarVol = fullRate.dollarVolume - spikeRate.dollarVolume;
    const baselinePerMinute = baselineDollarVol / remainingMinutes;

    if (baselinePerMinute <= 0) return null;

    const ratio = spikeRate.perMinuteRate / baselinePerMinute;
    if (ratio < this.config.spikeMultiplier) return null;

    const priceChange = state.getPriceChange(spikeSec);
    if (Math.abs(priceChange) < this.config.minPriceMoveCents) return null;

    const side = priceChange > 0 ? "yes" as const : "no" as const;
    const price = side === "yes" ? state.yesAsk : state.noBid;

    if (price <= 0) return null;

    return {
      model: this.name,
      ticker: state.ticker,
      side,
      price,
      reason: `Volume ${ratio.toFixed(1)}x baseline, price ${priceChange > 0 ? "+" : ""}${priceChange}c in ${this.config.spikeWindowMinutes}min`,
    };
  }
}
