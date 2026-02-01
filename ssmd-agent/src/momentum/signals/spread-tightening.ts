import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface SpreadTighteningConfig {
  spreadWindowMinutes: number;
  narrowingThreshold: number; // 0-1, e.g. 0.5 = 50% narrowing
  weight: number;
}

const ZERO: SignalResult = { name: "spread-tightening", score: 0, confidence: 0, reason: "" };

export class SpreadTightening implements Signal {
  readonly name = "spread-tightening";
  private readonly config: SpreadTighteningConfig;

  constructor(config: SpreadTighteningConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const windowSec = this.config.spreadWindowMinutes * 60;
    const history = state.getSpreadHistory(windowSec);

    // Need at least 5 samples
    if (history.length < 5) return ZERO;

    // Split into baseline (~first 70%) and recent (~last 30%)
    // +1 ensures baseline gets at least 70% of items, avoiding boundary contamination
    const splitIdx = Math.min(Math.floor(history.length * 0.7) + 1, history.length - 1);
    const baseline = history.slice(0, splitIdx);
    const recent = history.slice(splitIdx);

    if (baseline.length === 0 || recent.length === 0) return ZERO;

    // Compute average spread for baseline and recent
    const avgBaselineSpread = baseline.reduce((sum, s) => sum + s.spread, 0) / baseline.length;
    const avgRecentSpread = recent.reduce((sum, s) => sum + s.spread, 0) / recent.length;

    // Guard against zero baseline spread (avoid division by zero)
    if (avgBaselineSpread <= 0) return ZERO;

    // Narrowing ratio = 1 - avgRecentSpread / avgBaselineSpread
    const narrowingRatio = 1 - avgRecentSpread / avgBaselineSpread;

    // If narrowing < threshold, return zero score
    if (narrowingRatio < this.config.narrowingThreshold) return ZERO;

    // Direction from midpoint shift
    const avgBaselineMidpoint = baseline.reduce((sum, s) => sum + s.midpoint, 0) / baseline.length;
    const avgRecentMidpoint = recent.reduce((sum, s) => sum + s.midpoint, 0) / recent.length;
    const midpointShift = avgRecentMidpoint - avgBaselineMidpoint;

    // Score = direction * narrowingRatio, clamped to [-1, 1]
    const direction = midpointShift > 0 ? 1 : midpointShift < 0 ? -1 : 0;
    const rawScore = direction * narrowingRatio;
    const score = Math.max(-1, Math.min(1, rawScore));

    // Confidence = min(historyLength / 20, 1)
    const confidence = Math.min(history.length / 20, 1);

    const side = direction > 0 ? "YES" : "NO";
    const reason = `spread narrowed ${(narrowingRatio * 100).toFixed(0)}% (${avgBaselineSpread.toFixed(1)}→${avgRecentSpread.toFixed(1)}), midpoint shift ${side}`;

    return { name: this.name, score, confidence, reason };
  }
}
