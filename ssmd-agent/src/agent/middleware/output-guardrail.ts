import { detectHallucination } from "./validators/hallucination.ts";
import { checkToxicity } from "./validators/toxicity.ts";

export interface OutputGuardrailOptions {
  toxicityEnabled?: boolean;
  hallucinationEnabled?: boolean;
}

export interface GuardrailResult {
  allowed: boolean;
  reason?: string;
  content?: string;
}

const DEFAULT_OPTIONS: OutputGuardrailOptions = {
  toxicityEnabled: true,
  hallucinationEnabled: true,
};

export function applyOutputGuardrail(
  content: string,
  options: OutputGuardrailOptions = {}
): GuardrailResult {
  const opts = { ...DEFAULT_OPTIONS, ...options };

  // Check toxicity
  if (opts.toxicityEnabled) {
    const toxicity = checkToxicity(content);
    if (toxicity.toxic) {
      return {
        allowed: false,
        reason: `Blocked: toxic content detected (${toxicity.category})`,
      };
    }
  }

  // Check hallucination
  if (opts.hallucinationEnabled) {
    const hallucination = detectHallucination(content);
    if (hallucination.detected) {
      return {
        allowed: false,
        reason: `Blocked: hallucination detected (${hallucination.pattern})`,
      };
    }
  }

  return { allowed: true, content };
}
