import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { PositionManager } from "../../src/momentum/position-manager.ts";

function makeManager(balance = 500, tradeSize = 100, drawdownPct = 10) {
  return new PositionManager({
    startingBalance: balance,
    tradeSize,
    drawdownHaltPercent: drawdownPct,
    takeProfitCents: 5,
    stopLossCents: 5,
    timeStopMinutes: 15,
    makerFeePerContract: 0,
    takerFeePerContract: 2,
  });
}

Deno.test("PositionManager opens position and deducts cash", () => {
  const pm = makeManager();
  const pos = pm.openPosition("model-1", "TEST-1", "yes", 50, 1000);
  assertEquals(pos !== null, true);
  assertEquals(pos!.contracts, 200);
  assertEquals(pm.cash < 500, true);
  assertEquals(pm.openPositions.length, 1);
});

Deno.test("PositionManager rejects when insufficient cash", () => {
  const pm = makeManager(500, 100);
  pm.openPosition("m1", "T1", "yes", 50, 1000);
  pm.openPosition("m1", "T2", "yes", 50, 1001);
  pm.openPosition("m1", "T3", "yes", 50, 1002);
  pm.openPosition("m1", "T4", "yes", 50, 1003);
  pm.openPosition("m1", "T5", "yes", 50, 1004);
  const pos = pm.openPosition("m1", "T6", "yes", 50, 1005);
  assertEquals(pos, null);
});

Deno.test("PositionManager take-profit exit", () => {
  const pm = makeManager();
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(55, 0, 0, "TEST-1", 1001);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "take-profit");
  assertEquals(pm.openPositions.length, 0);
  assertEquals(pm.cash > 500, true);
});

Deno.test("PositionManager stop-loss exit", () => {
  const pm = makeManager();
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(45, 0, 0, "TEST-1", 1001);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "stop-loss");
  assertEquals(pm.openPositions.length, 0);
  assertEquals(pm.cash < 500, true);
});

Deno.test("PositionManager time-stop exit", () => {
  const pm = makeManager();
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(50, 0, 0, "TEST-1", 1000 + 16 * 60);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "time-stop");
});

Deno.test("PositionManager prevents duplicate model+ticker", () => {
  const pm = makeManager();
  const p1 = pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const p2 = pm.openPosition("m1", "TEST-1", "yes", 52, 1001);
  assertEquals(p1 !== null, true);
  assertEquals(p2, null);
  const p3 = pm.openPosition("m2", "TEST-1", "yes", 52, 1002);
  assertEquals(p3 !== null, true);
});

Deno.test("PositionManager NO side take-profit", () => {
  const pm = makeManager();
  pm.openPosition("m1", "TEST-1", "no", 50, 1000);
  const exit = pm.checkExits(45, 0, 0, "TEST-1", 1001);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "take-profit");
});

Deno.test("PositionManager force-close before market close", () => {
  const pm = makeManager();
  pm.openPosition("m1", "TEST-1", "yes", 50, 1000);
  const exit = pm.checkExits(52, 1200, 2, "TEST-1", 1081);
  assertEquals(exit.length, 1);
  assertEquals(exit[0].reason, "force-close");
});

Deno.test("PositionManager drawdown halt", () => {
  const pm = makeManager(500, 100, 10);
  // Each loss: -5 cents * 200 contracts = -$10 gross, +$4 taker fee = -$14
  // Need to lose ~$50 to hit halt at $450
  for (let i = 0; i < 10; i++) {
    if (pm.isHalted) break;
    const ticker = `T${i}`;
    pm.openPosition("m1", ticker, "yes", 50, 1000 + i * 2);
    pm.checkExits(45, 0, 0, ticker, 1001 + i * 2);
  }
  assertEquals(pm.isHalted, true);
});
