import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  type SweepResult,
  rankResults,
  formatResultsTable,
  parseSummaryJson,
} from "../../../src/momentum/sweep/results.ts";

function makeResult(overrides: Partial<SweepResult>): SweepResult {
  return {
    configId: "test",
    params: {},
    trades: 10,
    wins: 5,
    losses: 5,
    winRate: 0.5,
    netPnl: 0,
    maxDrawdown: 5.0,
    halted: false,
    status: "completed",
    error: undefined,
    ...overrides,
  };
}

Deno.test("rankResults: sorts by pnl descending by default", () => {
  const results = [
    makeResult({ configId: "a", netPnl: -10 }),
    makeResult({ configId: "b", netPnl: 50 }),
    makeResult({ configId: "c", netPnl: 20 }),
  ];
  const ranked = rankResults(results, { sortBy: "pnl" });
  assertEquals(ranked.map(r => r.configId), ["b", "c", "a"]);
});

Deno.test("rankResults: sorts by winrate", () => {
  const results = [
    makeResult({ configId: "a", winRate: 0.3 }),
    makeResult({ configId: "b", winRate: 0.8 }),
    makeResult({ configId: "c", winRate: 0.5 }),
  ];
  const ranked = rankResults(results, { sortBy: "winrate" });
  assertEquals(ranked.map(r => r.configId), ["b", "c", "a"]);
});

Deno.test("rankResults: sorts by drawdown ascending (lower is better)", () => {
  const results = [
    makeResult({ configId: "a", maxDrawdown: 10 }),
    makeResult({ configId: "b", maxDrawdown: 2 }),
    makeResult({ configId: "c", maxDrawdown: 5 }),
  ];
  const ranked = rankResults(results, { sortBy: "drawdown" });
  assertEquals(ranked.map(r => r.configId), ["b", "c", "a"]);
});

Deno.test("rankResults: filters by minTrades", () => {
  const results = [
    makeResult({ configId: "a", trades: 2 }),
    makeResult({ configId: "b", trades: 10 }),
  ];
  const ranked = rankResults(results, { sortBy: "pnl", minTrades: 5 });
  assertEquals(ranked.length, 1);
  assertEquals(ranked[0].configId, "b");
});

Deno.test("rankResults: excludeHalted filters halted configs", () => {
  const results = [
    makeResult({ configId: "a", halted: true }),
    makeResult({ configId: "b", halted: false }),
  ];
  const ranked = rankResults(results, { sortBy: "pnl", excludeHalted: true });
  assertEquals(ranked.length, 1);
  assertEquals(ranked[0].configId, "b");
});

Deno.test("rankResults: failed results sort to bottom", () => {
  const results = [
    makeResult({ configId: "a", status: "failed", netPnl: 0 }),
    makeResult({ configId: "b", netPnl: -10 }),
  ];
  const ranked = rankResults(results, { sortBy: "pnl" });
  assertEquals(ranked[0].configId, "b");
  assertEquals(ranked[1].configId, "a");
});

Deno.test("formatResultsTable: produces table with header and rows", () => {
  const results = [
    makeResult({ configId: "t80-w120", netPnl: 42, winRate: 0.75, trades: 8, maxDrawdown: 3.2, halted: false }),
  ];
  const table = formatResultsTable(results);
  assertEquals(table.includes("Rank"), true);
  assertEquals(table.includes("t80-w120"), true);
  assertEquals(table.includes("42"), true);
});

Deno.test("parseSummaryJson: aggregates across multiple results", () => {
  const json = JSON.stringify({
    results: [
      { trades: 5, wins: 3, losses: 2, netPnl: 10 },
      { trades: 3, wins: 1, losses: 2, netPnl: -5 },
    ],
    portfolio: { drawdownPercent: 8.5, halted: false },
  });
  const result = parseSummaryJson(json, "test-id", { foo: 1 });
  assertEquals(result.configId, "test-id");
  assertEquals(result.trades, 8);
  assertEquals(result.wins, 4);
  assertEquals(result.losses, 4);
  assertEquals(result.netPnl, 5);
  assertEquals(result.winRate, 0.5);
  assertEquals(result.maxDrawdown, 8.5);
  assertEquals(result.halted, false);
  assertEquals(result.status, "completed");
});

Deno.test("parseSummaryJson: handles empty results array", () => {
  const json = JSON.stringify({ results: [], portfolio: {} });
  const result = parseSummaryJson(json, "empty", {});
  assertEquals(result.trades, 0);
  assertEquals(result.winRate, 0);
  assertEquals(result.netPnl, 0);
  assertEquals(result.maxDrawdown, 0);
  assertEquals(result.halted, false);
});

Deno.test("parseSummaryJson: handles halted portfolio", () => {
  const json = JSON.stringify({
    results: [{ trades: 10, wins: 2, losses: 8, netPnl: -50 }],
    portfolio: { drawdownPercent: 25.0, halted: true },
  });
  const result = parseSummaryJson(json, "halted", {});
  assertEquals(result.halted, true);
  assertEquals(result.maxDrawdown, 25.0);
});
