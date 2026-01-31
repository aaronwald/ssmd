import type { MarketState } from "../market-state.ts";

export interface SignalResult {
  name: string;
  score: number;       // -1.0 (strong NO) to +1.0 (strong YES)
  confidence: number;  // 0.0 to 1.0
  reason: string;
}

export interface Signal {
  readonly name: string;
  evaluate(state: MarketState): SignalResult;
}

export interface ComposerDecision {
  enter: boolean;
  side: "yes" | "no";
  price: number;
  score: number;
  signals: SignalResult[];
}
