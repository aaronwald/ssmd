// ssmd-notifier/src/senders/mod.ts
import type { SignalFire, Destination } from "../types.ts";

/** Sender interface for notification destinations */
export interface Sender {
  send(fire: SignalFire, dest: Destination): Promise<void>;
}

export { NtfySender } from "./ntfy.ts";
