# New Momentum Signals Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add 5 new standalone signals (Trade Size Concentration, Cross-Side Flow Asymmetry, Bid-Ask Spread Velocity, Volume-Price Divergence, Trade Clustering) and create sweep YAMLs for each.

**Architecture:** Each signal implements the `Signal` interface (`evaluate(state: MarketState) → SignalResult`), gets a Zod schema in `config.ts`, and is registered in `runner.ts`. No changes to MarketState or Composer. All signals default to `enabled: false`.

**Tech Stack:** Deno, TypeScript, Zod schemas, `deno test`

**Worktree:** `/home/wald/repos/899bushwick/.worktrees/new-signals/ssmd`

**Test command:** `cd /home/wald/repos/899bushwick/.worktrees/new-signals/ssmd/ssmd-agent && deno test --allow-read --allow-write --allow-net --allow-env test/momentum/`

**Design doc:** `docs/plans/2026-02-01-new-signals-design.md`

---

### Task 1: Trade Size Concentration Signal

**Files:**
- Create: `ssmd-agent/src/momentum/signals/trade-concentration.ts`
- Create: `ssmd-agent/test/momentum/trade-concentration.test.ts`
- Modify: `ssmd-agent/src/momentum/config.ts:98` (add schema before closing `}).default({})`)
- Modify: `ssmd-agent/src/momentum/runner.ts:17,111` (add import and registration)

**Step 1: Add Zod schema to config.ts**

In `ssmd-agent/src/momentum/config.ts`, add after the `tradeImbalance` schema block (line 98), before the closing `}).default({})` on line 99:

```typescript
    tradeConcentration: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(1.0),
      windowSec: z.number().default(120),
      minTrades: z.number().default(5),
      concentrationThreshold: z.number().default(0.15),
    }).default({}),
```

**Step 2: Write the test file**

Create `ssmd-agent/test/momentum/trade-concentration.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { TradeConcentration } from "../../src/momentum/signals/trade-concentration.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 120,
  minTrades: 5,
  concentrationThreshold: 0.15,
  weight: 1.0,
};

Deno.test("TradeConcentration: no signal with insufficient trades", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 10, 50));
  state.update(makeTrade("TEST", 101, "yes", 10, 50));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeConcentration: no signal when trades are evenly distributed", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  // 10 trades of equal size — HHI = 1/10 = 0.10, below threshold 0.15
  for (let i = 0; i < 10; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 50));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeConcentration: fires when one large trade dominates YES side", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  // 1 large YES trade + 4 small NO trades
  state.update(makeTrade("TEST", 100, "yes", 50, 55));
  state.update(makeTrade("TEST", 101, "no", 2, 45));
  state.update(makeTrade("TEST", 102, "no", 2, 45));
  state.update(makeTrade("TEST", 103, "no", 2, 45));
  state.update(makeTrade("TEST", 104, "no", 2, 45));
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (YES direction)");
  assertEquals(result.confidence > 0, true);
  assertEquals(result.name, "trade-concentration");
});

Deno.test("TradeConcentration: fires negative when large trade dominates NO side", () => {
  const signal = new TradeConcentration(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "no", 50, 45));
  state.update(makeTrade("TEST", 101, "yes", 2, 55));
  state.update(makeTrade("TEST", 102, "yes", 2, 55));
  state.update(makeTrade("TEST", 103, "yes", 2, 55));
  state.update(makeTrade("TEST", 104, "yes", 2, 55));
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (NO direction)");
});

Deno.test("TradeConcentration: score magnitude scales with concentration", () => {
  const signal = new TradeConcentration(defaultConfig);

  // Moderate concentration
  const state1 = new MarketState("TEST");
  state1.update(makeTrade("TEST", 100, "yes", 20, 55));
  for (let i = 1; i < 6; i++) {
    state1.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  const r1 = signal.evaluate(state1);

  // High concentration
  const state2 = new MarketState("TEST");
  state2.update(makeTrade("TEST", 100, "yes", 100, 55));
  for (let i = 1; i < 6; i++) {
    state2.update(makeTrade("TEST", 100 + i, "yes", 1, 55));
  }
  const r2 = signal.evaluate(state2);

  assertEquals(r2.score > r1.score, true, "higher concentration should produce higher score");
});
```

**Step 3: Run test to verify it fails**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/trade-concentration.test.ts`
Expected: FAIL — `TradeConcentration` module not found

**Step 4: Write the signal implementation**

Create `ssmd-agent/src/momentum/signals/trade-concentration.ts`:

```typescript
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
```

**Step 5: Run test to verify it passes**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/trade-concentration.test.ts`
Expected: 5 passed, 0 failed

**Step 6: Register in runner.ts**

In `ssmd-agent/src/momentum/runner.ts`:

Add import at line 17 (after TradeImbalance import):
```typescript
import { TradeConcentration } from "./signals/trade-concentration.ts";
```

Add registration block after line 111 (after the tradeImbalance block):
```typescript
  if (config.signals.tradeConcentration.enabled) {
    signals.push(new TradeConcentration({
      windowSec: config.signals.tradeConcentration.windowSec,
      minTrades: config.signals.tradeConcentration.minTrades,
      concentrationThreshold: config.signals.tradeConcentration.concentrationThreshold,
      weight: config.signals.tradeConcentration.weight,
    }));
    weights.push(config.signals.tradeConcentration.weight);
  }
```

**Step 7: Run all momentum tests**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/`
Expected: All pass (90 existing + 5 new = 95)

**Step 8: Commit**

```bash
git add ssmd-agent/src/momentum/signals/trade-concentration.ts \
       ssmd-agent/test/momentum/trade-concentration.test.ts \
       ssmd-agent/src/momentum/config.ts \
       ssmd-agent/src/momentum/runner.ts
git commit -m "feat(signals): add trade-concentration signal (HHI-based)"
```

---

### Task 2: Cross-Side Flow Asymmetry Signal

**Files:**
- Create: `ssmd-agent/src/momentum/signals/flow-asymmetry.ts`
- Create: `ssmd-agent/test/momentum/flow-asymmetry.test.ts`
- Modify: `ssmd-agent/src/momentum/config.ts` (add schema after tradeConcentration)
- Modify: `ssmd-agent/src/momentum/runner.ts` (add import and registration)

**Step 1: Add Zod schema to config.ts**

Add after the `tradeConcentration` block:

```typescript
    flowAsymmetry: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(1.0),
      windowSec: z.number().default(120),
      minTrades: z.number().default(6),
      asymmetryThreshold: z.number().default(2),
    }).default({}),
```

**Step 2: Write the test file**

Create `ssmd-agent/test/momentum/flow-asymmetry.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { FlowAsymmetry } from "../../src/momentum/signals/flow-asymmetry.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 120,
  minTrades: 6,
  asymmetryThreshold: 2,
  weight: 1.0,
};

Deno.test("FlowAsymmetry: no signal with insufficient trades", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 5, 55));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("FlowAsymmetry: no signal when trades only on one side", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  for (let i = 0; i < 8; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("FlowAsymmetry: no signal when prices are symmetric", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  // YES trades at 55, NO trades at 45 → 100-45=55, asymmetry = 55-55 = 0
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 104 + i, "no", 5, 45));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("FlowAsymmetry: positive when YES buyers pay above implied", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  // YES buyers at 58, NO buyers at 45 → implied YES from NO = 100-45=55
  // Asymmetry = 58 - 55 = 3, above threshold
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 58));
  }
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 104 + i, "no", 5, 45));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (YES conviction)");
  assertEquals(result.name, "flow-asymmetry");
});

Deno.test("FlowAsymmetry: negative when NO buyers show conviction", () => {
  const signal = new FlowAsymmetry(defaultConfig);
  const state = new MarketState("TEST");
  // YES buyers at 55, NO buyers at 42 → implied YES from NO = 100-42=58
  // Asymmetry = 55 - 58 = -3, negative = NO conviction
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 100 + i, "yes", 5, 55));
  }
  for (let i = 0; i < 4; i++) {
    state.update(makeTrade("TEST", 104 + i, "no", 5, 42));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (NO conviction)");
});
```

**Step 3: Run test to verify it fails**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/flow-asymmetry.test.ts`
Expected: FAIL — module not found

**Step 4: Write the signal implementation**

Create `ssmd-agent/src/momentum/signals/flow-asymmetry.ts`:

```typescript
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
```

**Step 5: Run test to verify it passes**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/flow-asymmetry.test.ts`
Expected: 5 passed, 0 failed

**Step 6: Register in runner.ts**

Add import:
```typescript
import { FlowAsymmetry } from "./signals/flow-asymmetry.ts";
```

Add registration block (after tradeConcentration block):
```typescript
  if (config.signals.flowAsymmetry.enabled) {
    signals.push(new FlowAsymmetry({
      windowSec: config.signals.flowAsymmetry.windowSec,
      minTrades: config.signals.flowAsymmetry.minTrades,
      asymmetryThreshold: config.signals.flowAsymmetry.asymmetryThreshold,
      weight: config.signals.flowAsymmetry.weight,
    }));
    weights.push(config.signals.flowAsymmetry.weight);
  }
```

**Step 7: Run all momentum tests**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/`
Expected: All pass (95 + 5 = 100)

**Step 8: Commit**

```bash
git add ssmd-agent/src/momentum/signals/flow-asymmetry.ts \
       ssmd-agent/test/momentum/flow-asymmetry.test.ts \
       ssmd-agent/src/momentum/config.ts \
       ssmd-agent/src/momentum/runner.ts
git commit -m "feat(signals): add flow-asymmetry signal (cross-side price conviction)"
```

---

### Task 3: Bid-Ask Spread Velocity Signal

**Files:**
- Create: `ssmd-agent/src/momentum/signals/spread-velocity.ts`
- Create: `ssmd-agent/test/momentum/spread-velocity.test.ts`
- Modify: `ssmd-agent/src/momentum/config.ts` (add schema)
- Modify: `ssmd-agent/src/momentum/runner.ts` (add import and registration)

**Step 1: Add Zod schema to config.ts**

Add after the `flowAsymmetry` block:

```typescript
    spreadVelocity: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(0.8),
      windowSec: z.number().default(30),
      minSnapshots: z.number().default(5),
      velocityThreshold: z.number().default(0.1),
    }).default({}),
```

**Step 2: Write the test file**

Create `ssmd-agent/test/momentum/spread-velocity.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { SpreadVelocity } from "../../src/momentum/signals/spread-velocity.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTicker(ticker: string, ts: number, yesBid: number, yesAsk: number): MarketRecord {
  return { type: "ticker", ticker, ts, price: (yesBid + yesAsk) / 2, yes_bid: yesBid, yes_ask: yesAsk, volume: 0, dollar_volume: 0 };
}

const defaultConfig = {
  windowSec: 30,
  minSnapshots: 5,
  velocityThreshold: 0.1,
  weight: 0.8,
};

Deno.test("SpreadVelocity: no signal with insufficient snapshots", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTicker("TEST", 100, 45, 55));
  state.update(makeTicker("TEST", 105, 46, 54));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("SpreadVelocity: no signal when spread is stable", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  for (let i = 0; i < 6; i++) {
    state.update(makeTicker("TEST", 100 + i * 5, 45, 55));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("SpreadVelocity: fires when spread narrows rapidly with rising midpoint", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  // Spread narrows from 10 to 2, midpoint rises (YES direction)
  state.update(makeTicker("TEST", 100, 45, 55)); // spread=10, mid=50
  state.update(makeTicker("TEST", 105, 47, 55)); // spread=8, mid=51
  state.update(makeTicker("TEST", 110, 49, 55)); // spread=6, mid=52
  state.update(makeTicker("TEST", 115, 51, 55)); // spread=4, mid=53
  state.update(makeTicker("TEST", 120, 53, 55)); // spread=2, mid=54
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (rising midpoint = YES)");
  assertEquals(result.name, "spread-velocity");
});

Deno.test("SpreadVelocity: fires negative when midpoint falls during narrowing", () => {
  const signal = new SpreadVelocity(defaultConfig);
  const state = new MarketState("TEST");
  // Spread narrows, midpoint falls (NO direction)
  state.update(makeTicker("TEST", 100, 45, 55)); // spread=10, mid=50
  state.update(makeTicker("TEST", 105, 45, 53)); // spread=8, mid=49
  state.update(makeTicker("TEST", 110, 45, 51)); // spread=6, mid=48
  state.update(makeTicker("TEST", 115, 45, 49)); // spread=4, mid=47
  state.update(makeTicker("TEST", 120, 45, 47)); // spread=2, mid=46
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (falling midpoint = NO)");
});

Deno.test("SpreadVelocity: confidence higher for more linear narrowing", () => {
  const signal = new SpreadVelocity(defaultConfig);
  // Linear narrowing should have high R²
  const state = new MarketState("TEST");
  state.update(makeTicker("TEST", 100, 45, 55)); // spread=10
  state.update(makeTicker("TEST", 105, 46, 54)); // spread=8
  state.update(makeTicker("TEST", 110, 47, 53)); // spread=6
  state.update(makeTicker("TEST", 115, 48, 52)); // spread=4
  state.update(makeTicker("TEST", 120, 49, 51)); // spread=2
  const result = signal.evaluate(state);
  assertEquals(result.confidence > 0.5, true, "linear narrowing should give high confidence");
});
```

**Step 3: Run test to verify it fails**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/spread-velocity.test.ts`
Expected: FAIL — module not found

**Step 4: Write the signal implementation**

Create `ssmd-agent/src/momentum/signals/spread-velocity.ts`:

```typescript
import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface SpreadVelocityConfig {
  windowSec: number;
  minSnapshots: number;
  velocityThreshold: number;
  weight: number;
}

const ZERO: SignalResult = { name: "spread-velocity", score: 0, confidence: 0, reason: "" };

export class SpreadVelocity implements Signal {
  readonly name = "spread-velocity";
  private readonly config: SpreadVelocityConfig;

  constructor(config: SpreadVelocityConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const snapshots = state.getSpreadHistory(this.config.windowSec);
    if (snapshots.length < this.config.minSnapshots) return ZERO;

    // Linear regression: slope of spread over time
    const n = snapshots.length;
    let sumT = 0, sumS = 0, sumTS = 0, sumTT = 0, sumSS = 0;
    const t0 = snapshots[0].ts;
    for (const s of snapshots) {
      const t = s.ts - t0;
      sumT += t;
      sumS += s.spread;
      sumTS += t * s.spread;
      sumTT += t * t;
      sumSS += s.spread * s.spread;
    }

    const denomT = n * sumTT - sumT * sumT;
    if (denomT === 0) return ZERO;

    const slope = (n * sumTS - sumT * sumS) / denomT;

    // Slope is cents/second — negative = tightening
    if (Math.abs(slope) < this.config.velocityThreshold) return ZERO;

    // R² for confidence
    const meanS = sumS / n;
    const ssTot = sumSS - n * meanS * meanS;
    const intercept = (sumS - slope * sumT) / n;
    let ssRes = 0;
    for (const s of snapshots) {
      const t = s.ts - t0;
      const predicted = intercept + slope * t;
      ssRes += (s.spread - predicted) * (s.spread - predicted);
    }
    const rSquared = ssTot > 0 ? 1 - ssRes / ssTot : 0;

    // Direction from midpoint shift
    const firstMid = snapshots[0].midpoint;
    const lastMid = snapshots[n - 1].midpoint;
    const midShift = lastMid - firstMid;
    if (midShift === 0) return ZERO;

    const direction = midShift > 0 ? 1 : -1;

    // Score: normalize slope magnitude (cap at 0.5 cents/sec)
    const magnitude = Math.min(Math.abs(slope) / 0.5, 1);
    const score = direction * magnitude;

    // Confidence: R² of the regression
    const confidence = Math.max(rSquared, 0);

    const side = direction > 0 ? "YES" : "NO";
    const reason = `${side} spread velocity ${slope.toFixed(3)}c/s, R²=${rSquared.toFixed(2)} (${n} snapshots)`;

    return { name: this.name, score, confidence, reason };
  }
}
```

**Step 5: Run test to verify it passes**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/spread-velocity.test.ts`
Expected: 5 passed, 0 failed

**Step 6: Register in runner.ts**

Add import:
```typescript
import { SpreadVelocity } from "./signals/spread-velocity.ts";
```

Add registration block:
```typescript
  if (config.signals.spreadVelocity.enabled) {
    signals.push(new SpreadVelocity({
      windowSec: config.signals.spreadVelocity.windowSec,
      minSnapshots: config.signals.spreadVelocity.minSnapshots,
      velocityThreshold: config.signals.spreadVelocity.velocityThreshold,
      weight: config.signals.spreadVelocity.weight,
    }));
    weights.push(config.signals.spreadVelocity.weight);
  }
```

**Step 7: Run all momentum tests**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/`
Expected: All pass (100 + 5 = 105)

**Step 8: Commit**

```bash
git add ssmd-agent/src/momentum/signals/spread-velocity.ts \
       ssmd-agent/test/momentum/spread-velocity.test.ts \
       ssmd-agent/src/momentum/config.ts \
       ssmd-agent/src/momentum/runner.ts
git commit -m "feat(signals): add spread-velocity signal (dSpread/dt with regression)"
```

---

### Task 4: Volume-Price Divergence Signal

**Files:**
- Create: `ssmd-agent/src/momentum/signals/volume-price-divergence.ts`
- Create: `ssmd-agent/test/momentum/volume-price-divergence.test.ts`
- Modify: `ssmd-agent/src/momentum/config.ts` (add schema)
- Modify: `ssmd-agent/src/momentum/runner.ts` (add import and registration)

**Step 1: Add Zod schema to config.ts**

Add after the `spreadVelocity` block:

```typescript
    volumePriceDivergence: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(0.8),
      windowSec: z.number().default(60),
      baselineWindowSec: z.number().default(300),
      volumeMultiplier: z.number().default(2.0),
      maxPriceMoveCents: z.number().default(2),
      minTrades: z.number().default(5),
    }).default({}),
```

**Step 2: Write the test file**

Create `ssmd-agent/test/momentum/volume-price-divergence.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { VolumePriceDivergence } from "../../src/momentum/signals/volume-price-divergence.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTicker(ticker: string, ts: number, price: number, volume: number, dollarVolume: number): MarketRecord {
  return { type: "ticker", ticker, ts, price, yes_bid: price - 2, yes_ask: price + 2, volume, dollar_volume: dollarVolume };
}

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 60,
  baselineWindowSec: 300,
  volumeMultiplier: 2.0,
  maxPriceMoveCents: 2,
  minTrades: 5,
  weight: 0.8,
};

Deno.test("VolumePriceDivergence: no signal without baseline volume", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Only recent data, no baseline established
  state.update(makeTicker("TEST", 100, 50, 1000, 50000));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumePriceDivergence: no signal when volume is at baseline", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Build baseline over 300s
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("TEST", 100 + i * 30, 50, 1000 + i * 100, 50000 + i * 5000));
  }
  // Recent volume at same rate (not elevated)
  state.update(makeTicker("TEST", 400, 50, 2100, 105000));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumePriceDivergence: no signal when price already moved", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Build baseline
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("TEST", 100 + i * 30, 50, 1000 + i * 100, 50000 + i * 5000));
  }
  // High volume BUT price moved 5 cents (above maxPriceMoveCents=2)
  state.update(makeTicker("TEST", 395, 50, 2000, 100000));
  state.update(makeTicker("TEST", 400, 55, 3000, 200000));
  // Add trades for direction
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("TEST", 395 + i, "yes", 5, 55));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("VolumePriceDivergence: fires when volume surges but price flat", () => {
  const signal = new VolumePriceDivergence(defaultConfig);
  const state = new MarketState("TEST");
  // Build baseline: steady volume
  for (let i = 0; i < 10; i++) {
    state.update(makeTicker("TEST", 100 + i * 30, 50, 1000 + i * 100, 50000 + i * 5000));
  }
  // Recent: volume spike, price stays at 50
  state.update(makeTicker("TEST", 395, 50, 2000, 100000));
  state.update(makeTicker("TEST", 400, 50, 2500, 150000));
  // Add YES-dominant trades for direction
  for (let i = 0; i < 6; i++) {
    state.update(makeTrade("TEST", 395 + i, "yes", 10, 50));
  }
  const result = signal.evaluate(state);
  assertEquals(result.name, "volume-price-divergence");
  // Direction depends on trade flow — YES dominant trades should give positive
  assertEquals(result.score > 0 || result.score < 0, true, "should fire a signal");
});
```

**Step 3: Run test to verify it fails**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/volume-price-divergence.test.ts`
Expected: FAIL — module not found

**Step 4: Write the signal implementation**

Create `ssmd-agent/src/momentum/signals/volume-price-divergence.ts`:

```typescript
import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface VolumePriceDivergenceConfig {
  windowSec: number;
  baselineWindowSec: number;
  volumeMultiplier: number;
  maxPriceMoveCents: number;
  minTrades: number;
  weight: number;
}

const ZERO: SignalResult = { name: "volume-price-divergence", score: 0, confidence: 0, reason: "" };

export class VolumePriceDivergence implements Signal {
  readonly name = "volume-price-divergence";
  private readonly config: VolumePriceDivergenceConfig;

  constructor(config: VolumePriceDivergenceConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const recentRate = state.getVolumeRate(this.config.windowSec);
    const baselineRate = state.getVolumeRate(this.config.baselineWindowSec);

    // Need baseline volume to compare against
    if (baselineRate.perMinuteRate <= 0) return ZERO;

    // Compute volume ratio (recent vs baseline per-minute rate)
    const ratio = recentRate.perMinuteRate / baselineRate.perMinuteRate;
    if (ratio < this.config.volumeMultiplier) return ZERO;

    // Check that price hasn't already moved
    const priceChange = Math.abs(state.getPriceChange(this.config.windowSec));
    if (priceChange > this.config.maxPriceMoveCents) return ZERO;

    // Get trade flow for direction
    const flow = state.getTradeFlow(this.config.windowSec);
    if (flow.totalTrades < this.config.minTrades) return ZERO;

    const direction = flow.dominantSide === "yes" ? 1 : -1;

    // Score: normalize volume ratio (2x→0, 5x→1)
    const magnitude = Math.min((ratio - 1) / 4, 1);
    const score = direction * magnitude;

    // Confidence: higher ratio + flatter price = more confident
    const priceFlat = 1 - (priceChange / this.config.maxPriceMoveCents);
    const confidence = Math.min(ratio / 5, 1) * priceFlat;

    const side = flow.dominantSide.toUpperCase();
    const reason = `${side} vol-price divergence: ${ratio.toFixed(1)}x volume, ${priceChange.toFixed(0)}c price move (${flow.totalTrades} trades)`;

    return { name: this.name, score, confidence, reason };
  }
}
```

**Step 5: Run test to verify it passes**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/volume-price-divergence.test.ts`
Expected: 4 passed, 0 failed

**Step 6: Register in runner.ts**

Add import:
```typescript
import { VolumePriceDivergence } from "./signals/volume-price-divergence.ts";
```

Add registration block:
```typescript
  if (config.signals.volumePriceDivergence.enabled) {
    signals.push(new VolumePriceDivergence({
      windowSec: config.signals.volumePriceDivergence.windowSec,
      baselineWindowSec: config.signals.volumePriceDivergence.baselineWindowSec,
      volumeMultiplier: config.signals.volumePriceDivergence.volumeMultiplier,
      maxPriceMoveCents: config.signals.volumePriceDivergence.maxPriceMoveCents,
      minTrades: config.signals.volumePriceDivergence.minTrades,
      weight: config.signals.volumePriceDivergence.weight,
    }));
    weights.push(config.signals.volumePriceDivergence.weight);
  }
```

**Step 7: Run all momentum tests**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/`
Expected: All pass (105 + 4 = 109)

**Step 8: Commit**

```bash
git add ssmd-agent/src/momentum/signals/volume-price-divergence.ts \
       ssmd-agent/test/momentum/volume-price-divergence.test.ts \
       ssmd-agent/src/momentum/config.ts \
       ssmd-agent/src/momentum/runner.ts
git commit -m "feat(signals): add volume-price-divergence signal (coiled spring)"
```

---

### Task 5: Trade Clustering / Burst Detection Signal

**Files:**
- Create: `ssmd-agent/src/momentum/signals/trade-clustering.ts`
- Create: `ssmd-agent/test/momentum/trade-clustering.test.ts`
- Modify: `ssmd-agent/src/momentum/config.ts` (add schema)
- Modify: `ssmd-agent/src/momentum/runner.ts` (add import and registration)

**Step 1: Add Zod schema to config.ts**

Add after the `volumePriceDivergence` block:

```typescript
    tradeClustering: z.object({
      enabled: z.boolean().default(false),
      weight: z.number().default(1.0),
      windowSec: z.number().default(120),
      quietThresholdSec: z.number().default(15),
      burstGapSec: z.number().default(3),
      minBurstTrades: z.number().default(4),
    }).default({}),
```

**Step 2: Write the test file**

Create `ssmd-agent/test/momentum/trade-clustering.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MarketState } from "../../src/momentum/market-state.ts";
import { TradeClustering } from "../../src/momentum/signals/trade-clustering.ts";
import type { MarketRecord } from "../../src/state/types.ts";

function makeTrade(ticker: string, ts: number, side: string, count: number, price: number): MarketRecord {
  return { type: "trade", ticker, ts, side, count, price };
}

const defaultConfig = {
  windowSec: 120,
  quietThresholdSec: 15,
  burstGapSec: 3,
  minBurstTrades: 4,
  weight: 1.0,
};

Deno.test("TradeClustering: no signal with insufficient trades", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 5, 50));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeClustering: no signal when trades are evenly spaced (no burst)", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  // Trades every 10 seconds — no quiet period before a burst
  for (let i = 0; i < 8; i++) {
    state.update(makeTrade("TEST", 100 + i * 10, "yes", 5, 50));
  }
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeClustering: fires on burst after quiet period", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  // One trade, then 20s quiet, then burst of 5 rapid YES trades
  state.update(makeTrade("TEST", 100, "no", 2, 50));
  // quiet gap: 100 → 121 = 21 seconds (> quietThresholdSec=15)
  state.update(makeTrade("TEST", 121, "yes", 10, 52));
  state.update(makeTrade("TEST", 122, "yes", 8, 53));
  state.update(makeTrade("TEST", 123, "yes", 12, 54));
  state.update(makeTrade("TEST", 124, "yes", 6, 53));
  const result = signal.evaluate(state);
  assertEquals(result.score > 0, true, "should be positive (YES burst)");
  assertEquals(result.name, "trade-clustering");
});

Deno.test("TradeClustering: fires negative on NO burst", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 2, 50));
  // quiet gap
  state.update(makeTrade("TEST", 121, "no", 10, 48));
  state.update(makeTrade("TEST", 122, "no", 8, 47));
  state.update(makeTrade("TEST", 123, "no", 12, 46));
  state.update(makeTrade("TEST", 124, "no", 6, 47));
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should be negative (NO burst)");
});

Deno.test("TradeClustering: no signal when burst is too small", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  state.update(makeTrade("TEST", 100, "yes", 2, 50));
  // quiet gap, then only 3 trades (< minBurstTrades=4)
  state.update(makeTrade("TEST", 121, "yes", 10, 52));
  state.update(makeTrade("TEST", 122, "yes", 8, 53));
  state.update(makeTrade("TEST", 123, "yes", 12, 54));
  const result = signal.evaluate(state);
  assertEquals(result.score, 0);
});

Deno.test("TradeClustering: picks the most recent burst", () => {
  const signal = new TradeClustering(defaultConfig);
  const state = new MarketState("TEST");
  // First burst (YES)
  state.update(makeTrade("TEST", 50, "no", 2, 50));
  state.update(makeTrade("TEST", 70, "yes", 10, 52));
  state.update(makeTrade("TEST", 71, "yes", 8, 53));
  state.update(makeTrade("TEST", 72, "yes", 12, 54));
  state.update(makeTrade("TEST", 73, "yes", 6, 53));
  // Second burst (NO) — more recent
  state.update(makeTrade("TEST", 100, "no", 10, 48));
  state.update(makeTrade("TEST", 101, "no", 8, 47));
  state.update(makeTrade("TEST", 102, "no", 12, 46));
  state.update(makeTrade("TEST", 103, "no", 6, 47));
  const result = signal.evaluate(state);
  assertEquals(result.score < 0, true, "should pick most recent burst (NO)");
});
```

**Step 3: Run test to verify it fails**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/trade-clustering.test.ts`
Expected: FAIL — module not found

**Step 4: Write the signal implementation**

Create `ssmd-agent/src/momentum/signals/trade-clustering.ts`:

```typescript
import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface TradeClusteringConfig {
  windowSec: number;
  quietThresholdSec: number;
  burstGapSec: number;
  minBurstTrades: number;
  weight: number;
}

const ZERO: SignalResult = { name: "trade-clustering", score: 0, confidence: 0, reason: "" };

interface Burst {
  trades: { side: string; count: number; price: number; ts: number }[];
  startTs: number;
  endTs: number;
}

export class TradeClustering implements Signal {
  readonly name = "trade-clustering";
  private readonly config: TradeClusteringConfig;

  constructor(config: TradeClusteringConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const trades = state.getRecentTrades(this.config.windowSec);
    if (trades.length < this.config.minBurstTrades) return ZERO;

    // Find bursts: sequences where gap < burstGapSec, preceded by quiet >= quietThresholdSec
    const bursts: Burst[] = [];
    let currentBurst: Burst | null = null;

    for (let i = 0; i < trades.length; i++) {
      if (i === 0) {
        currentBurst = { trades: [trades[i]], startTs: trades[i].ts, endTs: trades[i].ts };
        continue;
      }

      const gap = trades[i].ts - trades[i - 1].ts;

      if (gap <= this.config.burstGapSec) {
        // Continue current burst
        currentBurst!.trades.push(trades[i]);
        currentBurst!.endTs = trades[i].ts;
      } else {
        // End current burst, check if it qualifies
        if (currentBurst && currentBurst.trades.length >= this.config.minBurstTrades) {
          // Check for quiet period before this burst
          const burstStart = currentBurst.startTs;
          const prevTradeTs = this.findPrevTradeTs(trades, currentBurst.trades[0], burstStart);
          const quietPeriod = burstStart - prevTradeTs;
          if (quietPeriod >= this.config.quietThresholdSec) {
            bursts.push(currentBurst);
          }
        }
        // Start new burst
        currentBurst = { trades: [trades[i]], startTs: trades[i].ts, endTs: trades[i].ts };
      }
    }

    // Check final burst
    if (currentBurst && currentBurst.trades.length >= this.config.minBurstTrades) {
      const burstStart = currentBurst.startTs;
      const prevTradeTs = this.findPrevTradeTs(trades, currentBurst.trades[0], burstStart);
      const quietPeriod = burstStart - prevTradeTs;
      if (quietPeriod >= this.config.quietThresholdSec) {
        bursts.push(currentBurst);
      }
    }

    if (bursts.length === 0) return ZERO;

    // Use the most recent burst
    const burst = bursts[bursts.length - 1];

    // Compute direction from burst trades
    let yesContracts = 0;
    let noContracts = 0;
    for (const t of burst.trades) {
      if (t.side === "yes") yesContracts += t.count;
      else if (t.side === "no") noContracts += t.count;
    }

    const total = yesContracts + noContracts;
    if (total === 0) return ZERO;

    const dominance = Math.max(yesContracts, noContracts) / total;
    const direction = yesContracts >= noContracts ? 1 : -1;

    // Score: dominance × burst intensity
    const burstDuration = Math.max(burst.endTs - burst.startTs, 1);
    const intensity = Math.min(burst.trades.length / burstDuration, 1); // trades per second, capped
    const score = direction * dominance * intensity;

    // Confidence: more trades in burst = higher confidence
    const confidence = Math.min(burst.trades.length / (this.config.minBurstTrades * 2), 1.0);

    const side = direction > 0 ? "YES" : "NO";
    const reason = `${side} burst: ${burst.trades.length} trades in ${burstDuration}s, ${(dominance * 100).toFixed(0)}% dominance, ${total} contracts`;

    return { name: this.name, score, confidence, reason };
  }

  private findPrevTradeTs(
    allTrades: { ts: number }[],
    firstBurstTrade: { ts: number },
    burstStart: number,
  ): number {
    // Find the trade just before the burst started
    let prevTs = burstStart - this.config.windowSec; // default: window start
    for (const t of allTrades) {
      if (t === firstBurstTrade) break;
      prevTs = t.ts;
    }
    return prevTs;
  }
}
```

**Step 5: Run test to verify it passes**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/trade-clustering.test.ts`
Expected: 6 passed, 0 failed

**Step 6: Register in runner.ts**

Add import:
```typescript
import { TradeClustering } from "./signals/trade-clustering.ts";
```

Add registration block:
```typescript
  if (config.signals.tradeClustering.enabled) {
    signals.push(new TradeClustering({
      windowSec: config.signals.tradeClustering.windowSec,
      quietThresholdSec: config.signals.tradeClustering.quietThresholdSec,
      burstGapSec: config.signals.tradeClustering.burstGapSec,
      minBurstTrades: config.signals.tradeClustering.minBurstTrades,
      weight: config.signals.tradeClustering.weight,
    }));
    weights.push(config.signals.tradeClustering.weight);
  }
```

**Step 7: Run all momentum tests**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/`
Expected: All pass (109 + 6 = 115)

**Step 8: Commit**

```bash
git add ssmd-agent/src/momentum/signals/trade-clustering.ts \
       ssmd-agent/test/momentum/trade-clustering.test.ts \
       ssmd-agent/src/momentum/config.ts \
       ssmd-agent/src/momentum/runner.ts
git commit -m "feat(signals): add trade-clustering signal (burst detection)"
```

---

### Task 6: Sweep YAML Files

**Files:**
- Create: `ssmd-agent/experiments/sweeps/concentration-sweep.yaml`
- Create: `ssmd-agent/experiments/sweeps/flow-asymmetry-sweep.yaml`
- Create: `ssmd-agent/experiments/sweeps/spread-velocity-sweep.yaml`
- Create: `ssmd-agent/experiments/sweeps/vol-price-divergence-sweep.yaml`
- Create: `ssmd-agent/experiments/sweeps/trade-clustering-sweep.yaml`

All sweep files use `base: ../deployed.yaml` and the same date range. Each enables only its signal and sets `composer.minSignals: [1]`.

**Step 1: Create concentration-sweep.yaml**

```yaml
name: concentration-tuning
base: ../deployed.yaml

parameters:
  signals.tradeConcentration.enabled: [true]
  signals.tradeConcentration.concentrationThreshold: [0.10, 0.15, 0.20, 0.30]
  signals.tradeConcentration.windowSec: [60, 120, 300]
  signals.tradeConcentration.minTrades: [5, 10]
  composer.minSignals: [1]

dateRange:
  from: "2026-01-16"
  to: "2026-01-26"

maxParallel: 5
image: "0.3.0"
```

**Step 2: Create flow-asymmetry-sweep.yaml**

```yaml
name: flow-asymmetry-tuning
base: ../deployed.yaml

parameters:
  signals.flowAsymmetry.enabled: [true]
  signals.flowAsymmetry.asymmetryThreshold: [1, 2, 3, 5]
  signals.flowAsymmetry.windowSec: [60, 120, 300]
  signals.flowAsymmetry.minTrades: [5, 8, 12]
  composer.minSignals: [1]

dateRange:
  from: "2026-01-16"
  to: "2026-01-26"

maxParallel: 5
image: "0.3.0"
```

**Step 3: Create spread-velocity-sweep.yaml**

```yaml
name: spread-velocity-tuning
base: ../deployed.yaml

parameters:
  signals.spreadVelocity.enabled: [true]
  signals.spreadVelocity.velocityThreshold: [0.05, 0.10, 0.15, 0.25]
  signals.spreadVelocity.windowSec: [15, 30, 60]
  composer.minSignals: [1]

dateRange:
  from: "2026-01-16"
  to: "2026-01-26"

maxParallel: 5
image: "0.3.0"
```

**Step 4: Create vol-price-divergence-sweep.yaml**

```yaml
name: vol-price-divergence-tuning
base: ../deployed.yaml

parameters:
  signals.volumePriceDivergence.enabled: [true]
  signals.volumePriceDivergence.volumeMultiplier: [1.5, 2.0, 3.0]
  signals.volumePriceDivergence.windowSec: [30, 60, 120]
  signals.volumePriceDivergence.maxPriceMoveCents: [1, 2, 3]
  composer.minSignals: [1]

dateRange:
  from: "2026-01-16"
  to: "2026-01-26"

maxParallel: 5
image: "0.3.0"
```

**Step 5: Create trade-clustering-sweep.yaml**

```yaml
name: trade-clustering-tuning
base: ../deployed.yaml

parameters:
  signals.tradeClustering.enabled: [true]
  signals.tradeClustering.quietThresholdSec: [10, 15, 30]
  signals.tradeClustering.burstGapSec: [2, 3, 5]
  signals.tradeClustering.minBurstTrades: [3, 5, 8]
  composer.minSignals: [1]

dateRange:
  from: "2026-01-16"
  to: "2026-01-26"

maxParallel: 5
image: "0.3.0"
```

**Step 6: Commit**

```bash
git add ssmd-agent/experiments/sweeps/
git commit -m "feat(sweeps): add isolation sweep YAMLs for 5 new signals"
```

---

### Task 7: Final Verification

**Step 1: Run full momentum test suite**

Run: `deno test --allow-read --allow-write --allow-net --allow-env test/momentum/`
Expected: 115 tests, 0 failures

**Step 2: Type check**

Run: `cd /home/wald/repos/899bushwick/.worktrees/new-signals/ssmd && make agent-check`
Expected: Pass (may have pre-existing kalshi.test.ts errors — those are unrelated)

**Step 3: Verify config defaults don't break existing configs**

Run: `deno eval "import { MomentumConfigSchema } from './ssmd-agent/src/momentum/config.ts'; const r = MomentumConfigSchema.parse({ nats: { url: 'nats://localhost:4222', stream: 'TEST' } }); console.log('New signals all default disabled:', !r.signals.tradeConcentration.enabled && !r.signals.flowAsymmetry.enabled && !r.signals.spreadVelocity.enabled && !r.signals.volumePriceDivergence.enabled && !r.signals.tradeClustering.enabled);"`
Expected: `New signals all default disabled: true`
