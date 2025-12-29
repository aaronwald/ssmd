// ssmd-agent/src/config.ts

// Expected API version - update when adding tools that require new endpoints
export const EXPECTED_API_VERSION = "0.3.0";

export const config = {
  apiUrl: Deno.env.get("SSMD_API_URL") ?? "http://localhost:8080",
  apiKey: Deno.env.get("SSMD_DATA_API_KEY") ?? "",
  anthropicApiKey: Deno.env.get("ANTHROPIC_API_KEY") ?? "",
  model: Deno.env.get("SSMD_MODEL") ?? "claude-sonnet-4-20250514",
  skillsPath: Deno.env.get("SSMD_SKILLS_PATH") ?? "./skills",
  promptsPath: Deno.env.get("SSMD_PROMPTS_PATH") ?? "./prompts",
  signalsPath: Deno.env.get("SSMD_SIGNALS_PATH") ?? "./signals",
  natsUrl: Deno.env.get("NATS_URL") ?? "nats://localhost:4222",
  natsStream: Deno.env.get("NATS_STREAM") ?? "PROD_KALSHI",
};

export function validateConfig(): void {
  if (!config.apiKey) {
    throw new Error("SSMD_DATA_API_KEY required");
  }
  if (!config.anthropicApiKey) {
    throw new Error("ANTHROPIC_API_KEY required");
  }
}

/**
 * Check ssmd-data API version compatibility.
 * Warns if server is older or unreachable.
 */
export async function checkApiVersion(): Promise<void> {
  try {
    const res = await fetch(`${config.apiUrl}/version`, {
      signal: AbortSignal.timeout(5000),
    });

    if (!res.ok) {
      if (res.status === 404) {
        console.warn(
          `⚠️  ssmd-data server does not support /version endpoint (server may be outdated)`
        );
        console.warn(
          `   Expected API version: ${EXPECTED_API_VERSION}. Some tools may not work.`
        );
      } else {
        console.warn(`⚠️  ssmd-data /version check failed: ${res.status}`);
      }
      return;
    }

    const { version } = await res.json() as { version: string };

    if (version !== EXPECTED_API_VERSION) {
      console.warn(
        `⚠️  API version mismatch: server=${version}, expected=${EXPECTED_API_VERSION}`
      );
      console.warn(`   Some tools may not work correctly.`);
    }
  } catch (err) {
    if (err instanceof DOMException && err.name === "TimeoutError") {
      console.warn(`⚠️  ssmd-data server not reachable at ${config.apiUrl}`);
    } else {
      console.warn(`⚠️  Failed to check API version: ${err}`);
    }
  }
}
