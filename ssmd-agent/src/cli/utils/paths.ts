// Path utilities for finding ssmd directories
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";

/**
 * Find the exchanges root directory by walking up from cwd
 */
export async function findExchangesRoot(): Promise<string> {
  let dir = Deno.cwd();

  while (dir !== "/" && dir !== "") {
    try {
      const exchangesPath = join(dir, "exchanges");
      const stat = await Deno.stat(exchangesPath);
      if (stat.isDirectory) {
        return exchangesPath;
      }
    } catch {
      // Not found, keep looking
    }

    const parent = join(dir, "..");
    if (parent === dir) break;
    dir = parent;
  }

  throw new Error("exchanges directory not found. Run 'ssmd init' first.");
}

/**
 * Get the feeds directory path
 */
export async function getFeedsDir(): Promise<string> {
  const root = await findExchangesRoot();
  return join(root, "feeds");
}

/**
 * Get the schemas directory path
 */
export async function getSchemasDir(): Promise<string> {
  const root = await findExchangesRoot();
  return join(root, "schemas");
}

/**
 * Get the environments directory path
 */
export async function getEnvsDir(): Promise<string> {
  const root = await findExchangesRoot();
  return join(root, "environments");
}
