/**
 * ssmd verify-hourly - Verify current hourly KXBTCD contract is in secmaster
 *
 * Lightweight smoke check that the secmaster sync is discovering hourly
 * KXBTCD events. Runs as an hourly CronJob; exits non-zero if the
 * currently-trading contract is missing.
 */

interface VerifyHourlyFlags {
  json?: boolean;
}

interface CheckResult {
  name: string;
  passed: boolean;
  detail: string;
}

interface EventRow {
  eventTicker: string;
  strikeDate: string | null;
  title: string;
  marketCount: number;
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

export async function handleVerifyHourly(flags: VerifyHourlyFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const data = await fetchJson(
    `${apiUrl}/v1/events?series=KXBTCD&status=active&limit=500`,
    apiKey,
  );

  const events = data.events as EventRow[] | undefined;
  if (!Array.isArray(events)) {
    console.error("[FAIL] Unexpected response: missing events array");
    Deno.exit(1);
  }

  const now = Date.now();
  const ninetyMin = 90 * 60 * 1000;
  const twoHours = 2 * 60 * 60 * 1000;

  // Parse strikeDates into timestamps
  const strikeTimes = events
    .filter((e) => e.strikeDate)
    .map((e) => ({
      ticker: e.eventTicker,
      strikeMs: new Date(e.strikeDate!).getTime(),
    }));

  // Check 1: current contract — at least one event with strikeDate in [now, now+90min]
  const currentContract = strikeTimes.find(
    (e) => e.strikeMs >= now && e.strikeMs <= now + ninetyMin,
  );
  const check1: CheckResult = {
    name: "current-contract",
    passed: !!currentContract,
    detail: currentContract
      ? `${currentContract.ticker} settles at ${new Date(currentContract.strikeMs).toISOString()}`
      : `No event with strikeDate in [now, now+90min]`,
  };

  // Check 2: pre-open coverage — at least one event with strikeDate > now+2h
  const futureContract = strikeTimes.find((e) => e.strikeMs > now + twoHours);
  const check2: CheckResult = {
    name: "pre-open-coverage",
    passed: !!futureContract,
    detail: futureContract
      ? `${futureContract.ticker} settles at ${new Date(futureContract.strikeMs).toISOString()}`
      : `No event with strikeDate > now+2h`,
  };

  const results = [check1, check2];

  if (flags.json) {
    console.log(JSON.stringify({
      totalEvents: events.length,
      checks: results,
    }, null, 2));
  } else {
    console.log(`KXBTCD active events: ${events.length}`);
    for (const r of results) {
      const tag = r.passed ? "[PASS]" : "[FAIL]";
      console.log(`${tag} ${r.name}: ${r.detail}`);
    }
  }

  // Fail if current contract is missing (Check 1 is critical)
  if (!check1.passed) {
    Deno.exit(1);
  }
}
