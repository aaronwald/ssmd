export interface InjectionResult {
  detected: boolean;
  pattern?: string;
}

const injectionPatterns = [
  /ignore\s+(all\s+)?(previous|prior|above)\s+(instructions|prompts|rules)/i,
  /disregard\s+(all\s+)?(previous|prior|above|everything)/i,
  /forget\s+(all\s+)?(previous|prior|above|your)\s+(instructions|rules|training)/i,
  /you\s+are\s+now\s+(a|an|DAN|evil|unrestricted)/i,
  /pretend\s+(you're|you\s+are)\s+(a|an|not)\s+(AI|assistant|bound)/i,
  /bypass\s+(your|all|any)\s+(restrictions|rules|safety|filters)/i,
  /jailbreak/i,
  /\bDAN\b.*\bdo\s+anything\s+now\b/i,
];

export function detectInjection(text: string): InjectionResult {
  for (const pattern of injectionPatterns) {
    if (pattern.test(text)) {
      return { detected: true, pattern: pattern.source };
    }
  }
  return { detected: false };
}
