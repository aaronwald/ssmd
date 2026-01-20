// Shared kubectl utilities with environment context support
// All commands should use these helpers to ensure correct cluster/namespace targeting

import { getEnvContext } from "./env-context.ts";

export interface KubectlOptions {
  // Override environment (--env flag)
  env?: string;
  // Override namespace (-n flag) - takes precedence over env
  namespace?: string;
}

// Execute kubectl with environment context
export async function kubectl(
  args: string[],
  options: KubectlOptions = {}
): Promise<string> {
  const context = await getEnvContext(options.env);

  // Build args with context
  const fullArgs = ["--context", context.cluster];

  // Add namespace if not already in args
  if (!args.includes("-n") && !args.includes("--namespace")) {
    const ns = options.namespace ?? context.namespace;
    fullArgs.push("-n", ns);
  }

  fullArgs.push(...args);

  const cmd = new Deno.Command("kubectl", {
    args: fullArgs,
    stdout: "piped",
    stderr: "piped",
  });
  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(`kubectl failed: ${err}`);
  }

  return new TextDecoder().decode(stdout);
}

// Stream kubectl output (for logs, etc.)
export async function kubectlStream(
  args: string[],
  options: KubectlOptions = {}
): Promise<void> {
  const context = await getEnvContext(options.env);

  // Build args with context
  const fullArgs = ["--context", context.cluster];

  // Add namespace if not already in args
  if (!args.includes("-n") && !args.includes("--namespace")) {
    const ns = options.namespace ?? context.namespace;
    fullArgs.push("-n", ns);
  }

  fullArgs.push(...args);

  const cmd = new Deno.Command("kubectl", {
    args: fullArgs,
    stdout: "inherit",
    stderr: "inherit",
  });

  const { code } = await cmd.output();

  if (code !== 0) {
    throw new Error(`kubectl failed with code ${code}`);
  }
}

// Execute flux CLI with environment context
export async function flux(
  args: string[],
  options: KubectlOptions = {}
): Promise<string> {
  const context = await getEnvContext(options.env);

  // Flux uses --context for kubeconfig context
  const fullArgs = ["--context", context.cluster, ...args];

  const cmd = new Deno.Command("flux", {
    args: fullArgs,
    stdout: "piped",
    stderr: "piped",
  });
  const { stdout, stderr, code } = await cmd.output();

  if (code !== 0) {
    const err = new TextDecoder().decode(stderr);
    throw new Error(err.trim());
  }

  return new TextDecoder().decode(stdout);
}

// Get the current environment name for display
export async function getCurrentEnvDisplay(envOverride?: string): Promise<string> {
  const context = await getEnvContext(envOverride);
  return `${context.envName} (${context.cluster}/${context.namespace})`;
}
