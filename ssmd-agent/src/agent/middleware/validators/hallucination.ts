export interface HallucinationResult {
  detected: boolean;
  pattern?: string;
}

const HALLUCINATION_PATTERNS = [
  // Claims specific data without tool call
  /the current price is \$[\d.]+/i,
  /there are \d+ active markets/i,
  // Invented ticker symbols (6+ uppercase letters)
  /ticker [A-Z]{6,}/,
  // Overconfident predictions
  /will definitely/i,
  /guaranteed to/i,
  /100% certain/i,
  /100% sure/i,
  /absolutely will/i,
  /certain to (win|lose|happen)/i,
];

export function detectHallucination(text: string): HallucinationResult {
  for (const pattern of HALLUCINATION_PATTERNS) {
    if (pattern.test(text)) {
      return { detected: true, pattern: pattern.source };
    }
  }
  return { detected: false };
}
