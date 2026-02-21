import { detectPII, redactPII, hasPII } from "./pii.ts";
import { detectInjection } from "./injection.ts";
import type { Database } from "../db/mod.ts";
import { getSettingValue } from "../db/mod.ts";

export { detectPII, redactPII, hasPII } from "./pii.ts";
export { detectInjection } from "./injection.ts";

// ============================================================================
// MODEL ALLOWLIST
// Add models here to allow them through the proxy. Format: "provider/model-name"
// See https://openrouter.ai/models for available models
// ============================================================================
const ALLOWED_MODELS: ReadonlySet<string> = new Set([
  "anthropic/claude-sonnet-4.5",
  "anthropic/claude-sonnet-4.6",
  "qwen/qwen3-embedding-8b",
  "x-ai/grok-code-fast-1"
]);

/**
 * Check if a model is allowed through the proxy.
 * Returns { allowed: true } or { allowed: false, reason: string }
 */
export function checkModelAllowed(model: string): { allowed: boolean; reason?: string } {
  if (ALLOWED_MODELS.has(model)) {
    return { allowed: true };
  }
  return {
    allowed: false,
    reason: `Model "${model}" is not in the allowlist. Allowed: ${[...ALLOWED_MODELS].join(", ")}`,
  };
}

export interface GuardrailSettings {
  piiEnabled: boolean;
  piiAction: "block" | "redact";
  injectionEnabled: boolean;
  maxTokens: number | null;
}

export async function getGuardrailSettings(db: Database): Promise<GuardrailSettings> {
  const [piiEnabled, piiAction, injectionEnabled, maxTokens] = await Promise.all([
    getSettingValue(db, "guardrail_pii_enabled", false),
    getSettingValue(db, "guardrail_pii_action", "block"),
    getSettingValue(db, "guardrail_injection_enabled", false),
    getSettingValue(db, "guardrail_max_tokens", null),
  ]);

  return {
    piiEnabled: piiEnabled as boolean,
    piiAction: piiAction as "block" | "redact",
    injectionEnabled: injectionEnabled as boolean,
    maxTokens: maxTokens as number | null,
  };
}

export interface GuardrailCheckResult {
  allowed: boolean;
  reason?: string;
  modifiedMessages?: Array<{ role: string; content: string }>;
}

export function applyGuardrails(
  messages: Array<{ role: string; content: string }>,
  settings: GuardrailSettings
): GuardrailCheckResult {
  // Check injection in user messages
  if (settings.injectionEnabled) {
    for (const msg of messages) {
      if (msg.role === "user") {
        const injection = detectInjection(msg.content);
        if (injection.detected) {
          return { allowed: false, reason: "Prompt injection detected" };
        }
      }
    }
  }

  // Check PII
  if (settings.piiEnabled) {
    for (const msg of messages) {
      if (hasPII(msg.content)) {
        if (settings.piiAction === "block") {
          return { allowed: false, reason: "PII detected in request" };
        }
        // Redact mode - modify messages
        const modifiedMessages = messages.map((m) => ({
          ...m,
          content: redactPII(m.content),
        }));
        return { allowed: true, modifiedMessages };
      }
    }
  }

  return { allowed: true };
}
