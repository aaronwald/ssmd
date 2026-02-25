/**
 * diagnosis.ts - AI-powered analysis of health and DQ results
 *
 * Gathers 7 days of health scores from PostgreSQL, enriches with live data
 * from ssmd-data-ts (freshness, volume), makes a single Claude API call
 * via the data-ts OpenRouter proxy, and sends an HTML diagnosis email.
 */
import { getDb, closeDb } from "../../lib/db/mod.ts";
import { listDailyScores } from "../../lib/db/health.ts";
import { config } from "../../config.ts";
import nodemailer from "nodemailer";

// Model for diagnosis (must be in the OpenRouter allowlist)
const DIAGNOSIS_MODEL = "anthropic/claude-sonnet-4.6";

// --- System prompt ---
// Inlined because CLI runs as a deno compile binary (no filesystem access).
// To update: edit below, tag cli-ts-v*, push, bump image tag in
// clusters/gke-prod/apps/ssmd/cronjobs/diagnosis-daily.yaml

const DIAGNOSIS_SYSTEM_PROMPT = `You are an ssmd pipeline health analyst. Given 7 days of health scores, \
data freshness metrics, and volume data, produce a diagnosis.

Output JSON with:
- overall_status: "GREEN" | "YELLOW" | "RED"
- summary: 1-2 sentence executive summary
- feed_diagnoses: array of { feed, status, issue, likely_cause, action }
- trends: array of { feed, direction: "improving"|"stable"|"degrading", note }
- recommendations: array of prioritized action items

Focus on: score drops vs 7-day average, stale feeds (>7h), \
volume anomalies, coverage gaps. If everything looks healthy, say so concisely.

Output ONLY valid JSON. No markdown, no code fences, no explanation.`;

// --- Types ---

interface Diagnosis {
  overall_status: "GREEN" | "YELLOW" | "RED";
  summary: string;
  feed_diagnoses: FeedDiagnosis[];
  trends: TrendEntry[];
  recommendations: string[];
}

interface FeedDiagnosis {
  feed: string;
  status: "GREEN" | "YELLOW" | "RED";
  issue: string;
  likely_cause: string;
  action: string;
}

interface TrendEntry {
  feed: string;
  direction: "improving" | "stable" | "degrading";
  note: string;
}

interface DiagnosisFlags {
  _: (string | number)[];
  json?: boolean;
}

// --- Entry point ---

export async function handleDiagnosis(subcommand: string, flags: DiagnosisFlags): Promise<void> {
  switch (subcommand) {
    case "analyze":
      await runDiagnosisAnalysis(flags);
      break;
    case "help":
    default:
      printDiagnosisHelp();
      break;
  }
}

// --- Main analysis ---

async function runDiagnosisAnalysis(flags: DiagnosisFlags): Promise<void> {
  const jsonOutput = flags.json === true;
  const today = new Date().toISOString().slice(0, 10);
  const sevenDaysAgo = new Date(Date.now() - 7 * 24 * 60 * 60 * 1000).toISOString().slice(0, 10);

  const apiUrl = config.apiUrl;
  const apiKey = config.apiKey;

  if (!apiKey) {
    console.error("SSMD_DATA_API_KEY is required for diagnosis");
    Deno.exit(1);
  }

  const db = getDb();

  try {
    // 1. Gather health scores from PostgreSQL (7-day history)
    if (!jsonOutput) console.log("Gathering 7-day health scores...");
    const scores = await listDailyScores(db, { from: sevenDaysAgo, to: today, limit: 500 });

    if (scores.length === 0) {
      console.error("No health scores found in the last 7 days. Run 'ssmd health daily' first.");
      Deno.exit(1);
    }

    // 2. Fetch live freshness from data-ts API
    if (!jsonOutput) console.log("Fetching data freshness...");
    const freshness = await apiGet(`${apiUrl}/v1/data/freshness`, apiKey);

    // 3. Fetch today's volume summary from data-ts API
    if (!jsonOutput) console.log("Fetching volume summary...");
    const volume = await apiGet(`${apiUrl}/v1/data/volume?date=${today}`, apiKey);

    // 4. Call Claude via data-ts proxy
    if (!jsonOutput) console.log("Requesting AI diagnosis...");
    const diagnosis = await callClaude(apiUrl, apiKey, DIAGNOSIS_SYSTEM_PROMPT, { scores, freshness, volume });

    // 5. Output or send email
    if (jsonOutput) {
      console.log(JSON.stringify({ date: today, ...diagnosis }));
    } else {
      printDiagnosis(today, diagnosis);
    }

    // 6. Send email (always)
    await sendDiagnosisEmail(today, diagnosis);
  } finally {
    await closeDb();
  }
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

function extractJson(content: string): string {
  const fenceMatch = content.match(/```(?:json)?\s*\n?([\s\S]*?)\n?\s*```/);
  if (fenceMatch) return fenceMatch[1].trim();
  const firstBrace = content.indexOf("{");
  const lastBrace = content.lastIndexOf("}");
  if (firstBrace !== -1 && lastBrace > firstBrace) {
    return content.slice(firstBrace, lastBrace + 1).trim();
  }
  return content.trim();
}

async function callClaude(
  apiUrl: string,
  apiKey: string,
  systemPrompt: string,
  data: { scores: unknown; freshness: unknown; volume: unknown },
): Promise<Diagnosis> {
  const res = await fetch(`${apiUrl}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-API-Key": apiKey,
    },
    body: JSON.stringify({
      model: DIAGNOSIS_MODEL,
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

  // Handle content being string, array of content blocks, or null
  const rawContent = result.choices?.[0]?.message?.content;
  let content: string;
  if (typeof rawContent === "string") {
    content = rawContent;
  } else if (Array.isArray(rawContent)) {
    content = rawContent
      .filter((block: Record<string, unknown>) => block.type === "text")
      .map((block: Record<string, unknown>) => block.text)
      .join("");
  } else {
    content = "";
  }

  // Check for truncation
  const finishReason = result.choices?.[0]?.finish_reason;
  if (finishReason === "length") {
    console.error(`WARN: Claude response truncated (finish_reason=length, max_tokens=2500)`);
  }

  // Extract JSON from response — try multiple strategies
  const cleaned = extractJson(content);

  try {
    const parsed = JSON.parse(cleaned) as Diagnosis;
    // Validate required fields
    if (!parsed.overall_status || !parsed.summary) {
      throw new Error("Missing required fields in diagnosis");
    }
    return {
      overall_status: parsed.overall_status,
      summary: parsed.summary,
      feed_diagnoses: parsed.feed_diagnoses ?? [],
      trends: parsed.trends ?? [],
      recommendations: (parsed.recommendations ?? []).map((r): string => {
        if (typeof r === "string") return r;
        if (r && typeof r === "object") {
          const obj = r as Record<string, unknown>;
          for (const key of ["action", "recommendation", "text", "description", "title"]) {
            if (typeof obj[key] === "string") return obj[key] as string;
          }
          return JSON.stringify(r);
        }
        return String(r);
      }),
    };
  } catch (e) {
    console.error(`Failed to parse Claude response: ${e}`);
    console.error(`Content type: ${typeof rawContent}, isArray: ${Array.isArray(rawContent)}`);
    console.error(`Finish reason: ${finishReason}`);
    console.error(`Raw content (first 500): ${JSON.stringify(rawContent).slice(0, 500)}`);
    console.error(`Cleaned (first 500): ${cleaned.slice(0, 500)}`);
    // Return a fallback diagnosis
    return {
      overall_status: "YELLOW",
      summary: "AI diagnosis failed to parse. Manual review recommended.",
      feed_diagnoses: [],
      trends: [],
      recommendations: ["Review health dashboard manually — AI analysis encountered a parsing error."],
    };
  }
}

// --- Console output ---

function printDiagnosis(date: string, d: Diagnosis): void {
  console.log();
  console.log(`Diagnosis: ${d.overall_status} — ${date}`);
  console.log("=".repeat(60));
  console.log();
  console.log(`Summary: ${d.summary}`);

  if (d.feed_diagnoses.length > 0) {
    console.log();
    console.log("Feed Diagnoses:");
    for (const fd of d.feed_diagnoses) {
      console.log(`  ${fd.feed}: ${fd.status}`);
      if (fd.issue) console.log(`    Issue: ${fd.issue}`);
      if (fd.likely_cause) console.log(`    Cause: ${fd.likely_cause}`);
      if (fd.action) console.log(`    Action: ${fd.action}`);
    }
  }

  if (d.trends.length > 0) {
    console.log();
    console.log("Trends:");
    for (const t of d.trends) {
      console.log(`  ${t.feed}: ${t.direction} — ${t.note}`);
    }
  }

  if (d.recommendations.length > 0) {
    console.log();
    console.log("Recommendations:");
    for (const r of d.recommendations) {
      console.log(`  - ${r}`);
    }
  }
  console.log();
}

// --- Email ---

function escapeHtml(str: unknown): string {
  const s = typeof str === "string" ? str : String(str ?? "");
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function gradeColor(status: string): string {
  return { GREEN: "#2e7d32", YELLOW: "#f9a825", RED: "#c62828" }[status] ?? "#333";
}

function statusBadge(status: string): string {
  const color = gradeColor(status);
  return `<span style="display:inline-block;padding:2px 10px;border-radius:4px;background:${color};color:#fff;font-weight:bold;font-size:13px">${status}</span>`;
}

function buildDiagnosisEmailHtml(date: string, d: Diagnosis): string {
  const gc = gradeColor(d.overall_status);

  // Feed diagnosis table
  let feedTableHtml = "";
  if (d.feed_diagnoses.length > 0) {
    const rows = d.feed_diagnoses.map((fd) => `<tr>
      <td style="padding:6px 8px;border-bottom:1px solid #eee">${escapeHtml(fd.feed)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee">${statusBadge(fd.status)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee">${escapeHtml(fd.issue || "—")}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;font-size:12px;color:#666">${escapeHtml(fd.likely_cause || "—")}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;font-size:12px">${escapeHtml(fd.action || "—")}</td>
    </tr>`).join("");

    feedTableHtml = `<h2 style="color:#333;font-size:16px;margin-top:24px">Feed Diagnoses</h2>
    <table style="width:100%;border-collapse:collapse;font-size:13px">
      <tr>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Feed</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Status</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Issue</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Likely Cause</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Action</th>
      </tr>
      ${rows}
    </table>`;
  }

  // Trends table
  let trendsHtml = "";
  if (d.trends.length > 0) {
    const directionIcon = (dir: string) => {
      switch (dir) {
        case "improving": return '<span style="color:#2e7d32">&#x25B2;</span>';
        case "degrading": return '<span style="color:#c62828">&#x25BC;</span>';
        default: return '<span style="color:#888">&#x25CF;</span>';
      }
    };

    const rows = d.trends.map((t) => `<tr>
      <td style="padding:6px 8px;border-bottom:1px solid #eee">${escapeHtml(t.feed)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee">${directionIcon(t.direction)} ${escapeHtml(t.direction)}</td>
      <td style="padding:6px 8px;border-bottom:1px solid #eee;font-size:12px;color:#666">${escapeHtml(t.note)}</td>
    </tr>`).join("");

    trendsHtml = `<h2 style="color:#333;font-size:16px;margin-top:24px">7-Day Trends</h2>
    <table style="width:100%;border-collapse:collapse;font-size:13px">
      <tr>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Feed</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Direction</th>
        <th style="background:#f8f8f8;text-align:left;padding:8px;border-bottom:2px solid #ddd">Note</th>
      </tr>
      ${rows}
    </table>`;
  }

  // Recommendations list
  let recsHtml = "";
  if (d.recommendations.length > 0) {
    const items = d.recommendations.map((r, i) =>
      `<li style="padding:4px 0;color:#333"><strong>${i + 1}.</strong> ${escapeHtml(r)}</li>`
    ).join("");
    recsHtml = `<h2 style="color:#333;font-size:16px;margin-top:24px">Recommendations</h2>
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
    <h1>
      ${statusBadge(d.overall_status)}
      AI Diagnosis — ${date}
    </h1>

    <p style="font-size:14px;color:#333;line-height:1.5;margin-top:16px;padding:12px;background:#f8f9fa;border-radius:6px;border-left:4px solid ${gc}">
      ${escapeHtml(d.summary)}
    </p>

    ${feedTableHtml}
    ${trendsHtml}
    ${recsHtml}

    <div class="footer">
      Generated by ssmd diagnosis analyze with Claude at ${new Date().toISOString()}
    </div>
  </div>
</body>
</html>`;
}

async function sendDiagnosisEmail(date: string, diagnosis: Diagnosis): Promise<void> {
  const host = Deno.env.get("SMTP_HOST") ?? "smtp.gmail.com";
  const port = Number(Deno.env.get("SMTP_PORT") ?? "587");
  const user = Deno.env.get("SMTP_USER");
  const pass = Deno.env.get("SMTP_PASS");
  const to = Deno.env.get("SMTP_TO");

  if (!user || !pass || !to) {
    console.error("[diagnosis] SMTP_USER, SMTP_PASS, and SMTP_TO required for email");
    return;
  }

  const transporter = nodemailer.createTransport({
    host,
    port,
    secure: false,
    auth: { user, pass },
  });

  const html = buildDiagnosisEmailHtml(date, diagnosis);

  await transporter.sendMail({
    from: user,
    to,
    subject: `[SSMD] Diagnosis ${diagnosis.overall_status} — ${date}`,
    html,
  });

  console.log(`Diagnosis email sent to ${to}`);
}

// --- Help ---

function printDiagnosisHelp(): void {
  console.log("Usage: ssmd diagnosis <command> [options]");
  console.log();
  console.log("AI-powered analysis of health and DQ results");
  console.log();
  console.log("COMMANDS:");
  console.log("  analyze         Run diagnosis analysis and send email report");
  console.log();
  console.log("OPTIONS:");
  console.log("  --json          Output structured JSON to stdout");
  console.log();
  console.log("ENVIRONMENT VARIABLES:");
  console.log("  DATABASE_URL        PostgreSQL connection string (required)");
  console.log("  SSMD_API_URL        data-ts API base URL (default: http://localhost:8080)");
  console.log("  SSMD_DATA_API_KEY   data-ts API key (required, also used for Claude proxy)");
  console.log("  SMTP_USER           SMTP username (required for email)");
  console.log("  SMTP_PASS           SMTP password (required for email)");
  console.log("  SMTP_TO             Email recipient (required for email)");
  console.log();
  console.log("EXAMPLES:");
  console.log("  ssmd diagnosis analyze");
  console.log("  ssmd diagnosis analyze --json");
}
