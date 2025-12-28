// CLI entry point
import { run } from "./mod.ts";

if (import.meta.main) {
  await run(Deno.args);
}
