// Standalone entry point for lifecycle-consumer daemon.
// Compiled separately from main CLI to avoid DuckDB dependency.
import { runLifecycleConsumer } from "./commands/lifecycle-consumer.ts";

await runLifecycleConsumer();
