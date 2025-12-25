---
name: monitor-spread
description: Generate spread monitoring signals for prediction markets
---

# Spread Monitoring Signals

Use when user wants alerts on bid-ask spread widening.

## Workflow

1. `sample_data` with type="orderbook" to get orderbook records
2. `orderbook_builder` to see spread distribution
3. Generate signal with appropriate threshold
4. `run_backtest` to validate fire frequency
5. Adjust threshold if needed

## Template

```typescript
export const signal = {
  id: "{{ticker}}-spread-alert",
  name: "{{ticker}} Spread Alert",
  requires: ["orderbook"],

  evaluate(state: { orderbook: OrderBookState }): boolean {
    return state.orderbook.ticker.startsWith("{{ticker}}")
        && state.orderbook.spreadPercent > {{threshold}};
  },

  payload(state: { orderbook: OrderBookState }) {
    return {
      ticker: state.orderbook.ticker,
      spread: state.orderbook.spread,
      spreadPercent: state.orderbook.spreadPercent,
      bestBid: state.orderbook.bestBid,
      bestAsk: state.orderbook.bestAsk,
    };
  },
};
```

## Thresholds

- 0.03 (3%): Catches most spread widening, may be noisy
- 0.05 (5%): Good default for prediction markets
- 0.10 (10%): Only catches significant events
