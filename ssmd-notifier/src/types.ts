// ssmd-notifier/src/types.ts

/** Signal fire event from NATS */
export interface SignalFire {
  signalId: string;
  ts: number;
  ticker: string;
  payload: unknown;
}

/** Match rule for routing */
export interface MatchRule {
  field: string;
  operator: "eq" | "contains";
  value: string;
}

/** ntfy-specific configuration */
export interface NtfyConfig {
  server?: string;
  topic: string;
  priority?: "min" | "low" | "default" | "high" | "urgent";
}

/** Notification destination */
export interface Destination {
  name: string;
  type: "ntfy";
  config: NtfyConfig;
  match?: MatchRule;
}

/** Notifier configuration */
export interface NotifierConfig {
  natsUrl: string;
  subjects: string[];
  destinations: Destination[];
}
