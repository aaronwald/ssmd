import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface TradeImbalanceConfig {
  windowSec: number;
  minTrades: number;
  imbalanceThreshold: number;
  sustainedWindowSec: number;
  sustainedThreshold: number;
  weight: number;
}

const ZERO: SignalResult = { name: "trade-imbalance", score: 0, confidence: 0, reason: "" };

export class TradeImbalance implements Signal {
  readonly name = "trade-imbalance";
  private readonly config: TradeImbalanceConfig;

  constructor(config: TradeImbalanceConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    // Step 1: Get trade flow for primary window
    const flow = state.getTradeFlow(this.config.windowSec);

    // Step 2: Need enough trades for statistical meaning
    if (flow.totalTrades < this.config.minTrades) return ZERO;

    // Step 3: Check imbalance threshold
    if (flow.dominance < this.config.imbalanceThreshold) return ZERO;

    // Step 4: Confirm with sustained window (shorter, recent)
    const sustained = state.getTradeFlow(this.config.sustainedWindowSec);
    if (sustained.totalTrades < 3) return ZERO;
    if (sustained.dominantSide !== flow.dominantSide) return ZERO;
    if (sustained.dominance < this.config.sustainedThreshold) return ZERO;

    // Step 5: Direction
    const direction = flow.dominantSide === "yes" ? 1 : -1;

    // Step 6: Score — maps dominance 0.5→0, 1.0→1.0
    const magnitude = (flow.dominance - 0.5) * 2;
    const score = direction * magnitude;

    // Step 7: Confidence — more trades = higher confidence, capped at 1.0
    const tradeConfidence = Math.min(flow.totalTrades / 20, 1.0);
    const sustainedAgreement = sustained.dominance;
    const confidence = tradeConfidence * sustainedAgreement;

    const side = flow.dominantSide.toUpperCase();
    const reason = `${side} imbalance ${(flow.dominance * 100).toFixed(0)}% over ${this.config.windowSec}s (${flow.totalTrades} trades), sustained ${(sustained.dominance * 100).toFixed(0)}%`;

    return { name: this.name, score, confidence, reason };
  }
}
