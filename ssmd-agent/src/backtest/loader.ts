import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { parse as parseYaml } from "yaml";
import { BacktestManifestSchema, type BacktestManifest } from "../lib/types/backtest.ts";

/**
 * Loaded signal with code and optional manifest
 */
export interface LoadedSignal {
  /** Signal identifier extracted from code */
  id: string;
  /** Optional human-readable name */
  name?: string;
  /** Required state types */
  requires: string[];
  /** Raw signal code */
  code: string;
  /** Parsed backtest manifest if present */
  manifest: BacktestManifest | null;
  /** Path to signal.ts file */
  path: string;
}

/**
 * Load a signal from a directory or direct path.
 * Extracts metadata from the signal code and loads any backtest.yaml manifest.
 */
export async function loadSignal(signalPath: string): Promise<LoadedSignal> {
  // Determine directory and code path
  let dir = signalPath;
  let codePath = signalPath;

  if (signalPath.endsWith(".ts")) {
    dir = signalPath.replace(/\/signal\.ts$/, "").replace(/\/[^/]+\.ts$/, "");
    codePath = signalPath;
  } else {
    codePath = join(signalPath, "signal.ts");
  }

  // Read signal code
  const code = await Deno.readTextFile(codePath);

  // Try to read manifest
  let manifest: BacktestManifest | null = null;
  try {
    const manifestPath = join(dir, "backtest.yaml");
    const manifestContent = await Deno.readTextFile(manifestPath);
    manifest = BacktestManifestSchema.parse(parseYaml(manifestContent));
  } catch {
    // No manifest, that's ok
  }

  // Extract signal metadata from code using regex
  const idMatch = code.match(/id:\s*["']([^"']+)["']/);
  const nameMatch = code.match(/name:\s*["']([^"']+)["']/);
  const requiresMatch = code.match(/requires:\s*\[([^\]]*)\]/);

  const id = idMatch?.[1] ?? "unknown";
  const name = nameMatch?.[1];
  const requires = requiresMatch?.[1]
    ?.split(",")
    .map((s) => s.trim().replace(/["']/g, ""))
    .filter(Boolean) ?? [];

  return {
    id,
    name,
    requires,
    code,
    manifest,
    path: codePath,
  };
}

/**
 * Get the current git SHA (short form)
 */
export async function getGitSha(cwd?: string): Promise<string> {
  const cmd = new Deno.Command("git", {
    args: ["rev-parse", "--short", "HEAD"],
    stdout: "piped",
    stderr: "piped",
    cwd,
  });
  const result = await cmd.output();

  if (!result.success) {
    throw new Error("Failed to get git SHA");
  }

  return new TextDecoder().decode(result.stdout).trim();
}

/**
 * Check if a signal path has uncommitted changes
 */
export async function isSignalDirty(signalPath: string, cwd?: string): Promise<boolean> {
  const cmd = new Deno.Command("git", {
    args: ["status", "--porcelain", signalPath],
    stdout: "piped",
    stderr: "piped",
    cwd,
  });
  const result = await cmd.output();
  const output = new TextDecoder().decode(result.stdout).trim();
  return output.length > 0;
}

/**
 * Expand a date range into an array of YYYY-MM-DD strings
 */
export function expandDateRange(from: string, to: string): string[] {
  const dates: string[] = [];
  const start = new Date(from);
  const end = new Date(to);

  const current = new Date(start);
  while (current <= end) {
    dates.push(current.toISOString().split("T")[0]);
    current.setDate(current.getDate() + 1);
  }

  return dates;
}

/**
 * Get the effective dates for a loaded signal.
 * Uses explicit dates if provided, otherwise expands date_range.
 */
export function getEffectiveDates(
  manifest: BacktestManifest | null,
  overrideDates?: string[],
  overrideFrom?: string,
  overrideTo?: string
): string[] {
  // Command-line overrides take precedence
  if (overrideDates && overrideDates.length > 0) {
    return overrideDates;
  }
  if (overrideFrom && overrideTo) {
    return expandDateRange(overrideFrom, overrideTo);
  }

  // Fall back to manifest
  if (!manifest) {
    return [];
  }

  if (manifest.dates) {
    return manifest.dates;
  }

  if (manifest.date_range) {
    return expandDateRange(manifest.date_range.from, manifest.date_range.to);
  }

  return [];
}
