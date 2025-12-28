// Init command: create exchanges directory structure
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";

const SUBDIRS = ["feeds", "schemas", "environments"];

const GITKEEP_DIRS = ["feeds", "schemas", "environments"];

/**
 * Initialize the exchanges directory structure
 */
export async function initExchanges(path?: string): Promise<string> {
  const targetDir = path ?? join(Deno.cwd(), "exchanges");

  // Check if already exists
  try {
    const stat = await Deno.stat(targetDir);
    if (stat.isDirectory) {
      throw new Error(`Directory already exists: ${targetDir}`);
    }
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) throw e;
  }

  // Create root directory
  await Deno.mkdir(targetDir, { recursive: true });
  console.log(`Created: ${targetDir}`);

  // Create subdirectories with .gitkeep files
  for (const subdir of SUBDIRS) {
    const subdirPath = join(targetDir, subdir);
    await Deno.mkdir(subdirPath, { recursive: true });
    console.log(`Created: ${subdirPath}`);

    if (GITKEEP_DIRS.includes(subdir)) {
      const gitkeepPath = join(subdirPath, ".gitkeep");
      await Deno.writeTextFile(gitkeepPath, "");
    }
  }

  return targetDir;
}
