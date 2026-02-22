/**
 * ssmd billing - Billing aggregation and credit management (via API)
 *
 * Subcommands:
 *   aggregate    Aggregate yesterday's API request log into billing_daily_summary
 *   credit       Issue a credit to a key
 *   balance      Show balance for a key
 */

interface BillingFlags {
  _: (string | number)[];
  date?: string;
  "dry-run"?: boolean;
  email?: string;
  amount?: string;
  description?: string;
  json?: boolean;
}

function getApiConfig(): { apiUrl: string; apiKey: string } {
  const apiUrl = Deno.env.get("SSMD_API_URL") || "http://localhost:8080";
  const apiKey = Deno.env.get("SSMD_DATA_API_KEY") || Deno.env.get("SSMD_API_KEY") || "";
  if (!apiKey) {
    console.error("Error: SSMD_DATA_API_KEY or SSMD_API_KEY environment variable required");
    Deno.exit(1);
  }
  return { apiUrl, apiKey };
}

function apiHeaders(apiKey: string): Record<string, string> {
  return {
    "Content-Type": "application/json",
    "X-API-Key": apiKey,
  };
}

export async function handleBilling(
  subcommand: string,
  flags: BillingFlags,
): Promise<void> {
  switch (subcommand) {
    case "aggregate":
      await runAggregate(flags);
      break;
    case "credit":
      await issueCredit(flags);
      break;
    case "balance":
      await showBalance(flags);
      break;
    default:
      printBillingHelp();
      break;
  }
}

async function runAggregate(flags: BillingFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();
  const dryRun = Boolean(flags["dry-run"]);

  const body: Record<string, unknown> = {};
  if (flags.date) body.date = flags.date;
  if (dryRun) body.dry_run = true;

  console.log(`[billing] Requesting aggregation${flags.date ? ` for ${flags.date}` : ""}${dryRun ? " (dry-run)" : ""}...`);

  const res = await fetch(`${apiUrl}/v1/billing/aggregate`, {
    method: "POST",
    headers: apiHeaders(apiKey),
    body: JSON.stringify(body),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const result = await res.json() as {
    date: string;
    dryRun: boolean;
    requestsProcessed: number;
    groups: number;
    totalCostUsd: number;
    llmCostUsd: number;
    message?: string;
    details?: Array<{ keyPrefix: string; endpoint: string; requests: number; bytes: number; costUsd: number }>;
  };

  if (result.message) {
    console.log(`[billing] ${result.message}`);
    return;
  }

  console.log(`[billing] Date: ${result.date}`);
  console.log(`[billing] Processed ${result.requestsProcessed} requests across ${result.groups} groups`);

  if (dryRun && result.details) {
    for (const d of result.details) {
      console.log(`  [dry-run] ${d.keyPrefix} ${d.endpoint}: ${d.requests} reqs, ${d.bytes} bytes, $${d.costUsd.toFixed(6)}`);
    }
  }

  console.log(`[billing] Total API cost: $${result.totalCostUsd.toFixed(6)}, LLM cost: $${result.llmCostUsd.toFixed(6)}`);
  console.log(`[billing] Done.`);
}

async function issueCredit(flags: BillingFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const keyPrefix = flags._[2] as string;
  if (!keyPrefix) {
    console.error("Error: key prefix is required");
    console.log("Usage: ssmd billing credit <prefix> --amount 500 --description 'Research credit'");
    Deno.exit(1);
  }

  const amount = flags.amount ? parseFloat(flags.amount) : 0;
  if (!amount || amount <= 0) {
    console.error("Error: --amount must be a positive number (USD)");
    Deno.exit(1);
  }

  const description = flags.description ?? "Manual credit";

  const res = await fetch(`${apiUrl}/v1/billing/credit`, {
    method: "POST",
    headers: apiHeaders(apiKey),
    body: JSON.stringify({
      key_prefix: keyPrefix,
      amount_usd: amount,
      description,
    }),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const result = await res.json() as { credited: boolean; entry: { keyPrefix: string; amountUsd: string; description: string } };
  console.log(`Credited $${amount.toFixed(2)} to ${keyPrefix}`);
  console.log(`  Description: ${description}`);
}

async function showBalance(flags: BillingFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const keyPrefix = flags._[2] as string;
  if (!keyPrefix) {
    // Show all billable keys via the billing report endpoint
    const now = new Date();
    const month = `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}`;

    const res = await fetch(`${apiUrl}/v1/billing/report?month=${month}`, {
      headers: apiHeaders(apiKey),
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: res.statusText }));
      console.error(`Error: ${(err as Record<string, string>).error ?? res.statusText}`);
      Deno.exit(1);
    }

    const report = await res.json() as {
      month: string;
      keys: Array<{ keyPrefix: string; userEmail: string; totalRequests: number; totalBytes: number }>;
    };

    // For each key in the report, fetch their balance
    for (const k of report.keys) {
      const balRes = await fetch(`${apiUrl}/v1/billing/balance?key_prefix=${k.keyPrefix}`, {
        headers: apiHeaders(apiKey),
      });

      if (balRes.ok) {
        const bal = await balRes.json() as { credits: number; debits: number; balance: number };
        const status = bal.balance >= 0 ? "OK" : "DEFICIT";
        console.log(
          `  ${k.keyPrefix}  ${k.userEmail.padEnd(30)}  credits: $${bal.credits.toFixed(2).padStart(10)}  debits: $${bal.debits.toFixed(2).padStart(10)}  balance: $${bal.balance.toFixed(2).padStart(10)}  [${status}]`,
        );
      }
    }
    return;
  }

  // Show specific key
  const res = await fetch(`${apiUrl}/v1/billing/balance?key_prefix=${keyPrefix}`, {
    headers: apiHeaders(apiKey),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const bal = await res.json() as { keyPrefix: string; credits: number; debits: number; balance: number };

  if (flags.json) {
    console.log(JSON.stringify(bal, null, 2));
    return;
  }

  console.log(`\n=== Billing Balance: ${keyPrefix} ===`);
  console.log(`  Credits:  $${bal.credits.toFixed(2)}`);
  console.log(`  Debits:   $${bal.debits.toFixed(2)}`);
  console.log(`  Balance:  $${bal.balance.toFixed(2)}`);

  // Fetch ledger for recent entries
  const ledgerRes = await fetch(`${apiUrl}/v1/billing/ledger?key_prefix=${keyPrefix}`, {
    headers: apiHeaders(apiKey),
  });

  if (ledgerRes.ok) {
    const ledger = await ledgerRes.json() as {
      entries: Array<{ entryType: string; amountUsd: string; description: string; createdAt: string }>;
    };
    if (ledger.entries.length > 0) {
      console.log(`\n  Recent entries:`);
      const recent = ledger.entries.slice(-10);
      for (const e of recent) {
        const sign = e.entryType === "credit" ? "+" : "-";
        const date = new Date(e.createdAt).toISOString().slice(0, 10);
        console.log(`    ${date}  ${sign}$${parseFloat(e.amountUsd).toFixed(2).padStart(10)}  ${e.description}`);
      }
    }
  }
}

function printBillingHelp(): void {
  console.log("Usage: ssmd billing <command> [options]");
  console.log();
  console.log("Billing aggregation and credit management");
  console.log();
  console.log("COMMANDS:");
  console.log("  aggregate    Aggregate API request log into daily billing summaries");
  console.log("  credit       Issue a credit to a key");
  console.log("  balance      Show billing balance for a key (or all billable keys)");
  console.log();
  console.log("OPTIONS (aggregate):");
  console.log("  --date DATE     Date to aggregate (YYYY-MM-DD, default: yesterday)");
  console.log("  --dry-run       Show what would be aggregated without writing");
  console.log();
  console.log("OPTIONS (credit):");
  console.log("  <prefix>            Key prefix to credit");
  console.log("  --amount USD        Amount in USD (required)");
  console.log("  --description TEXT  Credit description");
  console.log();
  console.log("OPTIONS (balance):");
  console.log("  [prefix]    Key prefix (omit to show all billable keys)");
  console.log("  --json      Output JSON format");
  console.log();
  console.log("ENVIRONMENT:");
  console.log("  SSMD_API_URL        API base URL (default: http://localhost:8080)");
  console.log("  SSMD_DATA_API_KEY   Admin API key for billing operations");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd billing aggregate");
  console.log("  ssmd billing aggregate --date 2026-02-21 --dry-run");
  console.log("  ssmd billing credit sk_live_abc --amount 500 --description 'Research credit'");
  console.log("  ssmd billing balance");
  console.log("  ssmd billing balance sk_live_abc");
}
