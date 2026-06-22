import { assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  FEED_CONFIG,
  feedDescription,
  type FeedInfo,
  gcsDirPrefix,
  parseFileType,
  scanLayout,
  usesFlatLayout,
} from "../../../src/lib/gcs/signed-urls.ts";

const massive = FEED_CONFIG["massive"];
const hols = FEED_CONFIG["hols"];
const kraken = FEED_CONFIG["kraken-spot"];

Deno.test("FEED_CONFIG registers massive with all three message types", () => {
  assertEquals(massive.prefix, "massive");
  assertEquals(massive.stream, "massive");
  assertEquals(massive.messageTypes, ["ohlcv_1s", "ohlcv_1m", "ohlcv_1d"]);
  assertEquals(massive.flatMessageTypes, ["ohlcv_1d"]);
  assertEquals(massive.flatStream, "equities/daily");
  // Description is non-empty so /v1/data/feeds surfaces it.
  assertEquals(feedDescription("massive").length > 0, true);
});

Deno.test("hols feed migrated to flat:true (behavior unchanged)", () => {
  assertEquals(hols.flat, true);
});

Deno.test("usesFlatLayout: massive ohlcv_1d is flat, raw types are nested", () => {
  assertEquals(usesFlatLayout(massive, "ohlcv_1d"), true);
  assertEquals(usesFlatLayout(massive, "ohlcv_1m"), false);
  assertEquals(usesFlatLayout(massive, "ohlcv_1s"), false);
  // No msgType: massive is not fully flat.
  assertEquals(usesFlatLayout(massive), false);
});

Deno.test("usesFlatLayout: hols is flat for every message type", () => {
  assertEquals(usesFlatLayout(hols, "ohlcv"), true);
  assertEquals(usesFlatLayout(hols), true);
});

Deno.test("usesFlatLayout: nested-only feeds are never flat", () => {
  assertEquals(usesFlatLayout(kraken, "ticker"), false);
  assertEquals(usesFlatLayout(kraken), false);
});

Deno.test("gcsDirPrefix: massive ohlcv_1d resolves to flat daily path", () => {
  // Flat layout uses flatStream (equities/daily): the download path the
  // generator writes to must be massive/equities/daily/{date}/.
  assertEquals(
    gcsDirPrefix(massive, "2026-06-20", true),
    "massive/equities/daily/2026-06-20/",
  );
});

Deno.test("gcsDirPrefix: massive raw types resolve to nested archiver path", () => {
  assertEquals(
    gcsDirPrefix(massive, "2026-06-20", false),
    "massive/massive/massive/2026-06-20/",
  );
});

Deno.test("gcsDirPrefix: hols flat path is unchanged from prior behavior", () => {
  assertEquals(
    gcsDirPrefix(hols, "2026-06-20", true),
    "hols/crypto/daily/2026-06-20/",
  );
});

Deno.test("gcsDirPrefix: nested kraken path is unchanged", () => {
  assertEquals(
    gcsDirPrefix(kraken, "2026-06-20", false),
    "kraken-spot/kraken-spot/spot/2026-06-20/",
  );
});

Deno.test("gcsDirPrefix: fails loudly on a misconfigured feed", () => {
  const broken: FeedInfo = {
    prefix: "",
    stream: "x",
    messageTypes: ["t"],
    description: "broken",
  };
  assertThrows(() => gcsDirPrefix(broken, "2026-06-20", false), Error, "missing prefix");
});

Deno.test("scanLayout: massive populates BOTH flat and nested layouts", () => {
  assertEquals(scanLayout(massive, true), true);
  assertEquals(scanLayout(massive, false), true);
});

Deno.test("scanLayout: hols only populates the flat layout", () => {
  assertEquals(scanLayout(hols, true), true);
  assertEquals(scanLayout(hols, false), false);
});

Deno.test("scanLayout: nested-only feeds only populate the nested layout", () => {
  assertEquals(scanLayout(kraken, true), false);
  assertEquals(scanLayout(kraken, false), true);
});

Deno.test("parseFileType: flat name has no time-slot, hour is the date", () => {
  const r = parseFileType("ohlcv-1d-massive", true, "2026-06-20");
  assertEquals(r, { fileType: "ohlcv-1d-massive", hour: "2026-06-20" });
});

Deno.test("parseFileType: nested archiver name splits on last underscore", () => {
  const r = parseFileType("ohlcv_1m_0930", false, "2026-06-20");
  assertEquals(r, { fileType: "ohlcv_1m", hour: "0930" });
});

Deno.test("parseFileType: nested name without underscore is skipped (null)", () => {
  assertEquals(parseFileType("ohlcv1m", false, "2026-06-20"), null);
});
