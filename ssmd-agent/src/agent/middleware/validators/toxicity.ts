export interface ToxicityResult {
  toxic: boolean;
  category?: string;
  pattern?: string;
}

interface ToxicPattern {
  pattern: RegExp;
  category: string;
}

const TOXIC_PATTERNS: ToxicPattern[] = [
  // Threats
  { pattern: /\b(kill|murder|hurt|attack|destroy)\s+(you|them|him|her)\b/i, category: "threat" },
  { pattern: /\bi('ll| will)\s+(kill|hurt|attack)\b/i, category: "threat" },

  // Profanity
  { pattern: /\b(damn|hell|crap|stupid|idiot|moron|dumb)\b/i, category: "profanity" },
  { pattern: /\b(f+u+c+k+|s+h+i+t+|a+s+s+h+o+l+e+)\b/i, category: "profanity" },

  // Hate speech patterns
  { pattern: /\ball\s+(those|these)\s+\w+\s+are\s+(worthless|stupid|evil)\b/i, category: "hate" },
  { pattern: /\b(hate|despise)\s+(all|every)\b/i, category: "hate" },

  // Insults
  { pattern: /\byou\s+(idiot|moron|fool|loser)\b/i, category: "insult" },
];

export function checkToxicity(text: string): ToxicityResult {
  for (const { pattern, category } of TOXIC_PATTERNS) {
    if (pattern.test(text)) {
      return { toxic: true, category, pattern: pattern.source };
    }
  }
  return { toxic: false };
}
