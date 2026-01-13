// ssmd-notifier/src/router.ts
import type { SignalFire, MatchRule, Destination } from "./types.ts";

/**
 * Check if a signal fire matches a single rule.
 */
export function matches(fire: SignalFire, rule: MatchRule): boolean {
  const fieldValue = fire[rule.field as keyof SignalFire];
  if (fieldValue === undefined) return false;

  const strValue = String(fieldValue);
  switch (rule.operator) {
    case "eq":
      return strValue === rule.value;
    case "contains":
      return strValue.includes(rule.value);
    default:
      return false;
  }
}

/**
 * Check if a fire should be routed to a destination.
 * No match rule = route all fires.
 */
export function shouldRoute(fire: SignalFire, dest: Destination): boolean {
  if (!dest.match) return true;
  return matches(fire, dest.match);
}
