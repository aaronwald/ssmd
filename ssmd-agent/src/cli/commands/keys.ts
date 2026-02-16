/**
 * ssmd keys - Manage API keys for data sharing
 *
 * Subcommands:
 *   create  Create a new API key with optional expiration
 *   list    List all API keys
 *   revoke  Revoke an API key by prefix
 */

interface KeysFlags {
  _: (string | number)[];
  email?: string;
  scopes?: string;
  expires?: string;
  name?: string;
  json?: boolean;
  feeds?: string;
  "date-from"?: string;
  "date-to"?: string;
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

export async function handleKeys(subcommand: string, flags: KeysFlags): Promise<void> {
  switch (subcommand) {
    case "create":
      await createKey(flags);
      break;
    case "list":
      await listKeys(flags);
      break;
    case "revoke":
      await revokeKey(flags);
      break;
    case "help":
    default:
      printKeysHelp();
      break;
  }
}

async function createKey(flags: KeysFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const email = flags.email;
  if (!email) {
    console.error("Error: --email is required");
    console.log("Usage: ssmd keys create --email alice@uni.edu --feeds kalshi,polymarket --date-from 2026-01-01 --date-to 2026-06-30");
    Deno.exit(1);
  }

  // Validate feeds (required)
  const feedsStr = flags.feeds;
  if (!feedsStr) {
    console.error("Error: --feeds is required");
    console.log("Usage: ssmd keys create --email alice@uni.edu --feeds kalshi,polymarket --date-from 2026-01-01 --date-to 2026-06-30");
    Deno.exit(1);
  }
  const allowedFeeds = feedsStr.split(",").map((f) => f.trim()).filter(Boolean);

  // Validate date range (required)
  const dateFrom = flags["date-from"];
  const dateTo = flags["date-to"];
  if (!dateFrom || !dateTo) {
    console.error("Error: --date-from and --date-to are required (YYYY-MM-DD)");
    console.log("Usage: ssmd keys create --email alice@uni.edu --feeds kalshi --date-from 2026-01-01 --date-to 2026-06-30");
    Deno.exit(1);
  }
  const dateRegex = /^\d{4}-\d{2}-\d{2}$/;
  if (!dateRegex.test(dateFrom) || !dateRegex.test(dateTo)) {
    console.error("Error: --date-from and --date-to must be YYYY-MM-DD format");
    Deno.exit(1);
  }

  const scopesStr = flags.scopes ?? "datasets:read";
  const scopes = scopesStr.split(",").map((s) => s.trim());
  const name = flags.name ?? `${email} key`;

  // Parse expires (e.g., "72h" -> 72)
  let expiresInHours: number | undefined;
  if (flags.expires) {
    const match = flags.expires.match(/^(\d+)h$/);
    if (!match) {
      console.error("Error: --expires must be in format like '72h'");
      Deno.exit(1);
    }
    expiresInHours = parseInt(match[1], 10);
    if (expiresInHours < 1 || expiresInHours > 720) {
      console.error("Error: --expires must be between 1h and 720h (30 days)");
      Deno.exit(1);
    }
  }

  const body: Record<string, unknown> = {
    name,
    scopes,
    userEmail: email,
    allowedFeeds,
    dateRangeStart: dateFrom,
    dateRangeEnd: dateTo,
  };
  if (expiresInHours !== undefined) {
    body.expiresInHours = expiresInHours;
  }

  const res = await fetch(`${apiUrl}/v1/keys`, {
    method: "POST",
    headers: apiHeaders(apiKey),
    body: JSON.stringify(body),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error creating key: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const result = await res.json();

  if (flags.json) {
    console.log(JSON.stringify(result, null, 2));
  } else {
    console.log();
    console.log("API Key Created (save this - it won't be shown again):");
    console.log("=".repeat(60));
    console.log(`  Key:        ${result.key}`);
    console.log(`  Prefix:     ${result.prefix}`);
    console.log(`  Name:       ${result.name}`);
    console.log(`  Scopes:     ${result.scopes.join(", ")}`);
    console.log(`  Feeds:      ${result.allowedFeeds.join(", ")}`);
    console.log(`  Date range: ${result.dateRangeStart} → ${result.dateRangeEnd}`);
    console.log(`  Created:    ${result.createdAt}`);
    if (result.expiresAt) {
      console.log(`  Expires:    ${result.expiresAt}`);
    }
    console.log();
  }
}

async function listKeys(flags: KeysFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const res = await fetch(`${apiUrl}/v1/keys`, {
    headers: apiHeaders(apiKey),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error listing keys: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const { keys } = await res.json() as {
    keys: Array<{
      prefix: string;
      name: string;
      userEmail: string;
      scopes: string[];
      lastUsedAt: string | null;
      createdAt: string;
      expiresAt: string | null;
      allowedFeeds: string[];
      dateRangeStart: string;
      dateRangeEnd: string;
    }>;
  };

  if (flags.json) {
    console.log(JSON.stringify(keys, null, 2));
    return;
  }

  if (keys.length === 0) {
    console.log("No API keys found.");
    return;
  }

  console.log();
  console.log(`${"PREFIX".padEnd(14)} ${"EMAIL".padEnd(25)} ${"FEEDS".padEnd(30)} ${"DATE RANGE".padEnd(25)} ${"EXPIRES".padEnd(20)}`);
  console.log("-".repeat(115));

  for (const k of keys) {
    const feeds = k.allowedFeeds.join(",");
    const farFuture = k.dateRangeEnd >= "2099-01-01";
    const dateRange = farFuture
      ? `${k.dateRangeStart} →`
      : `${k.dateRangeStart} → ${k.dateRangeEnd}`;
    const expires = k.expiresAt ? new Date(k.expiresAt).toISOString().slice(0, 16) : "never";
    console.log(
      `${k.prefix.padEnd(14)} ${k.userEmail.padEnd(25)} ${feeds.padEnd(30)} ${dateRange.padEnd(25)} ${expires.padEnd(20)}`
    );
  }
  console.log();
}

async function revokeKey(flags: KeysFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const prefix = flags._[2] as string;
  if (!prefix) {
    console.error("Error: key prefix is required");
    console.log("Usage: ssmd keys revoke <prefix>");
    Deno.exit(1);
  }

  const res = await fetch(`${apiUrl}/v1/keys/${prefix}`, {
    method: "DELETE",
    headers: apiHeaders(apiKey),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error revoking key: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const result = await res.json();
  if (result.revoked) {
    console.log(`Key ${prefix} revoked.`);
  } else {
    console.log(`Key ${prefix} was not found or already revoked.`);
  }
}

function printKeysHelp(): void {
  console.log("Usage: ssmd keys <command> [options]");
  console.log();
  console.log("Manage API keys for data sharing");
  console.log();
  console.log("COMMANDS:");
  console.log("  create    Create a new API key");
  console.log("  list      List all API keys");
  console.log("  revoke    Revoke an API key by prefix");
  console.log();
  console.log("OPTIONS (create):");
  console.log("  --email EMAIL         User email (required)");
  console.log("  --feeds FEEDS         Comma-separated feed names (required, e.g., kalshi,polymarket)");
  console.log("  --date-from DATE      Start of allowed date range, YYYY-MM-DD (required)");
  console.log("  --date-to DATE        End of allowed date range, YYYY-MM-DD (required, use 2099-12-31 for open-ended)");
  console.log("  --scopes SCOPES       Comma-separated scopes (default: datasets:read)");
  console.log("  --expires HOURS       Expiration (e.g., 72h, max 720h)");
  console.log("  --name NAME           Key name/description");
  console.log("  --json                Output JSON format");
  console.log();
  console.log("OPTIONS (list):");
  console.log("  --json              Output JSON format");
  console.log();
  console.log("ENVIRONMENT:");
  console.log("  SSMD_API_URL        API base URL (default: http://localhost:8080)");
  console.log("  SSMD_DATA_API_KEY   Admin API key for key management");
  console.log();
  console.log("EXAMPLES:");
  console.log('  ssmd keys create --email alice@uni.edu --feeds kalshi,polymarket --date-from 2026-01-01 --date-to 2026-06-30 --scopes datasets:read');
  console.log("  ssmd keys list");
  console.log("  ssmd keys revoke sk_live_abc123");
}
