import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface VolumePriceDivergenceConfig {
  windowSec: number;
  baselineWindowSec: number;
  volumeMultiplier: number;
  maxPriceMoveCents: number;
  minTrades: number;
  weight: number;
}

const ZERO: SignalResult = { name: "volume-price-divergence", score: 0, confidence: 0, reason: "" };

export class VolumePriceDivergence implements Signal {
  readonly name = "volume-price-divergence";
  private readonly config: VolumePriceDivergenceConfig;

  constructor(config: VolumePriceDivergenceConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const recentRate = state.getVolumeRate(this.config.windowSec);
    const baselineRate = state.getVolumeRate(this.config.baselineWindowSec);

    // Need baseline volume to compare against
    if (baselineRate.perMinuteRate <= 0) return ZERO;

    // Compute volume ratio (recent vs baseline per-minute rate)
    const ratio = recentRate.perMinuteRate / baselineRate.perMinuteRate;
    if (ratio < this.config.volumeMultiplier) return ZERO;

    // Check that price hasn't already moved
    const priceChange = Math.abs(state.getPriceChange(this.config.windowSec));
    if (priceChange > this.config.maxPriceMoveCents) return ZERO;

    // Get trade flow for direction
    const flow = state.getTradeFlow(this.config.windowSec);
    if (flow.totalTrades < this.config.minTrades) return ZERO;

    const direction = flow.dominantSide === "yes" ? 1 : -1;

    // Score: normalize volume ratio (2x→0, 5x→1)
    const magnitude = Math.min((ratio - 1) / 4, 1);
    const score = direction * magnitude;

    // Confidence: higher ratio + flatter price = more confident
    const priceFlat = 1 - (priceChange / this.config.maxPriceMoveCents);
    const confidence = Math.min(ratio / 5, 1) * priceFlat;

    const side = flow.dominantSide.toUpperCase();
    const reason = `${side} vol-price divergence: ${ratio.toFixed(1)}x volume, ${priceChange.toFixed(0)}c price move (${flow.totalTrades} trades)`;

    return { name: this.name, score, confidence, reason };
  }
}
