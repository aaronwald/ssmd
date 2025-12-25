// ssmd-agent/src/config.ts
export const config = {
  dataUrl: Deno.env.get("SSMD_DATA_URL") ?? "http://localhost:8080",
  dataApiKey: Deno.env.get("SSMD_DATA_API_KEY") ?? "",
  anthropicApiKey: Deno.env.get("ANTHROPIC_API_KEY") ?? "",
  model: Deno.env.get("SSMD_MODEL") ?? "claude-sonnet-4-20250514",
  skillsPath: Deno.env.get("SSMD_SKILLS_PATH") ?? "./skills",
  signalsPath: Deno.env.get("SSMD_SIGNALS_PATH") ?? "./signals",
};

export function validateConfig(): void {
  if (!config.dataApiKey) {
    throw new Error("SSMD_DATA_API_KEY required");
  }
  if (!config.anthropicApiKey) {
    throw new Error("ANTHROPIC_API_KEY required");
  }
}
