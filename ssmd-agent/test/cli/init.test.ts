import { assertEquals, assertExists } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { initExchanges } from "../../src/cli/commands/init.ts";
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";

Deno.test("initExchanges creates directory structure", async () => {
  const tmpDir = await Deno.makeTempDir();
  const targetDir = join(tmpDir, "exchanges");

  await initExchanges(targetDir);

  // Check directories exist
  const stat = await Deno.stat(targetDir);
  assertEquals(stat.isDirectory, true);

  for (const subdir of ["feeds", "schemas", "environments"]) {
    const subdirStat = await Deno.stat(join(targetDir, subdir));
    assertEquals(subdirStat.isDirectory, true);

    // Check .gitkeep exists
    const gitkeepStat = await Deno.stat(join(targetDir, subdir, ".gitkeep"));
    assertEquals(gitkeepStat.isFile, true);
  }

  // Cleanup
  await Deno.remove(tmpDir, { recursive: true });
});

Deno.test("initExchanges fails if directory exists", async () => {
  const tmpDir = await Deno.makeTempDir();
  const targetDir = join(tmpDir, "exchanges");

  // Create first
  await initExchanges(targetDir);

  // Second attempt should fail
  let threw = false;
  try {
    await initExchanges(targetDir);
  } catch {
    threw = true;
  }
  assertEquals(threw, true);

  // Cleanup
  await Deno.remove(tmpDir, { recursive: true });
});
