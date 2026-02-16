/**
 * ssmd share - Generate signed URLs for parquet data
 *
 * Usage:
 *   ssmd share --feed kalshi --date 2026-02-15 [--type ticker] [--expires 12h]
 *   ssmd share feeds   List available feeds
 */

interface ShareFlags {
  _: (string | number)[];
  feed?: string;
  date?: string;
  from?: string;
  to?: string;
  type?: string;
  expires?: string;
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

export async function handleShare(subcommand: string, flags: ShareFlags): Promise<void> {
  if (subcommand === "feeds") {
    await listFeeds(flags);
    return;
  }

  if (subcommand === "help") {
    printShareHelp();
    return;
  }

  // Default: generate signed URLs
  await generateUrls(flags);
}

async function generateUrls(flags: ShareFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const feed = flags.feed;
  if (!feed) {
    console.error("Error: --feed is required");
    console.log("Usage: ssmd share --feed kalshi --date 2026-02-15 [--type ticker] [--expires 12h]");
    Deno.exit(1);
  }

  // Support --date (single day) or --from/--to (range)
  let from = flags.from ?? flags.date;
  let to = flags.to ?? flags.date;

  if (!from || !to) {
    console.error("Error: --date or --from/--to is required");
    Deno.exit(1);
  }

  const expires = flags.expires ?? "12h";

  const params = new URLSearchParams({
    feed,
    from,
    to,
    expires,
  });
  if (flags.type) {
    params.set("type", flags.type);
  }

  const res = await fetch(`${apiUrl}/v1/data/download?${params}`, {
    headers: apiHeaders(apiKey),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const result = await res.json() as {
    feed: string;
    from: string;
    to: string;
    type: string | null;
    files: Array<{
      path: string;
      name: string;
      type: string;
      hour: string;
      bytes: number;
      signedUrl: string;
      expiresAt: string;
    }>;
    expiresIn: string;
  };

  if (flags.json) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }

  if (result.files.length === 0) {
    console.log(`No parquet files found for ${feed} from ${from} to ${to}`);
    return;
  }

  console.log(`# ${result.files.length} files | ${feed} | ${from} to ${to} | expires ${result.expiresIn}`);

  // Output signed URLs, one per line (pipe-friendly)
  for (const file of result.files) {
    console.log(file.signedUrl);
  }
}

async function listFeeds(flags: ShareFlags): Promise<void> {
  const { apiUrl, apiKey } = getApiConfig();

  const res = await fetch(`${apiUrl}/v1/data/feeds`, {
    headers: apiHeaders(apiKey),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    console.error(`Error: ${(err as Record<string, string>).error ?? res.statusText}`);
    Deno.exit(1);
  }

  const { feeds } = await res.json() as {
    feeds: Array<{
      name: string;
      prefix: string;
      stream: string;
      messageTypes: string[];
    }>;
  };

  if (flags.json) {
    console.log(JSON.stringify(feeds, null, 2));
    return;
  }

  console.log();
  console.log("Available feeds:");
  console.log("-".repeat(60));
  for (const f of feeds) {
    console.log(`  ${f.name.padEnd(20)} ${f.stream.padEnd(12)} types: ${f.messageTypes.join(", ")}`);
  }
  console.log();
}

function printShareHelp(): void {
  console.log("Usage: ssmd share [command] [options]");
  console.log();
  console.log("Generate signed URLs for parquet data sharing");
  console.log();
  console.log("COMMANDS:");
  console.log("  feeds             List available data feeds");
  console.log("  (default)         Generate signed URLs for a feed");
  console.log();
  console.log("OPTIONS:");
  console.log("  --feed FEED       Feed name: kalshi, kraken-futures, polymarket (required)");
  console.log("  --date DATE       Single date (YYYY-MM-DD)");
  console.log("  --from DATE       Start date (YYYY-MM-DD)");
  console.log("  --to DATE         End date (YYYY-MM-DD)");
  console.log("  --type TYPE       Filter by message type (e.g., ticker, trade)");
  console.log("  --expires HOURS   URL expiration (default: 12h, max: 12h)");
  console.log("  --json            Output full JSON response");
  console.log();
  console.log("ENVIRONMENT:");
  console.log("  SSMD_API_URL        API base URL (default: http://localhost:8080)");
  console.log("  SSMD_DATA_API_KEY   API key with datasets:read scope");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd share --feed kalshi --date 2026-02-15");
  console.log("  ssmd share --feed kalshi --date 2026-02-15 --type ticker");
  console.log("  ssmd share --feed kraken-futures --from 2026-02-10 --to 2026-02-15 --json");
  console.log("  ssmd share feeds");
  console.log();
  console.log("PIPE-FRIENDLY:");
  console.log("  ssmd share --feed kalshi --date 2026-02-15 | xargs -n1 curl -o /tmp/");
}
