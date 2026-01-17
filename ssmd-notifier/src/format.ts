// ssmd-notifier/src/format.ts

/**
 * Convert camelCase to Title Case
 * e.g., "dollarVolume" -> "Dollar Volume"
 */
function camelToTitle(str: string): string {
  return str
    .replace(/([A-Z])/g, " $1")
    .replace(/^./, (c) => c.toUpperCase())
    .trim();
}

/**
 * Format a number with thousand separators
 */
function formatNumber(n: number, decimals = 2): string {
  if (Number.isInteger(n)) {
    return n.toLocaleString("en-US");
  }
  return n.toLocaleString("en-US", {
    minimumFractionDigits: 0,
    maximumFractionDigits: decimals,
  });
}

/**
 * Format milliseconds to human-readable duration
 * e.g., 1800000 -> "30m", 3600000 -> "1h"
 */
function formatDuration(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);

  if (days > 0) return `${days}d`;
  if (hours > 0) return `${hours}h`;
  if (minutes > 0) return `${minutes}m`;
  return `${seconds}s`;
}

/**
 * Check if a string is an ISO 8601 date
 */
function isIsoDate(str: string): boolean {
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/.test(str);
}

/**
 * Format an ISO date to compact form
 * e.g., "2026-01-16T21:30:00.000Z" -> "2026-01-16 21:30 UTC"
 */
function formatDate(isoStr: string): string {
  const date = new Date(isoStr);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  const hours = String(date.getUTCHours()).padStart(2, "0");
  const mins = String(date.getUTCMinutes()).padStart(2, "0");
  return `${year}-${month}-${day} ${hours}:${mins} UTC`;
}

/**
 * Check if a key suggests currency value
 */
function isCurrencyKey(key: string): boolean {
  const lower = key.toLowerCase();
  return (
    lower.includes("dollar") ||
    lower.includes("price") ||
    lower.includes("cost") ||
    lower.includes("amount")
  );
}

/**
 * Check if a key suggests percentage value
 */
function isPercentageKey(key: string): boolean {
  const lower = key.toLowerCase();
  return lower.includes("ratio") || lower.includes("percent");
}

/**
 * Check if a key suggests duration in milliseconds
 */
function isDurationKey(key: string): boolean {
  return key.endsWith("Ms") || key.endsWith("ms");
}

/**
 * Format a single value based on its key and type
 */
function formatValue(key: string, value: unknown): string {
  if (value === null || value === undefined) {
    return "-";
  }

  if (typeof value === "string") {
    if (isIsoDate(value)) {
      return formatDate(value);
    }
    return value;
  }

  if (typeof value === "number") {
    if (isDurationKey(key)) {
      return formatDuration(value);
    }
    if (isCurrencyKey(key)) {
      return `$${formatNumber(value)}`;
    }
    if (isPercentageKey(key)) {
      return `${formatNumber(value * 100, 1)}%`;
    }
    return formatNumber(value);
  }

  if (typeof value === "boolean") {
    return value ? "Yes" : "No";
  }

  if (Array.isArray(value)) {
    return value.map((v) => String(v)).join(", ");
  }

  if (typeof value === "object") {
    // Skip nested objects for now
    return JSON.stringify(value);
  }

  return String(value);
}

/**
 * Format any payload object to human-readable key-value text
 */
export function formatPayload(payload: unknown): string {
  if (payload === null || payload === undefined) {
    return "";
  }

  if (typeof payload !== "object") {
    return String(payload);
  }

  const lines: string[] = [];
  const obj = payload as Record<string, unknown>;

  for (const [key, value] of Object.entries(obj)) {
    const label = camelToTitle(key);
    const formatted = formatValue(key, value);
    lines.push(`${label}: ${formatted}`);
  }

  return lines.join("\n");
}
