/**
 * ssmd billing - Billing aggregation and credit management (via API)
 *
 * Subcommands:
 *   aggregate    Aggregate yesterday's API request log into billing_ledger debits
 *   credit       Issue a credit to a key
 *   balance      Show balance for a key
 *   report       Send daily billing report email
 */

import nodemailer from "nodemailer";

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
    case "report":
      await sendBillingReport(flags);
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

async function sendBillingReport(_flags: BillingFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const now = new Date();
  const month = `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}`;
  const dateStr = now.toISOString().slice(0, 10);

  console.log(`[billing] Generating report for ${month}...`);

  // Fetch monthly report
  const reportRes = await fetch(`${apiUrl}/v1/billing/report?month=${month}`, {
    headers: apiHeaders(apiKey),
  });

  if (!reportRes.ok) {
    const err = await reportRes.json().catch(() => ({ error: reportRes.statusText }));
    console.error(`Error fetching report: ${(err as Record<string, string>).error ?? reportRes.statusText}`);
    Deno.exit(1);
  }

  const report = await reportRes.json() as {
    month: string;
    keys: Array<{
      keyPrefix: string;
      keyName: string;
      userEmail: string;
      totalRequests: number;
      totalBytes: number;
      totalErrors: number;
    }>;
  };

  // Fetch balance for each key
  const rows: Array<{
    prefix: string;
    name: string;
    email: string;
    requests: number;
    bytes: number;
    errors: number;
    credits: number;
    debits: number;
    balance: number;
  }> = [];

  for (const k of report.keys) {
    const balRes = await fetch(`${apiUrl}/v1/billing/balance?key_prefix=${k.keyPrefix}`, {
      headers: apiHeaders(apiKey),
    });

    let credits = 0, debits = 0, balance = 0;
    if (balRes.ok) {
      const bal = await balRes.json() as { credits: number; debits: number; balance: number };
      credits = bal.credits;
      debits = bal.debits;
      balance = bal.balance;
    }

    rows.push({
      prefix: k.keyPrefix,
      name: k.keyName ?? "",
      email: k.userEmail,
      requests: k.totalRequests,
      bytes: k.totalBytes,
      errors: k.totalErrors ?? 0,
      credits,
      debits,
      balance,
    });
  }

  // Summary
  const totalKeys = rows.length;
  const totalRequests = rows.reduce((s, r) => s + r.requests, 0);
  const totalBytes = rows.reduce((s, r) => s + r.bytes, 0);

  // Always print to console
  console.log(`\n=== SSMD Billing Report — ${dateStr} (${month}) ===\n`);
  console.log(`  Billable Keys:    ${totalKeys}`);
  console.log(`  Total Requests:   ${totalRequests.toLocaleString()}`);
  console.log(`  Total Bytes:      ${formatBytes(totalBytes)}\n`);

  const col = (s: string, w: number) => s.length > w ? s.slice(0, w - 1) + "…" : s.padEnd(w);
  console.log(
    `  ${col("Prefix", 18)}${col("Name", 24)}${col("Email", 28)}${"Reqs".padStart(8)} ${"Bytes".padStart(10)} ${"Errs".padStart(6)} ${"Balance".padStart(12)} Status`,
  );
  console.log(`  ${"-".repeat(116)}`);
  for (const r of rows) {
    const status = r.balance >= 0 ? "OK" : "DEFICIT";
    console.log(
      `  ${col(r.prefix, 18)}${col(r.name, 24)}${col(r.email, 28)}${r.requests.toLocaleString().padStart(8)} ${formatBytes(r.bytes).padStart(10)} ${String(r.errors).padStart(6)} $${r.balance.toFixed(2).padStart(11)} ${status}`,
    );
  }
  console.log();

  // Send email if SMTP is configured
  const host = Deno.env.get("SMTP_HOST") ?? "smtp.gmail.com";
  const port = Number(Deno.env.get("SMTP_PORT") ?? "587");
  const user = Deno.env.get("SMTP_USER");
  const pass = Deno.env.get("SMTP_PASS");
  const to = Deno.env.get("SMTP_TO");

  if (!user || !pass || !to) {
    console.log("[billing] SMTP not configured, skipping email.");
    return;
  }

  const html = buildBillingEmailHtml(dateStr, month, rows, totalKeys, totalRequests, totalBytes);

  const transporter = nodemailer.createTransport({
    host,
    port,
    secure: false,
    auth: { user, pass },
  });

  await transporter.sendMail({
    from: user,
    to,
    subject: `[SSMD] Billing Report — ${dateStr}`,
    html,
  });

  console.log(`[billing] Report email sent to ${to}`);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function buildBillingEmailHtml(
  date: string,
  month: string,
  rows: Array<{
    prefix: string;
    name: string;
    email: string;
    requests: number;
    bytes: number;
    errors: number;
    credits: number;
    debits: number;
    balance: number;
  }>,
  totalKeys: number,
  totalRequests: number,
  totalBytes: number,
): string {
  const keyRows = rows.map((r) => {
    const status = r.balance >= 0 ? "OK" : "DEFICIT";
    const statusColor = r.balance >= 0 ? "#22c55e" : "#ef4444";
    return `<tr>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb; font-family: monospace;">${r.prefix}</td>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb;">${r.name}</td>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb;">${r.email}</td>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb; text-align: right;">${r.requests.toLocaleString()}</td>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb; text-align: right;">${formatBytes(r.bytes)}</td>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb; text-align: right;">${r.errors}</td>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb; text-align: right;">$${r.balance.toFixed(2)}</td>
      <td style="padding: 8px; border-bottom: 1px solid #e5e7eb; text-align: center; color: ${statusColor}; font-weight: bold;">${status}</td>
    </tr>`;
  }).join("\n");

  return `<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; max-width: 900px; margin: 0 auto; padding: 20px; color: #1f2937;">
  <div style="background: #f9fafb; border-radius: 8px; padding: 24px;">
    <h1 style="margin: 0 0 4px 0; font-size: 20px;">SSMD Billing Report</h1>
    <p style="margin: 0 0 20px 0; color: #6b7280;">${date} &mdash; Month: ${month}</p>

    <div style="display: flex; gap: 16px; margin-bottom: 24px;">
      <div style="background: white; border-radius: 6px; padding: 16px; flex: 1; border: 1px solid #e5e7eb;">
        <div style="font-size: 24px; font-weight: bold;">${totalKeys}</div>
        <div style="color: #6b7280; font-size: 13px;">Billable Keys</div>
      </div>
      <div style="background: white; border-radius: 6px; padding: 16px; flex: 1; border: 1px solid #e5e7eb;">
        <div style="font-size: 24px; font-weight: bold;">${totalRequests.toLocaleString()}</div>
        <div style="color: #6b7280; font-size: 13px;">Total Requests</div>
      </div>
      <div style="background: white; border-radius: 6px; padding: 16px; flex: 1; border: 1px solid #e5e7eb;">
        <div style="font-size: 24px; font-weight: bold;">${formatBytes(totalBytes)}</div>
        <div style="color: #6b7280; font-size: 13px;">Total Bytes</div>
      </div>
    </div>

    <table style="width: 100%; border-collapse: collapse; background: white; border-radius: 6px; border: 1px solid #e5e7eb;">
      <thead>
        <tr style="background: #f3f4f6;">
          <th style="padding: 8px; text-align: left; border-bottom: 2px solid #e5e7eb;">Prefix</th>
          <th style="padding: 8px; text-align: left; border-bottom: 2px solid #e5e7eb;">Name</th>
          <th style="padding: 8px; text-align: left; border-bottom: 2px solid #e5e7eb;">Email</th>
          <th style="padding: 8px; text-align: right; border-bottom: 2px solid #e5e7eb;">Requests</th>
          <th style="padding: 8px; text-align: right; border-bottom: 2px solid #e5e7eb;">Bytes</th>
          <th style="padding: 8px; text-align: right; border-bottom: 2px solid #e5e7eb;">Errors</th>
          <th style="padding: 8px; text-align: right; border-bottom: 2px solid #e5e7eb;">Balance</th>
          <th style="padding: 8px; text-align: center; border-bottom: 2px solid #e5e7eb;">Status</th>
        </tr>
      </thead>
      <tbody>
        ${keyRows}
      </tbody>
    </table>

    <p style="color: #9ca3af; font-size: 12px; margin-top: 20px;">
      Generated at ${new Date().toISOString()} by ssmd billing report
    </p>
  </div>
</body>
</html>`;
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
  console.log("  report       Send daily billing report email");
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
