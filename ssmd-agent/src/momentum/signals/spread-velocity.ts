import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface SpreadVelocityConfig {
  windowSec: number;
  minSnapshots: number;
  velocityThreshold: number;
  weight: number;
}

const ZERO: SignalResult = { name: "spread-velocity", score: 0, confidence: 0, reason: "" };

export class SpreadVelocity implements Signal {
  readonly name = "spread-velocity";
  private readonly config: SpreadVelocityConfig;

  constructor(config: SpreadVelocityConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const snapshots = state.getSpreadHistory(this.config.windowSec);
    if (snapshots.length < this.config.minSnapshots) return ZERO;

    // Linear regression: slope of spread over time
    const n = snapshots.length;
    let sumT = 0, sumS = 0, sumTS = 0, sumTT = 0, sumSS = 0;
    const t0 = snapshots[0].ts;
    for (const s of snapshots) {
      const t = s.ts - t0;
      sumT += t;
      sumS += s.spread;
      sumTS += t * s.spread;
      sumTT += t * t;
      sumSS += s.spread * s.spread;
    }

    const denomT = n * sumTT - sumT * sumT;
    if (denomT === 0) return ZERO;

    const slope = (n * sumTS - sumT * sumS) / denomT;

    // Slope is cents/second — negative = tightening
    if (Math.abs(slope) < this.config.velocityThreshold) return ZERO;

    // R² for confidence
    const meanS = sumS / n;
    const ssTot = sumSS - n * meanS * meanS;
    const intercept = (sumS - slope * sumT) / n;
    let ssRes = 0;
    for (const s of snapshots) {
      const t = s.ts - t0;
      const predicted = intercept + slope * t;
      ssRes += (s.spread - predicted) * (s.spread - predicted);
    }
    const rSquared = ssTot > 0 ? 1 - ssRes / ssTot : 0;

    // Direction from midpoint shift
    const firstMid = snapshots[0].midpoint;
    const lastMid = snapshots[n - 1].midpoint;
    const midShift = lastMid - firstMid;
    if (midShift === 0) return ZERO;

    const direction = midShift > 0 ? 1 : -1;

    // Score: normalize slope magnitude (cap at 0.5 cents/sec)
    const magnitude = Math.min(Math.abs(slope) / 0.5, 1);
    const score = direction * magnitude;

    // Confidence: R² of the regression
    const confidence = Math.max(rSquared, 0);

    const side = direction > 0 ? "YES" : "NO";
    const reason = `${side} spread velocity ${slope.toFixed(3)}c/s, R²=${rSquared.toFixed(2)} (${n} snapshots)`;

    return { name: this.name, score, confidence, reason };
  }
}
