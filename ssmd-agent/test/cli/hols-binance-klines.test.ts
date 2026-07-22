// ssmd-agent/test/cli/hols-binance-klines.test.ts
import { assert, assertEquals, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  fetchKlinesPage,
  formatKlinesError,
  type KlinesFailure,
  retryDelayMs,
} from "../../src/cli/commands/hols-binance-klines.ts";

// fetchFn stub: pops one queued response per call; records call count.
function stubFetch(queue: (() => Response)[]) {
  let calls = 0;
  const fn = (_input: URL | Request | string, _init?: RequestInit): Promise<Response> => {
    calls++;
    const next = queue.shift();
    if (!next) throw new Error("stubFetch queue exhausted");
    return Promise.resolve(next());
  };
  return { fn, callCount: () => calls };
}

const noSleep = { slept: [] as number[] };
function stubSleep(ms: number): Promise<void> {
  noSleep.slept.push(ms);
  return Promise.resolve();
}

const CANDLE = [1784737320000, "1", "2", "0.5", "1.5", "10", 1784737379999, "15", 3, "5", "7.5", "0"];

Deno.test("ok path returns candles", async () => {
  const { fn } = stubFetch([() => new Response(JSON.stringify([CANDLE]), { status: 200 })]);
  const res = await fetchKlinesPage("VETUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(res.ok);
  assertEquals(res.candles.length, 1);
});

Deno.test("451 is fatal geo-blocked, no retry", async () => {
  const { fn, callCount } = stubFetch([() => new Response("", { status: 451 })]);
  const res = await fetchKlinesPage("VETUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(!res.ok);
  assertEquals(res.failure.kind, "geo-blocked");
  assertEquals(res.failure.status, 451);
  assertEquals(callCount(), 1);
});

Deno.test("400 is fatal invalid-symbol with body detail, no retry", async () => {
  const { fn, callCount } = stubFetch([
    () => new Response(JSON.stringify({ code: -1121, msg: "Invalid symbol." }), { status: 400 }),
  ]);
  const res = await fetchKlinesPage("BOGUSUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(!res.ok);
  assertEquals(res.failure.kind, "invalid-symbol");
  assertEquals(res.failure.status, 400);
  assertStringIncludes(res.failure.detail ?? "", "-1121");
  assertEquals(callCount(), 1);
});

Deno.test("429 retries with Retry-After backoff then succeeds", async () => {
  noSleep.slept.length = 0;
  const { fn, callCount } = stubFetch([
    () => new Response("", { status: 429, headers: { "Retry-After": "2" } }),
    () => new Response(JSON.stringify([CANDLE]), { status: 200 }),
  ]);
  const res = await fetchKlinesPage("VETUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(res.ok);
  assertEquals(callCount(), 2);
  assertEquals(noSleep.slept, [2000]); // honored Retry-After: 2s
});

Deno.test("429 exhausted reports rate-limited with attempts", async () => {
  const { fn, callCount } = stubFetch([
    () => new Response("", { status: 429 }),
    () => new Response("", { status: 429 }),
    () => new Response("", { status: 429 }),
  ]);
  const res = await fetchKlinesPage("VETUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(!res.ok);
  assertEquals(res.failure.kind, "rate-limited");
  assertEquals(res.failure.status, 429);
  assertEquals(res.failure.attempts, 3);
  assertEquals(callCount(), 3);
});

Deno.test("418 (Binance IP ban) is treated as rate-limited", async () => {
  const { fn } = stubFetch([
    () => new Response("", { status: 418 }),
    () => new Response("", { status: 418 }),
    () => new Response("", { status: 418 }),
  ]);
  const res = await fetchKlinesPage("VETUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(!res.ok);
  assertEquals(res.failure.kind, "rate-limited");
  assertEquals(res.failure.status, 418);
});

Deno.test("5xx retries then reports http-error with last status", async () => {
  const { fn, callCount } = stubFetch([
    () => new Response("", { status: 500 }),
    () => new Response("", { status: 502 }),
    () => new Response("", { status: 502 }),
  ]);
  const res = await fetchKlinesPage("VETUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(!res.ok);
  assertEquals(res.failure.kind, "http-error");
  assertEquals(res.failure.status, 502);
  assertEquals(callCount(), 3);
});

Deno.test("network throw retries then reports network with error name", async () => {
  const fn = (): Promise<Response> => Promise.reject(new DOMException("timed out", "TimeoutError"));
  const res = await fetchKlinesPage("VETUSDT", "1m", 0, 60000, { fetchFn: fn, sleepFn: stubSleep });
  assert(!res.ok);
  assertEquals(res.failure.kind, "network");
  assertEquals(res.failure.attempts, 3);
  assertStringIncludes(res.failure.detail ?? "", "TimeoutError");
});

Deno.test("retryDelayMs honors Retry-After, caps at 30s, exponential otherwise", () => {
  const rl = (detail?: string): KlinesFailure => ({ kind: "rate-limited", status: 429, attempts: 1, detail });
  assertEquals(retryDelayMs(rl("2"), 1), 2000);
  assertEquals(retryDelayMs(rl("9999"), 1), 30000); // cap
  assertEquals(retryDelayMs(rl(undefined), 1), 4000); // 2000 * 2^1
  const net: KlinesFailure = { kind: "network", attempts: 1 };
  assertEquals(retryDelayMs(net, 1), 2000); // 1000 * 2^1 (existing behavior)
});

Deno.test("formatKlinesError names the cause and status", () => {
  assertEquals(
    formatKlinesError("VETUSDT", { kind: "geo-blocked", status: 451, attempts: 1 }),
    "Failed for VETUSDT: geo-blocked (HTTP 451)",
  );
  assertEquals(
    formatKlinesError("BOGUSUSDT", { kind: "invalid-symbol", status: 400, attempts: 1, detail: '{"code":-1121}' }),
    'Failed for BOGUSUSDT: invalid symbol (HTTP 400: {"code":-1121})',
  );
  assertEquals(
    formatKlinesError("VETUSDT", { kind: "rate-limited", status: 429, attempts: 3 }),
    "Failed for VETUSDT: rate-limited (HTTP 429) after 3 attempts",
  );
  assertEquals(
    formatKlinesError("VETUSDT", { kind: "http-error", status: 502, attempts: 3 }),
    "Failed for VETUSDT: HTTP 502 after 3 attempts",
  );
  assertEquals(
    formatKlinesError("VETUSDT", { kind: "network", attempts: 3, detail: "TimeoutError" }),
    "Failed for VETUSDT: network/timeout after 3 attempts (TimeoutError)",
  );
});
