// ssmd-agent/src/state/orderbook.ts
import type { MarketRecord, StateBuilder } from "./types.ts";

export interface OrderBookState {
  ticker: string;
  bestBid: number;
  bestAsk: number;
  spread: number;
  spreadPercent: number;
  lastUpdate: number;
}

export class OrderBookBuilder implements StateBuilder<OrderBookState> {
  id = "orderbook";
  private state: OrderBookState = this.initialState();

  update(record: MarketRecord): void {
    // Only process orderbook or ticker messages
    if (record.type !== "orderbook" && record.type !== "ticker") return;

    const yesBid = record.yes_bid ?? 0;
    const yesAsk = record.yes_ask ?? 0;

    this.state = {
      ticker: record.ticker,
      bestBid: yesBid,
      bestAsk: yesAsk,
      spread: yesAsk - yesBid,
      spreadPercent: yesAsk > 0 ? (yesAsk - yesBid) / yesAsk : 0,
      lastUpdate: record.ts,
    };
  }

  getState(): OrderBookState {
    return { ...this.state };
  }

  reset(): void {
    this.state = this.initialState();
  }

  private initialState(): OrderBookState {
    return {
      ticker: "",
      bestBid: 0,
      bestAsk: 0,
      spread: 0,
      spreadPercent: 0,
      lastUpdate: 0,
    };
  }
}
