import { assertEquals } from "jsr:@std/assert";
import { connectorRedIssues, type ArchiveFreshness } from "./health.ts";

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
    [{ feed: "massive", score: 0, archiveFeed: "massive" }],
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
