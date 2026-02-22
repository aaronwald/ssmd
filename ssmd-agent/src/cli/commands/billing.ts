/**
 * ssmd billing - Billing aggregation and credit management
 *
 * Subcommands:
 *   aggregate    Aggregate yesterday's API request log into billing_daily_summary
 *   credit       Issue a credit to a key
 *   balance      Show balance for a key
 */
import {
  getDb,
  closeDb,
  apiRequestLog,
  billingDailySummary,
  billingRates,
  billingLedger,
  apiKeys,
  llmUsageDaily,
} from "../../lib/db/mod.ts";
import { eq, and, gte, lt, isNull, sql, lte } from "drizzle-orm";

/** Map an API path to an endpoint tier for rate lookup */
function endpointTier(method: string, path: string): string {
  if (path.startsWith("/v1/data/download")) return "data_download";
  if (path.startsWith("/v1/data/")) return "data_query";
  if (path.startsWith("/v1/markets/lookup")) return "market_lookup";
  if (
    path.startsWith("/v1/events") ||
    path.startsWith("/v1/markets") ||
    path.startsWith("/v1/series") ||
    path.startsWith("/v1/pairs") ||
    path.startsWith("/v1/conditions") ||
    path.startsWith("/v1/fees")
  ) return "secmaster";
  if (path.startsWith("/v1/chat/completions")) return "llm_chat";
  // Default: treat as data_query
  return "data_query";
}

interface BillingFlags {
  _: (string | number)[];
  date?: string;
  "dry-run"?: boolean;
  email?: string;
  amount?: string;
  description?: string;
  json?: boolean;
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
  const db = getDb();
  const dryRun = Boolean(flags["dry-run"]);

  try {
    // Default: yesterday
    const dateParam = flags.date;
    let targetDate: string;
    if (dateParam) {
      targetDate = dateParam;
    } else {
      const d = new Date();
      d.setUTCDate(d.getUTCDate() - 1);
      targetDate = d.toISOString().slice(0, 10);
    }

    const nextDate = new Date(targetDate + "T00:00:00Z");
    nextDate.setUTCDate(nextDate.getUTCDate() + 1);
    const nextDateStr = nextDate.toISOString().slice(0, 10);

    console.log(`[billing] Aggregating requests for ${targetDate}...`);

    // Query api_request_log for the target date
    const rows = await db
      .select()
      .from(apiRequestLog)
      .where(
        and(
          gte(apiRequestLog.createdAt, new Date(targetDate + "T00:00:00Z")),
          lt(apiRequestLog.createdAt, new Date(nextDateStr + "T00:00:00Z")),
        ),
      );

    console.log(`[billing] Found ${rows.length} request log entries`);

    if (rows.length === 0) {
      console.log("[billing] No requests to aggregate.");
      await closeDb();
      return;
    }

    // Load current billing rates
    const rates = await db
      .select()
      .from(billingRates)
      .where(
        and(
          lte(billingRates.effectiveFrom, new Date(nextDateStr + "T00:00:00Z")),
          isNull(billingRates.effectiveTo),
        ),
      );

    // Build rate lookup: key-specific first, then global fallback
    const rateLookup = new Map<string, { perReq: number; perMb: number }>();
    for (const r of rates) {
      const key = r.keyPrefix
        ? `${r.keyPrefix}:${r.endpointTier}`
        : `_global:${r.endpointTier}`;
      rateLookup.set(key, {
        perReq: parseFloat(r.ratePerRequest),
        perMb: parseFloat(r.ratePerMb),
      });
    }

    function getRate(
      keyPrefix: string,
      tier: string,
    ): { perReq: number; perMb: number } {
      return (
        rateLookup.get(`${keyPrefix}:${tier}`) ??
        rateLookup.get(`_global:${tier}`) ?? { perReq: 0, perMb: 0 }
      );
    }

    // Group by (key_prefix, endpoint_tier)
    const groups = new Map<
      string,
      {
        keyPrefix: string;
        endpoint: string;
        count: number;
        bytes: number;
        errors: number;
      }
    >();

    for (const row of rows) {
      const tier = endpointTier(row.method, row.path);
      const groupKey = `${row.keyPrefix}:${tier}`;
      const existing = groups.get(groupKey);
      if (existing) {
        existing.count++;
        existing.bytes += row.responseBytes ?? 0;
        if (row.statusCode >= 400) existing.errors++;
      } else {
        groups.set(groupKey, {
          keyPrefix: row.keyPrefix,
          endpoint: tier,
          count: 1,
          bytes: row.responseBytes ?? 0,
          errors: row.statusCode >= 400 ? 1 : 0,
        });
      }
    }

    console.log(`[billing] ${groups.size} endpoint groups across keys`);

    // Compute costs and upsert
    let totalCost = 0;
    let upsertCount = 0;

    for (const [, group] of groups) {
      const rate = getRate(group.keyPrefix, group.endpoint);
      const requestCost = group.count * rate.perReq;
      const dataCost = (group.bytes / (1024 * 1024)) * rate.perMb;
      const costUsd = requestCost + dataCost;
      totalCost += costUsd;

      if (dryRun) {
        console.log(
          `  [dry-run] ${group.keyPrefix} ${group.endpoint}: ${group.count} reqs, ${group.bytes} bytes, $${costUsd.toFixed(6)}`,
        );
        continue;
      }

      // Upsert into billing_daily_summary (ON CONFLICT UPDATE for idempotent re-runs)
      await db
        .insert(billingDailySummary)
        .values({
          keyPrefix: group.keyPrefix,
          date: targetDate,
          endpoint: group.endpoint,
          requestCount: group.count,
          responseBytes: group.bytes,
          errorCount: group.errors,
          costUsd: costUsd.toFixed(6),
        })
        .onConflictDoUpdate({
          target: [
            billingDailySummary.keyPrefix,
            billingDailySummary.date,
            billingDailySummary.endpoint,
          ],
          set: {
            requestCount: sql`excluded.request_count`,
            responseBytes: sql`excluded.response_bytes`,
            errorCount: sql`excluded.error_count`,
            costUsd: sql`excluded.cost_usd`,
          },
        });

      upsertCount++;
    }

    // Also aggregate LLM usage costs for the day
    const llmRows = await db
      .select()
      .from(llmUsageDaily)
      .where(eq(llmUsageDaily.date, targetDate));

    let llmTotalCost = 0;
    for (const row of llmRows) {
      llmTotalCost += parseFloat(row.costUsd);
    }

    // Insert debit entries for each key's daily total
    if (!dryRun) {
      const keyTotals = new Map<string, number>();
      for (const [, group] of groups) {
        const rate = getRate(group.keyPrefix, group.endpoint);
        const cost = group.count * rate.perReq +
          (group.bytes / (1024 * 1024)) * rate.perMb;
        keyTotals.set(
          group.keyPrefix,
          (keyTotals.get(group.keyPrefix) ?? 0) + cost,
        );
      }

      // Add LLM costs by key
      for (const row of llmRows) {
        const cost = parseFloat(row.costUsd);
        if (cost > 0) {
          keyTotals.set(
            row.keyPrefix,
            (keyTotals.get(row.keyPrefix) ?? 0) + cost,
          );
        }
      }

      const month = targetDate.slice(0, 7);
      for (const [keyPrefix, total] of keyTotals) {
        if (total <= 0) continue;

        // Check if debit already exists for this key/month/date
        const existing = await db
          .select()
          .from(billingLedger)
          .where(
            and(
              eq(billingLedger.keyPrefix, keyPrefix),
              eq(billingLedger.entryType, "debit"),
              eq(billingLedger.referenceMonth, month),
              eq(
                billingLedger.description,
                `Usage for ${targetDate}`,
              ),
            ),
          );

        if (existing.length === 0) {
          await db.insert(billingLedger).values({
            keyPrefix,
            entryType: "debit",
            amountUsd: total.toFixed(6),
            description: `Usage for ${targetDate}`,
            referenceMonth: month,
            actor: "billing-aggregate",
          });
        }
      }
    }

    console.log(`[billing] Upserted ${upsertCount} summary rows`);
    console.log(
      `[billing] Total API cost: $${totalCost.toFixed(6)}, LLM cost: $${llmTotalCost.toFixed(6)}`,
    );
    console.log(`[billing] Done.`);
  } finally {
    await closeDb();
  }
}

async function issueCredit(flags: BillingFlags): Promise<void> {
  const db = getDb();

  try {
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

    // Verify key exists
    const key = await db
      .select()
      .from(apiKeys)
      .where(eq(apiKeys.keyPrefix, keyPrefix))
      .limit(1);

    if (key.length === 0) {
      console.error(`Error: key ${keyPrefix} not found`);
      Deno.exit(1);
    }

    const description = flags.description ?? "Manual credit";
    const actor = flags.email ?? "cli";

    await db.insert(billingLedger).values({
      keyPrefix,
      entryType: "credit",
      amountUsd: amount.toFixed(6),
      description,
      actor,
    });

    console.log(`Credited $${amount.toFixed(2)} to ${keyPrefix} (${key[0].userEmail})`);
    console.log(`  Description: ${description}`);
  } finally {
    await closeDb();
  }
}

async function showBalance(flags: BillingFlags): Promise<void> {
  const db = getDb();

  try {
    const keyPrefix = flags._[2] as string;
    if (!keyPrefix) {
      // Show all billable keys
      const keys = await db
        .select()
        .from(apiKeys)
        .where(and(eq(apiKeys.billable, true), isNull(apiKeys.revokedAt)));

      for (const k of keys) {
        const entries = await db
          .select()
          .from(billingLedger)
          .where(eq(billingLedger.keyPrefix, k.keyPrefix));

        let credits = 0;
        let debits = 0;
        for (const e of entries) {
          const amt = parseFloat(e.amountUsd);
          if (e.entryType === "credit") credits += amt;
          else debits += amt;
        }

        const balance = credits - debits;
        const status = balance >= 0 ? "OK" : "DEFICIT";
        console.log(
          `  ${k.keyPrefix}  ${k.userEmail.padEnd(30)}  credits: $${credits.toFixed(2).padStart(10)}  debits: $${debits.toFixed(2).padStart(10)}  balance: $${balance.toFixed(2).padStart(10)}  [${status}]`,
        );
      }
      return;
    }

    // Show specific key
    const entries = await db
      .select()
      .from(billingLedger)
      .where(eq(billingLedger.keyPrefix, keyPrefix));

    let credits = 0;
    let debits = 0;
    for (const e of entries) {
      const amt = parseFloat(e.amountUsd);
      if (e.entryType === "credit") credits += amt;
      else debits += amt;
    }

    console.log(`\n=== Billing Balance: ${keyPrefix} ===`);
    console.log(`  Credits:  $${credits.toFixed(2)}`);
    console.log(`  Debits:   $${debits.toFixed(2)}`);
    console.log(`  Balance:  $${(credits - debits).toFixed(2)}`);

    if (flags.json) {
      console.log(JSON.stringify({ keyPrefix, credits, debits, balance: credits - debits }, null, 2));
    }

    if (entries.length > 0) {
      console.log(`\n  Recent entries:`);
      const recent = entries.slice(-10);
      for (const e of recent) {
        const sign = e.entryType === "credit" ? "+" : "-";
        const date = new Date(e.createdAt).toISOString().slice(0, 10);
        console.log(
          `    ${date}  ${sign}$${parseFloat(e.amountUsd).toFixed(2).padStart(10)}  ${e.description}`,
        );
      }
    }
  } finally {
    await closeDb();
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
  console.log("  --email EMAIL       Actor email for audit trail");
  console.log();
  console.log("OPTIONS (balance):");
  console.log("  [prefix]    Key prefix (omit to show all billable keys)");
  console.log("  --json      Output JSON format");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd billing aggregate");
  console.log("  ssmd billing aggregate --date 2026-02-21 --dry-run");
  console.log("  ssmd billing credit sk_live_abc --amount 500 --description 'Research credit'");
  console.log("  ssmd billing balance");
  console.log("  ssmd billing balance sk_live_abc");
}
