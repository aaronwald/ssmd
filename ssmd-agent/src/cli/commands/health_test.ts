import { assertEquals } from "jsr:@std/assert";
import {
  allExpectedSlots,
  type ArchiveFreshness,
  clampScore,
  connectorRedIssues,
  DQ_ACTIVE_HOURS,
  findGaps,
} from "./health.ts";

const fresh: ArchiveFreshness = { ageHours: 1.5, stale: false };
const stale: ArchiveFreshness = { ageHours: 30, stale: true };

Deno.test("connectorRedIssues: score 0 + stale archive → RED", () => {
  const issues = connectorRedIssues(
    [{ feed: "kalshi-crypto", score: 0, archiveFeed: "kalshi" }],
    { kalshi: stale },
  );
  assertEquals(issues.length, 1);
  assertEquals(issues[0].includes("kalshi-crypto connector down"), true);
});

Deno.test("connectorRedIssues: score 0 + FRESH archive → no RED (false-RED fix)", () => {
  // The sparse 15M crypto feed: NATS stream momentarily empty but archive fresh.
  const issues = connectorRedIssues(
    [{ feed: "kalshi-crypto", score: 0, archiveFeed: "kalshi" }],
    { kalshi: fresh },
  );
  assertEquals(issues, []);
});

Deno.test("connectorRedIssues: score > 0 → never RED regardless of archive", () => {
  const issues = connectorRedIssues(
    [{ feed: "binance", score: 100, archiveFeed: "binance" }],
    { binance: stale },
  );
  assertEquals(issues, []);
});

Deno.test("connectorRedIssues: score 0 + missing freshness entry → RED (archive missing)", () => {
  const issues = connectorRedIssues(
    [{ feed: "kraken-spot", score: 0, archiveFeed: "kraken-spot" }],
    {}, // no freshness data → treat as stale/missing
  );
  assertEquals(issues.length, 1);
  assertEquals(issues[0].includes("archive missing"), true);
});

Deno.test("connectorRedIssues: mixed fleet only REDs the genuinely-down feed", () => {
  const issues = connectorRedIssues(
    [
      { feed: "kalshi-crypto", score: 0, archiveFeed: "kalshi" }, // fresh → ok
      { feed: "binance", score: 100, archiveFeed: "binance" }, // healthy
      { feed: "kraken-spot", score: 0, archiveFeed: "kraken-spot" }, // stale → RED
    ],
    { kalshi: fresh, binance: fresh, "kraken-spot": stale },
  );
  assertEquals(issues.length, 1);
  assertEquals(issues[0].includes("kraken-spot connector down"), true);
});

Deno.test("allExpectedSlots yields 24 hourly slots matching the real parquet cadence", () => {
  const slots = allExpectedSlots();
  assertEquals(slots.length, 24);
  assertEquals(slots[0], "0000");
  assertEquals(slots[1], "0100");
  assertEquals(slots[23], "2300");
});

Deno.test("DQ_ACTIVE_HOURS covers all 24 hours (production has data in every hour)", () => {
  assertEquals(DQ_ACTIVE_HOURS, 24);
});

Deno.test("findGaps reports no gaps for a complete hourly day", () => {
  const files = allExpectedSlots().map((t) => ({
    path: "", name: `ticker_${t}.parquet`, sizeBytes: 100, msgType: "ticker", time: t,
  }));
  assertEquals(findGaps(files), []);
});

Deno.test("findGaps measures a single missing hour as 60 minutes", () => {
  const files = allExpectedSlots().filter((t) => t !== "0300").map((t) => ({
    path: "", name: `ticker_${t}.parquet`, sizeBytes: 100, msgType: "ticker", time: t,
  }));
  const gaps = findGaps(files);
  assertEquals(gaps.length, 1);
  assertEquals(gaps[0].durationMinutes, 60);
  assertEquals(gaps[0].start, "03:00");
});

Deno.test("clampScore bounds scores into [0,100]", () => {
  assertEquals(clampScore(104), 100);
  assertEquals(clampScore(-5), 0);
  assertEquals(clampScore(87.4), 87);
});

Deno.test("clampScore treats non-finite input as 0 rather than propagating NaN", () => {
  assertEquals(clampScore(NaN), 0);
  assertEquals(clampScore(Infinity), 100);
  assertEquals(clampScore(-Infinity), 0);
});
