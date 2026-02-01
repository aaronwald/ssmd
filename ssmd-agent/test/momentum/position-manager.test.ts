import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { PositionManager } from "../../src/momentum/position-manager.ts";

function makeManager(balance = 500, minContracts = 100, maxContracts = 100, drawdownPct = 10) {
  return new PositionManager({
    startingBalance: balance,
    tradeSize: 100,
    minContracts,
    maxContracts,
    drawdownHaltPercent: drawdownPct,
    takeProfitCents: 5,
    stopLossCents: 5,
    timeStopMinutes: 15,
    makerFeePerContract: 0,
    takerFeePerContract: 2,
  });
}

Deno.test("PositionManager opens position and deducts cash", () => {
  const pm = makeManager(500, 100, 100);
  const pos = pm.openPosition("model-1", "TEST-1", "yes", 50, 1000);
  assertEquals(pos !== null, true);
  assertEquals(pos!.contracts, 100);
  assertEquals(pm.cash < 500, true);
  assertEquals(pm.openPositions.length, 1);
});

Deno.test("PositionManager random contract sizing within range", () => {
  const pm = makeManager(10000, 10, 200);
  const contracts: number[] = [];
  for (let i = 0; i < 20; i++) {
    const ticker = `T${i}`;
    const pos = pm.openPosition("m1", ticker, "yes", 50, 1000 + i);
    if (pos) contracts.push(pos.contracts);
  }
  // All contracts should be in [10, 200]
  for (const c of contracts) {
    assertEquals(c >= 10 && c <= 200, true, `contracts ${c} out of range`);
  }
  assertEquals(contracts.length > 0, true);
});

Deno.test("PositionManager rejects when insufficient cash", () => {
  const pm = makeManager(50, 100, 100);
  // At price 50, 100 contracts costs $50. Second should fail.
  pm.openPosition("m1", "T1", "yes", 50, 1000);
  const pos = pm.openPosition("m1", "T2", "yes", 50, 1001);
  assertEquals(pos, null);
});

Deno.test("PositionManager take-profit exit", () => {
  const pm = makeManager(500, 100, 100);
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(55, 0, 0, "TEST-1", 1001);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "take-profit");
  assertEquals(pm.openPositions.length, 0);
  assertEquals(pm.cash > 500, true);
});

Deno.test("PositionManager stop-loss exit", () => {
  const pm = makeManager(500, 100, 100);
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(45, 0, 0, "TEST-1", 1001);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "stop-loss");
  assertEquals(pm.openPositions.length, 0);
  assertEquals(pm.cash < 500, true);
});

Deno.test("PositionManager time-stop exit", () => {
  const pm = makeManager(500, 100, 100);
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(50, 0, 0, "TEST-1", 1000 + 16 * 60);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "time-stop");
});

Deno.test("PositionManager prevents duplicate model+ticker", () => {
  const pm = makeManager(500, 100, 100);
  const p1 = pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const p2 = pm.openPosition("m1", "TEST-1", "yes", 52, 1001);
  assertEquals(p1 !== null, true);
  assertEquals(p2, null);
  const p3 = pm.openPosition("m2", "TEST-1", "yes", 52, 1002);
  assertEquals(p3 !== null, true);
});

Deno.test("PositionManager NO side take-profit", () => {
  const pm = makeManager(500, 100, 100);
  pm.openPosition("m1", "TEST-1", "no", 50, 1000);
  const exit = pm.checkExits(45, 0, 0, "TEST-1", 1001);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "take-profit");
});

Deno.test("PositionManager force-close before market close", () => {
  const pm = makeManager(500, 100, 100);
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(52, 1200, 2, "TEST-1", 1081);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "force-close");
});

Deno.test("PositionManager drawdown halt", () => {
  const pm = makeManager(500, 100, 100, 10);
  // Each loss: -5 cents * 100 contracts = -$5 gross, +$2 taker fee = -$7
  // Need to lose ~$50 to hit halt at $450
  for (let i = 0; i < 20; i++) {
    if (pm.isHalted) break;
    const ticker = `T${i}`;
    pm.openPosition("m1", ticker, "yes", 50, 1000 + i * 2);
    pm.checkExits(45, 0, 0, ticker, 1001 + i * 2);
  }
  assertEquals(pm.isHalted, true);
});
