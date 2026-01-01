export interface PIIMatch {
  type: "email" | "credit_card" | "ssn" | "phone";
  match: string;
  start: number;
  end: number;
}

const patterns: Record<string, RegExp> = {
  email: /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g,
  credit_card: /\b(?:\d{4}[-\s]?){3}\d{4}\b/g,
  ssn: /\b\d{3}-\d{2}-\d{4}\b/g,
  phone: /(?:\(\d{3}\)|\b\d{3})[-.\s]?\d{3}[-.\s]?\d{4}\b/g,
};

export function detectPII(text: string): PIIMatch[] {
  const matches: PIIMatch[] = [];

  for (const [type, pattern] of Object.entries(patterns)) {
    const regex = new RegExp(pattern.source, pattern.flags);
    let match;
    while ((match = regex.exec(text)) !== null) {
      matches.push({
        type: type as PIIMatch["type"],
        match: match[0],
        start: match.index,
        end: match.index + match[0].length,
      });
    }
  }

  return matches;
}

export function redactPII(text: string): string {
  let result = text;
  const matches = detectPII(text);

  // Sort by position descending to replace from end
  matches.sort((a, b) => b.start - a.start);

  for (const m of matches) {
    const placeholder = `[REDACTED_${m.type.toUpperCase()}]`;
    result = result.slice(0, m.start) + placeholder + result.slice(m.end);
  }

  return result;
}

export function hasPII(text: string): boolean {
  return detectPII(text).length > 0;
}
