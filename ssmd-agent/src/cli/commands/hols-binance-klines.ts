// ssmd-agent/src/cli/commands/hols-binance-klines.ts
// Binance Spot REST klines page fetch with per-status failure classification.
// Dependency-free (no DuckDB/db imports) so it unit-tests without --allow-ffi,
// same pattern as hols-binance-agg.ts. fetchFn/sleepFn are injectable for tests.

const BINANCE_KLINES_URL = "https://data-api.binance.vision/api/v3/klines";
const BINANCE_CANDLES_PER_REQUEST = 1000;
const DEFAULT_MAX_RETRIES = 3;
const DEFAULT_TIMEOUT_MS = 15000;
const RETRY_AFTER_CAP_MS = 30000;

export type KlinesFailureKind =
  | "geo-blocked" // 451: fatal, do not retry
  | "invalid-symbol" // 400: fatal, do not retry
  | "rate-limited" // 429 or 418 (Binance IP-ban escalation): retry with Retry-After backoff
  | "http-error" // any other non-ok status: retry
  | "network"; // fetch threw (timeout, DNS, reset): retry

export interface KlinesFailure {
  kind: KlinesFailureKind;
  status?: number;
  attempts: number;
  detail?: string; // 400 body excerpt, Retry-After value, or thrown error name
}

export type KlinesPageResult =
  | { ok: true; candles: unknown[][] }
  | { ok: false; failure: KlinesFailure };

export interface KlinesFetchOpts {
  fetchFn?: typeof fetch;
  sleepFn?: (ms: number) => Promise<void>;
  maxRetries?: number;
  timeoutMs?: number;
}

function realSleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Backoff before retry `attempt` (1-based), given the previous failure. */
export function retryDelayMs(failure: KlinesFailure, attempt: number): number {
  if (failure.kind === "rate-limited") {
    const retryAfterSecs = Number(failure.detail);
    if (Number.isFinite(retryAfterSecs) && retryAfterSecs > 0) {
      return Math.min(retryAfterSecs * 1000, RETRY_AFTER_CAP_MS);
    }
    return 2000 * Math.pow(2, attempt);
  }
  return 1000 * Math.pow(2, attempt); // pre-existing exponential for transient errors
}

export function formatKlinesError(symbol: string, failure: KlinesFailure): string {
  switch (failure.kind) {
    case "geo-blocked":
      return `Failed for ${symbol}: geo-blocked (HTTP 451)`;
    case "invalid-symbol":
      return `Failed for ${symbol}: invalid symbol (HTTP 400${failure.detail ? `: ${failure.detail}` : ""})`;
    case "rate-limited":
      return `Failed for ${symbol}: rate-limited (HTTP ${failure.status}) after ${failure.attempts} attempts`;
    case "http-error":
      return `Failed for ${symbol}: HTTP ${failure.status} after ${failure.attempts} attempts`;
    case "network":
      return `Failed for ${symbol}: network/timeout after ${failure.attempts} attempts${
        failure.detail ? ` (${failure.detail})` : ""
      }`;
  }
}

export async function fetchKlinesPage(
  symbol: string,
  intervalStr: string,
  startTimeMs: number,
  endTimeMs: number,
  opts: KlinesFetchOpts = {},
): Promise<KlinesPageResult> {
  const fetchFn = opts.fetchFn ?? fetch;
  const sleepFn = opts.sleepFn ?? realSleep;
  const maxRetries = opts.maxRetries ?? DEFAULT_MAX_RETRIES;
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const url =
    `${BINANCE_KLINES_URL}?symbol=${symbol}&interval=${intervalStr}&startTime=${startTimeMs}&endTime=${endTimeMs}&limit=${BINANCE_CANDLES_PER_REQUEST}`;

  let last: KlinesFailure = { kind: "network", attempts: 0 };
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    if (attempt > 0) await sleepFn(retryDelayMs(last, attempt));
    try {
      const resp = await fetchFn(url, {
        signal: AbortSignal.timeout(timeoutMs),
        headers: { Accept: "application/json" },
      });
      if (resp.ok) {
        return { ok: true, candles: await resp.json() as unknown[][] };
      }
      if (resp.status === 451) {
        await resp.body?.cancel();
        return { ok: false, failure: { kind: "geo-blocked", status: 451, attempts: attempt + 1 } };
      }
      if (resp.status === 400) {
        const body = await resp.text().catch(() => "");
        return {
          ok: false,
          failure: { kind: "invalid-symbol", status: 400, attempts: attempt + 1, detail: body.slice(0, 200) },
        };
      }
      if (resp.status === 429 || resp.status === 418) {
        const retryAfter = resp.headers.get("Retry-After") ?? undefined;
        await resp.body?.cancel();
        last = { kind: "rate-limited", status: resp.status, attempts: attempt + 1, detail: retryAfter };
        continue;
      }
      await resp.body?.cancel();
      last = { kind: "http-error", status: resp.status, attempts: attempt + 1 };
    } catch (e) {
      last = {
        kind: "network",
        attempts: attempt + 1,
        detail: e instanceof Error ? e.name : String(e),
      };
    }
  }
  return { ok: false, failure: last };
}
