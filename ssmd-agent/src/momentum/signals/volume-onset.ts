import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface VolumeOnsetConfig {
  recentWindowSec: number;
  baselineWindowMinutes: number;
  onsetMultiplier: number;
  weight: number;
}

const ZERO: SignalResult = { name: "volume-onset", score: 0, confidence: 0, reason: "" };

export class VolumeOnset implements Signal {
  readonly name = "volume-onset";
  private readonly config: VolumeOnsetConfig;

  constructor(config: VolumeOnsetConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const recentSec = this.config.recentWindowSec;
    const baselineSec = this.config.baselineWindowMinutes * 60;

    // Step 1: Get volume rates for recent and full windows
    const recentRate = state.getVolumeRate(recentSec);
    const fullRate = state.getVolumeRate(baselineSec);

    // Step 2: Compute baseline rate excluding recent window
    const baselineDollar = fullRate.dollarVolume - recentRate.dollarVolume;

    // Step 3: Normalize to per-recentWindow rate
    const baselineWindowSec = baselineSec - recentSec;
    if (baselineWindowSec <= 0) return ZERO;
    const baselinePer30s = (baselineDollar / baselineWindowSec) * recentSec;

    // Step 4: If baselinePer30s <= 0, return ZERO
    if (baselinePer30s <= 0) return ZERO;

    // Step 5: Compute ratio
    const ratio = recentRate.dollarVolume / baselinePer30s;

    // Step 6: If ratio < onsetMultiplier, return ZERO
    if (ratio < this.config.onsetMultiplier) return ZERO;

    // Step 7: Direction from trade flow
    const trades = state.getRecentTrades(recentSec);
    let yesCount = 0;
    let noCount = 0;
    for (const t of trades) {
      if (t.side === "yes") yesCount += t.count;
      else if (t.side === "no") noCount += t.count;
    }
    const total = yesCount + noCount;

    // Step 8: If no trades, return ZERO
    if (total === 0) return ZERO;

    const direction = yesCount >= noCount ? 1 : -1;
    const dominantSide = yesCount >= noCount ? "YES" : "NO";

    // Step 9: Score magnitude
    const magnitude = Math.min((ratio - 1) / 3, 1);

    // Step 10: Confidence
    const baselineBuckets = baselineWindowSec / recentSec;
    const dominance = Math.max(yesCount, noCount) / total;
    const confidence = Math.min(baselineBuckets / 10, 1) * dominance;

    const score = direction * magnitude;

    // Step 11: Reason string
    const reason = `Volume ${ratio.toFixed(1)}x baseline in ${recentSec}s, ${dominantSide} flow ${(dominance * 100).toFixed(0)}%`;

    return { name: this.name, score, confidence, reason };
  }
}
