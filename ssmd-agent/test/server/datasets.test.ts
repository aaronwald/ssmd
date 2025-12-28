import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { listDatasets } from "../../src/server/handlers/datasets.ts";
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";

Deno.test("listDatasets returns empty for missing directory", async () => {
  const datasets = await listDatasets("/nonexistent/path");
  assertEquals(datasets, []);
});

Deno.test("listDatasets finds datasets in directory structure", async () => {
  const tmpDir = await Deno.makeTempDir();

  // Create test structure: /tmp/feed1/2025-01-01/
  const feedDir = join(tmpDir, "kalshi", "2025-01-01");
  await Deno.mkdir(feedDir, { recursive: true });

  // Create a manifest
  const manifest = {
    feed: "kalshi",
    date: "2025-01-01",
    total_records: 1000,
    tickers: ["TICKER1", "TICKER2"],
    total_bytes: 1024 * 1024,
    has_gaps: false,
  };
  await Deno.writeTextFile(
    join(feedDir, "manifest.json"),
    JSON.stringify(manifest)
  );

  const datasets = await listDatasets(tmpDir);

  assertEquals(datasets.length, 1);
  assertEquals(datasets[0].feed, "kalshi");
  assertEquals(datasets[0].date, "2025-01-01");
  assertEquals(datasets[0].records, 1000);
  assertEquals(datasets[0].tickers, 2);

  // Cleanup
  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("listDatasets filters by feed", async () => {
  const tmpDir = await Deno.makeTempDir();

  // Create two feeds
  for (const feed of ["feed1", "feed2"]) {
    const feedDir = join(tmpDir, feed, "2025-01-01");
    await Deno.mkdir(feedDir, { recursive: true });
    await Deno.writeTextFile(
      join(feedDir, "manifest.json"),
      JSON.stringify({ feed, date: "2025-01-01", total_records: 100, tickers: [], total_bytes: 0 })
    );
  }

  const datasets = await listDatasets(tmpDir, "feed1");

  assertEquals(datasets.length, 1);
  assertEquals(datasets[0].feed, "feed1");

  // Cleanup
  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("listDatasets filters by date range", async () => {
  const tmpDir = await Deno.makeTempDir();

  // Create datasets for multiple dates
  for (const date of ["2025-01-01", "2025-01-15", "2025-02-01"]) {
    const feedDir = join(tmpDir, "kalshi", date);
    await Deno.mkdir(feedDir, { recursive: true });
    await Deno.writeTextFile(
      join(feedDir, "manifest.json"),
      JSON.stringify({ feed: "kalshi", date, total_records: 100, tickers: [], total_bytes: 0 })
    );
  }

  const datasets = await listDatasets(tmpDir, undefined, "2025-01-10", "2025-01-20");

  assertEquals(datasets.length, 1);
  assertEquals(datasets[0].date, "2025-01-15");

  // Cleanup
  await Deno.remove(tmpDir, { recursive: true });
});
