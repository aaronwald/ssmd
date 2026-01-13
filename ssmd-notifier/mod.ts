// ssmd-notifier/mod.ts
import { loadConfig } from "./src/config.ts";
import { runConsumer } from "./src/consumer.ts";
import { startServer } from "./src/server.ts";

console.log("=== SSMD Notifier ===");
console.log();

try {
  const config = loadConfig();

  console.log(`NATS: ${config.natsUrl}`);
  console.log(`Stream: ${config.stream}`);
  console.log(`Consumer: ${config.consumer}`);
  if (config.filterSubject) {
    console.log(`Filter: ${config.filterSubject}`);
  }
  console.log(`Destinations: ${config.destinations.length}`);
  for (const dest of config.destinations) {
    console.log(`  - ${dest.name} (${dest.type})`);
  }
  console.log();

  // Start health server
  startServer(9090);

  // Run consumer (blocks until shutdown)
  await runConsumer(config);
} catch (e) {
  console.error(`Fatal error: ${e}`);
  Deno.exit(1);
}
