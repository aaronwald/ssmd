// ssmd-agent/src/cli-utils.ts
// Utility functions for CLI output formatting

/**
 * Format tool call arguments for display
 */
export function formatArgs(input: unknown): string {
  if (typeof input === "object" && input !== null) {
    const obj = input as Record<string, unknown>;
    const parts = Object.entries(obj)
      .filter(([_, v]) => v !== undefined)
      .map(([k, v]) => `${k}=${JSON.stringify(v)}`);
    return parts.join(", ");
  }
  return String(input);
}

/**
 * Format tool result for display (truncated/summarized)
 */
export function formatResult(output: unknown): string {
  if (typeof output === "string") {
    try {
      const parsed = JSON.parse(output);
      if (Array.isArray(parsed)) {
        return `${parsed.length} items`;
      }
      if (parsed.count !== undefined) {
        return `${parsed.count} snapshots`;
      }
      if (parsed.fires !== undefined) {
        return `${parsed.fires} fires, ${parsed.errors?.length ?? 0} errors`;
      }
      if (parsed.sha) {
        return `Committed: ${parsed.sha}`;
      }
      return output.slice(0, 100) + (output.length > 100 ? "..." : "");
    } catch {
      return output.slice(0, 100) + (output.length > 100 ? "..." : "");
    }
  }
  return String(output);
}
