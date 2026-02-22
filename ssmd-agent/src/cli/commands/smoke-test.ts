/**
 * ssmd smoke-test - Integration smoke test for MCP/API endpoints
 *
 * Exercises the same endpoints the MCP tools use to verify API health after deploys.
 * Uses a dedicated integration API key (integration@ssmd.local).
 */

interface SmokeTestFlags {
  json?: boolean;
}

interface CheckResult {
  name: string;
  passed: boolean;
  durationMs: number;
  error?: string;
}

function getApiConfig(): { apiUrl: string; apiKey: string } {
  const apiUrl = Deno.env.get("SSMD_API_URL") || "http://localhost:8080";
  const apiKey = Deno.env.get("SSMD_API_KEY") || "";
  if (!apiKey) {
    console.error("Error: SSMD_API_KEY environment variable required");
    Deno.exit(1);
  }
  return { apiUrl, apiKey };
}

async function runCheck(
  name: string,
  fn: () => Promise<void>,
): Promise<CheckResult> {
  const start = performance.now();
  try {
    await fn();
    const durationMs = Math.round(performance.now() - start);
    return { name, passed: true, durationMs };
  } catch (e) {
    const durationMs = Math.round(performance.now() - start);
    const error = e instanceof Error ? e.message : String(e);
    return { name, passed: false, durationMs, error };
  }
}

async function fetchJson(
  url: string,
  apiKey: string,
): Promise<Record<string, unknown>> {
  const res = await fetch(url, {
    headers: { "X-API-Key": apiKey },
  });
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${res.statusText}`);
  }
  return await res.json() as Record<string, unknown>;
}

export async function handleSmokeTest(flags: SmokeTestFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const checks: Array<{ name: string; fn: () => Promise<void> }> = [
    {
      name: "api-health",
      fn: async () => {
        const res = await fetch(`${apiUrl}/health`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const text = await res.text();
        if (!text.includes("ok")) throw new Error(`Expected "ok", got: ${text}`);
      },
    },
    {
      name: "list-feeds",
      fn: async () => {
        const data = await fetchJson(`${apiUrl}/v1/data/feeds`, apiKey);
        const feeds = data.feeds as unknown[];
        if (!Array.isArray(feeds) || feeds.length === 0) {
          throw new Error("Expected non-empty feeds array");
        }
      },
    },
    {
      name: "data-freshness",
      fn: async () => {
        const data = await fetchJson(`${apiUrl}/v1/data/freshness`, apiKey);
        if (!Array.isArray(data.feeds)) {
          throw new Error("Expected feeds array in freshness response");
        }
      },
    },
    {
      name: "trade-query",
      fn: async () => {
        const data = await fetchJson(
          `${apiUrl}/v1/data/trades?feed=kalshi`,
          apiKey,
        );
        if (!Array.isArray(data.trades)) {
          throw new Error("Expected trades array in trade response");
        }
      },
    },
    {
      name: "volume-query",
      fn: async () => {
        const data = await fetchJson(`${apiUrl}/v1/data/volume`, apiKey);
        if (!Array.isArray(data.feeds)) {
          throw new Error("Expected feeds array in volume response");
        }
      },
    },
    {
      name: "secmaster-stats",
      fn: async () => {
        const data = await fetchJson(`${apiUrl}/v1/secmaster/stats`, apiKey);
        const events = data.events as Record<string, unknown> | undefined;
        const markets = data.markets as Record<string, unknown> | undefined;
        if (typeof events?.total !== "number" || typeof markets?.total !== "number") {
          throw new Error("Expected events.total and markets.total in secmaster stats");
        }
      },
    },
    {
      name: "search-markets",
      fn: async () => {
        const data = await fetchJson(`${apiUrl}/v1/markets?limit=5`, apiKey);
        if (!Array.isArray(data.markets)) {
          throw new Error("Expected markets array");
        }
      },
    },
    {
      name: "search-pairs",
      fn: async () => {
        const data = await fetchJson(`${apiUrl}/v1/pairs?limit=5`, apiKey);
        if (!Array.isArray(data.pairs)) {
          throw new Error("Expected pairs array");
        }
      },
    },
  ];

  const results: CheckResult[] = [];
  for (const check of checks) {
    const result = await runCheck(check.name, check.fn);
    results.push(result);
    if (!flags.json) {
      if (result.passed) {
        console.log(`[PASS] ${result.name} (${result.durationMs}ms)`);
      } else {
        console.log(`[FAIL] ${result.name}: ${result.error}`);
      }
    }
  }

  const passed = results.filter((r) => r.passed).length;
  const total = results.length;
  const allPassed = passed === total;

  if (flags.json) {
    console.log(JSON.stringify({ passed, total, allPassed, checks: results }, null, 2));
  } else {
    console.log();
    if (allPassed) {
      console.log(`${passed}/${total} checks passed`);
    } else {
      console.log(`${passed}/${total} checks passed — FAIL`);
    }
  }

  if (!allPassed) {
    Deno.exit(1);
  }
}
