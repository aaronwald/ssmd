import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface TradeConcentrationConfig {
  windowSec: number;
  minTrades: number;
  concentrationThreshold: number;
  weight: number;
}

const ZERO: SignalResult = { name: "trade-concentration", score: 0, confidence: 0, reason: "" };

export class TradeConcentration implements Signal {
  readonly name = "trade-concentration";
  private readonly config: TradeConcentrationConfig;

  constructor(config: TradeConcentrationConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const trades = state.getRecentTrades(this.config.windowSec);
    if (trades.length < this.config.minTrades) return ZERO;

    // Compute total contracts
    let totalCount = 0;
    for (const t of trades) totalCount += t.count;
    if (totalCount === 0) return ZERO;

    // HHI = sum of squared market shares
    let hhi = 0;
    for (const t of trades) {
      const share = t.count / totalCount;
      hhi += share * share;
    }

    // Baseline HHI for N equal trades = 1/N
    const baseline = 1 / trades.length;
    if (hhi < this.config.concentrationThreshold) return ZERO;

    // Direction: weight by count per side
    let yesWeight = 0;
    let noWeight = 0;
    for (const t of trades) {
      if (t.side === "yes") yesWeight += t.count;
      else if (t.side === "no") noWeight += t.count;
    }
    if (yesWeight === 0 && noWeight === 0) return ZERO;
    const direction = yesWeight >= noWeight ? 1 : -1;

    // Score: normalize HHI (baseline→0, 1.0→1.0)
    const normalizedHhi = Math.min((hhi - baseline) / (1 - baseline), 1);
    const score = direction * normalizedHhi;

    // Confidence: based on total volume
    const confidence = Math.min(totalCount / 100, 1.0);

    const side = direction > 0 ? "YES" : "NO";
    const reason = `${side} concentration HHI=${hhi.toFixed(3)} (${trades.length} trades, ${totalCount} contracts)`;

    return { name: this.name, score, confidence, reason };
  }
}
