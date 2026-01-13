// ssmd-notifier/src/config.ts
import type { NotifierConfig, Destination } from "./types.ts";

/**
 * Parse destinations JSON string.
 */
export function parseDestinations(json: string): Destination[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    throw new Error("Failed to parse destinations JSON");
  }

  if (!Array.isArray(parsed)) {
    throw new Error("Destinations must be an array");
  }

  return parsed as Destination[];
}

/**
 * Load configuration from environment variables.
 */
export function loadConfig(): NotifierConfig {
  const natsUrl = Deno.env.get("NATS_URL");
  if (!natsUrl) {
    throw new Error("NATS_URL environment variable is required");
  }

  const stream = Deno.env.get("STREAM");
  if (!stream) {
    throw new Error("STREAM environment variable is required");
  }

  const consumer = Deno.env.get("CONSUMER");
  if (!consumer) {
    throw new Error("CONSUMER environment variable is required");
  }

  // Optional filter subject (e.g., "signals.volume-1m-30min.>")
  const filterSubject = Deno.env.get("FILTER_SUBJECT");

  const configPath = Deno.env.get("DESTINATIONS_CONFIG");
  if (!configPath) {
    throw new Error("DESTINATIONS_CONFIG environment variable is required");
  }

  const destJson = Deno.readTextFileSync(configPath);
  const destinations = parseDestinations(destJson);

  return { natsUrl, stream, consumer, filterSubject, destinations };
}
