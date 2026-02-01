import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface FlowAsymmetryConfig {
  windowSec: number;
  minTrades: number;
  asymmetryThreshold: number;
  weight: number;
}

const ZERO: SignalResult = { name: "flow-asymmetry", score: 0, confidence: 0, reason: "" };

export class FlowAsymmetry implements Signal {
  readonly name = "flow-asymmetry";
  private readonly config: FlowAsymmetryConfig;

  constructor(config: FlowAsymmetryConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const trades = state.getRecentTrades(this.config.windowSec);
    if (trades.length < this.config.minTrades) return ZERO;

    // Split by side
    let yesWeightedPrice = 0, yesContracts = 0;
    let noWeightedPrice = 0, noContracts = 0;
    for (const t of trades) {
      if (t.side === "yes") {
        yesWeightedPrice += t.price * t.count;
        yesContracts += t.count;
      } else if (t.side === "no") {
        noWeightedPrice += t.price * t.count;
        noContracts += t.count;
      }
    }

    // Need trades on both sides
    if (yesContracts === 0 || noContracts === 0) return ZERO;

    const avgYesPrice = yesWeightedPrice / yesContracts;
    const avgNoPrice = noWeightedPrice / noContracts;

    // In Kalshi: YES + NO ≈ 100. Implied YES from NO side = 100 - avgNoPrice
    const impliedYesFromNo = 100 - avgNoPrice;
    const asymmetry = avgYesPrice - impliedYesFromNo;

    if (Math.abs(asymmetry) < this.config.asymmetryThreshold) return ZERO;

    // Direction: positive asymmetry = YES conviction
    const direction = asymmetry > 0 ? 1 : -1;

    // Score: normalize asymmetry (cap at 10 cents)
    const magnitude = Math.min(Math.abs(asymmetry) / 10, 1);
    const score = direction * magnitude;

    // Confidence: more trades = higher confidence
    const confidence = Math.min(trades.length / 20, 1.0);

    const side = direction > 0 ? "YES" : "NO";
    const reason = `${side} flow asymmetry ${asymmetry.toFixed(1)}c (avgYes=${avgYesPrice.toFixed(1)}, impliedYes=${impliedYesFromNo.toFixed(1)}, ${trades.length} trades)`;

    return { name: this.name, score, confidence, reason };
  }
}
