import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult, ComposerDecision } from "./types.ts";

export interface ComposerConfig {
  entryThreshold: number;
  minSignals: number;
}

export class Composer {
  private readonly signals: Signal[];
  private readonly weights: number[];
  private readonly config: ComposerConfig;

  constructor(signals: Signal[], weights: number[], config: ComposerConfig) {
    this.signals = signals;
    this.weights = weights;
    this.config = config;
  }

  evaluate(state: MarketState): ComposerDecision {
    const noEntry = (signals: SignalResult[] = []): ComposerDecision =>
      ({ enter: false, side: "yes", price: 0, score: 0, signals });

    // 1. Run all signals, collect results
    const results: SignalResult[] = this.signals.map((s) => s.evaluate(state));

    // 2. Filter to non-zero scores
    const nonZero = results.filter((r) => r.score !== 0);

    // 3. Count signals agreeing on positive vs negative direction
    const positive = nonZero.filter((r) => r.score > 0);
    const negative = nonZero.filter((r) => r.score < 0);

    // 4. Pick dominant direction (positive = YES, negative = NO)
    const posCount = positive.length;
    const negCount = negative.length;

    if (posCount === 0 && negCount === 0) return noEntry();

    const side: "yes" | "no" = posCount >= negCount ? "yes" : "no";
    const agreeing = side === "yes" ? positive : negative;
    const agreeingCount = agreeing.length;

    // 5. Check that agreeing count >= minSignals
    if (agreeingCount < this.config.minSignals) return noEntry(nonZero);

    // 6. Compute weighted sum from agreeing signals: sum(|score| * confidence * weight)
    let weightedSum = 0;
    for (const r of agreeing) {
      const idx = results.indexOf(r);
      const weight = this.weights[idx];
      weightedSum += Math.abs(r.score) * r.confidence * weight;
    }

    // 7. If weightedSum < entryThreshold, no entry (include signals for diagnostics)
    if (weightedSum < this.config.entryThreshold) return noEntry(nonZero);

    // 8. Entry price: yesAsk for YES, noBid for NO
    const price = side === "yes" ? state.yesAsk : state.noBid;

    // 9. If price <= 0, no entry
    if (price <= 0) return noEntry(agreeing);

    return {
      enter: true,
      side,
      price,
      score: weightedSum,
      signals: agreeing,
    };
  }
}
