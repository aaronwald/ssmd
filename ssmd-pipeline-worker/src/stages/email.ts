import type { StageConfig, StageResult } from "../types.ts";
import type { ExecuteContext } from "./mod.ts";

const MAX_EMAILS_PER_HOUR = 10;
const emailCounts = new Map<string, { count: number; windowStart: number }>();

export function checkEmailRateLimit(pipelineId: string): boolean {
  const now = Date.now();
  const entry = emailCounts.get(pipelineId);

  if (!entry || now - entry.windowStart > 3600_000) {
    emailCounts.set(pipelineId, { count: 1, windowStart: now });
    return true;
  }

  if (entry.count >= MAX_EMAILS_PER_HOUR) {
    return false;
  }

  entry.count++;
  return true;
}

export function validateRecipient(to: string, allowlist: string[]): boolean {
  return allowlist.some((allowed) => to.toLowerCase() === allowed.toLowerCase());
}

export async function executeEmail(
  config: StageConfig,
  ctx: ExecuteContext,
  signal: AbortSignal,
  pipelineId?: string,
): Promise<StageResult> {
  const to = config.to;
  const subject = config.subject;
  const html = config.html ?? config.template;

  if (!to || !subject || !html) {
    return { status: "failed", error: "email stage requires 'to', 'subject', and 'html' in config" };
  }

  const allowlistStr = Deno.env.get("EMAIL_RECIPIENT_ALLOWLIST") ?? "";
  const allowlist = allowlistStr.split(",").map((s) => s.trim()).filter(Boolean);

  if (allowlist.length > 0 && !validateRecipient(to, allowlist)) {
    return { status: "failed", error: `Recipient '${to}' not in allowlist` };
  }

  if (pipelineId && !checkEmailRateLimit(pipelineId)) {
    return { status: "failed", error: `Rate limit exceeded: max ${MAX_EMAILS_PER_HOUR} emails/hour per pipeline` };
  }

  try {
    const resp = await fetch(`${ctx.dataTsUrl}/v1/internal/email`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${ctx.adminApiKey}`,
      },
      body: JSON.stringify({ to, subject, html }),
      signal,
    });

    if (!resp.ok) {
      const text = await resp.text();
      return { status: "failed", error: `Email endpoint returned ${resp.status}: ${text.slice(0, 500)}` };
    }

    return {
      status: "completed",
      output: { sent: true, to, subject },
    };
  } catch (err) {
    return {
      status: "failed",
      error: err instanceof Error ? err.message : String(err),
    };
  }
}
