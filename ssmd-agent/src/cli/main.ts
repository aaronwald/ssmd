// CLI entry point
import { run } from "./mod.ts";
import { sendFailureEmail } from "./utils/notify.ts";

if (import.meta.main) {
  try {
    await run(Deno.args);
  } catch (error) {
    const shouldNotify =
      Deno.args.includes("--notify-on-failure") ||
      Deno.env.get("NOTIFY_ON_FAILURE") === "true";

    if (shouldNotify) {
      const command = Deno.args.filter((a) => a !== "--notify-on-failure").join(" ");
      try {
        await sendFailureEmail(command, error as Error);
      } catch (notifyErr) {
        console.error("[notify] Failed to send notification:", notifyErr);
      }
    }

    console.error(error);
    Deno.exit(1);
  }
}
