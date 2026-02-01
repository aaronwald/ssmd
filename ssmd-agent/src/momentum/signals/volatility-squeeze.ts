import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface VolatilitySqueezeConfig {
  squeezeWindowMinutes: number;
  compressionThreshold: number;  // e.g. 0.4 = recent stddev < 40% of baseline
  expansionThreshold: number;    // e.g. 1.5 = recent stddev > 150% of baseline
  minBaselineStdDev: number;     // filter dead markets
  maxExpansionRatio: number;     // caps score magnitude
  minSnapshots: number;
  weight: number;
}

interface SqueezeState {
  inSqueeze: boolean;
  lowestRatio: number;
  enteredAt: number;
}

const ZERO: SignalResult = { name: "volatility-squeeze", score: 0, confidence: 0, reason: "" };

function stddev(values: number[]): number {
  if (values.length < 2) return 0;
  const mean = values.reduce((s, v) => s + v, 0) / values.length;
  const variance = values.reduce((s, v) => s + (v - mean) ** 2, 0) / (values.length - 1);
  return Math.sqrt(variance);
}

export class VolatilitySqueeze implements Signal {
  readonly name = "volatility-squeeze";
  private readonly config: VolatilitySqueezeConfig;
  private readonly squeezeStates = new Map<string, SqueezeState>();

  constructor(config: VolatilitySqueezeConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const windowSec = this.config.squeezeWindowMinutes * 60;
    const history = state.getSpreadHistory(windowSec);

    if (history.length < this.config.minSnapshots) return ZERO;

    // Extract prices (midpoints) for stddev calculation
    const prices = history.map(s => s.midpoint);

    // Split into baseline (first 70%) and recent (last 30%)
    const splitIdx = Math.min(Math.floor(prices.length * 0.7) + 1, prices.length - 1);
    const baselinePrices = prices.slice(0, splitIdx);
    const recentPrices = prices.slice(splitIdx);

    if (baselinePrices.length < 2 || recentPrices.length < 2) return ZERO;

    const baselineStdDev = stddev(baselinePrices);
    const recentStdDev = stddev(recentPrices);

    // Filter dead markets
    if (baselineStdDev < this.config.minBaselineStdDev) return ZERO;

    const squeezeRatio = recentStdDev / baselineStdDev;
    const ticker = state.ticker;

    let ss = this.squeezeStates.get(ticker);
    if (!ss) {
      ss = { inSqueeze: false, lowestRatio: 1, enteredAt: 0 };
      this.squeezeStates.set(ticker, ss);
    }

    // State machine
    if (squeezeRatio < this.config.compressionThreshold) {
      // Entering or continuing compression
      if (!ss.inSqueeze) {
        ss.inSqueeze = true;
        ss.lowestRatio = squeezeRatio;
        ss.enteredAt = state.lastTs;
      } else {
        ss.lowestRatio = Math.min(ss.lowestRatio, squeezeRatio);
      }
      return ZERO; // Wait for expansion
    }

    if (ss.inSqueeze && squeezeRatio > this.config.expansionThreshold) {
      // Breakout detected
      const lowestRatio = ss.lowestRatio;

      // Reset state
      ss.inSqueeze = false;
      ss.lowestRatio = 1;

      // Direction from recent price change
      const recentPriceChange = state.getPriceChange(60);
      if (recentPriceChange === 0) return ZERO;

      const direction = recentPriceChange > 0 ? 1 : -1;
      const magnitude = Math.min(squeezeRatio / this.config.maxExpansionRatio, 1.0);
      const score = direction * magnitude;

      // Confidence: how tight the squeeze was
      const tightness = 1 - (lowestRatio / this.config.compressionThreshold);
      const dataDensity = Math.min(history.length / 20, 1.0);
      const confidence = Math.max(0, tightness) * dataDensity;

      const side = direction > 0 ? "YES" : "NO";
      const reason = `squeeze breakout: ratio ${squeezeRatio.toFixed(2)} (was ${lowestRatio.toFixed(2)}), ${side} direction`;

      return { name: this.name, score, confidence, reason };
    }

    // Normal state or expansion without prior compression — reset if squeeze expired
    if (ss.inSqueeze) {
      // Auto-expire squeeze if too old (> 2x window)
      const maxAge = windowSec * 2;
      if (state.lastTs - ss.enteredAt > maxAge) {
        ss.inSqueeze = false;
        ss.lowestRatio = 1;
      }
    }

    return ZERO;
  }
}
