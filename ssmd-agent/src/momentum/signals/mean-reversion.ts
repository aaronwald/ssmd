import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface MeanReversionConfig {
  anchorWindowMinutes: number;
  deviationThresholdCents: number;
  maxDeviationCents: number;
  recentWindowSec: number;
  stallWindowSec: number;
  minRecentChangeCents: number;
  minTrades: number;
  weight: number;
}

const ZERO: SignalResult = { name: "mean-reversion", score: 0, confidence: 0, reason: "" };

export class MeanReversion implements Signal {
  readonly name = "mean-reversion";
  private readonly config: MeanReversionConfig;

  constructor(config: MeanReversionConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const windowSec = this.config.anchorWindowMinutes * 60;
    const history = state.getSpreadHistory(windowSec);

    // Need sufficient data for anchor computation
    if (history.length < 5) return ZERO;

    // Use first 70% of window as baseline for anchor (exclude recent spike)
    const baselineCount = Math.max(3, Math.floor(history.length * 0.7));
    const baseline = history.slice(0, baselineCount);
    const anchor = baseline.reduce((sum, s) => sum + s.midpoint, 0) / baseline.length;

    // Deviation from anchor
    const deviation = state.lastPrice - anchor;
    const absDeviation = Math.abs(deviation);

    // Not overextended enough
    if (absDeviation < this.config.deviationThresholdCents) return ZERO;

    // Check move is recent (not stale)
    const recentChange = Math.abs(state.getPriceChange(this.config.recentWindowSec));
    if (recentChange < this.config.minRecentChangeCents) return ZERO;

    // Check move has stalled (don't catch falling knife)
    const veryRecentChange = state.getPriceChange(this.config.stallWindowSec);
    // If still moving away from anchor, skip
    if (deviation > 0 && veryRecentChange > 0) return ZERO;
    if (deviation < 0 && veryRecentChange < 0) return ZERO;

    // Score: opposite to deviation (revert toward anchor)
    const direction = deviation > 0 ? -1 : 1;
    const magnitude = Math.min(absDeviation / this.config.maxDeviationCents, 1.0);
    const score = direction * magnitude;

    // Confidence: speed of move * trade activity
    const speedFactor = Math.min(recentChange / absDeviation, 1.0);
    const flow = state.getTradeFlow(this.config.recentWindowSec);
    const tradeFactor = Math.min(flow.totalTrades / this.config.minTrades, 1.0);
    const confidence = speedFactor * tradeFactor;

    const side = direction > 0 ? "YES" : "NO";
    const reason = `price ${state.lastPrice}c deviated ${absDeviation.toFixed(1)}c from anchor ${anchor.toFixed(1)}c, reverting ${side}`;

    return { name: this.name, score, confidence, reason };
  }
}
