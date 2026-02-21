/**
 * volume-analysis.ts - AI-powered daily trade volume analysis
 *
 * Gathers cross-feed volume, per-feed trade details, and event-level
 * aggregation from ssmd-data-ts, makes a single Claude API call via
 * the data-ts OpenRouter proxy, and sends an HTML analysis email.
 *
 * No PostgreSQL needed — purely API-driven (GCS parquet via data-ts DuckDB).
 */
import { config } from "../../config.ts";
import nodemailer from "nodemailer";

// Model for volume analysis (must be in the OpenRouter allowlist)
const VOLUME_MODEL = "anthropic/claude-sonnet-4.6";

// --- System prompt ---
// Inlined because CLI runs as a deno compile binary (no filesystem access).
// To update: edit below, tag cli-ts-v*, push, bump image tag in
// clusters/gke-prod/apps/ssmd/cronjobs/volume-daily.yaml

const VOLUME_SYSTEM_PROMPT = `You are an ssmd market data volume analyst. Given daily trade volume data \
across Kalshi, Kraken Futures, and Polymarket feeds, produce a volume analysis.

You receive:
- volume: Cross-feed volume summary (trade counts, total volume, active tickers per feed)
- trades: Per-feed top tickers by trade count (top 20 each)
- events: Event-level aggregation (Kalshi events, Polymarket conditions, Kraken instruments)
- ticker_names: Map of Polymarket token IDs to human-readable market names. \
Always use the market name instead of the raw token ID in your output. \
For Kalshi, the ticker is already human-readable.

Volume units differ by feed: contracts (Kalshi), USD (Polymarket), base currency (Kraken). \
Do NOT sum across feeds. Compare within each feed only.

Output JSON with:
- summary: 1-2 sentence executive summary of today's trading activity
- feed_summaries: array of { feed, trade_count, volume, volume_unit, top_ticker, notable }
- market_highlights: array of { ticker, feed, why } — up to 5 interesting markets (unusual volume, new listings, etc.)
- observations: array of strings — cross-feed patterns, anomalies, or notable absences

Focus on: volume spikes vs typical activity, concentration (are a few tickers dominating?), \
cross-feed themes (same event traded on multiple exchanges), and any feeds with zero or very low activity.

Output ONLY valid JSON. No markdown, no code fences, no explanation.`;

// --- Types ---

interface VolumeAnalysis {
  summary: string;
  feed_summaries: FeedSummary[];
  market_highlights: MarketHighlight[];
  observations: string[];
}

interface FeedSummary {
  feed: string;
  trade_count: number;
  volume: number;
  volume_unit: string;
  top_ticker: string;
  notable: string;
}

interface MarketHighlight {
  ticker: string;
  feed: string;
  why: string;
}

interface VolumeFlags {
  _: (string | number)[];
  json?: boolean;
}

// --- Entry point ---

export async function handleVolume(subcommand: string, flags: VolumeFlags): Promise<void> {
  switch (subcommand) {
    case "analyze":
      await runVolumeAnalysis(flags);
      break;
    case "help":
    default:
      printVolumeHelp();
      break;
  }
}

// --- Main analysis ---

async function runVolumeAnalysis(flags: VolumeFlags): Promise<void> {
  const jsonOutput = flags.json === true;
  const today = new Date().toISOString().slice(0, 10);

  const apiUrl = config.apiUrl;
  const apiKey = config.apiKey;

  if (!apiKey) {
    console.error("SSMD_DATA_API_KEY is required for volume analysis");
    Deno.exit(1);
  }

  // 1. Gather volume data from data-ts API (no PostgreSQL needed)
  if (!jsonOutput) console.log("Fetching cross-feed volume summary...");
  const volume = await apiGet(`${apiUrl}/v1/data/volume?date=${today}`, apiKey);

  if (!jsonOutput) console.log("Fetching per-feed trade details...");
  const [kalshiTrades, krakenTrades, polyTrades] = await Promise.all([
    apiGet(`${apiUrl}/v1/data/trades?feed=kalshi&date=${today}&limit=20`, apiKey),
    apiGet(`${apiUrl}/v1/data/trades?feed=kraken-futures&date=${today}&limit=20`, apiKey),
    apiGet(`${apiUrl}/v1/data/trades?feed=polymarket&date=${today}&limit=20`, apiKey),
  ]);

  if (!jsonOutput) console.log("Fetching event-level aggregation...");
  const [kalshiEvents, polyEvents] = await Promise.all([
    apiGet(`${apiUrl}/v1/data/events?feed=kalshi&date=${today}&limit=10`, apiKey),
    apiGet(`${apiUrl}/v1/data/events?feed=polymarket&date=${today}&limit=10`, apiKey),
  ]);

  // 1b. Resolve Polymarket token IDs to market names via secmaster lookup
  const tickerNames = await resolvePolymarketNames(apiUrl, apiKey, polyTrades);

  // 2. Call Claude via data-ts proxy
  if (!jsonOutput) console.log("Requesting AI volume analysis...");
  const analysis = await callClaude(apiUrl, apiKey, VOLUME_SYSTEM_PROMPT, {
    volume,
    trades: { kalshi: kalshiTrades, kraken: krakenTrades, polymarket: polyTrades },
    events: { kalshi: kalshiEvents, polymarket: polyEvents },
    ticker_names: tickerNames,
  });

  // 3. Output or send email
  if (jsonOutput) {
    console.log(JSON.stringify({ date: today, ...analysis }));
  } else {
    printAnalysis(today, analysis);
  }

  // 4. Send email (always)
  await sendVolumeEmail(today, analysis);
}

// --- API helpers ---

async function apiGet(url: string, apiKey: string): Promise<unknown> {
  try {
    const res = await fetch(url, {
      headers: {
        "X-API-Key": apiKey,
      },
      signal: AbortSignal.timeout(15000),
    });

    if (!res.ok) {
      console.error(`API GET ${url} failed: ${res.status} ${res.statusText}`);
      return null;
    }

    return await res.json();
  } catch (err) {
    console.error(`API GET ${url} error: ${err}`);
    return null;
  }
}

async function resolvePolymarketNames(
  apiUrl: string,
  apiKey: string,
  polyTrades: unknown,
): Promise<Record<string, string>> {
  const names: Record<string, string> = {};
  try {
    // deno-lint-ignore no-explicit-any
    const trades = (polyTrades as any)?.trades;
    if (!Array.isArray(trades) || trades.length === 0) return names;

    const ids = trades
      .map((t: Record<string, unknown>) => t.ticker ?? t.instrument)
      .filter((id: unknown): id is string => typeof id === "string")
      .slice(0, 20);

    if (ids.length === 0) return names;

    const lookup = await apiGet(
      `${apiUrl}/v1/markets/lookup?ids=${ids.join(",")}&feed=polymarket`,
      apiKey,
    );
    // deno-lint-ignore no-explicit-any
    const markets = (lookup as any)?.markets;
    if (Array.isArray(markets)) {
      for (const m of markets) {
        if (m.id && m.name) names[m.id] = m.name;
      }
    }
  } catch {
    // Non-fatal — Claude will use raw IDs if lookup fails
  }
  return names;
}

async function callClaude(
  apiUrl: string,
  apiKey: string,
  systemPrompt: string,
  data: Record<string, unknown>,
): Promise<VolumeAnalysis> {
  const res = await fetch(`${apiUrl}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-API-Key": apiKey,
    },
    body: JSON.stringify({
      model: VOLUME_MODEL,
      messages: [
        { role: "system", content: systemPrompt },
        { role: "user", content: JSON.stringify(data) },
      ],
      max_tokens: 2500,
    }),
    signal: AbortSignal.timeout(60000),
  });

  if (!res.ok) {
    const body = await res.text();
    throw new Error(`Claude API call failed: ${res.status} ${body.slice(0, 200)}`);
  }

  const result = await res.json();
  const content = result.choices?.[0]?.message?.content ?? "";

  // Parse JSON from Claude's response (strip code fences if present)
  const cleaned = content.replace(/^```(?:json)?\s*/, "").replace(/\s*```$/, "").trim();

  try {
    const parsed = JSON.parse(cleaned) as VolumeAnalysis;
    if (!parsed.summary) {
      throw new Error("Missing required field: summary");
    }
    return {
      summary: parsed.summary,
      feed_summaries: parsed.feed_summaries ?? [],
      market_highlights: parsed.market_highlights ?? [],
      observations: parsed.observations ?? [],
    };
  } catch (e) {
    console.error(`Failed to parse Claude response: ${e}`);
    console.error(`Raw response: ${cleaned.slice(0, 500)}`);
    return {
      summary: "AI volume analysis failed to parse. Manual review recommended.",
      feed_summaries: [],
      market_highlights: [],
      observations: ["Volume analysis encountered a parsing error — review data-ts endpoints manually."],
    };
  }
}

// --- Console output ---

function printAnalysis(date: string, a: VolumeAnalysis): void {
  console.log();
  console.log(`Volume Analysis — ${date}`);
  console.log("=".repeat(60));
  console.log();
  console.log(`Summary: ${a.summary}`);

  if (a.feed_summaries.length > 0) {
    console.log();
    console.log("Feed Summaries:");
    for (const fs of a.feed_summaries) {
      console.log(`  ${fs.feed}: ${fs.trade_count} trades, ${fs.volume} ${fs.volume_unit}`);
      if (fs.top_ticker) console.log(`    Top ticker: ${fs.top_ticker}`);
      if (fs.notable) console.log(`    Notable: ${fs.notable}`);
    }
  }

  if (a.market_highlights.length > 0) {
    console.log();
    console.log("Market Highlights:");
    for (const mh of a.market_highlights) {
      console.log(`  ${mh.ticker} (${mh.feed}): ${mh.why}`);
    }
  }

  if (a.observations.length > 0) {
    console.log();
    console.log("Observations:");
    for (const o of a.observations) {
      console.log(`  - ${o}`);
    }
  }
  console.log();
}

// --- Email ---

function escapeHtml(str: unknown): string {
  const s = typeof str === "string" ? str : String(str ?? "");
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function buildVolumeEmailHtml(date: string, a: VolumeAnalysis): string {
  // Feed summary table
  let feedTableHtml = "";
  if (a.feed_summaries.length > 0) {
    const rows = a.feed_summaries.map((fs) => `<tr>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;font-weight:bold">${escapeHtml(fs.feed)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;text-align:right">${escapeHtml(fs.trade_count)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;text-align:right">${escapeHtml(fs.volume)} ${escapeHtml(fs.volume_unit)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee">${escapeHtml(fs.top_ticker)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;font-size:12px;color:#666">${escapeHtml(fs.notable || "—")}</td>
    </tr>`).join("");

    feedTableHtml = `<h2 style="color:#333;font-size:16px;margin-top:24px">Feed Summaries</h2>
    <table style="width:100%;border-collapse:collapse;font-size:13px">
      <tr>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Feed</th>
        <th style="background:#f8f8f8;text-align:right;padding:8px;border-bottom:2px solid #ddd">Trades</th>
        <th style="background:#f8f8f8;text-align:right;padding:8px;border-bottom:2px solid #ddd">Volume</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Top Ticker</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Notable</th>
      </tr>
      ${rows}
    </table>`;
  }

  // Market highlights
  let highlightsHtml = "";
  if (a.market_highlights.length > 0) {
    const rows = a.market_highlights.map((mh) => `<tr>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;font-weight:bold">${escapeHtml(mh.ticker)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee">${escapeHtml(mh.feed)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;font-size:12px;color:#666">${escapeHtml(mh.why)}</td>
    </tr>`).join("");

    highlightsHtml = `<h2 style="color:#333;font-size:16px;margin-top:24px">Market Highlights</h2>
    <table style="width:100%;border-collapse:collapse;font-size:13px">
      <tr>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Ticker</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Feed</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Why</th>
      </tr>
      ${rows}
    </table>`;
  }

  // Observations
  let obsHtml = "";
  if (a.observations.length > 0) {
    const items = a.observations.map((o, i) =>
      `<li style="padding:4px 0;color:#333"><strong>${i + 1}.</strong> ${escapeHtml(o)}</li>`
    ).join("");
    obsHtml = `<h2 style="color:#333;font-size:16px;margin-top:24px">Observations</h2>
    <ol style="padding-left:20px;font-size:13px">${items}</ol>`;
  }

  return `<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }
  .container { max-width: 700px; margin: 0 auto; background: #fff; border-radius: 8px; padding: 24px; }
  h1 { font-size: 20px; border-bottom: 2px solid #e0e0e0; padding-bottom: 8px; }
  .footer { margin-top: 24px; font-size: 11px; color: #999; border-top: 1px solid #eee; padding-top: 12px; }
</style>
</head>
<body>
  <div class="container">
    <h1>Volume Analysis — ${date}</h1>

    <p style="font-size:14px;color:#333;line-height:1.5;margin-top:16px;padding:12px;background:#f8f9fa;border-radius:6px;border-left:4px solid #1565c0">
      ${escapeHtml(a.summary)}
    </p>

    ${feedTableHtml}
    ${highlightsHtml}
    ${obsHtml}

    <div class="footer">
      Generated by ssmd volume analyze with Claude at ${new Date().toISOString()}
    </div>
  </div>
</body>
</html>`;
}

async function sendVolumeEmail(date: string, analysis: VolumeAnalysis): Promise<void> {
  const host = Deno.env.get("SMTP_HOST") ?? "smtp.gmail.com";
  const port = Number(Deno.env.get("SMTP_PORT") ?? "587");
  const user = Deno.env.get("SMTP_USER");
  const pass = Deno.env.get("SMTP_PASS");
  const to = Deno.env.get("SMTP_TO");

  if (!user || !pass || !to) {
    console.error("[volume] SMTP_USER, SMTP_PASS, and SMTP_TO required for email");
    return;
  }

  const transporter = nodemailer.createTransport({
    host,
    port,
    secure: false,
    auth: { user, pass },
  });

  const html = buildVolumeEmailHtml(date, analysis);

  await transporter.sendMail({
    from: user,
    to,
    subject: `[SSMD] Volume Analysis — ${date}`,
    html,
  });

  console.log(`Volume analysis email sent to ${to}`);
}

// --- Help ---

function printVolumeHelp(): void {
  console.log("Usage: ssmd volume <command> [options]");
  console.log();
  console.log("AI-powered daily trade volume analysis");
  console.log();
  console.log("COMMANDS:");
  console.log("  analyze         Run volume analysis and send email report");
  console.log();
  console.log("OPTIONS:");
  console.log("  --json          Output structured JSON to stdout");
  console.log();
  console.log("ENVIRONMENT VARIABLES:");
  console.log("  SSMD_API_URL        data-ts API base URL (default: http://localhost:8080)");
  console.log("  SSMD_DATA_API_KEY   data-ts API key (required, also used for Claude proxy)");
  console.log("  SMTP_USER           SMTP username (required for email)");
  console.log("  SMTP_PASS           SMTP password (required for email)");
  console.log("  SMTP_TO             Email recipient (required for email)");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd volume analyze");
  console.log("  ssmd volume analyze --json");
}
