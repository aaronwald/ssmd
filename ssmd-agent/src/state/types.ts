// ssmd-agent/src/state/types.ts
export interface MarketRecord {
  type: string;
  ticker: string;
  ts: number;
  yes_bid?: number;
  yes_ask?: number;
  no_bid?: number;
  no_ask?: number;
  price?: number;
  count?: number;
  side?: string;
  [key: string]: unknown;
}

export interface StateBuilder<T> {
  id: string;
  update(record: MarketRecord): void;
  getState(): T;
  reset(): void;
}
