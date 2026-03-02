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

  const results: CheckResult[] = [check1, check2];

  // Checks 3 & 4 depend on a current contract from Check 1
  if (currentContract) {
    // Check 3: snap-coverage — verify at least one market has a live snap in Redis
    let check3: CheckResult;
    try {
      const marketsData = await fetchJson(
        `${apiUrl}/v1/markets?event=${currentContract.ticker}&limit=10`,
        apiKey,
      );
      const markets = marketsData.markets as Array<{ ticker: string }> | undefined;
      if (!Array.isArray(markets) || markets.length === 0) {
        check3 = {
          name: "snap-coverage",
          passed: false,
          detail: `No markets found for event ${currentContract.ticker}`,
        };
      } else {
        const tickerSlice = markets.slice(0, 5).map((m) => m.ticker);
        const snapData = await fetchJson(
          `${apiUrl}/v1/data/snap?feed=kalshi&tickers=${tickerSlice.join(",")}`,
          apiKey,
        );
        const snapshots = snapData.snapshots as unknown[] | undefined;
        const hasSnap = Array.isArray(snapshots) && snapshots.length > 0;
        check3 = {
          name: "snap-coverage",
          passed: hasSnap,
          detail: hasSnap
            ? `${(snapshots as unknown[]).length}/${tickerSlice.length} markets have live snaps`
            : `No live snaps for ${tickerSlice.length} markets of ${currentContract.ticker}`,
        };
      }
    } catch (err) {
      check3 = {
        name: "snap-coverage",
        passed: false,
        detail: `API error: ${err instanceof Error ? err.message : String(err)}`,
      };
    }
    results.push(check3);

    // Check 4: search-index — verify the current contract appears in event search
    let check4: CheckResult;
    try {
      const searchData = await fetchJson(
        `${apiUrl}/v1/monitor/search?q=${currentContract.ticker}&type=events&exchange=kalshi&limit=5`,
        apiKey,
      );
      const searchResults = searchData.results as Array<{ ticker: string }> | undefined;
      const found = Array.isArray(searchResults) && searchResults.length > 0;
      check4 = {
        name: "search-index",
        passed: found,
        detail: found
          ? `${searchResults!.length} event(s) found for ${currentContract.ticker}`
          : `No events found in search for ${currentContract.ticker}`,
      };
    } catch (err) {
      check4 = {
        name: "search-index",
        passed: false,
        detail: `API error: ${err instanceof Error ? err.message : String(err)}`,
      };
    }
    results.push(check4);
  }

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
