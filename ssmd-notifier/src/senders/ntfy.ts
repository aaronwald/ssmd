// ssmd-notifier/src/senders/ntfy.ts
import type { SignalFire, Destination } from "../types.ts";
import type { Sender } from "./mod.ts";
import { formatPayload } from "../format.ts";

const DEFAULT_SERVER = "https://ntfy.sh";

export class NtfySender implements Sender {
  formatTitle(fire: SignalFire): string {
    // HTTP headers must be ASCII - no emojis
    return `Signal ${fire.signalId}: ${fire.ticker}`;
  }

  formatBody(fire: SignalFire): string {
    return formatPayload(fire.payload);
  }

  buildUrl(dest: Destination): string {
    const server = dest.config.server ?? DEFAULT_SERVER;
    return `${server}/${dest.config.topic}`;
  }

  async send(fire: SignalFire, dest: Destination): Promise<void> {
    const url = this.buildUrl(dest);
    const headers: Record<string, string> = {
      "Title": this.formatTitle(fire),
    };

    if (dest.config.priority) {
      headers["Priority"] = dest.config.priority;
    }

    const response = await fetch(url, {
      method: "POST",
      headers,
      body: this.formatBody(fire),
    });

    if (!response.ok) {
      throw new Error(`ntfy request failed: ${response.status} ${response.statusText}`);
    }
  }
}
