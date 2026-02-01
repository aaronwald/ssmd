import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface PriceMomentumConfig {
  shortWindowSec: number;
  midWindowSec: number;
  longWindowSec: number;
  minTotalMoveCents: number;
  maxAccelRatio: number;
  minEntryPrice: number;
  maxEntryPrice: number;
  minTrades: number;
  weight: number;
}

const ZERO: SignalResult = { name: "price-momentum", score: 0, confidence: 0, reason: "" };

export class PriceMomentum implements Signal {
  readonly name = "price-momentum";
  private readonly config: PriceMomentumConfig;

  constructor(config: PriceMomentumConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const shortRate = state.getPriceRatePerMinute(this.config.shortWindowSec);
    const midRate = state.getPriceRatePerMinute(this.config.midWindowSec);
    const longRate = state.getPriceRatePerMinute(this.config.longWindowSec);

    // All three windows must agree on direction
    if (shortRate === 0 || midRate === 0 || longRate === 0) return ZERO;
    if (!sameSign(shortRate, midRate) || !sameSign(shortRate, longRate)) return ZERO;

    // Minimum total movement
    const totalMove = Math.abs(state.getPriceChange(this.config.longWindowSec));
    if (totalMove < this.config.minTotalMoveCents) return ZERO;

    // Acceleration check: short window rate must be >= long window rate
    const shortPerMin = Math.abs(shortRate);
    const longPerMin = Math.abs(longRate);
    if (longPerMin === 0) return ZERO;
    const accelRatio = shortPerMin / longPerMin;
    if (accelRatio < 1.0) return ZERO; // Decelerating

    // Remaining room check (tighter band for momentum)
    const price = state.lastPrice;
    if (shortRate > 0 && price > this.config.maxEntryPrice) return ZERO;
    if (shortRate < 0 && price < this.config.minEntryPrice) return ZERO;

    // Trade flow confirmation
    const flow = state.getTradeFlow(this.config.shortWindowSec);
    if (flow.totalTrades < this.config.minTrades) return ZERO;

    const priceDirection = shortRate > 0 ? "yes" : "no";
    if (flow.dominantSide !== priceDirection) return ZERO;

    // Score
    const direction = shortRate > 0 ? 1 : -1;
    const magnitude = Math.min(accelRatio / this.config.maxAccelRatio, 1.0);
    const score = direction * magnitude;

    // Confidence
    const flowAlignment = flow.dominance; // 0.5-1.0
    const midPerMin = Math.abs(midRate);
    const midShortRatio = shortPerMin > 0 ? Math.min(midPerMin / shortPerMin, 1.0) : 0;
    const confidence = flowAlignment * midShortRatio;

    const side = direction > 0 ? "YES" : "NO";
    const reason = `momentum ${side}: ${totalMove.toFixed(1)}c over ${this.config.longWindowSec}s, accel ${accelRatio.toFixed(1)}x, flow ${(flow.dominance * 100).toFixed(0)}% ${flow.dominantSide}`;

    return { name: this.name, score, confidence, reason };
  }
}

function sameSign(a: number, b: number): boolean {
  return (a > 0 && b > 0) || (a < 0 && b < 0);
}
