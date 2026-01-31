import type { MarketState } from "../market-state.ts";

export interface EntrySignal {
  model: string;
  ticker: string;
  side: "yes" | "no";
  price: number;
  reason: string;
}

export interface MomentumModel {
  readonly name: string;
  evaluate(state: MarketState): EntrySignal | null;
}
