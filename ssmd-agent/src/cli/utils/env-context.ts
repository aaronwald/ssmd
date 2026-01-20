// Environment context loader
// Reads/writes ~/.ssmd/config.yaml and provides current environment context

import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import { parse as parseYaml, stringify as stringifyYaml } from "https://deno.land/std@0.224.0/yaml/mod.ts";
import {
  EnvConfigSchema,
  type EnvConfig,
  type Environment,
  getDefaultConfig,
} from "../../lib/types/env-config.ts";

// Get the config directory path
export function getConfigDir(): string {
  const home = Deno.env.get("HOME") || Deno.env.get("USERPROFILE") || "";
  return join(home, ".ssmd");
}

// Get the config file path
export function getConfigPath(): string {
  return join(getConfigDir(), "config.yaml");
}

// Ensure config directory exists
async function ensureConfigDir(): Promise<void> {
  const dir = getConfigDir();
  try {
    await Deno.mkdir(dir, { recursive: true });
  } catch (e) {
    if (!(e instanceof Deno.errors.AlreadyExists)) {
      throw e;
    }
  }
}

// Load config from file, creating default if not exists
export async function loadConfig(): Promise<EnvConfig> {
  const configPath = getConfigPath();

  try {
    const content = await Deno.readTextFile(configPath);
    const parsed = parseYaml(content);
    return EnvConfigSchema.parse(parsed);
  } catch (e) {
    if (e instanceof Deno.errors.NotFound) {
      // Create default config
      const defaultConfig = getDefaultConfig();
      await saveConfig(defaultConfig);
      return defaultConfig;
    }
    throw e;
  }
}

// Save config to file
export async function saveConfig(config: EnvConfig): Promise<void> {
  await ensureConfigDir();
  const configPath = getConfigPath();
  const content = stringifyYaml(config as Record<string, unknown>);
  await Deno.writeTextFile(configPath, content);
}

// Get current environment name
export async function getCurrentEnvName(): Promise<string> {
  const config = await loadConfig();
  return config["current-env"];
}

// Get current environment config
export async function getCurrentEnv(): Promise<Environment> {
  const config = await loadConfig();
  const envName = config["current-env"];
  const env = config.environments[envName];

  if (!env) {
    throw new Error(`Environment '${envName}' not found in config`);
  }

  return env;
}

// Get environment by name
export async function getEnv(name: string): Promise<Environment> {
  const config = await loadConfig();
  const env = config.environments[name];

  if (!env) {
    throw new Error(`Environment '${name}' not found in config`);
  }

  return env;
}

// Set current environment
export async function setCurrentEnv(name: string): Promise<void> {
  const config = await loadConfig();

  if (!config.environments[name]) {
    throw new Error(`Environment '${name}' not found. Available: ${Object.keys(config.environments).join(", ")}`);
  }

  config["current-env"] = name;
  await saveConfig(config);
}

// List all environment names
export async function listEnvNames(): Promise<string[]> {
  const config = await loadConfig();
  return Object.keys(config.environments);
}

// Get environment context for kubectl commands
// Supports --env flag override
export async function getEnvContext(envOverride?: string): Promise<{
  envName: string;
  cluster: string;
  namespace: string;
  env: Environment;
}> {
  const config = await loadConfig();
  const envName = envOverride || config["current-env"];
  const env = config.environments[envName];

  if (!env) {
    throw new Error(`Environment '${envName}' not found in config`);
  }

  return {
    envName,
    cluster: env.cluster,
    namespace: env.namespace,
    env,
  };
}

// Build kubectl args with context and namespace from environment
export function buildKubectlArgs(
  args: string[],
  context: { cluster: string; namespace: string }
): string[] {
  const result = ["--context", context.cluster];

  // Add namespace if not already specified
  if (!args.includes("-n") && !args.includes("--namespace")) {
    result.push("-n", context.namespace);
  }

  return [...result, ...args];
}
