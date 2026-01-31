import type { MarketState } from "../market-state.ts";
import type { EntrySignal, MomentumModel } from "./types.ts";

export interface TradeFlowImbalanceConfig {
  dominanceThreshold: number;
  windowMinutes: number;
  minTrades: number;
  minPriceMoveCents: number;
}

export class TradeFlowImbalance implements MomentumModel {
  readonly name = "trade-flow";
  private config: TradeFlowImbalanceConfig;

  constructor(config: TradeFlowImbalanceConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): EntrySignal | null {
    const windowSec = this.config.windowMinutes * 60;
    const flow = state.getTradeFlow(windowSec);

    if (flow.totalTrades < this.config.minTrades) return null;
    if (flow.dominance < this.config.dominanceThreshold) return null;

    const priceChange = state.getPriceChange(windowSec);
    const flowUp = flow.dominantSide === "yes";
    const priceUp = priceChange > 0;

    if (flowUp !== priceUp) return null;
    if (Math.abs(priceChange) < this.config.minPriceMoveCents) return null;

    const side = flow.dominantSide;
    const price = side === "yes" ? state.yesAsk : state.noBid;

    if (price <= 0) return null;

    return {
      model: this.name,
      ticker: state.ticker,
      side,
      price,
      reason: `${(flow.dominance * 100).toFixed(0)}% ${side} flow (${flow.totalTrades} trades), price ${priceChange > 0 ? "+" : ""}${priceChange}c`,
    };
  }
}
