---
name: custom-signal
description: Template for custom signal logic
---

# Custom Signals

For signals that don't fit standard templates.

## Signal Interface

```typescript
export const signal = {
  id: string,           // Unique kebab-case identifier
  name: string,         // Human-readable name
  requires: string[],   // State builders needed: ["orderbook"]

  evaluate(state): boolean,  // Return true to fire
  payload(state): object,    // Data to include when fired
};
```

## State Fields

### OrderBook (state.orderbook)
- ticker: string
- bestBid: number
- bestAsk: number
- spread: number (ask - bid)
- spreadPercent: number (spread / ask)
- lastUpdate: number (Unix ms)

## Combining Conditions

```typescript
evaluate(state) {
  const book = state.orderbook;
  return book.spread > 0.05
      && book.ticker.startsWith("INXD")
      && book.bestBid > 0.20;
}
```

## Adding Cooldown (manual tracking)

```typescript
let lastFire = 0;
const COOLDOWN_MS = 60000; // 1 minute

evaluate(state) {
  if (Date.now() - lastFire < COOLDOWN_MS) return false;
  if (state.orderbook.spread > 0.05) {
    lastFire = Date.now();
    return true;
  }
  return false;
}
```
