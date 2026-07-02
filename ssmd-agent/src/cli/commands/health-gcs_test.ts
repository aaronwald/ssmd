import { assert, assertEquals } from "jsr:@std/assert";
import { parseGcsFileInfo } from "./health-gcs.ts";

Deno.test("parseGcsFileInfo extracts msgType + HHMM from a parquet object name", () => {
  const f = parseGcsFileInfo("ticker_1400.parquet", 12345);
  assert(f !== null, "expected a parsed GcsFileInfo, got null");
  assertEquals(f.msgType, "ticker");
  assertEquals(f.time, "1400");
  assertEquals(f.sizeBytes, 12345);
  assertEquals(f.name, "ticker_1400.parquet");
});

Deno.test("parseGcsFileInfo handles msgType with underscores (last underscore splits)", () => {
  const f = parseGcsFileInfo("order_book_0930.parquet", 7);
  assert(f !== null, "expected a parsed GcsFileInfo");
  assertEquals(f.msgType, "order_book");
  assertEquals(f.time, "0930");
});

Deno.test("parseGcsFileInfo rejects wrong ext", () => {
  assertEquals(parseGcsFileInfo("manifest.json", 1), null);
  assertEquals(parseGcsFileInfo("ticker_1400.parquet", 1, "jsonl"), null);
});

Deno.test("parseGcsFileInfo rejects names without an underscore", () => {
  assertEquals(parseGcsFileInfo("nounderscore.parquet", 1), null);
});
